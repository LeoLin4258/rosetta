//! Sidecar lifecycle: port allocation, spawn, health-wait, stop, probe.
//!
//! The shared state lives in a Tauri-managed registry so multiple commands
//! can see the same in-flight child. The registry is `tokio::sync::Mutex`
//! because the start command holds it across `.await` while polling the
//! sidecar's `/health` endpoint.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::process::{Child, Command as TokioCommand};
use tokio::sync::Mutex;

use super::profile::RuntimeProfile;
use super::status::{ManagedRuntimeProcessSnapshot, ManagedRuntimeState};

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
const HEALTH_INITIAL_DELAY: Duration = Duration::from_millis(150);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const LOG_TAIL_BYTES: u64 = 8 * 1024;
const STALE_SIDECAR_TERM_WAIT: Duration = Duration::from_millis(800);

/// Registry-shared lifecycle state. Wrapped in a `Mutex` so start/stop/probe
/// can serialize, since spawning has a brief `await` while we wait for the
/// first `/health` to clear.
#[derive(Default)]
pub struct ManagedRwkvRuntimeRegistry {
    inner: Arc<Mutex<RuntimeInner>>,
}

#[derive(Default)]
struct RuntimeInner {
    child: Option<Child>,
    port: Option<u16>,
    base_url: Option<String>,
    pid: Option<u32>,
    started_at_iso: Option<String>,
    last_error: Option<String>,
    state: Option<ManagedRuntimeState>,
}

// `ManagedRwkvRuntimeRegistry` is exposed via Tauri's `State` plumbing; no
// methods needed — start / stop / probe / snapshot are free functions that
// take `&ManagedRwkvRuntimeRegistry` so call sites read uniformly.

/// Outcome of a successful `start`. Returned to the frontend so it can show
/// the active port / pid immediately without a follow-up status call.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeStartResult {
    pub pid: u32,
    pub port: u16,
    pub base_url: String,
    pub started_at: String,
    pub command: Vec<String>,
    pub message: String,
}

/// Outcome of a `probe` call.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeProbeResult {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u128,
    pub base_url: Option<String>,
    pub message: String,
}

/// Snapshot suitable for the status command. Reads (does not mutate) the
/// registry; before returning it also reaps any zombie child that has
/// exited since the last call so the state stays accurate.
pub async fn current_process_snapshot(
    registry: &ManagedRwkvRuntimeRegistry,
) -> (ManagedRuntimeProcessSnapshot, Option<ManagedRuntimeState>) {
    let mut guard = registry.inner.lock().await;
    reap_exited_child(&mut guard);

    let snapshot = ManagedRuntimeProcessSnapshot {
        pid: guard.pid,
        port: guard.port,
        base_url: guard.base_url.clone(),
        started_at: guard.started_at_iso.clone(),
        last_error: guard.last_error.clone(),
    };
    (snapshot, guard.state)
}

