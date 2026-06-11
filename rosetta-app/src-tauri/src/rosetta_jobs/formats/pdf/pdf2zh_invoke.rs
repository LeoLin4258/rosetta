use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

use tokio::sync::oneshot;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
    managed_pdf2zh::{self, openai_shim::ShimProviderConfig, openai_shim::ShimRwkvMetrics},
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
    /// filtered "pages to translate" list of this run. Derived live from
    /// pdf2zh's tqdm output. `None` when the caller didn't supply page
    /// context (whole-document fallback path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_page: Option<u32>,
    /// Total pages in the filtered list. Paired with `current_page` so the
    /// UI can render "第 X/Y 页".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u32>,
    /// Cumulative translated characters returned by RWKV during this
    /// invocation. Keeps the status bar visibly moving even while pdf2zh's
    /// own output is quiet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_chars: Option<u64>,
}

/// Page-progress context for one pdf2zh invocation. The caller filters out
/// already-translated pages before chunking, so these reflect user-visible
/// progress ("3rd of 5 pages I asked to translate"), not absolute numbers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageProgressContext {
    /// Pages already completed in earlier chunks of this run.
    pub completed_before: u32,
    /// Pages handed to this invocation (== the tqdm denominator).
    pub chunk_len: u32,
    /// Total pages the whole run will translate.
    pub total: u32,
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
    /// `None` for callers that translate the whole document in a single
    /// invocation (no page numbering to report).
    pub page_progress: Option<PageProgressContext>,
    /// Characters already translated by earlier chunks of this run. Added to
    /// this invocation's shim counter so the UI's 已翻译 counter is monotonic
    /// across chunks instead of resetting per invocation.
    pub translated_chars_offset: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhOutput {
    pub mono_pdf: PathBuf,
    #[allow(dead_code)]
    pub dual_pdf: Option<PathBuf>,
    /// Time from invocation start to pdf2zh process spawn (status resolution,
    /// shim startup, RWKV role setup).
    pub warmup_ms: u64,
    /// Wall time of the pdf2zh process itself (parse + translate + render).
    pub process_ms: u64,
    /// RWKV request stats collected by the shim during this invocation.
    pub rwkv_metrics: crate::managed_pdf2zh::openai_shim::ShimRwkvMetricsSnapshot,
}

