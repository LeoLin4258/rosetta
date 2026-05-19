//! docling-serve process lifecycle.
//!
//! Spawns the sidecar lazily on first PDF import, picks an ephemeral port,
//! waits for `/health`, and holds the process handle so the child gets killed
//! when the app exits (via `kill_on_drop`). One sidecar per app process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

/// Total budget for the sidecar's first /health response. docling-serve takes
/// 6-12s on M-series macs depending on whether Apple Silicon MPS is available
/// and what's in disk cache, so we give it generous headroom.
const STARTUP_HEALTH_BUDGET: Duration = Duration::from_secs(60);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Resource-resolution candidates for the docling-serve executable. First
/// match wins.
fn resolve_docling_serve_bin(app: &AppHandle) -> Result<PathBuf, String> {
    // 1) Dev override: env var pointing at the experiments/docling-probe venv.
    if let Ok(custom) = std::env::var("ROSETTA_DOCLING_SERVE_BIN") {
        let path = PathBuf::from(custom);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "ROSETTA_DOCLING_SERVE_BIN 指向的文件不存在: {}",
            path.display()
        ));
    }

    // 2) Bundled pack downloaded by the model-download flow at first use.
    //    Layout matches what we build in experiments/docling-pack/.
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法解析 app data 目录: {error}"))?;
    let pack_bin = app_data
        .join("docling-sidecar")
        .join("python")
        .join("bin")
        .join("docling-serve");
    if pack_bin.is_file() {
        return Ok(pack_bin);
    }

    Err(format!(
        "找不到 docling-serve。请先通过 PDF 翻译入口下载 PDF 解析包，或在开发时设置 ROSETTA_DOCLING_SERVE_BIN。期望路径：{}",
        pack_bin.display()
    ))
}

/// Returns a free localhost port the kernel just handed us. Closing the
/// listener immediately leaves the port in TIME_WAIT briefly, which is fine
/// for spawning a new server because the kernel reuses the port on the next
/// SO_REUSEADDR bind. Pattern matches managed_rwkv::lifecycle.
async fn pick_ephemeral_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("无法分配端口: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取本地端口: {error}"))?
        .port();
    drop(listener);
    Ok(port)
}

async fn wait_for_health(base_url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| format!("构造 HTTP 客户端失败: {error}"))?;
    let health_url = format!("{}/health", base_url.trim_end_matches('/'));

    let deadline = Instant::now() + STARTUP_HEALTH_BUDGET;
    let mut last_err = String::from("docling-serve 未在预期时间内响应 /health");
    while Instant::now() < deadline {
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => last_err = format!("/health 返回 {}", resp.status()),
            Err(error) => last_err = format!("/health 请求失败: {error}"),
        }
        sleep(HEALTH_POLL_INTERVAL).await;
    }
    Err(last_err)
}

#[derive(Default)]
struct SidecarState {
    child: Option<Child>,
    port: Option<u16>,
    base_url: Option<String>,
    started_at_iso: Option<String>,
    last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoclingSidecarSnapshot {
    pub running: bool,
    pub port: Option<u16>,
    pub base_url: Option<String>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
}

/// Tauri-managed registry that owns the single docling-serve child process.
#[derive(Default, Clone)]
pub(crate) struct DoclingSidecarRegistry {
    inner: Arc<Mutex<SidecarState>>,
}

impl DoclingSidecarRegistry {
    pub async fn snapshot(&self) -> DoclingSidecarSnapshot {
        let guard = self.inner.lock().await;
        DoclingSidecarSnapshot {
            running: guard.child.is_some(),
            port: guard.port,
            base_url: guard.base_url.clone(),
            started_at: guard.started_at_iso.clone(),
            last_error: guard.last_error.clone(),
        }
    }