pub async fn start_sidecar(
    registry: &ManagedRwkvRuntimeRegistry,
    profile: &RuntimeProfile,
    sidecar_path: PathBuf,
    tokenizer_path: PathBuf,
    model_path: PathBuf,
    log_file: PathBuf,
    metallib_source: Option<PathBuf>,
) -> Result<ManagedRuntimeStartResult, String> {
    let mut guard = registry.inner.lock().await;
    reap_exited_child(&mut guard);

    if guard.child.is_some() {
        return Err("本地 RWKV 运行时已在运行；如需重启请先停止。".to_string());
    }

    // Sanity: required artifacts present. Lifecycle never lies — if any of
    // these is missing the user should have seen Install Plan say so.
    for (label, path) in [
        ("sidecar", sidecar_path.as_path()),
        ("tokenizer", tokenizer_path.as_path()),
        ("model", model_path.as_path()),
    ] {
        if !path.exists() {
            let msg = format!("{label} 文件不存在: {}", path.display());
            guard.last_error = Some(msg.clone());
            guard.state = Some(ManagedRuntimeState::Failed);
            return Err(msg);
        }
    }

    // MLX backend setup: the rwkv-mobile MLX backend mmaps `default.metallib`
    // from the process's working directory at startup. In dev `default.metallib`
    // is staged into `src-tauri/binaries/` next to the sidecar by
    // `fetch-rwkv-sidecar.sh`; in the bundle we ship it as a resource and
    // need to make sure a copy lives next to the binary (or that we cwd into
    // the right place) before spawn. If `metallib_source` is provided and the
    // sidecar's parent dir doesn't already contain a `default.metallib`,
    // best-effort copy one in. On signed `.app` installs the bundle dir may be
    // read-only — in that case we silently fall back to cwd'ing into the
    // source directory so MLX still finds the metallib relative to cwd.
    let sidecar_dir = sidecar_path
        .parent()
        .ok_or_else(|| "sidecar 路径没有父目录。".to_string())?
        .to_path_buf();
    let mut working_dir = sidecar_dir.clone();
    if profile.backend == "mlx" {
        let target = sidecar_dir.join("default.metallib");
        let need_copy = !target.is_file();
        if need_copy {
            if let Some(src) = metallib_source.as_deref() {
                if src.is_file() {
                    match std::fs::copy(src, &target) {
                        Ok(_) => {
                            eprintln!(
                                "[rwkv-lifecycle] staged default.metallib at {} (from {})",
                                target.display(),
                                src.display()
                            );
                        }
                        Err(error) => {
                            // Bundle case: Contents/MacOS may be read-only on
                            // a notarized install. Fall back to spawning with
                            // cwd set to the source's parent dir so the MLX
                            // backend still picks it up.
                            eprintln!(
                                "[rwkv-lifecycle] could not copy metallib to {}: {error}; falling back to cwd={}",
                                target.display(),
                                src.parent().map(|p| p.display().to_string()).unwrap_or_default()
                            );
                            if let Some(parent) = src.parent() {
                                working_dir = parent.to_path_buf();
                            }
                        }
                    }
                } else {
                    let msg = format!("MLX 后端需要的 default.metallib 不存在: {}", src.display());
                    guard.last_error = Some(msg.clone());
                    guard.state = Some(ManagedRuntimeState::Failed);
                    return Err(msg);
                }
            } else {
                let msg =
                    "MLX 后端启用，但找不到 default.metallib。请重新运行 fetch-rwkv-sidecar.sh。"
                        .to_string();
                guard.last_error = Some(msg.clone());
                guard.state = Some(ManagedRuntimeState::Failed);
                return Err(msg);
            }
        }
    }

    let port = pick_ephemeral_port().map_err(|error| {
        let msg = format!("无法分配本地端口: {error}");
        guard.last_error = Some(msg.clone());
        msg
    })?;
    let base_url = format!("http://{}:{port}", profile.bind_host);

    let args = build_command_args(profile, &sidecar_path, &tokenizer_path, &model_path, port);
    if let Err(error) =
        cleanup_stale_sidecars(profile, &sidecar_path, &tokenizer_path, &model_path).await
    {
        guard.last_error = Some(error.clone());
        guard.state = Some(ManagedRuntimeState::Failed);
        return Err(error);
    }
    let log = open_log_file(&log_file).map_err(|error| {
        let msg = format!("无法打开运行时日志: {error}");
        guard.last_error = Some(msg.clone());
        msg
    })?;

    let stdout = log
        .try_clone()
        .map_err(|error| format!("clone log handle: {error}"))?;
    let stderr = log;

    guard.state = Some(ManagedRuntimeState::Starting);
    let mut command = TokioCommand::new(&sidecar_path);
    command
        .args(&args[1..]) // [0] is sidecar path itself, kept for `command` echo
        .current_dir(&working_dir)
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    let child = command.spawn().map_err(|error| {
        let msg = format!("无法启动 sidecar 进程: {error}");
        guard.last_error = Some(msg.clone());
        guard.state = Some(ManagedRuntimeState::Failed);
        msg
    })?;
    let pid = child.id().unwrap_or(0);
    guard.child = Some(child);
    guard.port = Some(port);
    guard.base_url = Some(base_url.clone());
    guard.pid = Some(pid);
    guard.started_at_iso = Some(iso_now());
    guard.last_error = None;

    // Drop the lock while we wait for /health — other reads can see the
    // "starting" state we just set.
    drop(guard);

    let healthy = wait_for_health(&base_url, profile.health_path).await;
    let mut guard = registry.inner.lock().await;

    if let Err(error) = healthy {
        // Reap child so we don't leave a zombie; we already errored.
        if let Some(mut child) = guard.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        guard.port = None;
        guard.base_url = None;
        guard.pid = None;
        guard.started_at_iso = None;
        guard.last_error = Some(error.clone());
        guard.state = Some(ManagedRuntimeState::Failed);
        return Err(error);
    }

    guard.state = Some(ManagedRuntimeState::Ready);
    let started_at = guard.started_at_iso.clone().unwrap_or_else(iso_now);

    Ok(ManagedRuntimeStartResult {
        pid,
        port,
        base_url,
        started_at,
        command: args,
        message: "本地 RWKV 运行时已就绪。".to_string(),
    })
}

