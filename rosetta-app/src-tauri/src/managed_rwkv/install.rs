//! Model download / verify / install for the managed RWKV runtime.
//!
//! Walks `profile.model_download_urls` in order, streams the response into a
//! `.part` file in the profile's model directory, feeds every chunk into a
//! `Sha256` hasher, then on completion compares against `profile.model_sha256`.
//! Mismatched downloads are renamed to `.part.broken` so the user can see what
//! happened; the next install attempt cleans them up.
//!
//! ### Resume support
//!
//! When a `.part` file already exists and is shorter than the profile's
//! `model_size_bytes`, install re-hashes the existing prefix (a few seconds
//! on M-series silicon for 1 GB) and issues the next mirror request with
//! `Range: bytes=<existing>-`. If the server doesn't honor the range we fall
//! back to restarting that mirror from byte 0.
//!
//! ### Cancellation
//!
//! `cancel_managed_rwkv_install` flips an `AtomicBool` watched between stream
//! chunks. On cancel we keep the `.part` so the next install resumes; the
//! command returns `state: cancelled`.
//!
//! ### Progress
//!
//! Tauri emits `managed-rwkv://install-progress` events between chunks (no
//! more than ~10 / sec via rate-limit). The IPC-side `get_managed_rwkv_install_progress`
//! command also returns a snapshot for UIs that prefer polling.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use super::layout::RuntimeLayout;
use super::profile::RuntimeProfile;

const PART_SUFFIX: &str = ".part";
const BROKEN_SUFFIX: &str = ".part.broken";
const HEAD_TIMEOUT: Duration = Duration::from_secs(15);
const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const PROGRESS_EVENT_NAME: &str = "managed-rwkv://install-progress";
const PROGRESS_EMIT_INTERVAL_MS: u128 = 100;
const HASH_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallPhase {
    Idle,
    Preflight,
    Downloading,
    Verifying,
    WritingManifest,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub source_url: Option<String>,
    pub speed_bytes_per_sec: u64,
    pub started_at: Option<String>,
    pub message: String,
    pub last_error: Option<String>,
}

