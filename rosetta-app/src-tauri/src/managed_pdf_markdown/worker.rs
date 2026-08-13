use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Mutex as StdMutex,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

use super::{
    layout::{canonical_is_within, PdfMarkdownLayout},
    profile::{
        current_profile, PROTOCOL_VERSION, PYMUPDF4LLM_VERSION, PYMUPDF_LAYOUT_VERSION,
        PYMUPDF_VERSION,
    },
};
use crate::{managed_pdf2zh::layout::locate_managed_python_host, windows_process::HideConsole};

const WORKER_SCRIPT: &str = include_str!("rosetta_pdf_markdown_worker.py");
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const READY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownWorkerStatus {
    pub state: String,
    pub protocol: u32,
    pub last_error_code: Option<String>,
}

impl Default for PdfMarkdownWorkerStatus {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            protocol: PROTOCOL_VERSION,
            last_error_code: None,
        }
    }
}

#[derive(Default)]
pub struct PdfMarkdownWorkerState {
    process: Mutex<Option<WorkerProcess>>,
    status: StdMutex<PdfMarkdownWorkerStatus>,
    pid: AtomicU32,
    stopping: AtomicBool,
}

impl PdfMarkdownWorkerState {
    pub fn status_snapshot(&self) -> PdfMarkdownWorkerStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| PdfMarkdownWorkerStatus {
                state: "failed".into(),
                protocol: PROTOCOL_VERSION,
                last_error_code: Some("status-lock".into()),
            })
    }
    fn set_status(&self, state: &str, code: Option<&str>) {
        if let Ok(mut status) = self.status.lock() {
            status.state = state.into();
            status.last_error_code = code.map(str::to_string);
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(dead_code)] // ExtractWindow becomes live at the extraction-store checkpoint.
enum WorkerRequest<'a> {
    Hello,
    ExtractWindow {
        id: &'a str,
        #[serde(rename = "sourcePath")]
        source_path: &'a Path,
        pages: &'a [u32],
        #[serde(rename = "tempDir")]
        temp_dir: &'a Path,
    },
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(dead_code)] // Progress/result fields are consumed by Checkpoint 3.
pub enum WorkerEvent {
    Ready {
        protocol: u32,
        versions: std::collections::HashMap<String, String>,
        providers: Vec<String>,
        integration_boundary: String,
        cpu_only: bool,
    },
    WindowProgress {
        id: String,
        completed: u32,
        total: u32,
    },
    WindowResult {
        id: String,
        pages: Vec<serde_json::Value>,
    },
    Error {
        code: String,
        message: String,
    },
    Shutdown,
}

#[allow(dead_code)]
struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

fn sanitize_python_environment(command: &mut Command) {
    for key in [
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "PYTHONUSERBASE",
        "VIRTUAL_ENV",
        "CONDA_PREFIX",
        "CUDA_VISIBLE_DEVICES",
    ] {
        command.env_remove(key);
    }
    #[cfg(target_os = "linux")]
    command.env_remove("LD_LIBRARY_PATH");
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_file_name(format!(".worker-{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes).map_err(|_| "unable to stage PDF Markdown worker".to_string())?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&temp)
        .and_then(|file| file.sync_all())
        .map_err(|_| "unable to flush PDF Markdown worker".to_string())?;
    let backup = path.with_file_name(".rosetta_pdf_markdown_worker.py.previous");
    let _ = std::fs::remove_file(&backup);
    if path.exists() {
        std::fs::rename(path, &backup)
            .map_err(|_| "unable to stage PDF Markdown worker replacement".to_string())?;
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, path);
        }
        return Err(format!("unable to commit PDF Markdown worker: {error}"));
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

async fn read_bounded_line(reader: &mut BufReader<ChildStdout>) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| "worker protocol read failed".to_string())?;
        if available.is_empty() {
            return Err("worker protocol closed".into());
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if output.len().saturating_add(take) > MAX_RESPONSE_BYTES + 1 {
            return Err("worker-response-too-large".into());
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if output.last() == Some(&b'\n') {
            output.pop();
            if output.len() > MAX_RESPONSE_BYTES {
                return Err("worker-response-too-large".into());
            }
            return Ok(output);
        }
    }
}

async fn send_request(
    process: &mut WorkerProcess,
    request: &WorkerRequest<'_>,
) -> Result<(), String> {
    let mut bytes =
        serde_json::to_vec(request).map_err(|_| "unable to encode worker request".to_string())?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err("worker-request-too-large".into());
    }
    bytes.push(b'\n');
    process
        .stdin
        .write_all(&bytes)
        .await
        .map_err(|_| "worker protocol write failed".to_string())?;
    process
        .stdin
        .flush()
        .await
        .map_err(|_| "worker protocol write failed".to_string())
}