pub async fn stop_sidecar(
    registry: &ManagedRwkvRuntimeRegistry,
    profile: Option<&RuntimeProfile>,
    sidecar_path: Option<&Path>,
    tokenizer_path: Option<&Path>,
    model_path: Option<&Path>,
) -> Result<String, String> {
    let mut guard = registry.inner.lock().await;
    let Some(mut child) = guard.child.take() else {
        guard.state = Some(ManagedRuntimeState::Stopped);
        drop(guard);
        let cleaned = cleanup_stale_sidecars_if_signature_available(
            profile,
            sidecar_path,
            tokenizer_path,
            model_path,
        )
        .await?;
        return Ok(if cleaned > 0 {
            format!("已停止 {cleaned} 个遗留的本地 RWKV sidecar。")
        } else {
            "本地 RWKV 运行时未在运行。".to_string()
        });
    };

    // Try graceful kill (SIGKILL via tokio for now — rwkv_server has no
    // documented graceful shutdown mechanism). Always wait() to reap.
    let _ = child.kill().await;
    let _ = child.wait().await;

    guard.port = None;
    guard.base_url = None;
    guard.pid = None;
    guard.started_at_iso = None;
    guard.state = Some(ManagedRuntimeState::Stopped);
    guard.last_error = None;
    drop(guard);

    let cleaned = cleanup_stale_sidecars_if_signature_available(
        profile,
        sidecar_path,
        tokenizer_path,
        model_path,
    )
    .await?;
    Ok(if cleaned > 0 {
        format!("本地 RWKV 运行时已停止，并清理 {cleaned} 个遗留 sidecar。")
    } else {
        "本地 RWKV 运行时已停止。".to_string()
    })
}

pub async fn probe_sidecar(
    registry: &ManagedRwkvRuntimeRegistry,
    profile: &RuntimeProfile,
) -> ManagedRuntimeProbeResult {
    let base_url = {
        let guard = registry.inner.lock().await;
        guard.base_url.clone()
    };

    let Some(base) = base_url else {
        return ManagedRuntimeProbeResult {
            ok: false,
            status_code: None,
            latency_ms: 0,
            base_url: None,
            message: "本地 RWKV 运行时未在运行。".to_string(),
        };
    };

    let url = format!("{}{}", base, profile.health_path);
    // `.no_proxy()` is critical: reqwest reads `HTTPS_PROXY` by default, and
    // users running Tauri behind Clash routinely have that set so the install
    // step can reach HuggingFace. Without `.no_proxy()` every loopback /health
    // / batch_chat call would also be funnelled through Clash → fails.
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(PROBE_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(error) => {
            return ManagedRuntimeProbeResult {
                ok: false,
                status_code: None,
                latency_ms: 0,
                base_url: Some(base),
                message: format!("无法创建 HTTP client: {error}"),
            };
        }
    };

    let started_at = Instant::now();
    let result = client.get(&url).send().await;
    let latency_ms = started_at.elapsed().as_millis();
    match result {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let ok = (200..300).contains(&status_code);
            let message = if ok {
                "/health 探测成功。".to_string()
            } else {
                format!("/health 返回 HTTP {status_code}。")
            };
            ManagedRuntimeProbeResult {
                ok,
                status_code: Some(status_code),
                latency_ms,
                base_url: Some(base),
                message,
            }
        }
        Err(error) => ManagedRuntimeProbeResult {
            ok: false,
            status_code: None,
            latency_ms,
            base_url: Some(base),
            message: format!("/health 请求失败: {error}"),
        },
    }
}