impl InstallProgress {
    fn idle() -> Self {
        Self {
            phase: InstallPhase::Idle,
            bytes_done: 0,
            bytes_total: 0,
            source_url: None,
            speed_bytes_per_sec: 0,
            started_at: None,
            message: "尚未开始下载。".to_string(),
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstallOptions {
    /// When true, delete any existing model + `.part` + `.part.broken` before
    /// starting. Used by the "Repair" button after SHA256 fails.
    pub repair: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub schema_version: u32,
    pub profile_id: String,
    pub provider_id: String,
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub source_url: String,
    pub installed_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub ready: bool,
    pub installed: bool,
    pub phase: InstallPhase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub source_url: Option<String>,
    pub message: String,
    pub manifest_path: String,
}

#[derive(Default)]
pub struct InstallRegistry {
    inner: Arc<Mutex<InstallInner>>,
}

#[derive(Default)]
struct InstallInner {
    progress: Option<InstallProgress>,
    cancel: Option<Arc<AtomicBool>>,
}

impl InstallRegistry {
    pub async fn snapshot(&self) -> InstallProgress {
        let guard = self.inner.lock().await;
        guard
            .progress
            .clone()
            .unwrap_or_else(InstallProgress::idle)
    }

    pub async fn request_cancel(&self) -> bool {
        let guard = self.inner.lock().await;
        match guard.cancel.as_ref() {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }
}

/// Top-level install entry. Awaits the full download + verify; emits
/// progress events along the way.
pub async fn install_model(
    app: &AppHandle,
    registry: &InstallRegistry,
    profile: &'static RuntimeProfile,
    layout: &RuntimeLayout,
    options: InstallOptions,
) -> Result<InstallResult, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = registry.inner.lock().await;
        if guard
            .progress
            .as_ref()
            .is_some_and(|p| matches!(p.phase, InstallPhase::Preflight | InstallPhase::Downloading | InstallPhase::Verifying | InstallPhase::WritingManifest))
        {
            return Err("已有安装任务在进行中。".to_string());
        }
        guard.cancel = Some(cancel.clone());
        guard.progress = Some(InstallProgress {
            phase: InstallPhase::Preflight,
            bytes_done: 0,
            bytes_total: profile.model_size_bytes,
            source_url: None,
            speed_bytes_per_sec: 0,
            started_at: Some(iso_now()),
            message: "正在准备下载…".to_string(),
            last_error: None,
        });
    }
    emit_progress(app, registry).await;

    layout.ensure_dirs()?;

    if options.repair {
        cleanup_artifacts(layout)?;
    }

    let result = install_inner(app, registry, profile, layout, &cancel).await;

    // Always clear the cancel flag on exit so the next install can rebind one.
    {
        let mut guard = registry.inner.lock().await;
        guard.cancel = None;
    }

    result
}

async fn install_inner(
    app: &AppHandle,
    registry: &InstallRegistry,
    profile: &'static RuntimeProfile,
    layout: &RuntimeLayout,
    cancel: &Arc<AtomicBool>,
) -> Result<InstallResult, String> {
    // Already installed? Re-verify SHA256 to be safe and short-circuit.
    if layout.model_file.is_file() {
        match verify_existing_model(&layout.model_file, profile, cancel).await {
            Ok(()) => {
                let manifest_path = write_manifest(layout, profile, profile.model_download_urls[0])?;
                set_done(registry, profile, "模型已就绪。".to_string()).await;
                emit_progress(app, registry).await;
                return Ok(InstallResult {
                    ready: true,
                    installed: false,
                    phase: InstallPhase::Done,
                    bytes_done: profile.model_size_bytes,
                    bytes_total: profile.model_size_bytes,
                    source_url: None,
                    message: "模型已就绪（哈希匹配，跳过下载）。".to_string(),
                    manifest_path,
                });
            }
            Err(error) => {
                // Existing file is corrupt — rename it aside so the next
                // mirror attempt starts clean.
                let broken = with_suffix(&layout.model_file, BROKEN_SUFFIX);
                let _ = std::fs::rename(&layout.model_file, &broken);
                set_failed_message(
                    registry,
                    profile,
                    format!("现有模型文件校验失败，已重命名为 {}: {error}", broken.display()),
                )
                .await;
                emit_progress(app, registry).await;
            }
        }
    }

    let part_path = with_suffix(&layout.model_file, PART_SUFFIX);
    // Stale `.part.broken` files don't affect logic but they're noise.
    let _ = std::fs::remove_file(with_suffix(&layout.model_file, BROKEN_SUFFIX));

    let mut bytes_done_initial = 0u64;
    let mut hasher = Sha256::new();
    if part_path.is_file() {
        match rehash_existing_part(&part_path, &mut hasher, cancel).await {
            Ok(bytes) => {
                bytes_done_initial = bytes;
            }
            Err(error) => {
                // If we can't even read the part file, drop it and start over.
                let _ = std::fs::remove_file(&part_path);
                set_failed_message(
                    registry,
                    profile,
                    format!("无法读取既有断点文件，已删除: {error}"),
                )
                .await;
                hasher = Sha256::new();
            }
        }
    }
    update_progress(registry, |p| {
        p.bytes_done = bytes_done_initial;
        p.bytes_total = profile.model_size_bytes;
        p.phase = InstallPhase::Downloading;
        p.message = if bytes_done_initial > 0 {
            format!(
                "断点续传中（已恢复 {} bytes）…",
                bytes_done_initial
            )
        } else {
            "开始下载…".to_string()
        };
    })
    .await;
    emit_progress(app, registry).await;

    let mut last_error: Option<String> = None;
    for url in profile.model_download_urls {
        if cancel.load(Ordering::SeqCst) {
            set_cancelled(registry).await;
            emit_progress(app, registry).await;
            return Err("安装已取消。".to_string());
        }

        eprintln!("[rwkv-install] trying mirror: {url}");
        update_progress(registry, |p| {
            p.source_url = Some(url.to_string());
            p.message = format!("正在连接 {url}…");
        })
        .await;
        emit_progress(app, registry).await;

        match download_from_mirror(
            app,
            registry,
            url,
            profile,
            &part_path,
            &mut hasher,
            &mut bytes_done_initial,
            cancel,
        )
        .await
        {
            Ok(()) => {
                eprintln!("[rwkv-install] mirror succeeded: {url}");
                last_error = None;
                break;
            }
            Err(DownloadError::Cancelled) => {
                eprintln!("[rwkv-install] mirror cancelled by user: {url}");
                set_cancelled(registry).await;
                emit_progress(app, registry).await;
                return Err("安装已取消。".to_string());
            }
            Err(DownloadError::Mirror(msg)) => {
                eprintln!("[rwkv-install] mirror failed: {url} → {msg}");
                last_error = Some(msg.clone());
                // Reset hasher + bytes_done because the next mirror's stream
                // restarts from byte 0 (we can't guarantee continuity across
                // mirrors that may serve subtly different files).
                hasher = Sha256::new();
                bytes_done_initial = 0;
                let _ = std::fs::remove_file(&part_path);
                update_progress(registry, |p| {
                    p.bytes_done = 0;
                    p.message = format!("镜像 {url} 失败，尝试下一个: {msg}");
                    p.last_error = Some(msg.clone());
                })
                .await;
                emit_progress(app, registry).await;
            }
            Err(DownloadError::Fatal(msg)) => {
                eprintln!("[rwkv-install] fatal error: {msg}");
                set_failed_message(registry, profile, msg.clone()).await;
                emit_progress(app, registry).await;
                return Err(msg);
            }
        }
    }

    if let Some(msg) = last_error {
        let full = format!("所有镜像都未能下载模型: {msg}");
        eprintln!("[rwkv-install] all mirrors exhausted: {full}");
        set_failed_message(registry, profile, full.clone()).await;
        emit_progress(app, registry).await;
        return Err(full);
    }

    // Verification.
    update_progress(registry, |p| {
        p.phase = InstallPhase::Verifying;
        p.message = "校验 SHA256…".to_string();
    })
    .await;
    emit_progress(app, registry).await;

    let actual_hex = hex_lower(&hasher.finalize());
    if actual_hex != profile.model_sha256 {
        let broken = with_suffix(&layout.model_file, BROKEN_SUFFIX);
        let _ = std::fs::rename(&part_path, &broken);
        let msg = format!(
            "SHA256 不匹配（预期 {}，实际 {}），已保留为 {} 以便排查。",
            profile.model_sha256,
            actual_hex,
            broken.display()
        );
        set_failed_message(registry, profile, msg.clone()).await;
        emit_progress(app, registry).await;
        return Err(msg);
    }

    // Rename .part → final filename atomically (same dir).
    if let Err(error) = std::fs::rename(&part_path, &layout.model_file) {
        let msg = format!("无法重命名 .part 到最终文件: {error}");
        set_failed_message(registry, profile, msg.clone()).await;
        emit_progress(app, registry).await;
        return Err(msg);
    }

    // Manifest.
    update_progress(registry, |p| {
        p.phase = InstallPhase::WritingManifest;
        p.message = "写入安装清单…".to_string();
    })
    .await;
    emit_progress(app, registry).await;
    let source_url = {
        let guard = registry.inner.lock().await;
        guard
            .progress
            .as_ref()
            .and_then(|p| p.source_url.clone())
            .unwrap_or_else(|| profile.model_download_urls[0].to_string())
    };
    let manifest_path = write_manifest(layout, profile, &source_url)?;

    set_done(registry, profile, "本地 RWKV 模型已就绪。".to_string()).await;
    emit_progress(app, registry).await;

    Ok(InstallResult {
        ready: true,
        installed: true,
        phase: InstallPhase::Done,
        bytes_done: profile.model_size_bytes,
        bytes_total: profile.model_size_bytes,
        source_url: Some(source_url),
        message: "本地 RWKV 模型已下载并校验完成。".to_string(),
        manifest_path,
    })
}

enum DownloadError {
    /// User-initiated cancel — keep `.part` for resume.
    Cancelled,
    /// Mirror-specific error — try the next mirror.
    Mirror(String),
    /// Non-recoverable (disk full, etc.) — surface immediately.
    Fatal(String),
}

#[allow(clippy::too_many_arguments)] // download loop pulls together many independent inputs; grouping would hurt readability
async fn download_from_mirror(
    app: &AppHandle,
    registry: &InstallRegistry,
    url: &str,
    profile: &RuntimeProfile,
    part_path: &Path,
    hasher: &mut Sha256,
    bytes_done: &mut u64,
    cancel: &Arc<AtomicBool>,
) -> Result<(), DownloadError> {
    let client = reqwest::Client::builder()
        .connect_timeout(STREAM_CONNECT_TIMEOUT)
        // No total request timeout — large file, slow proxies, etc.
        .build()
        .map_err(|e| DownloadError::Fatal(format!("HTTP client 创建失败: {e}")))?;

    // HEAD: verify size before downloading bulk.
    let head = client
        .head(url)
        .timeout(HEAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| DownloadError::Mirror(format!("HEAD 请求失败: {e}")))?;
    if !head.status().is_success() {
        return Err(DownloadError::Mirror(format!(
            "HEAD 返回 HTTP {}",
            head.status().as_u16()
        )));
    }
    if let Some(len) = head
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        if len != profile.model_size_bytes {
            return Err(DownloadError::Mirror(format!(
                "镜像 Content-Length 与预期不符（预期 {}，实际 {}）",
                profile.model_size_bytes, len
            )));
        }
    }

    // GET (optionally with Range for resume).
    let mut request = client.get(url);
    if *bytes_done > 0 && *bytes_done < profile.model_size_bytes {
        request = request.header(
            reqwest::header::RANGE,
            format!("bytes={}-", bytes_done),
        );
    }
    let response = request
        .send()
        .await
        .map_err(|e| DownloadError::Mirror(format!("GET 请求失败: {e}")))?;

    let status = response.status();
    let accept_range = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !status.is_success() {
        return Err(DownloadError::Mirror(format!("GET 返回 HTTP {}", status.as_u16())));
    }

    // If we requested Range and the server ignored it (200 instead of 206),
    // it gave us the full file. Rewind our hasher + truncate the .part file.
    let mut append_mode = true;
    if *bytes_done > 0 && !accept_range {
        *hasher = Sha256::new();
        *bytes_done = 0;
        update_progress(registry, |p| {
            p.bytes_done = 0;
            p.message = "镜像未支持 Range，重新从头下载…".to_string();
        })
        .await;
        emit_progress(app, registry).await;
        append_mode = false;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append_mode)
        .truncate(!append_mode)
        .open(part_path)
        .await
        .map_err(|e| DownloadError::Fatal(format!("打开 .part 文件失败: {e}")))?;

    let mut stream = response.bytes_stream();
    let mut last_emit = Instant::now();
    let mut last_bytes = *bytes_done;
    let mut last_window = Instant::now();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err(DownloadError::Cancelled);
        }
        let bytes = chunk
            .map_err(|e| DownloadError::Mirror(format!("流读取失败: {e}")))?;
        hasher.update(&bytes);
        file.write_all(&bytes)
            .await
            .map_err(|e| DownloadError::Fatal(format!("写入 .part 失败: {e}")))?;
        *bytes_done += bytes.len() as u64;

