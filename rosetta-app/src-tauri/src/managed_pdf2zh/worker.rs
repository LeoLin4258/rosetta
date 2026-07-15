//! Persistent pdf2zh worker process.
//!
//! Importing and preparing pdf2zh's layout stack is too expensive to pay for
//! every PDF job. This module keeps one warm Python worker per app session
//! and feeds it typed prepare/render/dispose jobs over a line-based JSON
//! protocol.
//!
//! The worker script is embedded in the app binary and written under the
//! sidecar root at spawn time, so already-installed packs get the worker
//! without re-downloading anything. The v2 product path fails clearly when
//! the worker or engine contract is unavailable.
//!
//! Cancellation kills the worker's whole process group; the next run pays one
//! re-import. There is no idle
//! reaper — the worker stays warm for the lifetime of the app process so the
//! header indicator can stay "已就绪" and translate clicks are always cheap.

use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex as StdMutex,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot, Mutex},
};

use super::build_static_status;
use crate::windows_process::HideConsole;

const WORKER_SCRIPT: &str = include_str!("rosetta_pdf2zh_worker.py");
/// First spawn includes Python imports and ONNX layout warmup; be generous.
const READY_TIMEOUT: Duration = Duration::from_secs(300);

fn sanitize_python_environment(command: &mut Command) {
    command.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    #[cfg(target_os = "linux")]
    command.env_remove("LD_LIBRARY_PATH");
}

#[cfg(test)]
mod tests {
    use super::{sanitize_python_environment, WORKER_SCRIPT};
    use tokio::process::Command;