pub(crate) async fn invoke_pdf2zh(
    app: &AppHandle,
    source_path: &Path,
    output_dir: &Path,
    options: Pdf2zhInvokeOptions,
    cancel_rx: oneshot::Receiver<()>,
) -> Result<Pdf2zhOutput, PdfError> {
    // Shared live-progress state for this invocation:
    // - `pages_done` is updated from pdf2zh's tqdm output (now delivered in
    //   real time because the stderr reader splits on `\r`),
    // - `last_percent` keeps the most recent tqdm percent so the chars ticker
    //   doesn't blank it,
    // - translated chars come from the shim metrics once the shim exists.
    let page_progress = options.page_progress;
    let pages_done = Arc::new(AtomicU32::new(0));
    let last_percent = Arc::new(AtomicU32::new(0)); // 0 = none, else percent+1
    let emit = |phase: &str, percent: Option<u8>, message: &str| {
        emit_progress(
            app,
            &options.job_id,
            phase,
            percent,
            message,
            page_progress,
            pages_done.load(Ordering::Relaxed),
            None,
        );
    };
    let invoke_started = std::time::Instant::now();

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
    let _ = options.ignore_cache;

    emit("translate", Some(0), "正在翻译 PDF…");
    let warmup_ms = invoke_started.elapsed().as_millis() as u64;
    let process_started = std::time::Instant::now();
    let mut cancel_rx = cancel_rx;

    // Heartbeat: push the cumulative translated-character count to the UI
    // every 500 ms while RWKV batches return, so the status bar visibly moves
    // even when pdf2zh's own output is between updates. Aborted on drop, so
    // every exit path (success, failure, cancel) stops it.
    let metrics = Arc::clone(&shim.metrics);
    let chars_offset = options.translated_chars_offset;
    let _chars_ticker = AbortOnDrop(tokio::spawn({
        let app = app.clone();
        let job_id = options.job_id.clone();
        let metrics = Arc::clone(&metrics);
        let pages_done = Arc::clone(&pages_done);
        let last_percent = Arc::clone(&last_percent);
        async move {
            let mut last_chars = 0u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let chars = chars_offset + metrics.snapshot().total_output_chars;
                if chars == last_chars {
                    continue;
                }
                last_chars = chars;
                let percent = match last_percent.load(Ordering::Relaxed) {
                    0 => None,
                    stored => Some((stored - 1).min(100) as u8),
                };
                emit_progress(
                    &app,
                    &job_id,
                    "translate",
                    percent,
                    "正在翻译…",
                    page_progress,
                    pages_done.load(Ordering::Relaxed),
                    Some(chars),
                );
            }
        }
    }));

    let output_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    // Live tee of pdf2zh's output to disk so we can see what it's doing even
    // when it hangs (in which case the failure-only `rosetta-pdf2zh-output.log`
    // never gets written). Opened best-effort; logging failures are swallowed.
    let live_log_path = output_dir.join("rosetta-pdf2zh-live.log");
    let live_log: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&live_log_path)
            .ok(),
    ));

    // Preferred path: the persistent worker, which keeps the ~13 s Python
    // import (torch + doclayout) warm across invocations. Falls back to the
    // one-shot CLI below when the worker can't be started (broken pack, no
    // bundled python, …).
    let worker_payload = serde_json::json!({
        "file": source_path.to_string_lossy(),
        "outputDir": output_dir.to_string_lossy(),
        "tmpDir": temp_dir.to_string_lossy(),
        "pages": options.pages.clone(),
        "langIn": pdf2zh_lang(&options.source_lang),
        "langOut": pdf2zh_lang(&options.target_lang),
        "service": "openai:rwkv",
        "thread": thread_count,
        "env": {
            "OPENAI_BASE_URL": openai_base_url.clone(),
            "OPENAI_API_KEY": "rosetta-local",
            "OPENAI_MODEL": "rwkv",
        },
    });
    let worker_outcome = {
        let mut on_stderr = |line: &str| {
            remember_line(&output_lines, line);
            append_live_log(&live_log, "worker", line);
            handle_pdf2zh_line(
                app,
                &options.job_id,
                line,
                page_progress,
                &pages_done,
                &last_percent,
                Some((&metrics, chars_offset)),
            );
        };
        crate::managed_pdf2zh::worker::translate_via_worker(
            app,
            worker_payload,
            &mut on_stderr,
            &mut cancel_rx,
        )
        .await
    };

    use crate::managed_pdf2zh::worker::WorkerTranslateOutcome;
    let mut worker_completed = false;
    match worker_outcome {
        WorkerTranslateOutcome::Completed => {
            worker_completed = true;
        }
        WorkerTranslateOutcome::Cancelled => {
            return Err(PdfError::Cancelled);
        }
        WorkerTranslateOutcome::JobFailed(message)
        | WorkerTranslateOutcome::WorkerLost(message) => {
            let tail = output_lines
                .lock()
                .ok()
                .map(|lines| lines.join("\n"))
                .unwrap_or_default();
            let output_log = output_dir.join("rosetta-pdf2zh-output.log");
            let _ = std::fs::write(&output_log, format!("{tail}\n--- worker error ---\n{message}"));
            return Err(PdfError::Pdf2zhFailed(format!(
                "PDF 版面处理没有完成。请重试；若持续失败，可查看日志：{}",
                output_log.display()
            )));
        }
        WorkerTranslateOutcome::Unavailable(reason) => {
            append_live_log(
                &live_log,
                "worker",
                &format!("worker unavailable, falling back to CLI: {reason}"),
            );
        }
    }

    if !worker_completed {
        // Fallback: one-shot CLI invocation (pays the full import per call).
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
        // Start pdf2zh in its own process group so cancellation can kill the
        // whole tree. pdf2zh is a Python launcher that spawns multiprocessing
        // workers; killing only the immediate child leaves those workers (and
        // their in-flight RWKV requests) running, which is why "stop" used to
        // appear to do nothing on large PDFs.
        #[cfg(unix)]
        command.process_group(0);
        if let Some(pages) = &options.pages {
            let pages_arg = pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            command.arg("--pages").arg(pages_arg);
        }

        let mut child = command.spawn().map_err(|error| {
            PdfError::Pdf2zhFailed(format!("启动 {} 失败: {error}", bin.display()))
        })?;

        let stderr = child.stderr.take();
        let stdout = child.stdout.take();
        let stderr_task = stderr.map(|stream| {
            let app = app.clone();
            let job_id = options.job_id.clone();
            let output_lines = Arc::clone(&output_lines);
            let live_log = Arc::clone(&live_log);
            let pages_done = Arc::clone(&pages_done);
            let last_percent = Arc::clone(&last_percent);
            let metrics = Arc::clone(&metrics);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    remember_line(&output_lines, &line);
                    append_live_log(&live_log, "stderr", &line);
                    handle_pdf2zh_line(
                        &app,
                        &job_id,
                        &line,
                        page_progress,
                        &pages_done,
                        &last_percent,
                        Some((&metrics, chars_offset)),
                    );
                }
            })
        });
        let stdout_task = stdout.map(|stream| {
            let app = app.clone();
            let job_id = options.job_id.clone();
            let output_lines = Arc::clone(&output_lines);
            let live_log = Arc::clone(&live_log);
            let pages_done = Arc::clone(&pages_done);
            let last_percent = Arc::clone(&last_percent);
            let metrics = Arc::clone(&metrics);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    remember_line(&output_lines, &line);
                    append_live_log(&live_log, "stdout", &line);
                    handle_pdf2zh_line(
                        &app,
                        &job_id,
                        &line,
                        page_progress,
                        &pages_done,
                        &last_percent,
                        Some((&metrics, chars_offset)),
                    );
                }
            })
        });

        let exit_status = tokio::select! {
            result = child.wait() => {
                result.map_err(|error| PdfError::Pdf2zhFailed(format!("等待 pdf2zh 结束失败: {error}")))?
            }
            _ = &mut cancel_rx => {
                crate::managed_pdf2zh::worker::kill_process_tree(&mut child).await;
                if let Some(task) = stderr_task { task.abort(); }
                if let Some(task) = stdout_task { task.abort(); }
                return Err(PdfError::Cancelled);
            }
        };
        if let Some(task) = stderr_task {
            let _ = task.await;
        }
        if let Some(task) = stdout_task {
            let _ = task.await;
        }

        if !exit_status.success() {
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
                exit_status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string()),
                output_log.display()
            )));
        }
    }

    let process_ms = process_started.elapsed().as_millis() as u64;
    let rwkv_metrics = shim.metrics.snapshot();
    drop(shim);

    emit("render", Some(95), "正在整理译文 PDF…");
    let mono_pdf = find_pdf2zh_output(output_dir, source_path, "zh")
        .or_else(|| find_pdf2zh_output(output_dir, source_path, "mono"))
        .ok_or_else(|| PdfError::Pdf2zhFailed("未生成译文 PDF。".to_string()))?;
    let dual_pdf = find_pdf2zh_output(output_dir, source_path, "dual");
    emit("render", Some(100), "译文 PDF 已生成。");
    Ok(Pdf2zhOutput {
        mono_pdf,
        dual_pdf,
        warmup_ms,
        process_ms,
        rwkv_metrics,
    })
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