pub fn read_log_tail(log_path: &std::path::Path) -> Result<Vec<String>, String> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }
    let meta = std::fs::metadata(log_path).map_err(|e| format!("stat log: {e}"))?;
    let size = meta.len();
    let start = size.saturating_sub(LOG_TAIL_BYTES);

    let mut file = std::fs::File::open(log_path).map_err(|e| format!("open log: {e}"))?;
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("seek log: {e}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("read log: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    Ok(text.lines().map(|line| line.to_string()).collect())
}

// -----------------------------------------------------------------------------
// Internals
// -----------------------------------------------------------------------------

fn build_command_args(
    profile: &RuntimeProfile,
    sidecar_path: &Path,
    tokenizer_path: &Path,
    model_path: &Path,
    port: u16,
) -> Vec<String> {
    vec![
        sidecar_path.display().to_string(),
        "--model".to_string(),
        model_path.display().to_string(),
        "--tokenizer".to_string(),
        tokenizer_path.display().to_string(),
        "--backend".to_string(),
        profile.backend.to_string(),
        "--host".to_string(),
        profile.bind_host.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--model-name".to_string(),
        profile.model_name_arg.to_string(),
    ]
}

fn pick_ephemeral_port() -> std::io::Result<u16> {
    // Bind to :0, read assigned port, drop the socket. There's a tiny race
    // window before the sidecar binds, but it's acceptable for v1: the next
    // claim only happens within seconds and we re-attempt if start fails.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn open_log_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

async fn wait_for_health(base_url: &str, health_path: &str) -> Result<(), String> {
    let url = format!("{base_url}{health_path}");
    // See `probe_sidecar` for the proxy reasoning. Loopback never goes
    // through the system / env proxy.
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(error) => return Err(format!("无法创建 HTTP client: {error}")),
    };

    tokio::time::sleep(HEALTH_INITIAL_DELAY).await;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err(format!(
                "/health 在 {} 秒内未就绪。",
                STARTUP_TIMEOUT.as_secs()
            ));
        }
        match client.get(&url).send().await {
            Ok(resp) if (200..300).contains(&resp.status().as_u16()) => return Ok(()),
            _ => {
                tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
            }
        }
    }
}

fn reap_exited_child(inner: &mut RuntimeInner) {
    let Some(child) = inner.child.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(status)) => {
            inner.child = None;
            inner.port = None;
            inner.base_url = None;
            inner.pid = None;
            inner.last_error = Some(format!("Sidecar 进程已退出 (status={status})."));
            inner.state = Some(ManagedRuntimeState::Failed);
        }
        Ok(None) => {} // still running
        Err(_) => {}   // couldn't poll; leave as-is, next call retries
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidecarProcess {
    pid: u32,
    command: String,
}

