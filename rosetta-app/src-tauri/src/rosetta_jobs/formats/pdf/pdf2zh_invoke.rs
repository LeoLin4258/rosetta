use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
};

use tokio::sync::oneshot;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
    managed_pdf2zh,
    rosetta_jobs::formats::pdf::errors::PdfError,
};

const PDF2ZH_PROGRESS_EVENT: &str = "rosetta-pdf2zh-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pdf2zhProgressPayload {
    pub job_id: String,
    pub phase: String,
    pub percent: Option<u8>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhInvokeOptions {
    pub job_id: String,
    pub rwkv_base_url: String,
    pub source_lang: String,
    pub target_lang: String,
    pub timeout_ms: u64,
    pub ignore_cache: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhOutput {
    pub mono_pdf: PathBuf,
    #[allow(dead_code)]
    pub dual_pdf: Option<PathBuf>,
}

pub(crate) async fn invoke_pdf2zh(
    app: &AppHandle,
    source_path: &Path,
    output_dir: &Path,
    options: Pdf2zhInvokeOptions,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<Pdf2zhOutput, PdfError> {
    let status = managed_pdf2zh::build_static_status(app).map_err(PdfError::RuntimeMissing)?;
    if !status.install_plan.ready {
        return Err(PdfError::RuntimeMissing(status.install_plan.message));
    }
    let bin = status
        .bin_path
        .ok_or_else(|| PdfError::RuntimeMissing("找不到 PDF 版面处理组件。".to_string()))?;
    status
        .layout
        .ensure_dirs()
        .map_err(PdfError::RuntimeMissing)?;
    std::fs::create_dir_all(output_dir)
        .map_err(|error| PdfError::Read(format!("无法创建 pdf2zh 输出目录: {error}")))?;
    let temp_dir = output_dir.join("tmp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|error| PdfError::Read(format!("无法创建 pdf2zh 临时目录: {error}")))?;
    let debug = pdf2zh_debug_enabled();

    emit_progress(app, &options.job_id, "parse", Some(5), "正在准备 PDF 版面...");
    let shim_log_file = output_dir.join("rosetta-pdf2zh-shim.log");
    let shim = managed_pdf2zh::openai_shim::spawn_shim(
        options.rwkv_base_url.clone(),
        options.source_lang.clone(),
        options.target_lang.clone(),
        options.timeout_ms,
        shim_log_file.clone(),
        debug,
    )
    .await
    .map_err(PdfError::Pdf2zhFailed)?;

    let openai_base_url = shim.base_url();
    let thread_count = shim.batch_size;
    std::fs::write(
        output_dir.join("rosetta-pdf2zh-command.log"),
        format!(
            "bin={}\nsource={}\noutput_dir={}\ntemp_dir={}\nopenai_base_url={}\nservice=openai:rwkv\nsource_lang={}\ntarget_lang={}\nthreads={}\ndebug={}\nshim_log={}\n",
            bin.display(),
            source_path.display(),
            output_dir.display(),
            temp_dir.display(),
            openai_base_url,
            options.source_lang,
            options.target_lang,
            thread_count,
            debug,
            shim_log_file.display(),
        ),
    )
    .ok();
    let mut command = tokio::process::Command::new(&bin);
    command
        .arg(source_path)
        .arg("-li")
        .arg(pdf2zh_lang(&options.source_lang))
        .arg("-lo")
        .arg(pdf2zh_lang(&options.target_lang))
        .arg("-s")
        .arg("openai:rwkv")
        .arg("-t")
        .arg(thread_count.to_string())
        .current_dir(output_dir)
        .env("OPENAI_BASE_URL", &openai_base_url)
        .env("OPENAI_API_KEY", "rosetta-local")
        .env("OPENAI_MODEL", "rwkv")
        .env("TMPDIR", &temp_dir)
        .env("TEMP", &temp_dir)
        .env("TMP", &temp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let _ = options.ignore_cache;

    emit_progress(app, &options.job_id, "translate", Some(0), "正在翻译 PDF...");
    let mut child = command
        .spawn()
        .map_err(|error| PdfError::Pdf2zhFailed(format!("启动 {} 失败: {error}", bin.display())))?;

    let stderr = child.stderr.take();
    let stdout = child.stdout.take();
    let output_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr_task = stderr.map(|stream| {
        let app = app.clone();
        let job_id = options.job_id.clone();
        let output_lines = Arc::clone(&output_lines);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                remember_line(&output_lines, &line);
                handle_pdf2zh_line(&app, &job_id, &line);
            }
        })
    });
    let stdout_task = stdout.map(|stream| {
        let app = app.clone();
        let job_id = options.job_id.clone();
        let output_lines = Arc::clone(&output_lines);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                remember_line(&output_lines, &line);
                handle_pdf2zh_line(&app, &job_id, &line);
            }
        })
    });

    let exit_status = tokio::select! {
        result = child.wait() => {
            result.map_err(|error| PdfError::Pdf2zhFailed(format!("等待 pdf2zh 结束失败: {error}")))?
        }
        _ = cancel_rx => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            if let Some(task) = stderr_task { task.abort(); }
            if let Some(task) = stdout_task { task.abort(); }
            drop(shim);
            return Err(PdfError::Cancelled);
        }
    };
    if let Some(task) = stderr_task {
        let _ = task.await;
    }
    if let Some(task) = stdout_task {
        let _ = task.await;
    }
    drop(shim);
    let status = exit_status;

    if !status.success() {
        let tail = output_lines
            .lock()
            .ok()
            .map(|lines| lines.join("\n"))
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "无 stderr/stdout 输出。".to_string());
        let output_log = output_dir.join("rosetta-pdf2zh-output.log");
        let _ = std::fs::write(&output_log, &tail);
        return Err(PdfError::Pdf2zhFailed(format!(
            "PDF 版面处理没有完成（退出码：{}）。请重试；若持续失败，可查看日志：{}",
            status.code().map_or_else(|| "signal".to_string(), |code| code.to_string()),
            output_log.display()
        )));
    }

    emit_progress(app, &options.job_id, "render", Some(95), "正在整理译文 PDF...");
    let mono_pdf = find_pdf2zh_output(output_dir, source_path, "zh")
        .or_else(|| find_pdf2zh_output(output_dir, source_path, "mono"))
        .ok_or_else(|| PdfError::Pdf2zhFailed("未生成译文 PDF。".to_string()))?;
    let dual_pdf = find_pdf2zh_output(output_dir, source_path, "dual");
    emit_progress(app, &options.job_id, "render", Some(100), "译文 PDF 已生成。");
    Ok(Pdf2zhOutput { mono_pdf, dual_pdf })
}

