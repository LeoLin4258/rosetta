//! Persistent pdf2zh worker process.
//!
//! Importing pdf2zh's layout stack (torch + torchvision + opencv via
//! doclayout_yolo) costs ~13 s per process while the model load itself is
//! ~0.07 s. The one-shot CLI invocation paid that import for every chunk of
//! pages; this module keeps one warm Python worker per app session and feeds
//! it translate jobs over a line-based JSON protocol.
//!
//! The worker script is embedded in the app binary and written under the
//! sidecar root at spawn time, so already-installed packs get the worker
//! without re-downloading anything. Callers fall back to the one-shot CLI
//! when the worker can't be started.
//!
//! Cancellation kills the worker's whole process group (translation threads
//! and any descendants); the next run pays one re-import. There is no idle
//! reaper — the worker stays warm for the lifetime of the app process so the
//! header indicator can stay "已就绪" and translate clicks are always cheap.
//! At ~600 MB resident torch memory that's a deliberate trade.

use std::{path::PathBuf, process::Stdio, sync::Mutex as StdMutex, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot, Mutex},
};

use super::build_static_status;

const WORKER_SCRIPT: &str = include_str!("rosetta_pdf2zh_worker.py");
/// First spawn includes the ~13 s torch import plus, on a fresh machine, the
/// layout-model download — be generous.
const READY_TIMEOUT: Duration = Duration::from_secs(300);

/// Status broadcast to the frontend so the header can show a live "PDF 引擎"
/// indicator. Updated by [`set_worker_status`] which both stores the latest
/// snapshot in [`WorkerState::status`] and emits a Tauri event so every
/// window sees the change immediately.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhWorkerStatus {
    /// One of: "idle" (never started this session), "starting" (importing
    /// torch + handshake in flight), "ready" (warm, idle, accepting jobs),
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