async fn cleanup_stale_sidecars(
    profile: &RuntimeProfile,
    sidecar_path: &Path,
    tokenizer_path: &Path,
    model_path: &Path,
) -> Result<usize, String> {
    let processes = list_sidecar_processes()?;
    let stale = processes
        .into_iter()
        .filter(|process| {
            is_matching_managed_sidecar(process, profile, sidecar_path, tokenizer_path, model_path)
        })
        .collect::<Vec<_>>();

    if stale.is_empty() {
        return Ok(0);
    }

    for process in &stale {
        terminate_process(process.pid, "TERM")?;
    }

    tokio::time::sleep(STALE_SIDECAR_TERM_WAIT).await;

    let remaining = list_sidecar_processes()?
        .into_iter()
        .filter(|process| stale.iter().any(|stale| stale.pid == process.pid))
        .filter(|process| {
            is_matching_managed_sidecar(process, profile, sidecar_path, tokenizer_path, model_path)
        })
        .collect::<Vec<_>>();

    for process in &remaining {
        terminate_process(process.pid, "KILL")?;
    }

    eprintln!(
        "[managed-rwkv] cleaned {} stale sidecar process(es)",
        stale.len()
    );
    Ok(stale.len())
}

async fn cleanup_stale_sidecars_if_signature_available(
    profile: Option<&RuntimeProfile>,
    sidecar_path: Option<&Path>,
    tokenizer_path: Option<&Path>,
    model_path: Option<&Path>,
) -> Result<usize, String> {
    let (Some(profile), Some(sidecar_path), Some(tokenizer_path), Some(model_path)) =
        (profile, sidecar_path, tokenizer_path, model_path)
    else {
        return Ok(0);
    };
    cleanup_stale_sidecars(profile, sidecar_path, tokenizer_path, model_path).await
}