    #[test]
    fn worker_clears_packaged_app_python_environment() {
        let mut command = Command::new("python");
        command
            .env("PYTHONHOME", "/tmp/app/usr")
            .env("PYTHONPATH", "/tmp/app/usr/lib/python3.12");
        #[cfg(target_os = "linux")]
        command.env("LD_LIBRARY_PATH", "/tmp/app/usr/lib");

        sanitize_python_environment(&mut command);

        let removed = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.is_none().then(|| key.to_string_lossy().into_owned()))
            .collect::<Vec<_>>();
        assert!(removed.iter().any(|key| key == "PYTHONHOME"));
        assert!(removed.iter().any(|key| key == "PYTHONPATH"));
        #[cfg(target_os = "linux")]
        assert!(removed.iter().any(|key| key == "LD_LIBRARY_PATH"));
    }

    #[test]
    fn persistent_worker_never_enters_job_output_directory() {
        assert!(
            !WORKER_SCRIPT.contains("os.chdir(output_dir)"),
            "a persistent Windows worker must not hold the per-job output directory as its cwd"
        );
    }

    #[test]
    fn worker_is_thin_rosetta_engine_protocol_host() {
        assert!(
            WORKER_SCRIPT.contains("from pdf2zh import rosetta_engine as engine"),
            "the warm worker must delegate PDF internals to the fork-owned Rosetta engine"
        );
        assert!(
            WORKER_SCRIPT.contains("prepare_pdf_window")
                && WORKER_SCRIPT.contains("render_pdf_window")
                && WORKER_SCRIPT.contains("dispose_pdf_window"),
            "the worker protocol should expose v2 prepare/render/dispose commands"
        );
        assert!(
            !WORKER_SCRIPT.contains("DeferredTranslationCollector")
                && !WORKER_SCRIPT.contains("PretranslatedTranslator")
                && !WORKER_SCRIPT.contains("OPENAI_BASE_URL")
                && !WORKER_SCRIPT.contains("ROSETTA_BATCH_BASE_URL"),
            "the v2 worker must not own replay translators or OpenAI/Rosetta shim wiring"
        );
    }

    #[test]
    fn worker_reports_engine_capabilities() {
        assert!(
            WORKER_SCRIPT.contains("\"capabilities\": capabilities"),
            "startup must report the PDF engine contract version and prewarm timings"
        );
    }

    #[test]
    fn worker_reuses_only_resettable_prepared_windows_and_reports_timings() {
        assert!(WORKER_SCRIPT.contains("callable(reset_run)"));
        assert!(WORKER_SCRIPT.contains("reset_run(active[\"preparedRunId\"])"));
        assert!(WORKER_SCRIPT.contains("\"cacheHit\": cache_hit"));
        assert!(WORKER_SCRIPT.contains("\"timingsMs\": timings"));
        assert!(WORKER_SCRIPT.contains("dispose_active_prepare_cache(engine)"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stale_worker_match_uses_pdf2zh_sidecar_signature() {
        let root = std::path::PathBuf::from(
            r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar",
        );
        let pack = root.join("pack").join("windows-amd64");
        let process = super::WindowsProcess {
            pid: std::process::id() + 1,
            command: format!(
                r#""{}\python\python.exe" {}\worker\rosetta_pdf2zh_worker.py"#,
                pack.display(),
                root.display()
            ),
        };

        assert!(super::is_matching_pdf2zh_worker(&process, &root, &pack));

        let unrelated = super::WindowsProcess {
            pid: std::process::id() + 2,
            command: r#""C:\Python311\python.exe" script.py"#.to_string(),
        };
        assert!(!super::is_matching_pdf2zh_worker(&unrelated, &root, &pack));
    }
}

/// Status broadcast to the frontend so the header can show a live "PDF 引擎"
/// indicator. Updated by [`set_worker_status`] which both stores the latest
/// snapshot in [`WorkerState::status`] and emits a Tauri event so every
/// window sees the change immediately.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhWorkerStatus {
    /// One of: "idle" (never started this session), "starting" (importing
    /// pdf2zh + layout warmup in flight), "ready" (warm, idle, accepting jobs),
    /// "translating" (a job is running), "failed" (last spawn errored),
    /// "not-installed" (pdf2zh pack missing — the indicator will hide
    /// itself in this state).
    pub state: String,
    pub message: Option<String>,
    /// Import wall time on the last successful spawn, surfaced for the
    /// status tooltip ("预热耗时 X.X s").
    #[serde(rename = "importMs")]
    pub import_ms: Option<u64>,
    /// 1-based phase within the warmup handshake, populated only while
    /// `state == "starting"`. Drives the "[N/M label]" detail string the
    /// frontend renders so a 30 s+ first-launch warm-up doesn't sit on a
    /// single static label.
    #[serde(rename = "warmupStep")]
    pub warmup_step: Option<u32>,
    #[serde(rename = "warmupTotalSteps")]
    pub warmup_total_steps: Option<u32>,
    #[serde(rename = "warmupLabel")]
    pub warmup_label: Option<String>,
}

impl Default for Pdf2zhWorkerStatus {
    fn default() -> Self {
        Self {
            state: "idle".to_string(),
            message: None,
            import_ms: None,
            warmup_step: None,
            warmup_total_steps: None,
            warmup_label: None,
        }
    }
}

const WORKER_STATUS_EVENT: &str = "rosetta-pdf2zh-worker-status";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfTranslationUnit {
    pub unit_id: String,
    pub page_number: u32,
    pub order_on_page: u32,
    pub source_text: String,
    pub source_chars: u64,
    pub kind: String,
    pub requires_translation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPageResult {
    pub page_number: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    pub source_unit_count: u32,
    pub translated_unit_count: u32,
    pub source_chars: u64,
    pub translated_chars: u64,
    pub empty_translation_count: u32,
    pub placeholder_mismatch_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedPdfRun {
    pub prepared_run_id: String,
    pub source_page_count: u32,
    pub pages: Vec<u32>,
    pub unit_count: u32,
    pub source_chars: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedPdfWindow {
    pub prepared_run: PreparedPdfRun,
    pub units: Vec<PdfTranslationUnit>,
    pub cache_hit: bool,
    pub timings_ms: PdfPrepareTimings,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPrepareTimings {
    pub total: u64,
    pub font_assets: u64,
    pub prepare_document: u64,
    pub layout: u64,
    pub unit_collection: u64,
    pub other: u64,
    pub cache_reset: u64,
}

pub struct WorkerState {
    inner: Mutex<Option<WorkerProcess>>,
    status: StdMutex<Pdf2zhWorkerStatus>,
    shutdown_requested: AtomicBool,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
            status: StdMutex::new(Pdf2zhWorkerStatus::default()),
            shutdown_requested: AtomicBool::new(false),
        }
    }
}

impl WorkerState {
    pub fn status_snapshot(&self) -> Pdf2zhWorkerStatus {
        self.status
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
    }

    fn should_shutdown(&self) -> bool {
        self.shutdown_requested.load(Ordering::SeqCst)
    }
}

/// Update the public worker status and broadcast it to every window. Called
/// from spawn / handshake / kill paths so the header indicator tracks the
/// real worker lifecycle without polling. Warmup fields default to None;
/// use [`set_warmup_progress`] to populate them while `starting`.
fn set_worker_status(
    app: &AppHandle,
    state: &str,
    message: Option<String>,
    import_ms: Option<u64>,
) {
    let next = Pdf2zhWorkerStatus {
        state: state.to_string(),
        message,
        import_ms,
        warmup_step: None,
        warmup_total_steps: None,
        warmup_label: None,
    };
    if let Some(worker_state) = app.try_state::<WorkerState>() {
        if let Ok(mut guard) = worker_state.status.lock() {
            *guard = next.clone();
        }
    }
    let _ = app.emit(WORKER_STATUS_EVENT, next);
}

/// Push a "starting" status update carrying the current warmup phase so the
/// header / topbar can render "[N/M label]" while the PDF worker is warming. State
/// stays "starting" until the worker emits its terminal `ready`/`fatal`.
fn set_warmup_progress(app: &AppHandle, step: u32, total: u32, label: String) {
    let next = Pdf2zhWorkerStatus {
        state: "starting".to_string(),
        message: None,
        import_ms: None,
        warmup_step: Some(step),
        warmup_total_steps: Some(total),
        warmup_label: Some(label),
    };
    if let Some(worker_state) = app.try_state::<WorkerState>() {
        if let Ok(mut guard) = worker_state.status.lock() {
            *guard = next.clone();
        }
    }
    let _ = app.emit(WORKER_STATUS_EVENT, next);
}

#[derive(Debug)]
pub enum WorkerTranslateOutcome {
    Completed,
    Cancelled,
    /// The job failed but the worker is still healthy (translator error,
    /// bad input, …).
    JobFailed(String),
    /// The worker process died mid-job.
    WorkerLost(String),
    /// No worker could be started (pack missing / old layout). The v2 product
    /// path reports this clearly instead of falling back to a CLI path.
    Unavailable(String),
}

#[derive(Debug, Deserialize)]
struct WorkerEvent {
    #[serde(default)]
    id: Option<String>,
    event: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default, rename = "importMs")]
    import_ms: Option<u64>,
    #[serde(default)]
    mps: Option<bool>,
    #[serde(default, rename = "mpsReason")]
    mps_reason: Option<String>,
    #[serde(default, rename = "yoloWarmupMs")]
    yolo_warmup_ms: Option<u64>,
    #[serde(default, rename = "yoloWarmupStatus")]
    yolo_warmup_status: Option<String>,
    #[serde(default, rename = "yoloWarmupDevice")]
    yolo_warmup_device: Option<String>,
    #[serde(default, rename = "yoloWarmupReason")]
    yolo_warmup_reason: Option<String>,
    #[serde(default)]
    capabilities: Option<serde_json::Value>,
    #[serde(default, rename = "preparedRun")]
    prepared_run: Option<PreparedPdfRun>,
    #[serde(default)]
    units: Option<Vec<PdfTranslationUnit>>,
    #[serde(default, rename = "cacheHit")]
    cache_hit: Option<bool>,
    #[serde(default, rename = "timingsMs")]
    timings_ms: Option<PdfPrepareTimings>,
    #[serde(default, rename = "pageResult")]
    page_result: Option<PdfPageResult>,
    /// 1-based phase index on `warming` events emitted during the import
    /// handshake. Paired with `total_steps` and `label`.
    #[serde(default)]
    step: Option<u32>,
    #[serde(default, rename = "totalSteps")]
    total_steps: Option<u32>,
    #[serde(default)]
    label: Option<String>,
}

pub struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::UnboundedReceiver<WorkerEvent>,
    stderr_lines: mpsc::UnboundedReceiver<String>,
    stderr_open: bool,
    next_job: u64,
}