pub struct WorkerState {
    inner: Mutex<Option<WorkerProcess>>,
    status: StdMutex<Pdf2zhWorkerStatus>,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
            status: StdMutex::new(Pdf2zhWorkerStatus::default()),
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
/// header / topbar can render "[N/M label]" while torch is loading. State
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
    /// No worker could be started (pack missing / old layout); caller should
    /// fall back to the one-shot CLI.
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
    /// 1-based absolute page number announced after a single page finishes
    /// translating. Paired with `file` (the single-page translated PDF
    /// written by the worker).
    #[serde(default, rename = "pageNumber")]
    page_number: Option<u32>,
    #[serde(default)]
    file: Option<String>,
    /// 1-based phase index on `warming` events emitted during the import
    /// handshake. Paired with `total_steps` and `label`.
    #[serde(default)]
    step: Option<u32>,
    #[serde(default, rename = "totalSteps")]
    total_steps: Option<u32>,
    #[serde(default)]
    label: Option<String>,
    /// Per-stage timings attached to the final `done` event (preprocessMs,
    /// translateMs, yoloMs, processPageMs, perPageSaveMs, …). Surfaced via
    /// stderr for the diagnostics log; not parsed structurally.
    #[serde(default)]
    timings: Option<serde_json::Value>,
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
        .ok_or_else(|| "pdf2zh pack 缺少内置 DocLayout-YOLO 模型，请更新 PDF 组件。".to_string())?;
    let python = status
        .layout
        .pack_dir
        .join("python")
        .join("bin")
        .join("python");
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
    command
        .arg(&script_path)
        .current_dir(&worker_dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("ROSETTA_DOCLAYOUT_MODEL", &doclayout_model)
        // Probe-gated MPS use in the worker; unsupported ops fall back to CPU
        // instead of erroring. Must be set before torch is imported.
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        // doclayout_yolo's ultralytics layer wants a writable config dir.
        .env("YOLO_CONFIG_DIR", &worker_dir)
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

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 pdf2zh worker 失败: {error}"))?;

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
            if let Ok(event) = serde_json::from_str::<WorkerEvent>(&line) {
                if events_tx.send(event).is_err() {
                    break;
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
    let ready = tokio::time::timeout(READY_TIMEOUT, async {
        while let Some(event) = worker.events.recv().await {
            match event.event.as_str() {
                "warming" => {
                    if let (Some(step), Some(total), Some(label)) =
                        (event.step, event.total_steps, event.label)
                    {
                        set_warmup_progress(app, step, total, label);
                    }
                }
                "ready" => {
                    let import_ms = event.import_ms.unwrap_or(0);
                    eprintln!(
                        "[pdf2zh-worker] ready (import {} ms, mps={}, reason={})",
                        import_ms,
                        event.mps.unwrap_or(false),
                        event.mps_reason.as_deref().unwrap_or("-")
                    );
                    return Ok(import_ms);
                }
                "fatal" => {
                    return Err(event
                        .message
                        .unwrap_or_else(|| "worker 启动失败。".to_string()));
                }
                _ => {}
            }
        }
        Err("worker 在就绪前退出。".to_string())
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
            set_worker_status(app, "failed", Some(msg.clone()), None);
            Err(msg)
        }
        Err(_) => {
            kill_process_tree(&mut worker.child).await;
            let msg = "pdf2zh worker 启动超时。".to_string();
            set_worker_status(app, "failed", Some(msg.clone()), None);
            Err(msg)
        }
    }
}

impl WorkerProcess {
    async fn run_translate(
        &mut self,
        mut payload: serde_json::Value,
        on_stderr: &mut (dyn FnMut(&str) + Send),
        on_page: Option<&mut (dyn FnMut(u32, PathBuf) + Send)>,
        cancel_rx: &mut oneshot::Receiver<()>,
    ) -> WorkerTranslateOutcome {
        self.next_job += 1;
        let job_id = format!("wjob-{}", self.next_job);
        payload["id"] = json!(job_id);
        payload["cmd"] = json!("translate");
        let mut line = payload.to_string();
        line.push('\n');

        if let Err(error) = self.stdin.write_all(line.as_bytes()).await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }
        if let Err(error) = self.stdin.flush().await {
            return WorkerTranslateOutcome::WorkerLost(format!("写入 worker 任务失败: {error}"));
        }

        let mut on_page = on_page;
        loop {
            tokio::select! {
                event = self.events.recv() => {
                    let Some(event) = event else {
                        return WorkerTranslateOutcome::WorkerLost(
                            "worker 进程意外退出。".to_string(),
                        );
                    };
                    match event.event.as_str() {
                        "page" if event.id.as_deref() == Some(job_id.as_str()) => {
                            if let (Some(cb), Some(page), Some(file)) =
                                (on_page.as_deref_mut(), event.page_number, event.file)
                            {
                                cb(page, PathBuf::from(file));
                            }
                        }
                        "done" if event.id.as_deref() == Some(job_id.as_str()) => {
                            if let Some(timings) = event.timings {
                                on_stderr(&format!("[pdf2zh-worker] timings {timings}"));
                            }
                            return WorkerTranslateOutcome::Completed;
                        }
                        "error" => {
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
}

/// Run one translate job on the shared worker, spawning it first if needed.
/// Serializes jobs via the state mutex (one PDF run is active at a time
/// anyway). Returns `Unavailable` when no worker can be started, so the
/// caller can fall back to the one-shot CLI.
pub(crate) async fn translate_via_worker(
    app: &AppHandle,
    payload: serde_json::Value,
    on_stderr: &mut (dyn FnMut(&str) + Send),
    on_page: Option<&mut (dyn FnMut(u32, PathBuf) + Send)>,
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
        .run_translate(payload, on_stderr, on_page, cancel_rx)
        .await;

    if matches!(
        outcome,
        WorkerTranslateOutcome::Cancelled | WorkerTranslateOutcome::WorkerLost(_)
    ) {
        if let Some(mut dead) = guard.take() {
            kill_process_tree(&mut dead.child).await;
        }
        // Worker is gone — drop guard before respawning so the lock is
        // available to the next caller, then trigger an async respawn so the
        // header indicator bounces back to "已就绪" without waiting for the
        // next translate click.
        drop(guard);
        set_worker_status(app, "idle", None, None);
        let app_clone = app.clone();
        tokio::spawn(async move {
            let _ = prewarm_worker(&app_clone).await;
        });
    } else {
        // Healthy worker stays warm.
        set_worker_status(app, "ready", None, None);
    }
    outcome
}

/// Start (or confirm) the warm worker without running a job. Called once at
/// app startup so the ~13 s import is paid before the user has a chance to
/// click translate, and re-called whenever a kill (cancel / process loss)
/// has left the slot empty so the header indicator returns to "已就绪".
pub(crate) async fn prewarm_worker(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<WorkerState>();

    let mut guard = state.inner.lock().await;
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

    let mut guard = state.inner.lock().await;
    let Some(mut worker) = guard.take() else {
        return false;
    };

    kill_process_tree(&mut worker.child).await;
    set_worker_status(app, "idle", None, None);
    true
}
