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
    managed_pdf2zh::{self, openai_shim::ShimProviderConfig},
    rosetta_jobs::formats::pdf::errors::PdfError,
};

const PDF2ZH_PROGRESS_EVENT: &str = "rosetta-pdf2zh-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pdf2zhProgressPayload {
    pub job_id: String,
    /// One of: `warmup` (sidecar/shim being prepared), `parse` (PDF layout
    /// extraction), `translate` (pdf2zh has reached translation), `render`
    /// (translated PDF being assembled). Frontend maps to UI labels.
    pub phase: String,
    pub percent: Option<u8>,
    pub message: String,
    /// 1-based index of the page currently being processed, within the
    /// filtered "pages to translate" list passed to this invocation.
    /// `None` when caller didn't supply per-page progress (whole-document
    /// fallback path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_page: Option<u32>,
    /// Total pages in the filtered list. Paired with `current_page` so the
    /// UI can render "第 X/Y 页".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhInvokeOptions {
    pub job_id: String,
    pub provider: ShimProviderConfig,
    pub source_lang: String,
    pub target_lang: String,
    pub timeout_ms: u64,
    pub ignore_cache: bool,
    pub pages: Option<Vec<u32>>,
    /// `(current_index_1_based, total_to_process)` from the caller's
    /// per-page iteration. The caller filters out already-translated pages
    /// before iterating, so this reflects the user-visible progress
    /// ("3rd of 5 pages I asked to translate"), not absolute page numbers.
    /// `None` for callers that translate the whole document in a single
    /// invocation.
    pub page_progress: Option<(u32, u32)>,
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
    // Helper: emit a progress event tagged with this invocation's page progress.
    // Defined locally so every call site below picks up the current page automatically
    // without threading the tuple through arg lists.
    let page_progress = options.page_progress;
    let emit = |phase: &str, percent: Option<u8>, message: &str| {
        emit_progress_with_page(
            app,
            &options.job_id,
            phase,
            percent,
            message,
            page_progress,
        );
    };

    // Phase: warmup — covers the time from "user clicked translate" to
    // "pdf2zh.py has actually started parsing". This is non-trivial (status
    // resolution, shim listener bind, in some runs `set_chat_roles_for_pair`
    // hitting the RWKV server) and used to be the silent gap that made the
    // UI feel frozen. Emit early so the topbar's elapsed timer has something
    // to count against from the start.
    emit("warmup", Some(0), "正在准备 PDF 翻译引擎…");

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

    emit("warmup", Some(20), "正在启动本地翻译 shim…");
    let shim_log_file = output_dir.join("rosetta-pdf2zh-shim.log");
    let shim = managed_pdf2zh::openai_shim::spawn_shim(
        options.provider.clone(),
        options.source_lang.clone(),
        options.target_lang.clone(),
        shim_log_file.clone(),
        debug,
    )
    .await
    .map_err(PdfError::Pdf2zhFailed)?;
    emit("warmup", Some(60), "翻译 shim 已就绪，启动 PDF 解析进程…");

    let openai_base_url = shim.base_url();
    // NOT `shim.batch_size`: that follows the RWKV server's reported batch
    // ceiling (16 on MLX, 12 on WebRWKV). Reusing it as pdf2zh's `--thread`
    // arg blows up pdf2zh.py's Python multiprocessing pool on small inputs.
    // See `PDF2ZH_THREAD_CEILING` in `openai_shim.rs`.
    let thread_count = shim.pdf2zh_thread_count;
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
        // Tell pdf2zh (and the OpenAI Python SDK underneath) to bypass any
        // system / shell proxy for the loopback shim. Without this, users
        // running Clash/Surge or with HTTP_PROXY set get every shim request
        // routed through their proxy, which returns 502 Bad Gateway because
        // it can't reach 127.0.0.1 from its egress. Set both NO_PROXY and the
        // lowercase no_proxy (Python's urllib reads the lowercase form). Also
        // explicitly clear HTTP_PROXY / HTTPS_PROXY / ALL_PROXY so the OpenAI
        // SDK's httpx client doesn't pick them up via httpx.Client defaults
        // (httpx ignores NO_PROXY for loopback only on some versions).
        .env("NO_PROXY", "127.0.0.1,localhost,::1")
        .env("no_proxy", "127.0.0.1,localhost,::1")
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("ALL_PROXY", "")
        .env("http_proxy", "")
        .env("https_proxy", "")
        .env("all_proxy", "")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(pages) = &options.pages {
        let pages_arg = pages
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        command.arg("--pages").arg(pages_arg);
    }
    let _ = options.ignore_cache;

    emit("translate", Some(0), "正在翻译 PDF…");
    let mut child = command
        .spawn()
        .map_err(|error| PdfError::Pdf2zhFailed(format!("启动 {} 失败: {error}", bin.display())))?;

    let stderr = child.stderr.take();
    let stdout = child.stdout.take();
    let output_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    // Live tee of pdf2zh's stdout+stderr to disk so we can see what it's doing
    // even when it hangs (in which case the failure-only `rosetta-pdf2zh-output.log`
    // never gets written). Lock-protected `Option<File>` because both reader
    // tasks share the same handle. Opened best-effort; logging failures are
    // swallowed to keep the runtime path quiet.
    let live_log_path = output_dir.join("rosetta-pdf2zh-live.log");
    let live_log: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&live_log_path)
            .ok(),
    ));
    let stderr_task = stderr.map(|stream| {
        let app = app.clone();
        let job_id = options.job_id.clone();
        let output_lines = Arc::clone(&output_lines);
        let live_log = Arc::clone(&live_log);
        let page_progress = options.page_progress;
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                remember_line(&output_lines, &line);
                append_live_log(&live_log, "stderr", &line);
                handle_pdf2zh_line(&app, &job_id, &line, page_progress);
            }
        })
    });
    let stdout_task = stdout.map(|stream| {
        let app = app.clone();
        let job_id = options.job_id.clone();
        let output_lines = Arc::clone(&output_lines);
        let live_log = Arc::clone(&live_log);
        let page_progress = options.page_progress;
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                remember_line(&output_lines, &line);
                append_live_log(&live_log, "stdout", &line);
                handle_pdf2zh_line(&app, &job_id, &line, page_progress);
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

    emit("render", Some(95), "正在整理译文 PDF…");
    let mono_pdf = find_pdf2zh_output(output_dir, source_path, "zh")
        .or_else(|| find_pdf2zh_output(output_dir, source_path, "mono"))
        .ok_or_else(|| PdfError::Pdf2zhFailed("未生成译文 PDF。".to_string()))?;
    let dual_pdf = find_pdf2zh_output(output_dir, source_path, "dual");
    emit("render", Some(100), "译文 PDF 已生成。");
    Ok(Pdf2zhOutput { mono_pdf, dual_pdf })
}

fn append_live_log(file: &Arc<Mutex<Option<std::fs::File>>>, stream: &str, line: &str) {
    use std::io::Write;
    let Ok(mut guard) = file.lock() else {
        return;
    };
    let Some(handle) = guard.as_mut() else {
        return;
    };
    let _ = writeln!(handle, "[{stream}] {line}");
    let _ = handle.flush();
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

fn handle_pdf2zh_line(
    app: &AppHandle,
    job_id: &str,
    line: &str,
    page_progress: Option<(u32, u32)>,
) {
    let lower = line.to_ascii_lowercase();
    let phase = if lower.contains("parse") || lower.contains("layout") {
        "parse"
    } else if lower.contains("save") || lower.contains("render") {
        "render"
    } else {
        "translate"
    };
    emit_progress_with_page(
        app,
        job_id,
        phase,
        parse_percent(line),
        line.trim(),
        page_progress,
    );
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

fn emit_progress_with_page(
    app: &AppHandle,
    job_id: &str,
    phase: &str,
    percent: Option<u8>,
    message: &str,
    page_progress: Option<(u32, u32)>,
) {
    let (current_page, total_pages) = match page_progress {
        Some((cur, total)) => (Some(cur), Some(total)),
        None => (None, None),
    };
    let _ = app.emit(
        PDF2ZH_PROGRESS_EVENT,
        Pdf2zhProgressPayload {
            job_id: job_id.to_string(),
            phase: phase.to_string(),
            percent,
            message: message.to_string(),
            current_page,
            total_pages,
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