async fn next_event(process: &mut WorkerProcess) -> Result<WorkerEvent, String> {
    let line = read_bounded_line(&mut process.stdout).await?;
    serde_json::from_slice(&line).map_err(|_| "worker-protocol-invalid-json".to_string())
}

async fn discard_process(state: &PdfMarkdownWorkerState, process: &mut Option<WorkerProcess>) {
    state.pid.store(0, Ordering::SeqCst);
    if let Some(mut process) = process.take() {
        kill_process_group(&mut process.child).await;
    }
}

fn validate_ready(event: &WorkerEvent) -> Result<(), String> {
    if let WorkerEvent::Error { code, .. } = event {
        return Err(code.clone());
    }
    let WorkerEvent::Ready {
        protocol,
        versions,
        providers,
        integration_boundary,
        cpu_only,
    } = event
    else {
        return Err("worker-version-preflight-failed".into());
    };
    if *protocol != PROTOCOL_VERSION
        || !cpu_only
        || integration_boundary != "to_json"
        || providers.as_slice() != ["CPUExecutionProvider"]
        || versions.get("pymupdf4llm").map(String::as_str) != Some(PYMUPDF4LLM_VERSION)
        || versions.get("pymupdf-layout").map(String::as_str) != Some(PYMUPDF_LAYOUT_VERSION)
        || versions.get("PyMuPDF").map(String::as_str) != Some(PYMUPDF_VERSION)
    {
        return Err("worker-version-preflight-failed".into());
    }
    Ok(())
}

fn retryable_start_error(error: &str) -> bool {
    matches!(
        error,
        "worker protocol closed"
            | "worker protocol read failed"
            | "worker-protocol-invalid-json"
            | "worker-version-preflight-failed"
    )
}

fn extraction_read_error(error: String, stopping: bool) -> String {
    if stopping && error == "worker protocol closed" {
        "worker-stopping".into()
    } else {
        error
    }
}

async fn spawn_worker(
    app: &AppHandle,
    state: &PdfMarkdownWorkerState,
) -> Result<WorkerProcess, String> {
    let profile = current_profile().ok_or_else(|| "unsupported-platform".to_string())?;
    let layout = PdfMarkdownLayout::from_app(app, profile)?;
    layout.validate_install(profile)?;
    let python = locate_managed_python_host(app)?;
    layout.ensure_dirs()?;
    let script = layout.worker_dir.join("rosetta_pdf_markdown_worker.py");
    atomic_write(&script, WORKER_SCRIPT.as_bytes())?;
    let jobs_root = app
        .path()
        .app_data_dir()
        .map_err(|_| "unable to locate job storage".to_string())?
        .join("jobs");
    std::fs::create_dir_all(&jobs_root).map_err(|_| "unable to prepare job storage".to_string())?;
    let mut command = Command::new(python);
    sanitize_python_environment(&mut command);
    command
        .arg(script)
        .current_dir(&layout.worker_dir)
        .env("PYTHONPATH", &layout.component_dir)
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("CUDA_VISIBLE_DEVICES", "")
        .env("ROSETTA_PDF_MARKDOWN_JOBS_ROOT", jobs_root)
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("ALL_PROXY", "")
        .env("NO_PROXY", "*")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .hide_console_on_windows();
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| "worker-spawn-failed".to_string())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "worker-stdin-unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "worker-stdout-unavailable".to_string())?;
    let mut process = WorkerProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 0,
    };
    state
        .pid
        .store(process.child.id().unwrap_or(0), Ordering::SeqCst);
    let ready = async {
        send_request(&mut process, &WorkerRequest::Hello).await?;
        let event = tokio::time::timeout(READY_TIMEOUT, next_event(&mut process))
            .await
            .map_err(|_| "worker-ready-timeout".to_string())??;
        validate_ready(&event)
    }
    .await;
    if let Err(error) = ready {
        kill_process_group(&mut process.child).await;
        state.pid.store(0, Ordering::SeqCst);
        return Err(error);
    }
    if state.stopping.load(Ordering::SeqCst) {
        kill_process_group(&mut process.child).await;
        state.pid.store(0, Ordering::SeqCst);
        return Err("worker-stopping".into());
    }
    Ok(process)
}