/// Aborts a background task when dropped, so progress tickers can't outlive
/// the invocation on any exit path.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn handle_pdf2zh_line(
    app: &AppHandle,
    job_id: &str,
    line: &str,
    ctx: Option<PageProgressContext>,
    pages_done: &AtomicU32,
    last_percent: &AtomicU32,
    metrics: Option<(&ShimRwkvMetrics, u64)>,
) {
    // pdf2zh's tqdm bar iterates over the pages of this invocation; its
    // "done/total" fraction is the authoritative per-page progress. Only
    // trust fractions whose denominator matches the chunk size to avoid
    // picking up unrelated "a/b" tokens from log lines.
    if let (Some(ctx), Some((done, denominator))) = (ctx, parse_tqdm_fraction(line)) {
        if denominator == ctx.chunk_len {
            pages_done.store(done.min(ctx.chunk_len), Ordering::Relaxed);
        }
    }
    let percent = parse_percent(line);
    if let Some(percent) = percent {
        last_percent.store(u32::from(percent) + 1, Ordering::Relaxed);
    }

    let lower = line.to_ascii_lowercase();
    let phase = if lower.contains("parse") || lower.contains("layout") {
        "parse"
    } else if lower.contains("save") || lower.contains("render") {
        "render"
    } else {
        "translate"
    };
    let translated_chars = metrics
        .map(|(metrics, offset)| offset + metrics.snapshot().total_output_chars)
        .filter(|chars| *chars > 0);
    emit_progress(
        app,
        job_id,
        phase,
        percent,
        line.trim(),
        ctx,
        pages_done.load(Ordering::Relaxed),
        translated_chars,
    );
}