/// Kill a process and all of its descendants. On unix the child must have
/// been started with `process_group(0)`; signalling `-pgid` reaches Python
/// multiprocessing workers / translation threads too. SIGTERM first so
/// workers can exit cleanly, then SIGKILL the group.
pub(crate) async fn kill_process_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::killpg(pid as i32, libc::SIGTERM);
        }
        let graceful = tokio::time::timeout(Duration::from_millis(1500), child.wait()).await;
        if graceful.is_err() {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        let _ = child.wait().await;
        return;
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .hide_console_on_windows()
            .status()
            .await;
        let _ = child.wait().await;
        return;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn spawn_worker(app: &AppHandle) -> Result<WorkerProcess, String> {
    let status = build_static_status(app)?;
    if !status.install_plan.ready {
        set_worker_status(
            app,
            "not-installed",
            Some(status.install_plan.message.clone()),
            None,
        );
        return Err(status.install_plan.message);
    }
    let doclayout_model = status
        .doclayout_model_path
        .clone()
        .ok_or_else(|| "pdf2zh pack 缺少内置 ONNX 版面模型，请更新 PDF 组件。".to_string())?;
    let python = status.layout.python_path(status.profile);
    if !python.is_file() {
        let msg = format!("pdf2zh pack 中找不到 Python 解释器: {}", python.display());
        set_worker_status(app, "failed", Some(msg.clone()), None);
        return Err(msg);
    }
    set_worker_status(app, "starting", None, None);
    status.layout.ensure_dirs()?;
    let worker_dir = status.layout.root_dir.join("worker");
    std::fs::create_dir_all(&worker_dir)
        .map_err(|error| format!("无法创建 worker 目录: {error}"))?;
    let script_path = worker_dir.join("rosetta_pdf2zh_worker.py");
    std::fs::write(&script_path, WORKER_SCRIPT)
        .map_err(|error| format!("无法写入 worker 脚本: {error}"))?;

    let mut command = Command::new(&python);
    sanitize_python_environment(&mut command);
    command
        .arg(&script_path)
        .current_dir(&worker_dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONNOUSERSITE", "1")
        .env("ROSETTA_DOCLAYOUT_MODEL", &doclayout_model)
        .env(
            "ROSETTA_BABELDOC_CACHE_DIR",
            status.layout.babeldoc_cache_dir(),
        )
        // Same loopback-proxy scrubbing as the CLI invocation: the shim is on
        // 127.0.0.1 and user proxies (Clash/Surge) can't reach it.
        .env("NO_PROXY", "127.0.0.1,localhost,::1")
        .env("no_proxy", "127.0.0.1,localhost,::1")
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("ALL_PROXY", "")
        .env("http_proxy", "")
        .env("https_proxy", "")
        .env("all_proxy", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    command.kill_on_drop(true);
    command.hide_console_on_windows();

    eprintln!("[pdf2zh-worker] === spawn ===");
    eprintln!("[pdf2zh-worker]   python:  {}", python.display());
    eprintln!("[pdf2zh-worker]   script:  {}", script_path.display());
    eprintln!("[pdf2zh-worker]   cwd:     {}", worker_dir.display());
    eprintln!("[pdf2zh-worker]   model:   {}", doclayout_model.display());
    eprintln!(
        "[pdf2zh-worker]   assets:  {}",
        status.layout.babeldoc_cache_dir().display()
    );

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 pdf2zh worker 失败: {error}"))?;
    eprintln!("[pdf2zh-worker]   pid:     {}", child.id().unwrap_or(0));

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "pdf2zh worker stdin 不可用。".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "pdf2zh worker stdout 不可用。".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "pdf2zh worker stderr 不可用。".to_string())?;

    let (events_tx, events) = mpsc::unbounded_channel::<WorkerEvent>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<WorkerEvent>(&line) {
                Ok(event) => {
                    if events_tx.send(event).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    eprintln!("[pdf2zh-worker:stdout] invalid protocol line ({error}): {line}")
                }
            }
        }
    });

    // Split stderr on BOTH `\n` and `\r`: pdf2zh's tqdm progress bar redraws
    // with carriage returns and never newlines until it finishes, so a plain
    // line reader would deliver the whole bar only at the end — which is
    // exactly the "looks frozen" symptom. CR-splitting turns every redraw
    // into a live progress line.
    let (stderr_tx, stderr_lines) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = BufReader::new(stderr);
        let mut pending: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    for &byte in &chunk[..read] {
                        if byte == b'\n' || byte == b'\r' {
                            if !pending.is_empty() {
                                let line = String::from_utf8_lossy(&pending).into_owned();
                                pending.clear();
                                if stderr_tx.send(line).is_err() {
                                    return;
                                }
                            }
                        } else {
                            pending.push(byte);
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            let _ = stderr_tx.send(String::from_utf8_lossy(&pending).into_owned());
        }
    });

    let mut worker = WorkerProcess {
        child,
        stdin,
        events,
        stderr_lines,
        stderr_open: true,
        next_job: 0,
    };

    // Handshake: wait for the worker to finish its heavy imports.
    // Drain stderr alongside stdout so that if the process crashes during
    // import (e.g. DLL load failure, missing module), we capture the real
    // error message instead of the opaque "worker 在就绪前退出".
    //
    // `stderr_capture` is shared via Arc so that it survives a timeout —
    // when READY_TIMEOUT fires the inner future is dropped but we can still
    // read the captured lines and include them in the error message.
    let stderr_capture = std::sync::Arc::new(StdMutex::new(Vec::<String>::new()));
    let capture_ref = stderr_capture.clone();
    let ready = tokio::time::timeout(READY_TIMEOUT, async {
        loop {
            tokio::select! {
                event = worker.events.recv() => {
                    let Some(event) = event else {
                        let detail = format_stderr_tail(&capture_ref);
                        let exit = match worker.child.try_wait() {
                            Ok(Some(status)) => format!(" exit status: {status}."),
                            Ok(None) => " stdout channel closed while process is still running.".to_string(),
                            Err(error) => format!(" unable to read exit status: {error}."),
                        };
                        return Err(format!("worker 在就绪前退出。{exit}{detail}"));
                    };
                    match event.event.as_str() {
                        "warming" => {
                            if let (Some(step), Some(total), Some(label)) =
                                (event.step, event.total_steps, event.label)
                            {
                                set_warmup_progress(app, step, total, label);
                            }
                        }
                        "ready" => {
                            let contract_version = event
                                .capabilities
                                .as_ref()
                                .and_then(|value| value.get("contractVersion"))
                                .and_then(|value| value.as_u64())
                                .unwrap_or(0);
                            if contract_version != 2 {
                                return Err(
                                    "PDF 组件不支持 Rosetta PDF engine contract v2，请更新 PDF 组件。"
                                        .to_string(),
                                );
                            }
                            let import_ms = event.import_ms.unwrap_or(0);
                            let yolo_warmup_ms = event
                                .yolo_warmup_ms
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".to_string());
                            eprintln!(
                                "[pdf2zh-worker] ready (import {} ms, mps={}, reason={}, layoutWarmupStatus={}, layoutWarmupMs={}, layoutWarmupDevice={}, layoutWarmupReason={})",
                                import_ms,
                                event.mps.unwrap_or(false),
                                event.mps_reason.as_deref().unwrap_or("-"),
                                event.yolo_warmup_status.as_deref().unwrap_or("-"),
                                yolo_warmup_ms,
                                event.yolo_warmup_device.as_deref().unwrap_or("-"),
                                event.yolo_warmup_reason.as_deref().unwrap_or("-")
                            );
                            return Ok(import_ms);
                        }
                        "fatal" => {
                            let detail = format_stderr_tail(&capture_ref);
                            return Err(format!(
                                "{}{}",
                                event.message.as_deref().unwrap_or("worker 启动失败。"),
                                detail
                            ));
                        }
                        _ => {}
                    }
                }
                line = worker.stderr_lines.recv(), if worker.stderr_open => {
                    match line {
                        Some(text) => {
                            eprintln!("[pdf2zh-worker:stderr] {text}");
                            if let Ok(mut cap) = capture_ref.lock() {
                                cap.push(text);
                            }
                        }
                        None => worker.stderr_open = false,
                    }
                }
            }
        }
    })
    .await;

    match ready {
        Ok(Ok(import_ms)) => {
            set_worker_status(app, "ready", None, Some(import_ms));
            Ok(worker)
        }
        Ok(Err(message)) => {
            kill_process_tree(&mut worker.child).await;
            let msg = format!("pdf2zh worker 启动失败: {message}");
            eprintln!("[pdf2zh-worker] {msg}");
            set_worker_status(app, "failed", Some(msg.clone()), None);
            Err(msg)
        }
        Err(_) => {
            kill_process_tree(&mut worker.child).await;
            let detail = format_stderr_tail(&stderr_capture);
            let msg = format!(
                "pdf2zh worker 启动超时 ({} 秒)。{detail}",
                READY_TIMEOUT.as_secs()
            );
            eprintln!("[pdf2zh-worker] {msg}");
            set_worker_status(app, "failed", Some(msg.clone()), None);
            Err(msg)
        }
    }
}