fn list_sidecar_processes() -> Result<Vec<SidecarProcess>, String> {
    let output = Command::new("ps")
        .args(["-ww", "-axo", "pid=,command="])
        .output()
        .map_err(|error| format!("无法列出本机进程: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps 返回失败状态: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter_map(parse_ps_line).collect())
}

fn parse_ps_line(line: &str) -> Option<SidecarProcess> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let split_at = trimmed.find(char::is_whitespace)?;
    let (pid_text, command) = trimmed.split_at(split_at);
    let pid = pid_text.parse::<u32>().ok()?;
    let command = command.trim_start();
    if command.is_empty() {
        return None;
    }
    Some(SidecarProcess {
        pid,
        command: command.to_string(),
    })
}

fn is_matching_managed_sidecar(
    process: &SidecarProcess,
    profile: &RuntimeProfile,
    sidecar_path: &Path,
    tokenizer_path: &Path,
    model_path: &Path,
) -> bool {
    if process.pid == std::process::id() {
        return false;
    }

    let command = process.command.as_str();
    command.contains(&sidecar_path.display().to_string())
        && command.contains("--model")
        && command.contains(&model_path.display().to_string())
        && command.contains("--tokenizer")
        && command.contains(&tokenizer_path.display().to_string())
        && command.contains("--backend")
        && command.contains(profile.backend)
        && command.contains("--model-name")
        && command.contains(profile.model_name_arg)
}

fn terminate_process(pid: u32, signal: &str) -> Result<(), String> {
    let output = Command::new("kill")
        .args([format!("-{signal}"), pid.to_string()])
        .output()
        .map_err(|error| format!("无法停止旧 sidecar 进程 {pid}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("No such process") {
        return Ok(());
    }
    Err(format!(
        "停止旧 sidecar 进程 {pid} 失败: kill -{signal} 返回 {} ({})",
        output.status,
        stderr.trim()
    ))
}

fn iso_now() -> String {
    // RFC3339-ish UTC without bringing in chrono. Resolution: seconds.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day, hour, min, sec) = secs_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn secs_to_ymdhms(mut secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    secs /= 60;
    let min = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    secs /= 24;
    // Days since epoch.
    let (year, month, day) = days_since_epoch_to_ymd(secs as i64);
    (year, month, day, hour, min, sec)
}

fn days_since_epoch_to_ymd(mut days: i64) -> (u32, u32, u32) {
    // Howard Hinnant's date algorithm (public domain), adapted.
    days += 719468;
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = (days - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as u32;
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_rwkv::profile::MACOS_ARM64_WEBRWKV;

    #[test]
    fn pick_ephemeral_port_returns_high_port() {
        let port = pick_ephemeral_port().expect("ephemeral port");
        assert!(port >= 1024, "expected non-privileged port, got {port}");
    }

    #[test]
    fn command_args_match_phase_0_validation_invocation() {
        let args = build_command_args(
            &MACOS_ARM64_WEBRWKV,
            &PathBuf::from("/bin/rwkv-server"),
            &PathBuf::from("/data/vocab.txt"),
            &PathBuf::from("/data/model.prefab"),
            8765,
        );
        // Spot-check the critical args that Phase 0 hand-validated.
        assert_eq!(args[0], "/bin/rwkv-server");
        assert!(args.iter().any(|a| a == "--backend"));
        let backend_idx = args.iter().position(|a| a == "--backend").unwrap();
        assert_eq!(args[backend_idx + 1], "web-rwkv");
        let host_idx = args.iter().position(|a| a == "--host").unwrap();
        assert_eq!(args[host_idx + 1], "127.0.0.1");
        let port_idx = args.iter().position(|a| a == "--port").unwrap();
        assert_eq!(args[port_idx + 1], "8765");
        let model_idx = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[model_idx + 1], "/data/model.prefab");
    }

    #[test]
    fn parse_ps_line_extracts_pid_and_full_command() {
        let line = "  12345 /Applications/Rosetta.app/Contents/MacOS/rwkv-server --model /Users/me/Library/Application Support/com.rosetta.desktop/model.prefab";
        let process = parse_ps_line(line).expect("process line should parse");
        assert_eq!(process.pid, 12345);
        assert!(process.command.contains("rwkv-server --model"));
        assert!(process.command.contains("Application Support"));
    }

    #[test]
    fn parse_ps_line_rejects_empty_or_pidless_lines() {
        assert_eq!(parse_ps_line(""), None);
        assert_eq!(parse_ps_line("   "), None);
        assert_eq!(parse_ps_line("not-a-pid command"), None);
    }

    #[test]
    fn matching_sidecar_requires_managed_runtime_signature() {
        let sidecar = PathBuf::from("/Applications/Rosetta.app/Contents/MacOS/rwkv-server");
        let tokenizer = PathBuf::from(
            "/Applications/Rosetta.app/Contents/Resources/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt",
        );
        let model = PathBuf::from(
            "/Users/me/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/model.prefab",
        );
        let process = SidecarProcess {
            pid: 4242,
            command: format!(
                "{} --model {} --tokenizer {} --backend web-rwkv --host 127.0.0.1 --port 64092 --model-name rwkv-translate",
                sidecar.display(),
                model.display(),
                tokenizer.display()
            ),
        };

        assert!(is_matching_managed_sidecar(
            &process,
            &MACOS_ARM64_WEBRWKV,
            &sidecar,
            &tokenizer,
            &model
        ));
    }

    #[test]
    fn matching_sidecar_rejects_other_rwkv_processes() {
        let sidecar = PathBuf::from("/Applications/Rosetta.app/Contents/MacOS/rwkv-server");
        let tokenizer = PathBuf::from("/Applications/Rosetta.app/Contents/Resources/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt");
        let model = PathBuf::from("/Users/me/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/model.prefab");
        let other = SidecarProcess {
            pid: 4243,
            command: "/opt/rwkv-server --model /tmp/other.prefab --tokenizer /tmp/vocab.txt --backend web-rwkv --model-name rwkv-translate".to_string(),
        };

        assert!(!is_matching_managed_sidecar(
            &other,
            &MACOS_ARM64_WEBRWKV,
            &sidecar,
            &tokenizer,
            &model
        ));
    }

    #[test]
    fn iso_now_has_z_suffix_and_t_separator() {
        let s = iso_now();
        assert!(s.ends_with('Z'), "got {s}");
        assert_eq!(s.chars().filter(|c| *c == 'T').count(), 1);
        // Length should be exactly 20: YYYY-MM-DDTHH:MM:SSZ.
        assert_eq!(s.len(), 20, "got {s}");
    }
}