        // Throttled progress: at most ~10 emits/sec.
        if last_emit.elapsed().as_millis() >= PROGRESS_EVENT_NAME_RATE_LIMIT {
            let elapsed = last_window.elapsed().as_secs_f64().max(0.001);
            let delta_bytes = bytes_done.saturating_sub(last_bytes);
            let speed = (delta_bytes as f64 / elapsed) as u64;
            last_bytes = *bytes_done;
            last_window = Instant::now();

            let total = profile.model_size_bytes;
            let bd = *bytes_done;
            let percent = bd
                .checked_mul(100)
                .and_then(|v| v.checked_div(total))
                .unwrap_or(0);
            update_progress(registry, |p| {
                p.bytes_done = bd;
                p.speed_bytes_per_sec = speed;
                p.message = format!(
                    "下载中 {}% ({:.1} MB/s)",
                    percent,
                    speed as f64 / (1024.0 * 1024.0)
                );
            })
            .await;
            emit_progress(app, registry).await;
            last_emit = Instant::now();
        }
    }

    file.flush()
        .await
        .map_err(|e| DownloadError::Fatal(format!("flush .part 失败: {e}")))?;
    drop(file);

    if *bytes_done != profile.model_size_bytes {
        return Err(DownloadError::Mirror(format!(
            "下载完成但字节数不符（预期 {}，实际 {}）",
            profile.model_size_bytes, *bytes_done
        )));
    }
    Ok(())
}