pub async fn prewarm(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<PdfMarkdownWorkerState>();
    let mut guard = state.process.lock().await;
    if state.stopping.load(Ordering::SeqCst) {
        return Err("worker-stopping".into());
    }
    if let Some(process) = guard.as_mut() {
        if process
            .child
            .try_wait()
            .map_err(|_| "worker-status-failed".to_string())?
            .is_some()
        {
            *guard = None;
            state.pid.store(0, Ordering::SeqCst);
        }
    }
    if guard.is_none() {
        state.set_status("starting", None);
        let first = spawn_worker(app, &state).await;
        match first {
            Ok(process) => *guard = Some(process),
            Err(error) if retryable_start_error(&error) => {
                tokio::time::sleep(Duration::from_millis(150)).await;
                match spawn_worker(app, &state).await {
                    Ok(process) => *guard = Some(process),
                    Err(retry_error) => {
                        state.set_status("failed", Some(&retry_error));
                        return Err(retry_error);
                    }
                }
            }
            Err(error) => {
                state.set_status("failed", Some(&error));
                return Err(error);
            }
        }
    }
    state.set_status("ready", None);
    Ok(true)
}

#[allow(dead_code)] // Internal integration seam for Checkpoint 3, never a Tauri command.
pub async fn extract_window(
    app: &AppHandle,
    source: &Path,
    pages: &[u32],
    temp_dir: &Path,
) -> Result<Vec<serde_json::Value>, String> {
    if pages.is_empty() || pages.len() > 10 || pages.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("invalid-page-window".into());
    }
    let jobs_root = app
        .path()
        .app_data_dir()
        .map_err(|_| "unable to locate job storage".to_string())?
        .join("jobs");
    if source.file_name().and_then(|s| s.to_str()) != Some("source.pdf")
        || !canonical_is_within(source, &jobs_root)
        || !canonical_is_within(temp_dir, &jobs_root)
        || !temp_dir.is_dir()
    {
        return Err("invalid-worker-path".into());
    }
    prewarm(app).await?;
    let state = app.state::<PdfMarkdownWorkerState>();
    let mut guard = state.process.lock().await;
    let id = {
        let process = guard
            .as_mut()
            .ok_or_else(|| "worker-unavailable".to_string())?;
        process.next_id += 1;
        format!("window-{}", process.next_id)
    };
    let request = WorkerRequest::ExtractWindow {
        id: &id,
        source_path: source,
        pages,
        temp_dir,
    };
    let send_result = send_request(
        guard
            .as_mut()
            .ok_or_else(|| "worker-unavailable".to_string())?,
        &request,
    )
    .await;
    if let Err(error) = send_result {
        discard_process(&state, &mut guard).await;
        state.set_status("failed", Some(&error));
        return Err(error);
    }
    state.set_status("extracting", None);
    loop {
        let event = next_event(
            guard
                .as_mut()
                .ok_or_else(|| "worker-unavailable".to_string())?,
        )
        .await;
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                let error = extraction_read_error(error, state.stopping.load(Ordering::SeqCst));
                discard_process(&state, &mut guard).await;
                state.set_status("failed", Some(&error));
                return Err(error);
            }
        };
        match event {
            WorkerEvent::WindowProgress { id: event_id, .. } if event_id == id => {}
            WorkerEvent::WindowResult {
                id: event_id,
                pages,
            } if event_id == id => {
                state.set_status("ready", None);
                return Ok(pages);
            }
            WorkerEvent::Error { code, .. } => {
                state.set_status("ready", Some(&code));
                return Err(code);
            }
            _ => {
                let error = "worker-protocol-unexpected-event";
                discard_process(&state, &mut guard).await;
                state.set_status("failed", Some(error));
                return Err(error.into());
            }
        }
    }
}

pub async fn shutdown(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<PdfMarkdownWorkerState>() else {
        return false;
    };
    state.stopping.store(true, Ordering::SeqCst);
    let pid = state.pid.swap(0, Ordering::SeqCst);
    if pid != 0 {
        terminate_process_group(pid).await;
    }
    let mut guard = state.process.lock().await;
    if let Some(mut process) = guard.take() {
        kill_process_group(&mut process.child).await;
    }
    state.pid.store(0, Ordering::SeqCst);
    state.set_status("idle", None);
    state.stopping.store(false, Ordering::SeqCst);
    pid != 0
}

pub async fn cancel(app: &AppHandle) -> bool {
    shutdown(app).await
}