fn format_stderr_tail(capture: &StdMutex<Vec<String>>) -> String {
    let lines = match capture.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return String::new(),
    };
    if lines.is_empty() {
        return String::new();
    }
    let tail: Vec<&str> = lines
        .iter()
        .rev()
        .take(30)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|s| s.as_str())
        .collect();
    format!("\n--- stderr ---\n{}", tail.join("\n"))
}

impl WorkerProcess {
    async fn run_prepare_pdf_window(
        &mut self,
        mut payload: serde_json::Value,
        on_stderr: &mut (dyn FnMut(&str) + Send),
        cancel_rx: &mut oneshot::Receiver<()>,
    ) -> Result<PreparedPdfWindow, WorkerTranslateOutcome> {
        self.next_job += 1;
        let job_id = format!("wjob-{}", self.next_job);
        payload["id"] = json!(job_id);
        payload["cmd"] = json!("prepare_pdf_window");
        let mut line = payload.to_string();
        line.push('\n');

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| {
                WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"))
            })?;
        self.stdin.flush().await.map_err(|error| {
            WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"))
        })?;

        loop {
            tokio::select! {
                event = self.events.recv() => {
                    let Some(event) = event else {
                        return Err(WorkerTranslateOutcome::WorkerLost(
                            "worker 进程意外退出。".to_string(),
                        ));
                    };
                    match event.event.as_str() {
                        "prepared_pdf_window" if event.id.as_deref() == Some(job_id.as_str()) => {
                            let prepared_run = event.prepared_run.ok_or_else(|| {
                                WorkerTranslateOutcome::JobFailed(
                                    "PDF worker prepared event missing preparedRun.".to_string(),
                                )
                            })?;
                            let units = event.units.unwrap_or_default();
                            return Ok(PreparedPdfWindow {
                                prepared_run,
                                units,
                                cache_hit: event.cache_hit.unwrap_or(false),
                                timings_ms: event.timings_ms.unwrap_or_default(),
                            });
                        }
                        "stage" if event.id.as_deref() == Some(job_id.as_str()) => {}
                        "error" if event.id.as_deref() == Some(job_id.as_str()) || event.id.is_none() => {
                            return Err(WorkerTranslateOutcome::JobFailed(
                                event.message.unwrap_or_else(|| "未知 worker 错误".to_string()),
                            ));
                        }
                        "fatal" => {
                            return Err(WorkerTranslateOutcome::WorkerLost(
                                event.message.unwrap_or_else(|| "worker 致命错误".to_string()),
                            ));
                        }
                        _ => {}
                    }
                }
                stderr_line = self.stderr_lines.recv(), if self.stderr_open => {
                    match stderr_line {
                        Some(text) => on_stderr(&text),
                        None => self.stderr_open = false,
                    }
                }
                _ = &mut *cancel_rx => {
                    kill_process_tree(&mut self.child).await;
                    return Err(WorkerTranslateOutcome::Cancelled);
                }
            }
        }
    }

    async fn run_render_pdf_window(
        &mut self,
        mut payload: serde_json::Value,
        on_stderr: &mut (dyn FnMut(&str) + Send),
        on_page_result: &mut (dyn FnMut(PdfPageResult) + Send),
        cancel_rx: &mut oneshot::Receiver<()>,
    ) -> WorkerTranslateOutcome {
        self.next_job += 1;
        let job_id = format!("wjob-{}", self.next_job);
        payload["id"] = json!(job_id);
        payload["cmd"] = json!("render_pdf_window");
        let mut line = payload.to_string();
        line.push('\n');

        if let Err(error) = self.stdin.write_all(line.as_bytes()).await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }
        if let Err(error) = self.stdin.flush().await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }

        loop {
            tokio::select! {
                event = self.events.recv() => {
                    let Some(event) = event else {
                        return WorkerTranslateOutcome::WorkerLost(
                            "worker 进程意外退出。".to_string(),
                        );
                    };
                    match event.event.as_str() {
                        "page_result" if event.id.as_deref() == Some(job_id.as_str()) => {
                            if let Some(page_result) = event.page_result {
                                on_page_result(page_result);
                            }
                        }
                        "done" if event.id.as_deref() == Some(job_id.as_str()) => {
                            return WorkerTranslateOutcome::Completed;
                        }
                        "error" if event.id.as_deref() == Some(job_id.as_str()) || event.id.is_none() => {
                            return WorkerTranslateOutcome::JobFailed(
                                event.message.unwrap_or_else(|| "未知 worker 错误".to_string()),
                            );
                        }
                        "fatal" => {
                            return WorkerTranslateOutcome::WorkerLost(
                                event.message.unwrap_or_else(|| "worker 致命错误".to_string()),
                            );
                        }
                        _ => {}
                    }
                }
                stderr_line = self.stderr_lines.recv(), if self.stderr_open => {
                    match stderr_line {
                        Some(text) => on_stderr(&text),
                        None => self.stderr_open = false,
                    }
                }
                _ = &mut *cancel_rx => {
                    kill_process_tree(&mut self.child).await;
                    return WorkerTranslateOutcome::Cancelled;
                }
            }
        }
    }

    async fn run_dispose_pdf_window(&mut self, prepared_run_id: &str) -> WorkerTranslateOutcome {
        self.next_job += 1;
        let job_id = format!("wjob-{}", self.next_job);
        let mut line = json!({
            "id": job_id,
            "cmd": "dispose_pdf_window",
            "preparedRunId": prepared_run_id,
        })
        .to_string();
        line.push('\n');

        if let Err(error) = self.stdin.write_all(line.as_bytes()).await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }
        if let Err(error) = self.stdin.flush().await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }

        loop {
            match self.events.recv().await {
                Some(event)
                    if event.id.as_deref() == Some(job_id.as_str())
                        && event.event == "disposed_pdf_window" =>
                {
                    return WorkerTranslateOutcome::Completed;
                }
                Some(event) if event.event == "fatal" => {
                    return WorkerTranslateOutcome::WorkerLost(
                        event
                            .message
                            .unwrap_or_else(|| "worker 致命错误".to_string()),
                    );
                }
                Some(_) => {}
                None => {
                    return WorkerTranslateOutcome::WorkerLost("worker 进程意外退出。".to_string());
                }
            }
        }
    }
}