const PROGRESS_EVENT_NAME_RATE_LIMIT: u128 = PROGRESS_EMIT_INTERVAL_MS;

async fn rehash_existing_part(
    path: &Path,
    hasher: &mut Sha256,
    cancel: &Arc<AtomicBool>,
) -> std::io::Result<u64> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; HASH_BUFFER_BYTES];
    let mut total = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(total);
        }
        let n = file.read(&mut buf).await?;
        if n == 0 {
            return Ok(total);
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
}

async fn verify_existing_model(
    path: &Path,
    profile: &RuntimeProfile,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("stat 失败: {e}"))?;
    if meta.len() != profile.model_size_bytes {
        return Err(format!(
            "字节数不符（预期 {}，实际 {}）",
            profile.model_size_bytes,
            meta.len()
        ));
    }
    let mut hasher = Sha256::new();
    let _ = rehash_existing_part(path, &mut hasher, cancel)
        .await
        .map_err(|e| format!("读取失败: {e}"))?;
    let actual = hex_lower(&hasher.finalize());
    if actual != profile.model_sha256 {
        return Err(format!(
            "SHA256 不匹配（预期 {}，实际 {}）",
            profile.model_sha256, actual
        ));
    }
    Ok(())
}

fn write_manifest(
    layout: &RuntimeLayout,
    profile: &RuntimeProfile,
    source_url: &str,
) -> Result<String, String> {
    let manifest = ModelManifest {
        schema_version: 1,
        profile_id: profile.id.to_string(),
        provider_id: profile.provider_id.to_string(),
        filename: profile.model_filename.to_string(),
        sha256: profile.model_sha256.to_string(),
        size_bytes: profile.model_size_bytes,
        source_url: source_url.to_string(),
        installed_at: iso_now(),
    };
    let json =
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("manifest 序列化失败: {e}"))?;
    std::fs::write(&layout.model_manifest_file, json.as_bytes())
        .map_err(|e| format!("写入 manifest 失败: {e}"))?;
    Ok(layout.model_manifest_file.display().to_string())
}