fn remember_line(lines: &Arc<Mutex<Vec<String>>>, line: &str) {
    let Ok(mut lines) = lines.lock() else {
        return;
    };
    lines.push(line.trim().to_string());
    if lines.len() > 30 {
        lines.remove(0);
    }
}

fn handle_pdf2zh_line(app: &AppHandle, job_id: &str, line: &str) {
    let lower = line.to_ascii_lowercase();
    let phase = if lower.contains("parse") || lower.contains("layout") {
        "parse"
    } else if lower.contains("save") || lower.contains("render") {
        "render"
    } else {
        "translate"
    };
    emit_progress(app, job_id, phase, parse_percent(line), line.trim());
}

fn parse_percent(line: &str) -> Option<u8> {
    let percent_pos = line.find('%')?;
    let digits = line[..percent_pos]
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let normalized = digits.chars().rev().collect::<String>();
    normalized.parse::<u8>().ok().map(|value| value.min(100))
}

fn emit_progress(app: &AppHandle, job_id: &str, phase: &str, percent: Option<u8>, message: &str) {
    let _ = app.emit(
        PDF2ZH_PROGRESS_EVENT,
        Pdf2zhProgressPayload {
            job_id: job_id.to_string(),
            phase: phase.to_string(),
            percent,
            message: message.to_string(),
        },
    );
}

fn find_pdf2zh_output(output_dir: &Path, source_path: &Path, kind: &str) -> Option<PathBuf> {
    let stem = source_path.file_stem().and_then(OsStr::to_str)?;
    let expected = output_dir.join(format!("{stem}-{kind}.pdf"));
    if expected.is_file() {
        return Some(expected);
    }
    std::fs::read_dir(output_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with(&format!("-{kind}.pdf")))
        })
}

fn pdf2zh_lang(lang: &str) -> &str {
    match lang {
        "zh-CN" | "zh-TW" | "zh" => "zh",
        "en" => "en",
        other => other,
    }
}

fn pdf2zh_debug_enabled() -> bool {
    std::env::var("ROSETTA_PDF2ZH_DEBUG")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
}