pub(crate) async fn prepare_pdf_window(
    app: &AppHandle,
    payload: serde_json::Value,
    on_stderr: &mut (dyn FnMut(&str) + Send),
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<PreparedPdfWindow, WorkerTranslateOutcome> {
    let state = app.state::<WorkerState>();
    let mut guard = state.inner.lock().await;
    if guard.is_none() {
        match spawn_worker(app).await {
            Ok(worker) => *guard = Some(worker),
            Err(message) => return Err(WorkerTranslateOutcome::Unavailable(message)),
        }
    }
    let worker = guard.as_mut().expect("worker present after ensure");
    set_worker_status(app, "translating", None, None);
    let outcome = worker
        .run_prepare_pdf_window(payload, on_stderr, cancel_rx)
        .await;
    match &outcome {
        Ok(_) | Err(WorkerTranslateOutcome::JobFailed(_)) => {
            set_worker_status(app, "ready", None, None);
        }
        Err(WorkerTranslateOutcome::Cancelled) | Err(WorkerTranslateOutcome::WorkerLost(_)) => {
            if let Some(mut dead) = guard.take() {
                kill_process_tree(&mut dead.child).await;
            }
            drop(guard);
            set_worker_status(app, "idle", None, None);
            if !state.should_shutdown() {
                let app_clone = app.clone();
                tokio::spawn(async move {
                    let _ = prewarm_worker(&app_clone).await;
                });
            }
        }
        Err(WorkerTranslateOutcome::Unavailable(_)) => {}
        Err(WorkerTranslateOutcome::Completed) => {}
    }
    outcome
}

pub(crate) async fn render_pdf_window(
    app: &AppHandle,
    payload: serde_json::Value,
    on_stderr: &mut (dyn FnMut(&str) + Send),
    on_page_result: &mut (dyn FnMut(PdfPageResult) + Send),
    cancel_rx: &mut oneshot::Receiver<()>,
) -> WorkerTranslateOutcome {
    let state = app.state::<WorkerState>();
    let mut guard = state.inner.lock().await;
    if guard.is_none() {
        match spawn_worker(app).await {
            Ok(worker) => *guard = Some(worker),
            Err(message) => return WorkerTranslateOutcome::Unavailable(message),
        }
    }
    let worker = guard.as_mut().expect("worker present after ensure");
    set_worker_status(app, "translating", None, None);
    let outcome = worker
        .run_render_pdf_window(payload, on_stderr, on_page_result, cancel_rx)
        .await;

    if matches!(
        outcome,
        WorkerTranslateOutcome::Cancelled | WorkerTranslateOutcome::WorkerLost(_)
    ) {
        if let Some(mut dead) = guard.take() {
            kill_process_tree(&mut dead.child).await;
        }
        drop(guard);
        set_worker_status(app, "idle", None, None);
        if !state.should_shutdown() {
            let app_clone = app.clone();
            tokio::spawn(async move {
                let _ = prewarm_worker(&app_clone).await;
            });
        }
    } else {
        set_worker_status(app, "ready", None, None);
    }
    outcome
}

pub(crate) async fn dispose_pdf_window(app: &AppHandle, prepared_run_id: &str) {
    let state = app.state::<WorkerState>();
    let mut guard = state.inner.lock().await;
    let Some(worker) = guard.as_mut() else {
        return;
    };
    let outcome = worker.run_dispose_pdf_window(prepared_run_id).await;
    if matches!(outcome, WorkerTranslateOutcome::WorkerLost(_)) {
        if let Some(mut dead) = guard.take() {
            kill_process_tree(&mut dead.child).await;
        }
        set_worker_status(app, "idle", None, None);
    }
}

/// Start (or confirm) the warm worker without running a job. Called once at
/// app startup so the ~13 s import is paid before the user has a chance to
/// click translate, and re-called whenever a kill (cancel / process loss)
/// has left the slot empty so the header indicator returns to "已就绪".
pub(crate) async fn prewarm_worker(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<WorkerState>();
    if state.should_shutdown() {
        return Ok(false);
    }

    let mut guard = state.inner.lock().await;
    if state.should_shutdown() {
        return Ok(false);
    }
    if guard.is_some() {
        // Already warm — make sure the broadcast status reflects it (the
        // frontend may have just connected and missed the original "ready"
        // event).
        set_worker_status(app, "ready", None, None);
        return Ok(true);
    }
    let worker = spawn_worker(app).await?;
    *guard = Some(worker);
    Ok(true)
}

pub(crate) async fn shutdown_worker(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<WorkerState>() else {
        return false;
    };
    state.request_shutdown();

    let mut guard = state.inner.lock().await;
    let Some(mut worker) = guard.take() else {
        return false;
    };

    kill_process_tree(&mut worker.child).await;
    set_worker_status(app, "idle", None, None);
    true
}

pub(crate) async fn stop_worker_for_install(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<WorkerState>() else {
        return false;
    };

    let mut guard = state.inner.lock().await;
    let Some(mut worker) = guard.take() else {
        return false;
    };

    kill_process_tree(&mut worker.child).await;
    set_worker_status(app, "idle", None, None);
    true
}

pub(crate) async fn cleanup_stale_workers_for_install(
    root_dir: &Path,
    pack_dir: &Path,
) -> Result<usize, String> {
    #[cfg(target_os = "windows")]
    {
        cleanup_stale_workers_windows(root_dir, pack_dir).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (root_dir, pack_dir);
        Ok(0)
    }
}

#[cfg(target_os = "windows")]
async fn cleanup_stale_workers_windows(root_dir: &Path, pack_dir: &Path) -> Result<usize, String> {
    let processes = list_windows_processes().await?;
    let stale = processes
        .into_iter()
        .filter(|process| is_matching_pdf2zh_worker(process, root_dir, pack_dir))
        .collect::<Vec<_>>();

    if stale.is_empty() {
        return Ok(0);
    }

    for process in &stale {
        terminate_windows_process_tree(process.pid).await?;
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    eprintln!(
        "[pdf2zh-worker] cleaned {} stale worker process(es) before install",
        stale.len()
    );
    Ok(stale.len())
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WindowsProcess {
    pid: u32,
    command: String,
}

#[cfg(target_os = "windows")]
async fn list_windows_processes() -> Result<Vec<WindowsProcess>, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process | Select-Object ProcessId,CommandLine,ExecutablePath | ConvertTo-Json -Compress",
        ])
        .hide_console_on_windows()
        .output()
        .await
        .map_err(|error| format!("无法列出 Windows 进程: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "PowerShell 进程查询失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct WinProcess {
        process_id: u32,
        command_line: Option<String>,
        executable_path: Option<String>,
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|error| format!("解析进程列表失败: {error}"))?;
    let processes: Vec<WinProcess> = if value.is_array() {
        serde_json::from_value(value).map_err(|error| format!("解析进程列表失败: {error}"))?
    } else {
        vec![serde_json::from_value(value).map_err(|error| format!("解析进程列表失败: {error}"))?]
    };

    Ok(processes
        .into_iter()
        .map(|process| WindowsProcess {
            pid: process.process_id,
            command: format!(
                "{} {}",
                process.executable_path.unwrap_or_default(),
                process.command_line.unwrap_or_default()
            ),
        })
        .collect())
}

#[cfg(target_os = "windows")]
fn is_matching_pdf2zh_worker(process: &WindowsProcess, root_dir: &Path, pack_dir: &Path) -> bool {
    if process.pid == std::process::id() {
        return false;
    }

    let command = process.command.to_lowercase();
    let root = root_dir.display().to_string().to_lowercase();
    let pack = pack_dir.display().to_string().to_lowercase();
    command.contains("rosetta_pdf2zh_worker.py")
        && (command.contains(&root) || command.contains(&pack))
}

#[cfg(target_os = "windows")]
async fn terminate_windows_process_tree(pid: u32) -> Result<(), String> {
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .hide_console_on_windows()
        .output()
        .await
        .map_err(|error| format!("无法停止旧 PDF worker 进程 {pid}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("not found") || stderr.contains("没有找到") {
        return Ok(());
    }
    Err(format!(
        "停止旧 PDF worker 进程 {pid} 失败: taskkill 返回 {} ({})",
        output.status,
        stderr.trim()
    ))
}