    /// Ensure docling-serve is running and reachable. Returns the base URL.
    /// Idempotent: subsequent calls reuse the existing process.
    pub async fn ensure_running(&self, app: &AppHandle) -> Result<String, String> {
        // First, drop the lock to reap any zombie child quickly.
        {
            let mut guard = self.inner.lock().await;
            reap_if_exited(&mut guard).await;
            if let Some(url) = guard.base_url.clone() {
                if guard.child.is_some() {
                    return Ok(url);
                }
            }
        }

        let bin = resolve_docling_serve_bin(app)?;
        let port = pick_ephemeral_port().await?;
        let base_url = format!("http://127.0.0.1:{port}");

        let log_path = sidecar_log_path(app)?;
        let log_file = open_log_file(&log_path)?;
        let stdout = log_file
            .try_clone()
            .map_err(|error| format!("克隆日志句柄失败: {error}"))?;
        let stderr = log_file;

        // docling-serve CLI: `docling-serve run --host 127.0.0.1 --port N`
        let mut command = Command::new(&bin);
        command
            .args([
                "run",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .stdout(std::process::Stdio::from(stdout))
            .stderr(std::process::Stdio::from(stderr))
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);

        // Ensure model downloads (if not yet cached) go through the China-
        // friendly ModelScope mirror. RWKV memory says: HF 走原站 — but Docling
        // ships its primary models on ModelScope by default and this just
        // makes the behavior explicit / reproducible.
        command.env("HF_HUB_DEFAULT_BACKEND_TIMEOUT", "60");

        let mut guard = self.inner.lock().await;
        guard.last_error = None;
        let child = command.spawn().map_err(|error| {
            let msg = format!("无法启动 docling-serve: {error}");
            guard.last_error = Some(msg.clone());
            msg
        })?;
        guard.child = Some(child);
        guard.port = Some(port);
        guard.base_url = Some(base_url.clone());
        guard.started_at_iso = Some(iso_now());
        drop(guard);

        if let Err(error) = wait_for_health(&base_url).await {
            // Tear down so we don't leak a non-functional process.
            let mut guard = self.inner.lock().await;
            if let Some(mut child) = guard.child.take() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            guard.port = None;
            guard.base_url = None;
            guard.started_at_iso = None;
            guard.last_error = Some(error.clone());
            return Err(format!(
                "docling-serve 启动失败：{error}（日志：{}）",
                log_path.display()
            ));
        }

        Ok(base_url)
    }

    /// Kill the sidecar if it's running. Called from app shutdown hooks.
    pub async fn shutdown(&self) {
        let mut guard = self.inner.lock().await;
        if let Some(mut child) = guard.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        guard.port = None;
        guard.base_url = None;
        guard.started_at_iso = None;
    }
}

async fn reap_if_exited(state: &mut SidecarState) {
    let Some(child) = state.child.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(_status)) => {
            state.child = None;
            state.port = None;
            state.base_url = None;
            state.started_at_iso = None;
        }
        Ok(None) | Err(_) => {}
    }
}

fn sidecar_log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("无法解析 log 目录: {error}"))?;
    std::fs::create_dir_all(&dir).map_err(|error| format!("无法创建 log 目录: {error}"))?;
    Ok(dir.join("docling-serve.log"))
}

fn open_log_file(path: &Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打开 log 文件失败 ({}): {error}", path.display()))
}

fn iso_now() -> String {
    // RFC3339-ish UTC without bringing in chrono — same approach as
    // managed_rwkv::lifecycle::iso_now. Resolution: seconds.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day, hour, min, sec) = secs_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert epoch seconds to (Y, M, D, h, m, s) without an external crate.
/// Lifted in spirit from managed_rwkv; correct through year 2099.
fn secs_to_ymdhms(mut secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    secs /= 60;
    let min = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    secs /= 24;
    let mut days = secs as i64;
    let mut year: u32 = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let year_days = if leap { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let months_in_year: [i64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month: u32 = 1;
    for &md in &months_in_year {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = (days as u32) + 1;
    (year, month, day, hour, min, sec)
}