fn cleanup_artifacts(layout: &RuntimeLayout) -> Result<(), String> {
    for path in [
        layout.model_file.clone(),
        with_suffix(&layout.model_file, PART_SUFFIX),
        with_suffix(&layout.model_file, BROKEN_SUFFIX),
        layout.model_manifest_file.clone(),
    ] {
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 {} 失败: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

async fn update_progress(
    registry: &InstallRegistry,
    mutate: impl FnOnce(&mut InstallProgress),
) {
    let mut guard = registry.inner.lock().await;
    if guard.progress.is_none() {
        guard.progress = Some(InstallProgress::idle());
    }
    if let Some(progress) = guard.progress.as_mut() {
        mutate(progress);
    }
}

async fn emit_progress(app: &AppHandle, registry: &InstallRegistry) {
    let progress = registry.snapshot().await;
    let _ = app.emit(PROGRESS_EVENT_NAME, progress);
}

async fn set_done(registry: &InstallRegistry, profile: &RuntimeProfile, message: String) {
    let mut guard = registry.inner.lock().await;
    let total = profile.model_size_bytes;
    if guard.progress.is_none() {
        guard.progress = Some(InstallProgress::idle());
    }
    if let Some(progress) = guard.progress.as_mut() {
        progress.phase = InstallPhase::Done;
        progress.bytes_done = total;
        progress.bytes_total = total;
        progress.message = message;
        progress.last_error = None;
    }
}

async fn set_cancelled(registry: &InstallRegistry) {
    let mut guard = registry.inner.lock().await;
    if let Some(progress) = guard.progress.as_mut() {
        progress.phase = InstallPhase::Cancelled;
        progress.message = "安装已取消（可点击重试继续）。".to_string();
    }
}

async fn set_failed_message(registry: &InstallRegistry, _profile: &RuntimeProfile, message: String) {
    let mut guard = registry.inner.lock().await;
    if let Some(progress) = guard.progress.as_mut() {
        progress.phase = InstallPhase::Failed;
        progress.message = message.clone();
        progress.last_error = Some(message);
    }
}

fn iso_now() -> String {
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
    let (year, month, day) = days_since_epoch_to_ymd(secs as i64);
    (year, month, day, hour, min, sec)
}

fn days_since_epoch_to_ymd(mut days: i64) -> (u32, u32, u32) {
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
    fn hex_lower_pads_each_byte_to_two_chars() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xff, 0x12]), "000fff12");
    }

    #[test]
    fn with_suffix_appends_after_full_filename() {
        let p = Path::new("/tmp/foo/model.prefab");
        assert_eq!(
            with_suffix(p, ".part"),
            PathBuf::from("/tmp/foo/model.prefab.part")
        );
        assert_eq!(
            with_suffix(p, ".part.broken"),
            PathBuf::from("/tmp/foo/model.prefab.part.broken")
        );
    }

    #[test]
    fn iso_now_format_matches_runtime_module() {
        let s = iso_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(s.chars().filter(|c| *c == 'T').count(), 1);
    }

    #[test]
    fn install_progress_idle_has_zero_bytes_and_empty_url() {
        let p = InstallProgress::idle();
        assert_eq!(p.phase, InstallPhase::Idle);
        assert_eq!(p.bytes_done, 0);
        assert!(p.source_url.is_none());
    }

    #[test]
    fn cleanup_artifacts_removes_all_phase_files() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-install-cleanup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let layout = RuntimeLayout::resolve(&tmp, &MACOS_ARM64_WEBRWKV);
        layout.ensure_dirs().unwrap();
        let part = with_suffix(&layout.model_file, PART_SUFFIX);
        let broken = with_suffix(&layout.model_file, BROKEN_SUFFIX);
        std::fs::write(&layout.model_file, b"x").unwrap();
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(&broken, b"x").unwrap();
        std::fs::write(&layout.model_manifest_file, b"{}").unwrap();

        cleanup_artifacts(&layout).unwrap();

        assert!(!layout.model_file.exists());
        assert!(!part.exists());
        assert!(!broken.exists());
        assert!(!layout.model_manifest_file.exists());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn write_manifest_serializes_camel_case_with_real_profile_data() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-install-manifest-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let layout = RuntimeLayout::resolve(&tmp, &MACOS_ARM64_WEBRWKV);
        layout.ensure_dirs().unwrap();

        let path = write_manifest(&layout, &MACOS_ARM64_WEBRWKV, "https://example.test/model").unwrap();
        assert_eq!(path, layout.model_manifest_file.display().to_string());

        let body = std::fs::read_to_string(&layout.model_manifest_file).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["profileId"], MACOS_ARM64_WEBRWKV.id);
        assert_eq!(value["providerId"], MACOS_ARM64_WEBRWKV.provider_id);
        assert_eq!(value["filename"], MACOS_ARM64_WEBRWKV.model_filename);
        assert_eq!(value["sha256"], MACOS_ARM64_WEBRWKV.model_sha256);
        assert_eq!(value["sizeBytes"], MACOS_ARM64_WEBRWKV.model_size_bytes);
        assert_eq!(value["sourceUrl"], "https://example.test/model");
        assert!(value["installedAt"].as_str().unwrap().ends_with('Z'));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