async fn terminate_process_group(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::killpg(pid as i32, libc::SIGTERM);
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        unsafe {
            libc::killpg(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .hide_console_on_windows()
            .status()
            .await;
    }
}

async fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::killpg(pid as i32, libc::SIGTERM);
        }
        if tokio::time::timeout(Duration::from_millis(1500), child.wait())
            .await
            .is_err()
        {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        let _ = child.wait().await;
        return;
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .hide_console_on_windows()
            .status()
            .await;
        let _ = child.wait().await;
        return;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_protocol_has_hard_bounds_and_pinned_call() {
        assert!(WORKER_SCRIPT.contains("MAX_REQUEST_BYTES = 64 * 1024"));
        assert!(WORKER_SCRIPT.contains("MAX_RESPONSE_BYTES = 64 * 1024 * 1024"));
        assert!(WORKER_SCRIPT.contains("engine.to_json("));
        assert!(!WORKER_SCRIPT.contains("to_markdown("));
        assert!(WORKER_SCRIPT.contains("use_ocr=False"));
        assert!(WORKER_SCRIPT.contains("force_text=False"));
        assert!(WORKER_SCRIPT.contains("write_images=True"));
        assert!(WORKER_SCRIPT.contains("redirect_stdout"));
        assert!(WORKER_SCRIPT.contains("os.dup(sys.stdout.fileno())"));
        assert!(WORKER_SCRIPT.contains("os.dup2(NULL_OUTPUT, sys.stdout.fileno())"));
    }

    #[test]
    fn only_transient_start_errors_are_retried() {
        assert!(retryable_start_error("worker protocol closed"));
        assert!(retryable_start_error("worker-protocol-invalid-json"));
        assert!(!retryable_start_error("version-mismatch"));
        assert!(!retryable_start_error("non-cpu-provider"));
    }

    #[test]
    fn protocol_close_is_cancellation_only_while_worker_is_stopping() {
        assert_eq!(
            extraction_read_error("worker protocol closed".into(), true),
            "worker-stopping"
        );
        assert_eq!(
            extraction_read_error("worker protocol closed".into(), false),
            "worker protocol closed"
        );
        assert_eq!(
            extraction_read_error("worker protocol read failed".into(), true),
            "worker protocol read failed"
        );
    }

    #[test]
    fn worker_preflight_preserves_explicit_worker_error() {
        let event = WorkerEvent::Error {
            code: "version-mismatch".into(),
            message: "failed".into(),
        };
        assert_eq!(validate_ready(&event), Err("version-mismatch".into()));
    }

    #[test]
    fn python_environment_is_sanitized_before_overlay_is_added() {
        let mut command = Command::new("python");
        command
            .env("PYTHONPATH", "unsafe")
            .env("PYTHONHOME", "unsafe")
            .env("CUDA_VISIBLE_DEVICES", "0");
        sanitize_python_environment(&mut command);
        let removed = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.is_none().then(|| key.to_string_lossy().into_owned()))
            .collect::<Vec<_>>();
        assert!(removed.iter().any(|key| key == "PYTHONPATH"));
        assert!(removed.iter().any(|key| key == "PYTHONHOME"));
        assert!(removed.iter().any(|key| key == "CUDA_VISIBLE_DEVICES"));
    }

    #[test]
    fn public_worker_status_contains_no_document_details() {
        let value = serde_json::to_string(&PdfMarkdownWorkerStatus::default()).unwrap();
        assert!(!value.contains("path"));
        assert!(!value.contains("text"));
    }

    #[test]
    fn worker_event_schema_rejects_unknown_and_cross_type_fields() {
        let ready = serde_json::from_str::<WorkerEvent>(
            r#"{"type":"ready","protocol":1,"versions":{"pymupdf4llm":"1.28.0","pymupdf-layout":"1.28.0","PyMuPDF":"1.28.0"},"providers":["CPUExecutionProvider"],"integrationBoundary":"to_json","cpuOnly":true}"#,
        )
        .unwrap();
        assert!(matches!(
            ready,
            WorkerEvent::Ready {
                integration_boundary,
                cpu_only: true,
                ..
            } if integration_boundary == "to_json"
        ));
        assert!(serde_json::from_str::<WorkerEvent>(
            r#"{"type":"error","code":"x","message":"failed","pages":[]}"#
        )
        .is_err());
        assert!(serde_json::from_str::<WorkerEvent>(
            r#"{"type":"windowResult","id":"x","pages":[],"completed":1}"#
        )
        .is_err());
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn process_tree_cancellation_terminates_worker_root() {
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 60"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .hide_console_on_windows()
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        terminate_process_group(pid).await;
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_tree_cancellation_terminates_worker_root() {
        let mut child = Command::new("sh");
        child
            .args(["-c", "sleep 60"])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = child.spawn().unwrap();
        let pid = child.id().unwrap();
        terminate_process_group(pid).await;
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