/// Extract tqdm's "done/total" fraction from a progress line like
/// ` 33%|███▎      | 6/18 [00:25<00:46,  3.9s/it]`. Requires the trailing
/// `[` so arbitrary "a/b" tokens (dates, paths, rates like `3.9s/it`) don't
/// match.
fn parse_tqdm_fraction(line: &str) -> Option<(u32, u32)> {
    let bytes = line.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte != b'/' {
            continue;
        }
        let mut start = index;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let mut end = index + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if start == index || end == index + 1 {
            continue;
        }
        if !line[end..].trim_start().starts_with('[') {
            continue;
        }
        if let (Ok(done), Ok(total)) = (
            line[start..index].parse::<u32>(),
            line[index + 1..end].parse::<u32>(),
        ) {
            if total > 0 && done <= total {
                return Some((done, total));
            }
        }
    }
    None
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

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    phase: &str,
    percent: Option<u8>,
    message: &str,
    ctx: Option<PageProgressContext>,
    pages_done_in_chunk: u32,
    translated_chars: Option<u64>,
) {
    let (current_page, total_pages) = match ctx {
        Some(ctx) => {
            let current_in_chunk = (pages_done_in_chunk + 1).min(ctx.chunk_len.max(1));
            (
                Some(ctx.completed_before + current_in_chunk),
                Some(ctx.total),
            )
        }
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
            translated_chars,
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

#[cfg(test)]
mod tests {
    use super::parse_tqdm_fraction;

    #[test]
    fn parses_tqdm_progress_fraction() {
        assert_eq!(
            parse_tqdm_fraction(" 33%|███▎      | 6/18 [00:25<00:46,  3.9s/it]"),
            Some((6, 18))
        );
        assert_eq!(
            parse_tqdm_fraction("100%|██████████| 10/10 [00:52<00:00,  5.24s/it]"),
            Some((10, 10))
        );
        assert_eq!(
            parse_tqdm_fraction("  0%|          | 0/10 [00:00<?, ?it/s]"),
            Some((0, 10))
        );
    }

    #[test]
    fn ignores_non_tqdm_slashes() {
        assert_eq!(parse_tqdm_fraction("saved to /tmp/out/file.pdf"), None);
        assert_eq!(parse_tqdm_fraction("date 2026/06/11 done"), None);
        assert_eq!(parse_tqdm_fraction("rate 3.9s/it without bracket"), None);
        assert_eq!(parse_tqdm_fraction(""), None);
    }
}
