use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use super::{
    capabilities::{read_pack_engine_capabilities, Pdf2zhEngineCapabilities},
    layout::{Pdf2zhLayout, DOCLAYOUT_MODEL_FILENAME},
    profile::Pdf2zhProfile,
};
const PROGRESS_EVENT_NAME: &str = "managed-pdf2zh://install-progress";
const PROGRESS_EMIT_INTERVAL_MS: u128 = 100;
const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_ARCHIVE_BYTES: u64 = 650 * 1024 * 1024;
const DOWNLOAD_PROTOCOL_TOLERANCE_BYTES: u64 = 64 * 1024;
const DISK_SAFETY_MARGIN_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_UNPACKED_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const DEFAULT_MAX_FILE_COUNT: u64 = 50_000;
const DEFAULT_MAX_SYMLINK_COUNT: u64 = 2_048;
const DEFAULT_MAX_SINGLE_FILE_BYTES: u64 = 512 * 1024 * 1024;
const LINUX_MAX_UNPACKED_BYTES: u64 = 1_555_956_170;
const LINUX_MAX_FILE_COUNT: u64 = 24_809;
const LINUX_MAX_SINGLE_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TAR_METADATA_BYTES: u64 = 1024 * 1024;
#[cfg(target_os = "windows")]
const PACK_DIR_DELETE_RETRY_COUNT: usize = 20;
#[cfg(target_os = "windows")]
const PACK_DIR_DELETE_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pdf2zhInstallPhase {
    Idle,
    Preflight,
    Downloading,
    Verifying,
    Extracting,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhInstallProgress {
    pub phase: Pdf2zhInstallPhase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub source_url: Option<String>,
    pub speed_bytes_per_sec: u64,
    pub started_at: Option<String>,
    pub message: String,
    pub last_error: Option<String>,
}

impl Pdf2zhInstallProgress {
    fn idle() -> Self {
        Self {
            phase: Pdf2zhInstallPhase::Idle,
            bytes_done: 0,
            bytes_total: 0,
            source_url: None,
            speed_bytes_per_sec: 0,
            started_at: None,
            message: "尚未开始安装 PDF 版面处理组件。".to_string(),
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pdf2zhInstallOptions {
    pub repair: bool,
    pub proxy_url: Option<String>,
    /// Optional archive URL for dogfood builds before the official release URL
    /// is pinned in [`Pdf2zhProfile`]. Supports `https://...` and `file://...`.
    pub pack_url: Option<String>,
    pub pack_sha256: Option<String>,
    pub pack_size_bytes: Option<u64>,
}

impl Pdf2zhInstallOptions {
    fn effective_proxy_url(&self) -> Option<&str> {
        self.proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhInstallResult {
    pub ready: bool,
    pub installed: bool,
    pub phase: Pdf2zhInstallPhase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub source_url: Option<String>,
    pub message: String,
    pub manifest_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Pdf2zhPackManifest {
    schema_version: u32,
    profile_id: String,
    pack_filename: String,
    sha256: Option<String>,
    size_bytes: Option<u64>,
    unpacked_size_bytes: u64,
    file_count: u64,
    symlink_count: u64,
    max_single_file_bytes: u64,
    source_url: String,
    installed_at: String,
    custom_pack: bool,
    engine_capability_schema_version: u32,
    engine_contract_version: u32,
    engine_revision: u32,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArchiveLimits {
    max_unpacked_bytes: u64,
    max_file_count: u64,
    max_symlink_count: u64,
    max_single_file_bytes: u64,
}

impl ArchiveLimits {
    fn for_profile(profile: &Pdf2zhProfile) -> Self {
        if profile.platform_os == "linux" {
            Self {
                max_unpacked_bytes: LINUX_MAX_UNPACKED_BYTES,
                max_file_count: LINUX_MAX_FILE_COUNT,
                max_symlink_count: DEFAULT_MAX_SYMLINK_COUNT,
                max_single_file_bytes: LINUX_MAX_SINGLE_FILE_BYTES,
            }
        } else {
            Self {
                max_unpacked_bytes: DEFAULT_MAX_UNPACKED_BYTES,
                max_file_count: DEFAULT_MAX_FILE_COUNT,
                max_symlink_count: DEFAULT_MAX_SYMLINK_COUNT,
                max_single_file_bytes: DEFAULT_MAX_SINGLE_FILE_BYTES,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ArchiveStats {
    unpacked_size_bytes: u64,
    file_count: u64,
    symlink_count: u64,
    max_single_file_bytes: u64,
}

#[derive(Default)]
pub struct Pdf2zhInstallRegistry {
    inner: Arc<Mutex<InstallInner>>,
}

#[derive(Default)]
struct InstallInner {
    progress: Option<Pdf2zhInstallProgress>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Pdf2zhInstallRegistry {
    pub async fn snapshot(&self) -> Pdf2zhInstallProgress {
        let guard = self.inner.lock().await;
        guard
            .progress
            .clone()
            .unwrap_or_else(Pdf2zhInstallProgress::idle)
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

pub async fn install_pack(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    profile: &'static Pdf2zhProfile,
    layout: &Pdf2zhLayout,
    options: Pdf2zhInstallOptions,
) -> Result<Pdf2zhInstallResult, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = registry.inner.lock().await;
        if guard.progress.as_ref().is_some_and(|progress| {
            matches!(
                progress.phase,
                Pdf2zhInstallPhase::Preflight
                    | Pdf2zhInstallPhase::Downloading
                    | Pdf2zhInstallPhase::Verifying
                    | Pdf2zhInstallPhase::Extracting
            )
        }) {
            return Err("已有 PDF 版面处理组件安装任务在进行中。".to_string());
        }
        guard.cancel = Some(cancel.clone());
        guard.progress = Some(Pdf2zhInstallProgress {
            phase: Pdf2zhInstallPhase::Preflight,
            bytes_done: 0,
            bytes_total: effective_size(profile, &options).unwrap_or(0),
            source_url: None,
            speed_bytes_per_sec: 0,
            started_at: Some(timestamp_ms_string()),
            message: "正在准备 PDF 版面处理组件安装…".to_string(),
            last_error: None,
        });
    }
    emit_progress(app, registry).await;

    let result = install_inner(app, registry, profile, layout, &options, &cancel).await;
    if let Err(message) = result.as_ref() {
        if cancel.load(Ordering::SeqCst) {
            set_cancelled(registry).await;
        } else if !matches!(
            registry.snapshot().await.phase,
            Pdf2zhInstallPhase::Failed | Pdf2zhInstallPhase::Cancelled
        ) {
            set_failed(registry, message.clone()).await;
        }
        emit_progress(app, registry).await;
    }
    {
        let mut guard = registry.inner.lock().await;
        guard.cancel = None;
    }
    result
}

async fn install_inner(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    profile: &'static Pdf2zhProfile,
    layout: &Pdf2zhLayout,
    options: &Pdf2zhInstallOptions,
    cancel: &Arc<AtomicBool>,
) -> Result<Pdf2zhInstallResult, String> {
    layout.ensure_dirs()?;

    if layout.managed_pack_ready(profile) && !options.repair {
        set_done(registry, "PDF 版面处理已就绪。".to_string()).await;
        emit_progress(app, registry).await;
        return Ok(Pdf2zhInstallResult {
            ready: true,
            installed: false,
            phase: Pdf2zhInstallPhase::Done,
            bytes_done: effective_size(profile, options).unwrap_or(0),
            bytes_total: effective_size(profile, options).unwrap_or(0),
            source_url: None,
            message: "PDF 版面处理已就绪，跳过安装。".to_string(),
            manifest_path: layout.manifest_file.display().to_string(),
        });
    }

    let urls = effective_urls(profile, options)?;
    let expected_sha = effective_sha(profile, options);
    let expected_size = effective_size(profile, options);
    let download_limit = archive_download_limit(expected_size)?;
    let archive_limits = ArchiveLimits::for_profile(profile);
    let custom_pack = has_custom_pack_url(options);
    let archive_path = layout.downloads_dir.join(profile.pack_filename);
    let part_path = layout
        .downloads_dir
        .join(format!("{}.part", profile.pack_filename));

    let _ = std::fs::remove_file(&archive_path);
    let _ = std::fs::remove_file(&part_path);
    let mut part_cleanup = FileCleanup::new(part_path.clone());
    ensure_disk_capacity(
        layout,
        profile,
        expected_size.unwrap_or(download_limit),
        archive_limits,
        custom_pack,
    )?;

    if cancel.load(Ordering::SeqCst) {
        set_cancelled(registry).await;
        emit_progress(app, registry).await;
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }

    let mut source_url = String::new();
    let url_count = urls.len();
    for (i, url) in urls.into_iter().enumerate() {
        let _ = tokio::fs::remove_file(&part_path).await;
        update_progress(registry, |progress| {
            progress.phase = Pdf2zhInstallPhase::Downloading;
            progress.source_url = Some(url.clone());
            progress.bytes_done = 0;
            progress.bytes_total = expected_size.unwrap_or(0);
            progress.message = if i == 0 {
                format!("正在获取 PDF 版面处理组件: {url}")
            } else {
                format!("正在尝试备用地址下载 PDF 版面处理组件: {url}")
            };
        })
        .await;
        emit_progress(app, registry).await;

        let result = if url.starts_with("file://") {
            copy_file_url(
                app,
                registry,
                url.trim_start_matches("file://"),
                &part_path,
                expected_size,
                download_limit,
                cancel,
            )
            .await
        } else {
            download_http(
                app,
                registry,
                &url,
                &part_path,
                expected_size,
                download_limit,
                options.effective_proxy_url(),
                cancel,
            )
            .await
        };

        match result {
            Ok(()) => {
                source_url = url;
                break;
            }
            Err(e) if cancel.load(Ordering::SeqCst) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(e);
            }
            Err(_) if i + 1 < url_count => {
                let _ = tokio::fs::remove_file(&part_path).await;
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(e);
            }
        }
    }

    tokio::fs::rename(&part_path, &archive_path)
        .await
        .map_err(|error| format!("无法重命名下载文件: {error}"))?;
    part_cleanup.disarm();

    update_progress(registry, |progress| {
        progress.phase = Pdf2zhInstallPhase::Verifying;
        progress.message = "正在校验 PDF 版面处理组件…".to_string();
    })
    .await;
    emit_progress(app, registry).await;

    let actual_sha = match sha256_file(&archive_path, cancel).await {
        Ok(sha) => sha,
        Err(e) => {
            let _ = std::fs::remove_file(&archive_path);
            return Err(e);
        }
    };
    if let Some(expected) = expected_sha.as_deref() {
        if actual_sha != expected {
            let _ = std::fs::remove_file(&archive_path);
            let message =
                format!("PDF 版面处理组件校验失败（预期 {expected}，实际 {actual_sha}）。");
            set_failed(registry, message.clone()).await;
            emit_progress(app, registry).await;
            return Err(message);
        }
    }
    if let Some(expected) = expected_size {
        let actual_size = std::fs::metadata(&archive_path)
            .map_err(|error| format!("无法读取组件文件大小: {error}"))?
            .len();
        if actual_size != expected {
            let _ = std::fs::remove_file(&archive_path);
            let message =
                format!("PDF 版面处理组件大小不匹配（预期 {expected}，实际 {actual_size}）。");
            set_failed(registry, message.clone()).await;
            emit_progress(app, registry).await;
            return Err(message);
        }
    }

    update_progress(registry, |progress| {
        progress.phase = Pdf2zhInstallPhase::Extracting;
        progress.message = "正在解压 PDF 版面处理组件…".to_string();
    })
    .await;
    emit_progress(app, registry).await;

    let (engine_capabilities, archive_stats, transaction) = extract_pack(
        app,
        &archive_path,
        layout,
        profile,
        archive_limits,
        custom_pack,
        cancel,
    )
    .await?;
    scrub_python_bytecode(&layout.pack_dir, cancel)?;
    if cancel.load(Ordering::SeqCst) {
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }
    let manifest_transaction = write_manifest(
        layout,
        profile,
        &source_url,
        expected_size,
        Some(actual_sha),
        custom_pack,
        &engine_capabilities,
        archive_stats,
    )?;
    transaction.commit().await;
    manifest_transaction.commit();

    set_done(registry, "PDF 版面处理组件已安装。".to_string()).await;
    emit_progress(app, registry).await;
    Ok(Pdf2zhInstallResult {
        ready: true,
        installed: true,
        phase: Pdf2zhInstallPhase::Done,
        bytes_done: expected_size.unwrap_or(0),
        bytes_total: expected_size.unwrap_or(0),
        source_url: Some(source_url),
        message: "PDF 版面处理组件已安装。".to_string(),
        manifest_path: layout.manifest_file.display().to_string(),
    })
}

async fn download_http(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    url: &str,
    target: &Path,
    expected_size: Option<u64>,
    download_limit: u64,
    proxy_url: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut builder = reqwest::Client::builder().connect_timeout(STREAM_CONNECT_TIMEOUT);
    if let Some(proxy) = proxy_url {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy)
                .map_err(|error| format!("PDF 版面处理组件代理 URL 无效: {error}"))?,
        );
    }
    let client = builder
        .build()
        .map_err(|error| format!("无法创建 PDF 版面处理组件下载 HTTP client: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("下载 PDF 版面处理组件失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 PDF 版面处理组件返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > download_limit)
    {
        return Err(format!(
            "PDF 版面处理组件下载超过安全上限（上限 {download_limit} bytes）。"
        ));
    }
    stream_response_to_file(
        app,
        registry,
        response,
        target,
        expected_size,
        download_limit,
        cancel,
    )
    .await
}

async fn stream_response_to_file(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    response: reqwest::Response,
    target: &Path,
    expected_size: Option<u64>,
    download_limit: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建下载目录: {error}"))?;
    }
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|error| format!("无法创建 pack 文件: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut bytes_done = 0u64;
    let mut last_bytes = 0u64;
    let mut last_window = Instant::now();
    let mut last_emit = Instant::now();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            set_cancelled(registry).await;
            emit_progress(app, registry).await;
            return Err("PDF 版面处理组件安装已取消。".to_string());
        }
        let bytes = chunk.map_err(|error| format!("读取 pack 下载流失败: {error}"))?;
        let next_bytes = checked_download_size(bytes_done, bytes.len(), download_limit)?;
        file.write_all(&bytes)
            .await
            .map_err(|error| format!("写入 pack 文件失败: {error}"))?;
        bytes_done = next_bytes;
        if last_emit.elapsed().as_millis() >= PROGRESS_EMIT_INTERVAL_MS {
            let elapsed = last_window.elapsed().as_secs_f64().max(0.001);
            let speed = ((bytes_done - last_bytes) as f64 / elapsed) as u64;
            last_bytes = bytes_done;
            last_window = Instant::now();
            update_progress(registry, |progress| {
                progress.bytes_done = bytes_done;
                progress.bytes_total = expected_size.unwrap_or(progress.bytes_total);
                progress.speed_bytes_per_sec = speed;
                progress.message = if let Some(total) = expected_size {
                    let percent = bytes_done
                        .saturating_mul(100)
                        .checked_div(total)
                        .unwrap_or(0);
                    format!("下载 PDF 版面处理组件中 {percent}%")
                } else {
                    format!("下载 PDF 版面处理组件中 ({bytes_done} bytes)")
                };
            })
            .await;
            emit_progress(app, registry).await;
            last_emit = Instant::now();
        }
    }
    file.flush()
        .await
        .map_err(|error| format!("刷新 pack 文件失败: {error}"))?;
    update_progress(registry, |progress| {
        progress.bytes_done = bytes_done;
        progress.bytes_total = expected_size.unwrap_or(bytes_done);
        progress.speed_bytes_per_sec = 0;
    })
    .await;
    emit_progress(app, registry).await;
    Ok(())
}

async fn copy_file_url(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    source: &str,
    target: &Path,
    expected_size: Option<u64>,
    download_limit: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        set_cancelled(registry).await;
        emit_progress(app, registry).await;
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }
    let source_size = tokio::fs::metadata(source)
        .await
        .map_err(|error| format!("无法读取本地 PDF 版面处理组件: {error}"))?
        .len();
    if source_size > download_limit {
        return Err(format!(
            "本地 PDF 版面处理组件超过安全上限（{source_size} > {download_limit} bytes）。"
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建下载目录: {error}"))?;
    }
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|error| format!("无法打开本地 PDF 版面处理组件: {error}"))?;
    let mut output = tokio::fs::File::create(target)
        .await
        .map_err(|error| format!("无法创建 pack 文件: {error}"))?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut bytes = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            set_cancelled(registry).await;
            emit_progress(app, registry).await;
            return Err("PDF 版面处理组件安装已取消。".to_string());
        }
        let read = input
            .read(&mut buffer)
            .await
            .map_err(|error| format!("读取本地 PDF 版面处理组件失败: {error}"))?;
        if read == 0 {
            break;
        }
        let next_bytes = checked_download_size(bytes, read, download_limit)?;
        output
            .write_all(&buffer[..read])
            .await
            .map_err(|error| format!("复制本地 PDF 版面处理组件失败: {error}"))?;
        bytes = next_bytes;
    }
    output
        .flush()
        .await
        .map_err(|error| format!("刷新本地 PDF 版面处理组件失败: {error}"))?;
    update_progress(registry, |progress| {
        progress.bytes_done = bytes;
        progress.bytes_total = expected_size.unwrap_or(bytes);
        progress.message = "已复制本地 PDF 版面处理组件。".to_string();
    })
    .await;
    emit_progress(app, registry).await;
    Ok(())
}

async fn sha256_file(path: &Path, cancel: &Arc<AtomicBool>) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("无法打开 pack 校验: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("PDF 版面处理组件安装已取消。".to_string());
        }
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("读取 pack 校验失败: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn archive_download_limit(expected_size: Option<u64>) -> Result<u64, String> {
    match expected_size {
        Some(expected) if expected > MAX_ARCHIVE_BYTES => Err(format!(
            "PDF 版面处理组件声明大小超过安全上限（{expected} > {MAX_ARCHIVE_BYTES} bytes）。"
        )),
        Some(expected) => Ok(expected
            .saturating_add(DOWNLOAD_PROTOCOL_TOLERANCE_BYTES)
            .min(MAX_ARCHIVE_BYTES)),
        None => Ok(MAX_ARCHIVE_BYTES),
    }
}

fn checked_download_size(current: u64, chunk_len: usize, limit: u64) -> Result<u64, String> {
    let next = current
        .checked_add(chunk_len as u64)
        .ok_or_else(|| "PDF 版面处理组件下载大小溢出。".to_string())?;
    if next > limit {
        return Err(format!(
            "PDF 版面处理组件下载超过安全上限（{next} > {limit} bytes）。"
        ));
    }
    Ok(next)
}

fn ensure_disk_capacity(
    layout: &Pdf2zhLayout,
    profile: &Pdf2zhProfile,
    archive_bytes: u64,
    limits: ArchiveLimits,
    custom_pack: bool,
) -> Result<(), String> {
    let unpacked_bytes = if custom_pack {
        limits.max_unpacked_bytes
    } else {
        profile
            .pack_unpacked_size_bytes
            .unwrap_or(limits.max_unpacked_bytes)
    };
    let old_pack_bytes = directory_regular_file_bytes(&layout.pack_dir)?;
    let required_free = archive_bytes
        .checked_add(unpacked_bytes)
        .and_then(|bytes| bytes.checked_add(DISK_SAFETY_MARGIN_BYTES))
        .ok_or_else(|| "PDF 版面处理组件磁盘空间计算溢出。".to_string())?;
    let available = fs2::available_space(&layout.root_dir)
        .map_err(|error| format!("无法检查 PDF 组件目录可用磁盘空间: {error}"))?;
    if available < required_free {
        return Err(format!(
            "PDF 版面处理组件安装空间不足：需要至少 {required_free} bytes 可用空间，当前为 {available} bytes（现有 pack {old_pack_bytes} bytes 会保留到新 pack 完整安装）。"
        ));
    }
    Ok(())
}

fn directory_regular_file_bytes(root: &Path) -> Result<u64, String> {
    if !root.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| format!("无法检查现有 PDF 组件目录: {error}"))?
        {
            let entry = entry.map_err(|error| format!("无法读取现有 PDF 组件目录项: {error}"))?;
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|error| format!("无法读取现有 PDF 组件文件信息: {error}"))?;
            if metadata.file_type().is_dir() {
                pending.push(entry.path());
            } else if metadata.file_type().is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or_else(|| "现有 PDF 组件体积计算溢出。".to_string())?;
            }
        }
    }
    Ok(total)
}

async fn extract_pack(
    app: &AppHandle,
    archive: &Path,
    layout: &Pdf2zhLayout,
    profile: &Pdf2zhProfile,
    limits: ArchiveLimits,
    custom_pack: bool,
    cancel: &Arc<AtomicBool>,
) -> Result<
    (
        Pdf2zhEngineCapabilities,
        ArchiveStats,
        PackInstallTransaction,
    ),
    String,
> {
    if cancel.load(Ordering::SeqCst) {
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }
    let staging = layout.root_dir.join("extract-staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| format!("无法创建解压目录: {error}"))?;
    let _staging_cleanup = DirectoryCleanup::new(staging.clone());
    let archive_path = archive.to_path_buf();
    let staging_path = staging.clone();
    let archive_filename = profile.pack_filename.to_string();
    let extraction_cancel = Arc::clone(cancel);
    let stats = tokio::task::spawn_blocking(move || {
        extract_archive_with_limits(
            &archive_path,
            &staging_path,
            &archive_filename,
            limits,
            &extraction_cancel,
        )
    })
    .await
    .map_err(|error| format!("PDF 版面处理组件解压任务失败: {error}"))??;

    if !custom_pack {
        if let Some(expected) = profile.pack_unpacked_size_bytes {
            if stats.unpacked_size_bytes != expected {
                return Err(format!(
                    "PDF 版面处理组件解压体积不匹配（预期 {expected}，实际 {}）。",
                    stats.unpacked_size_bytes
                ));
            }
        }
        if let Some(expected) = profile.pack_file_count {
            if stats.file_count != expected {
                return Err(format!(
                    "PDF 版面处理组件文件数不匹配（预期 {expected}，实际 {}）。",
                    stats.file_count
                ));
            }
        }
    }

    let candidate = if staging.join(profile.pack_directory_name).is_dir() {
        staging.join(profile.pack_directory_name)
    } else {
        staging.clone()
    };
    let bin = candidate.join(profile.bin_relative_path);
    if !bin.is_file() {
        return Err(format!(
            "PDF 版面处理组件结构不正确，缺少 {}",
            profile.bin_relative_path
        ));
    }
    let model = candidate.join("models").join(DOCLAYOUT_MODEL_FILENAME);
    if !model.is_file() {
        return Err(format!(
            "PDF 版面处理组件结构不正确，缺少 models/{DOCLAYOUT_MODEL_FILENAME}"
        ));
    }
    let engine_capabilities = read_pack_engine_capabilities(&candidate).map_err(|error| {
        format!("PDF 版面处理组件需要更新：{error}。请安装当前版本的 PDF 组件。")
    })?;

    if cancel.load(Ordering::SeqCst) {
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }

    if layout.pack_dir.exists() {
        if super::worker::stop_worker_for_install(app).await {
            eprintln!("[pdf2zh-install] stopped warm worker before replacing PDF component");
        }
        let cleaned =
            super::worker::cleanup_stale_workers_for_install(&layout.root_dir, &layout.pack_dir)
                .await?;
        if cleaned > 0 {
            eprintln!("[pdf2zh-install] stopped {cleaned} stale PDF worker process(es)");
        }
    }
    let transaction = PackInstallTransaction::activate(&candidate, &layout.pack_dir)?;
    Ok((engine_capabilities, stats, transaction))
}

struct DirectoryCleanup {
    path: PathBuf,
}

struct FileCleanup {
    path: PathBuf,
    armed: bool,
}

impl FileCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for FileCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl DirectoryCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for DirectoryCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct PackInstallTransaction {
    pack_dir: PathBuf,
    backup_dir: Option<PathBuf>,
    committed: bool,
}

struct ManifestInstallTransaction {
    manifest_path: PathBuf,
    backup_path: Option<PathBuf>,
    committed: bool,
}

impl ManifestInstallTransaction {
    fn replace(manifest_path: &Path, contents: &[u8]) -> Result<Self, String> {
        let parent = manifest_path
            .parent()
            .ok_or_else(|| "PDF 组件安装记录缺少父目录。".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 PDF 组件安装记录目录: {error}"))?;
        let suffix = transaction_suffix();
        let temporary_path = parent.join(format!(".installed.json.tmp-{suffix}"));
        let mut temporary_cleanup = FileCleanup::new(temporary_path.clone());
        let mut temporary = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|error| format!("无法创建 PDF 组件安装记录临时文件: {error}"))?;
        temporary
            .write_all(contents)
            .map_err(|error| format!("无法写入 PDF 组件安装记录临时文件: {error}"))?;
        temporary
            .sync_all()
            .map_err(|error| format!("无法持久化 PDF 组件安装记录临时文件: {error}"))?;
        drop(temporary);

        let backup_path = if manifest_path.exists() {
            let backup = parent.join(format!(".installed.json.previous-{suffix}"));
            std::fs::rename(manifest_path, &backup)
                .map_err(|error| format!("无法保留当前 PDF 组件安装记录用于回滚: {error}"))?;
            Some(backup)
        } else {
            None
        };

        if let Err(error) = std::fs::rename(&temporary_path, manifest_path) {
            if let Some(backup) = backup_path.as_ref() {
                if let Err(restore_error) = std::fs::rename(backup, manifest_path) {
                    return Err(format!(
                        "无法激活 PDF 组件安装记录: {error}；旧安装记录回滚失败: {restore_error}"
                    ));
                }
            }
            return Err(format!("无法激活 PDF 组件安装记录: {error}"));
        }
        temporary_cleanup.disarm();

        Ok(Self {
            manifest_path: manifest_path.to_path_buf(),
            backup_path,
            committed: false,
        })
    }

    fn commit(mut self) {
        self.committed = true;
        if let Some(backup) = self.backup_path.take() {
            if let Err(error) = std::fs::remove_file(&backup) {
                eprintln!(
                    "[pdf2zh-install] installed manifest is ready but old manifest cleanup failed: {error}"
                );
            }
        }
    }
}

impl Drop for ManifestInstallTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = std::fs::remove_file(&self.manifest_path);
        if let Some(backup) = self.backup_path.as_ref() {
            if let Err(error) = std::fs::rename(backup, &self.manifest_path) {
                eprintln!(
                    "[pdf2zh-install] failed to restore previous manifest from {}: {error}",
                    backup.display()
                );
            }
        }
    }
}

fn transaction_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

impl PackInstallTransaction {
    fn activate(candidate: &Path, pack_dir: &Path) -> Result<Self, String> {
        let parent = pack_dir
            .parent()
            .ok_or_else(|| "PDF 组件目录缺少父目录。".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| format!("无法创建组件目录: {error}"))?;
        let directory_name = pack_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pack");
        let backup_dir = if pack_dir.exists() {
            let backup = parent.join(format!(
                ".{directory_name}.previous-{}-{}",
                std::process::id(),
                timestamp_ms_string()
            ));
            std::fs::rename(pack_dir, &backup)
                .map_err(|error| format!("无法保留当前 PDF 组件用于回滚: {error}"))?;
            Some(backup)
        } else {
            None
        };

        if let Err(error) = std::fs::rename(candidate, pack_dir) {
            if let Some(backup) = backup_dir.as_ref() {
                let _ = std::fs::rename(backup, pack_dir);
            }
            return Err(format!("无法激活新 PDF 版面处理组件: {error}"));
        }

        Ok(Self {
            pack_dir: pack_dir.to_path_buf(),
            backup_dir,
            committed: false,
        })
    }

    async fn commit(mut self) {
        self.committed = true;
        if let Some(backup) = self.backup_dir.take() {
            if let Err(error) = remove_pack_dir(&backup).await {
                eprintln!("[pdf2zh-install] installed pack is ready but old backup cleanup failed: {error}");
            }
        }
    }
}

impl Drop for PackInstallTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        #[cfg(target_os = "windows")]
        clear_readonly_attrs(&self.pack_dir);
        let _ = std::fs::remove_dir_all(&self.pack_dir);
        if let Some(backup) = self.backup_dir.as_ref() {
            if let Err(error) = std::fs::rename(backup, &self.pack_dir) {
                eprintln!(
                    "[pdf2zh-install] failed to restore previous pack from {}: {error}",
                    backup.display()
                );
            }
        }
    }
}

async fn remove_pack_dir(pack_dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        clear_readonly_attrs(pack_dir);
        for attempt in 0..=PACK_DIR_DELETE_RETRY_COUNT {
            match std::fs::remove_dir_all(pack_dir) {
                Ok(()) => return Ok(()),
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound || !pack_dir.exists() =>
                {
                    return Ok(());
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::PermissionDenied
                        && attempt < PACK_DIR_DELETE_RETRY_COUNT =>
                {
                    eprintln!(
                        "[pdf2zh-install] pack directory still locked; retrying delete ({}/{})",
                        attempt + 1,
                        PACK_DIR_DELETE_RETRY_COUNT
                    );
                    clear_readonly_attrs(pack_dir);
                    tokio::time::sleep(PACK_DIR_DELETE_RETRY_DELAY).await;
                }
                Err(error) => {
                    return Err(format!(
                        "无法清理旧 PDF 版面处理组件 {}: {error}",
                        pack_dir.display()
                    ));
                }
            }
        }
        unreachable!("pack directory delete retry loop must return");
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::fs::remove_dir_all(pack_dir).map_err(|error| {
            format!(
                "无法清理旧 PDF 版面处理组件 {}: {error}",
                pack_dir.display()
            )
        })
    }
}

#[cfg(target_os = "windows")]
fn clear_readonly_attrs(path: &Path) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
    }

    if !metadata.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        clear_readonly_attrs(&entry.path());
    }
}

#[derive(Default)]
struct ArchivePlan {
    stats: ArchiveStats,
    seen_paths: HashSet<PathBuf>,
    regular_paths: HashSet<PathBuf>,
    non_link_paths: Vec<PathBuf>,
    symlink_targets: HashMap<PathBuf, PathBuf>,
    hardlink_targets: HashMap<PathBuf, PathBuf>,
}

impl ArchivePlan {
    fn register_path(&mut self, path: PathBuf) -> Result<(), String> {
        if !self.seen_paths.insert(path.clone()) {
            return Err(format!("archive 包含重复路径: {}", path.display()));
        }
        Ok(())
    }

    fn register_file(
        &mut self,
        path: PathBuf,
        size: u64,
        limits: ArchiveLimits,
    ) -> Result<(), String> {
        self.register_path(path.clone())?;
        if size > limits.max_single_file_bytes {
            return Err(format!(
                "archive 单文件超过安全上限: {} ({size} > {} bytes)",
                path.display(),
                limits.max_single_file_bytes
            ));
        }
        self.stats.unpacked_size_bytes = self
            .stats
            .unpacked_size_bytes
            .checked_add(size)
            .ok_or_else(|| "archive 解压体积计算溢出。".to_string())?;
        self.stats.file_count = self
            .stats
            .file_count
            .checked_add(1)
            .ok_or_else(|| "archive 文件数计算溢出。".to_string())?;
        self.stats.max_single_file_bytes = self.stats.max_single_file_bytes.max(size);
        if self.stats.unpacked_size_bytes > limits.max_unpacked_bytes {
            return Err(format!(
                "archive 解压体积超过安全上限（{} > {} bytes）。",
                self.stats.unpacked_size_bytes, limits.max_unpacked_bytes
            ));
        }
        if self.stats.file_count > limits.max_file_count {
            return Err(format!(
                "archive 文件数超过安全上限（{} > {}）。",
                self.stats.file_count, limits.max_file_count
            ));
        }
        self.regular_paths.insert(path.clone());
        self.non_link_paths.push(path);
        Ok(())
    }

    fn register_directory(&mut self, path: PathBuf) -> Result<(), String> {
        self.register_path(path.clone())?;
        self.non_link_paths.push(path);
        Ok(())
    }

    fn register_symlink(
        &mut self,
        path: PathBuf,
        target: PathBuf,
        limits: ArchiveLimits,
    ) -> Result<(), String> {
        self.register_path(path.clone())?;
        self.stats.symlink_count = self
            .stats
            .symlink_count
            .checked_add(1)
            .ok_or_else(|| "archive symlink 数计算溢出。".to_string())?;
        if self.stats.symlink_count > limits.max_symlink_count {
            return Err(format!(
                "archive symlink 数超过安全上限（{} > {}）。",
                self.stats.symlink_count, limits.max_symlink_count
            ));
        }
        validate_link_target(&path, &target, true)?;
        self.symlink_targets.insert(path, target);
        Ok(())
    }

    fn register_hardlink(
        &mut self,
        path: PathBuf,
        target: PathBuf,
        limits: ArchiveLimits,
    ) -> Result<(), String> {
        self.register_file(path.clone(), 0, limits)?;
        validate_link_target(&path, &target, false)?;
        self.hardlink_targets.insert(path, target);
        Ok(())
    }

    fn finish(self) -> Result<Self, String> {
        for path in &self.non_link_paths {
            if has_archive_symlink_ancestor(path, &self.symlink_targets) {
                return Err(format!(
                    "archive 路径经过 symlink，拒绝解压: {}",
                    path.display()
                ));
            }
        }
        for (path, target) in &self.hardlink_targets {
            if self.symlink_targets.contains_key(target)
                || (!self.regular_paths.contains(target)
                    && !self.hardlink_targets.contains_key(target))
            {
                return Err(format!(
                    "archive hardlink 目标不是受信任的普通文件: {} -> {}",
                    path.display(),
                    target.display()
                ));
            }
        }
        Ok(self)
    }
}

fn extract_archive_with_limits(
    archive_path: &Path,
    destination: &Path,
    archive_filename: &str,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<ArchiveStats, String> {
    if archive_filename.ends_with(".zip") {
        let plan = scan_zip_archive(archive_path, limits, cancel)?;
        extract_zip_archive(archive_path, destination, &plan, limits, cancel)?;
        Ok(plan.stats)
    } else if archive_filename.ends_with(".tar.gz") || archive_filename.ends_with(".tgz") {
        let plan = scan_tar_gz_archive(archive_path, limits, cancel)?;
        extract_tar_gz_archive(archive_path, destination, &plan, limits, cancel)?;
        Ok(plan.stats)
    } else {
        Err(format!(
            "不支持的 PDF 组件 archive 格式: {archive_filename}"
        ))
    }
}

fn scan_zip_archive(
    archive_path: &Path,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<ArchivePlan, String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 ZIP: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("无法读取 ZIP: {error}"))?;
    let mut plan = ArchivePlan::default();
    for index in 0..archive.len() {
        ensure_extraction_not_cancelled(cancel)?;
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("无法读取 ZIP 目录项: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP 包含越界路径: {}", entry.name()))?;
        let relative = safe_archive_path(&relative)?;
        if zip_entry_is_symlink(entry.unix_mode()) {
            return Err(format!("ZIP 包含不允许的 symlink: {}", relative.display()));
        }
        if entry.is_dir() {
            plan.register_directory(relative)?;
        } else {
            plan.register_file(relative, entry.size(), limits)?;
        }
    }
    plan.finish()
}

fn extract_zip_archive(
    archive_path: &Path,
    destination: &Path,
    plan: &ArchivePlan,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 ZIP: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("无法读取 ZIP: {error}"))?;
    let mut actual_total = 0u64;
    for index in 0..archive.len() {
        ensure_extraction_not_cancelled(cancel)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("无法读取 ZIP 目录项: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP 包含越界路径: {}", entry.name()))?;
        let relative = safe_archive_path(&relative)?;
        let output = destination.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| format!("无法创建 ZIP 目录 {}: {error}", output.display()))?;
            continue;
        }
        ensure_parent_directory(&output)?;
        let mut output_file = create_new_archive_file(&output)?;
        let mode = entry.unix_mode();
        let expected_size = entry.size();
        let written = copy_archive_entry(
            &mut entry,
            &mut output_file,
            &relative,
            expected_size,
            &mut actual_total,
            limits,
            cancel,
        )?;
        if written != expected_size {
            return Err(format!(
                "ZIP 文件大小不匹配: {}（声明 {}，实际 {written}）。",
                relative.display(),
                expected_size
            ));
        }
        apply_archive_mode(&output, mode)?;
    }
    if actual_total != plan.stats.unpacked_size_bytes {
        return Err("ZIP 实际解压体积与预检结果不一致。".to_string());
    }
    Ok(())
}

fn scan_tar_gz_archive(
    archive_path: &Path,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<ArchivePlan, String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 tar.gz: {error}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(CancellationReader::new(decoder, cancel));
    let entries = archive
        .entries()
        .map_err(|error| format!("无法读取 tar.gz 目录: {error}"))?;
    let mut plan = ArchivePlan::default();
    for entry in entries {
        ensure_extraction_not_cancelled(cancel)?;
        let entry = entry.map_err(|error| format!("无法读取 tar.gz 目录项: {error}"))?;
        let entry_type = entry.header().entry_type();
        if is_tar_metadata_entry(entry_type.as_byte()) {
            if entry.size() > MAX_TAR_METADATA_BYTES {
                return Err(format!(
                    "tar.gz metadata 超过安全上限（{} > {MAX_TAR_METADATA_BYTES} bytes）。",
                    entry.size()
                ));
            }
            continue;
        }
        let relative = safe_archive_path(
            &entry
                .path()
                .map_err(|error| format!("tar.gz 路径无法解析: {error}"))?,
        )?;
        if entry_type.is_dir() {
            ensure_empty_tar_link_or_directory(&entry, &relative)?;
            plan.register_directory(relative)?;
        } else if entry_type.is_file() {
            plan.register_file(relative, entry.size(), limits)?;
        } else if entry_type.is_symlink() {
            ensure_empty_tar_link_or_directory(&entry, &relative)?;
            let target = entry
                .link_name()
                .map_err(|error| format!("tar.gz symlink 无法解析: {error}"))?
                .ok_or_else(|| format!("tar.gz symlink 缺少目标: {}", relative.display()))?
                .into_owned();
            plan.register_symlink(relative, target, limits)?;
        } else if entry_type.is_hard_link() {
            ensure_empty_tar_link_or_directory(&entry, &relative)?;
            let target = entry
                .link_name()
                .map_err(|error| format!("tar.gz hardlink 无法解析: {error}"))?
                .ok_or_else(|| format!("tar.gz hardlink 缺少目标: {}", relative.display()))?
                .into_owned();
            plan.register_hardlink(relative, target, limits)?;
        } else {
            return Err(format!(
                "tar.gz 包含不允许的目录项类型 {}: {}",
                entry_type.as_byte(),
                relative.display()
            ));
        }
    }
    plan.finish()
}

fn extract_tar_gz_archive(
    archive_path: &Path,
    destination: &Path,
    plan: &ArchivePlan,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 tar.gz: {error}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(CancellationReader::new(decoder, cancel));
    let entries = archive
        .entries()
        .map_err(|error| format!("无法读取 tar.gz 目录: {error}"))?;
    let mut actual_total = 0u64;
    let mut hardlinks = Vec::new();
    let mut symlinks = Vec::new();
    for entry in entries {
        ensure_extraction_not_cancelled(cancel)?;
        let mut entry = entry.map_err(|error| format!("无法读取 tar.gz 目录项: {error}"))?;
        let entry_type = entry.header().entry_type();
        if is_tar_metadata_entry(entry_type.as_byte()) {
            continue;
        }
        let relative = safe_archive_path(
            &entry
                .path()
                .map_err(|error| format!("tar.gz 路径无法解析: {error}"))?,
        )?;
        let output = destination.join(&relative);
        if entry_type.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| format!("无法创建 tar.gz 目录 {}: {error}", output.display()))?;
        } else if entry_type.is_file() {
            ensure_parent_directory(&output)?;
            let mut output_file = create_new_archive_file(&output)?;
            let expected_size = entry.size();
            let mode = entry.header().mode().ok();
            let written = copy_archive_entry(
                &mut entry,
                &mut output_file,
                &relative,
                expected_size,
                &mut actual_total,
                limits,
                cancel,
            )?;
            if written != expected_size {
                return Err(format!(
                    "tar.gz 文件大小不匹配: {}（声明 {expected_size}，实际 {written}）。",
                    relative.display()
                ));
            }
            apply_archive_mode(&output, mode)?;
        } else if entry_type.is_hard_link() {
            let target = plan
                .hardlink_targets
                .get(&relative)
                .ok_or_else(|| "tar.gz hardlink 与预检结果不一致。".to_string())?
                .clone();
            hardlinks.push((relative, target));
        } else if entry_type.is_symlink() {
            let target = plan
                .symlink_targets
                .get(&relative)
                .ok_or_else(|| "tar.gz symlink 与预检结果不一致。".to_string())?
                .clone();
            symlinks.push((relative, target));
        }
    }
    if actual_total != plan.stats.unpacked_size_bytes {
        return Err("tar.gz 实际解压体积与预检结果不一致。".to_string());
    }
    create_archive_hardlinks(destination, hardlinks, cancel)?;
    for (path, target) in symlinks {
        ensure_extraction_not_cancelled(cancel)?;
        let output = destination.join(&path);
        ensure_parent_directory(&output)?;
        create_archive_symlink(&target, &output)?;
    }
    Ok(())
}

fn safe_archive_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                #[cfg(target_os = "windows")]
                if value.to_string_lossy().contains(':') {
                    return Err(format!(
                        "archive 路径包含 Windows alternate stream: {}",
                        path.display()
                    ));
                }
                normalized.push(value);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("archive 路径越界: {}", path.display()));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("archive 包含空路径。".to_string());
    }
    Ok(normalized)
}

fn validate_link_target(
    path: &Path,
    target: &Path,
    relative_to_parent: bool,
) -> Result<(), String> {
    if target.is_absolute() {
        return Err(format!(
            "archive link 使用绝对目标: {} -> {}",
            path.display(),
            target.display()
        ));
    }
    let base = if relative_to_parent {
        path.parent().unwrap_or_else(|| Path::new(""))
    } else {
        Path::new("")
    };
    normalize_relative_join(base, target).map(|_| ())
}

fn normalize_relative_join(base: &Path, target: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in base.components().chain(target.components()) {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("archive link 目标越界: {}", target.display()));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("archive link 目标越界: {}", target.display()));
            }
        }
    }
    Ok(normalized)
}

fn has_archive_symlink_ancestor(path: &Path, symlink_targets: &HashMap<PathBuf, PathBuf>) -> bool {
    let mut parent = path.parent();
    while let Some(candidate) = parent {
        if symlink_targets.contains_key(candidate) {
            return true;
        }
        parent = candidate.parent();
    }
    false
}

fn is_tar_metadata_entry(kind: u8) -> bool {
    matches!(kind, b'x' | b'g' | b'L' | b'K')
}

fn ensure_empty_tar_link_or_directory<R: Read>(
    entry: &tar::Entry<'_, R>,
    relative: &Path,
) -> Result<(), String> {
    if entry.size() != 0 {
        return Err(format!(
            "tar.gz 目录或链接条目包含异常数据: {} ({} bytes)",
            relative.display(),
            entry.size()
        ));
    }
    Ok(())
}

struct CancellationReader<'a, R> {
    inner: R,
    cancel: &'a AtomicBool,
}

impl<'a, R> CancellationReader<'a, R> {
    fn new(inner: R, cancel: &'a AtomicBool) -> Self {
        Self { inner, cancel }
    }
}

impl<R: Read> Read for CancellationReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "PDF component extraction cancelled",
            ));
        }
        self.inner.read(buffer)
    }
}

fn zip_entry_is_symlink(unix_mode: Option<u32>) -> bool {
    unix_mode.is_some_and(|mode| mode & 0o170_000 == 0o120_000)
}

fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 archive 目录 {}: {error}", parent.display()))?;
    }
    Ok(())
}

fn create_new_archive_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("无法创建 archive 文件 {}: {error}", path.display()))
}

fn copy_archive_entry<R: Read>(
    reader: &mut R,
    output: &mut File,
    relative: &Path,
    expected_size: u64,
    actual_total: &mut u64,
    limits: ArchiveLimits,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut written = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        ensure_extraction_not_cancelled(cancel)?;
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("读取 archive 文件 {} 失败: {error}", relative.display()))?;
        if read == 0 {
            break;
        }
        written = written
            .checked_add(read as u64)
            .ok_or_else(|| "archive 单文件体积计算溢出。".to_string())?;
        if written > expected_size || written > limits.max_single_file_bytes {
            return Err(format!(
                "archive 单文件超过声明或安全上限: {}",
                relative.display()
            ));
        }
        *actual_total = actual_total
            .checked_add(read as u64)
            .ok_or_else(|| "archive 解压体积计算溢出。".to_string())?;
        if *actual_total > limits.max_unpacked_bytes {
            return Err(format!(
                "archive 实际解压体积超过安全上限（{} > {} bytes）。",
                *actual_total, limits.max_unpacked_bytes
            ));
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("写入 archive 文件 {} 失败: {error}", relative.display()))?;
    }
    output
        .flush()
        .map_err(|error| format!("刷新 archive 文件 {} 失败: {error}", relative.display()))?;
    Ok(written)
}

fn create_archive_hardlinks(
    destination: &Path,
    hardlinks: Vec<(PathBuf, PathBuf)>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut pending = hardlinks;
    while !pending.is_empty() {
        let before = pending.len();
        let mut deferred = Vec::new();
        for (path, target) in pending {
            ensure_extraction_not_cancelled(cancel)?;
            let source = destination.join(&target);
            if !source.is_file() {
                deferred.push((path, target));
                continue;
            }
            let output = destination.join(&path);
            ensure_parent_directory(&output)?;
            std::fs::hard_link(&source, &output).map_err(|error| {
                format!(
                    "无法创建 archive hardlink {} -> {}: {error}",
                    output.display(),
                    source.display()
                )
            })?;
        }
        if deferred.len() == before {
            return Err("archive hardlink 目标链无法解析。".to_string());
        }
        pending = deferred;
    }
    Ok(())
}

#[cfg(unix)]
fn create_archive_symlink(target: &Path, output: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, output).map_err(|error| {
        format!(
            "无法创建 archive symlink {} -> {}: {error}",
            output.display(),
            target.display()
        )
    })
}

#[cfg(not(unix))]
fn create_archive_symlink(_target: &Path, output: &Path) -> Result<(), String> {
    Err(format!(
        "当前平台不允许 PDF 组件 archive symlink: {}",
        output.display()
    ))
}

#[cfg(unix)]
fn apply_archive_mode(path: &Path, mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))
            .map_err(|error| format!("无法设置 archive 文件权限 {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_archive_mode(_path: &Path, _mode: Option<u32>) -> Result<(), String> {
    Ok(())
}

fn ensure_extraction_not_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("PDF 版面处理组件安装已取消。".to_string())
    } else {
        Ok(())
    }
}

fn scrub_python_bytecode(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    scrub_python_bytecode_inner(root, cancel)?;
    Ok(())
}

fn scrub_python_bytecode_inner(dir: &Path, cancel: &AtomicBool) -> Result<(), String> {
    ensure_extraction_not_cancelled(cancel)?;
    for entry in std::fs::read_dir(dir).map_err(|error| format!("无法扫描目录: {error}"))? {
        ensure_extraction_not_cancelled(cancel)?;
        let entry = entry.map_err(|error| format!("无法读取目录项: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法读取目录项类型: {error}"))?;
        if file_type.is_dir() {
            if entry.file_name() == "__pycache__" {
                std::fs::remove_dir_all(&path).map_err(|error| {
                    format!("无法删除 Python bytecode 缓存 {}: {error}", path.display())
                })?;
            } else {
                scrub_python_bytecode_inner(&path, cancel)?;
            }
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "pyc") {
            std::fs::remove_file(&path)
                .map_err(|error| format!("无法删除 Python bytecode {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn write_manifest(
    layout: &Pdf2zhLayout,
    profile: &Pdf2zhProfile,
    source_url: &str,
    size_bytes: Option<u64>,
    sha256: Option<String>,
    custom_pack: bool,
    engine_capabilities: &Pdf2zhEngineCapabilities,
    archive_stats: ArchiveStats,
) -> Result<ManifestInstallTransaction, String> {
    let manifest = Pdf2zhPackManifest {
        schema_version: 2,
        profile_id: profile.id.to_string(),
        pack_filename: profile.pack_filename.to_string(),
        sha256,
        size_bytes,
        unpacked_size_bytes: archive_stats.unpacked_size_bytes,
        file_count: archive_stats.file_count,
        symlink_count: archive_stats.symlink_count,
        max_single_file_bytes: archive_stats.max_single_file_bytes,
        source_url: source_url.to_string(),
        installed_at: timestamp_ms_string(),
        custom_pack,
        engine_capability_schema_version: engine_capabilities.schema_version,
        engine_contract_version: engine_capabilities.engine_contract_version,
        engine_revision: engine_capabilities.engine_revision,
        capabilities: engine_capabilities.capabilities.clone(),
    };
    let contents = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("无法序列化 pdf2zh manifest: {error}"))?;
    ManifestInstallTransaction::replace(&layout.manifest_file, contents.as_bytes())
}

fn effective_urls(
    profile: &Pdf2zhProfile,
    options: &Pdf2zhInstallOptions,
) -> Result<Vec<String>, String> {
    if let Some(url) = options
        .pack_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(vec![url.to_string()]);
    }
    if let Ok(url) = std::env::var("ROSETTA_PDF2ZH_PACK_URL") {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return Ok(vec![url]);
        }
    }
    if profile.pack_download_urls.is_empty() {
        return Err(
            "尚未配置 PDF 版面处理组件下载地址。可先运行本地 staging 脚本，或设置 ROSETTA_PDF2ZH_PACK_URL 指向 .tar.gz。".to_string(),
        );
    }
    Ok(profile
        .pack_download_urls
        .iter()
        .map(|url| url.to_string())
        .collect())
}

fn has_custom_pack_url(options: &Pdf2zhInstallOptions) -> bool {
    options
        .pack_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
        || std::env::var("ROSETTA_PDF2ZH_PACK_URL")
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn effective_sha(profile: &Pdf2zhProfile, options: &Pdf2zhInstallOptions) -> Option<String> {
    let custom_sha = options
        .pack_sha256
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("ROSETTA_PDF2ZH_PACK_SHA256")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    if has_custom_pack_url(options) {
        custom_sha
    } else {
        custom_sha.or_else(|| profile.pack_sha256.map(str::to_string))
    }
}

fn effective_size(profile: &Pdf2zhProfile, options: &Pdf2zhInstallOptions) -> Option<u64> {
    let custom_size = options.pack_size_bytes.or_else(|| {
        std::env::var("ROSETTA_PDF2ZH_PACK_SIZE_BYTES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
    });
    if has_custom_pack_url(options) {
        custom_size
    } else {
        custom_size.or(profile.pack_size_bytes)
    }
}

async fn set_done(registry: &Pdf2zhInstallRegistry, message: String) {
    update_progress(registry, |progress| {
        progress.phase = Pdf2zhInstallPhase::Done;
        progress.speed_bytes_per_sec = 0;
        progress.message = message;
        progress.last_error = None;
    })
    .await;
}

async fn set_cancelled(registry: &Pdf2zhInstallRegistry) {
    update_progress(registry, |progress| {
        progress.phase = Pdf2zhInstallPhase::Cancelled;
        progress.speed_bytes_per_sec = 0;
        progress.message = "PDF 版面处理组件安装已取消。".to_string();
    })
    .await;
}

async fn set_failed(registry: &Pdf2zhInstallRegistry, message: String) {
    update_progress(registry, |progress| {
        progress.phase = Pdf2zhInstallPhase::Failed;
        progress.speed_bytes_per_sec = 0;
        progress.last_error = Some(message.clone());
        progress.message = message;
    })
    .await;
}

async fn update_progress<F>(registry: &Pdf2zhInstallRegistry, f: F)
where
    F: FnOnce(&mut Pdf2zhInstallProgress),
{
    let mut guard = registry.inner.lock().await;
    let progress = guard
        .progress
        .get_or_insert_with(Pdf2zhInstallProgress::idle);
    f(progress);
}

async fn emit_progress(app: &AppHandle, registry: &Pdf2zhInstallRegistry) {
    let progress = registry.snapshot().await;
    let _ = app.emit(PROGRESS_EVENT_NAME, progress);
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn timestamp_ms_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Cursor, Read, Write},
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use flate2::{write::GzEncoder, Compression};
    use zip::write::SimpleFileOptions;

    use super::{
        checked_download_size, copy_archive_entry, effective_sha, effective_size,
        extract_archive_with_limits, ArchiveLimits, DirectoryCleanup, FileCleanup,
        ManifestInstallTransaction, PackInstallTransaction, Pdf2zhInstallOptions,
    };
    use crate::managed_pdf2zh::profile::WINDOWS_AMD64_PDF2ZH;

    #[test]
    fn custom_pack_url_does_not_reuse_profile_hash_or_size() {
        let options = Pdf2zhInstallOptions {
            pack_url: Some("file://C:\\tmp\\rosetta-pdf2zh-windows-amd64.zip".to_string()),
            ..Default::default()
        };

        assert_eq!(effective_sha(&WINDOWS_AMD64_PDF2ZH, &options), None);
        assert_eq!(effective_size(&WINDOWS_AMD64_PDF2ZH, &options), None);
    }

    #[test]
    fn custom_pack_url_uses_explicit_hash_and_size_when_provided() {
        let options = Pdf2zhInstallOptions {
            pack_url: Some("file://C:\\tmp\\rosetta-pdf2zh-windows-amd64.zip".to_string()),
            pack_sha256: Some("abc123".to_string()),
            pack_size_bytes: Some(42),
            ..Default::default()
        };

        assert_eq!(
            effective_sha(&WINDOWS_AMD64_PDF2ZH, &options).as_deref(),
            Some("abc123")
        );
        assert_eq!(effective_size(&WINDOWS_AMD64_PDF2ZH, &options), Some(42));
    }

    #[test]
    fn profile_pack_url_uses_profile_hash_and_size() {
        let options = Pdf2zhInstallOptions::default();

        assert_eq!(
            effective_sha(&WINDOWS_AMD64_PDF2ZH, &options).as_deref(),
            WINDOWS_AMD64_PDF2ZH.pack_sha256
        );
        assert_eq!(
            effective_size(&WINDOWS_AMD64_PDF2ZH, &options),
            WINDOWS_AMD64_PDF2ZH.pack_size_bytes
        );
    }

    #[test]
    fn oversized_download_chunk_is_rejected_before_write() {
        let root = unique_temp_root("download-limit");
        std::fs::create_dir_all(&root).expect("create temp root");
        let part = root.join("pack.part");
        std::fs::write(&part, b"1234").expect("write partial");
        let cleanup = FileCleanup::new(part.clone());

        let error = checked_download_size(4, 3, 6).expect_err("chunk must exceed limit");

        assert!(error.contains("超过安全上限"));
        assert_eq!(std::fs::metadata(&part).expect("partial metadata").len(), 4);
        drop(cleanup);
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn zip_path_escape_is_rejected_without_writing_outside_staging() {
        let root = unique_temp_root("zip-path-escape");
        let archive = root.join("malicious.zip");
        let destination = root.join("staging");
        std::fs::create_dir_all(&destination).expect("create staging");
        write_zip(&archive, &[("../escaped.txt", b"escape")]);

        let error = extract_archive_with_limits(
            &archive,
            &destination,
            "test.zip",
            generous_test_limits(),
            &AtomicBool::new(false),
        )
        .expect_err("path escape must fail");

        assert!(error.contains("越界路径") || error.contains("路径越界"));
        assert!(!root.join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn zip_unpacked_bytes_and_file_count_are_bounded() {
        let root = unique_temp_root("zip-quotas");
        let archive = root.join("quota.zip");
        std::fs::create_dir_all(&root).expect("create temp root");
        write_zip(
            &archive,
            &[("pack/a.txt", b"1234"), ("pack/b.txt", b"5678")],
        );

        let byte_error = extract_archive_with_limits(
            &archive,
            &root.join("bytes-staging"),
            "test.zip",
            ArchiveLimits {
                max_unpacked_bytes: 7,
                ..generous_test_limits()
            },
            &AtomicBool::new(false),
        )
        .expect_err("unpacked byte quota must fail");
        assert!(byte_error.contains("解压体积超过安全上限"));

        let count_error = extract_archive_with_limits(
            &archive,
            &root.join("count-staging"),
            "test.zip",
            ArchiveLimits {
                max_file_count: 1,
                ..generous_test_limits()
            },
            &AtomicBool::new(false),
        )
        .expect_err("file count quota must fail");
        assert!(count_error.contains("文件数超过安全上限"));

        let single_file_error = extract_archive_with_limits(
            &archive,
            &root.join("single-file-staging"),
            "test.zip",
            ArchiveLimits {
                max_single_file_bytes: 3,
                ..generous_test_limits()
            },
            &AtomicBool::new(false),
        )
        .expect_err("single file quota must fail");
        assert!(single_file_error.contains("单文件超过安全上限"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_zip_extraction_reports_trusted_stats() {
        let root = unique_temp_root("zip-success");
        let archive = root.join("pack.zip");
        let destination = root.join("staging");
        std::fs::create_dir_all(&root).expect("create temp root");
        write_zip(
            &archive,
            &[("pack/a.txt", b"1234"), ("pack/b.txt", b"5678")],
        );

        let stats = extract_archive_with_limits(
            &archive,
            &destination,
            "test.zip",
            generous_test_limits(),
            &AtomicBool::new(false),
        )
        .expect("bounded extraction succeeds");

        assert_eq!(stats.unpacked_size_bytes, 8);
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.max_single_file_bytes, 4);
        assert_eq!(
            std::fs::read(destination.join("pack/a.txt")).unwrap(),
            b"1234"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tar_symlink_escape_is_rejected() {
        let root = unique_temp_root("tar-symlink-escape");
        let archive_path = root.join("malicious.tar.gz");
        let destination = root.join("staging");
        std::fs::create_dir_all(&destination).expect("create staging");
        let file = File::create(&archive_path).expect("create tar.gz");
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        header
            .set_link_name("../../outside")
            .expect("set malicious target");
        header.set_cksum();
        builder
            .append_data(&mut header, "pack/link", Cursor::new(Vec::<u8>::new()))
            .expect("append symlink");
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");

        let error = extract_archive_with_limits(
            &archive_path,
            &destination,
            "test.tar.gz",
            generous_test_limits(),
            &AtomicBool::new(false),
        )
        .expect_err("escaping symlink must fail");

        assert!(error.contains("link 目标越界"));
        assert!(!root.join("outside").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_tar_gz_extraction_reports_trusted_stats() {
        let root = unique_temp_root("tar-success");
        let archive_path = root.join("pack.tar.gz");
        let destination = root.join("staging");
        std::fs::create_dir_all(&root).expect("create temp root");
        let file = File::create(&archive_path).expect("create tar.gz");
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let contents = b"bounded tar extraction";
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(contents.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "pack/data.txt", Cursor::new(contents))
            .expect("append tar file");
        builder
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip");

        let stats = extract_archive_with_limits(
            &archive_path,
            &destination,
            "test.tar.gz",
            generous_test_limits(),
            &AtomicBool::new(false),
        )
        .expect("bounded tar extraction succeeds");

        assert_eq!(stats.unpacked_size_bytes, contents.len() as u64);
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.max_single_file_bytes, contents.len() as u64);
        assert_eq!(
            std::fs::read(destination.join("pack/data.txt")).unwrap(),
            contents
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn extraction_copy_observes_cancellation_between_chunks() {
        let root = unique_temp_root("extract-cancel");
        std::fs::create_dir_all(&root).expect("create temp root");
        let staging = root.join("staging");
        std::fs::create_dir_all(&staging).expect("create staging");
        let cleanup = DirectoryCleanup::new(staging.clone());
        let output_path = staging.join("partial.bin");
        let mut output = File::create(&output_path).expect("create output");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut reader = CancellingReader {
            cursor: Cursor::new(vec![7u8; 128 * 1024]),
            cancel: Arc::clone(&cancel),
            cancelled: false,
        };
        let mut actual_total = 0;

        let error = copy_archive_entry(
            &mut reader,
            &mut output,
            Path::new("pack/large.bin"),
            128 * 1024,
            &mut actual_total,
            generous_test_limits(),
            &cancel,
        )
        .expect_err("copy must observe cancellation");

        assert!(error.contains("已取消"));
        assert!(actual_total < 128 * 1024);
        drop(output);
        drop(cleanup);
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn pack_transaction_supports_fresh_and_upgrade_installs() {
        let root = unique_temp_root("pack-transaction-success");
        let live = root.join("pack").join("platform");
        let fresh = root.join("fresh");
        std::fs::create_dir_all(&fresh).expect("create fresh candidate");
        std::fs::write(fresh.join("marker"), b"fresh").expect("write fresh marker");

        PackInstallTransaction::activate(&fresh, &live)
            .expect("activate fresh install")
            .commit()
            .await;
        assert_eq!(
            std::fs::read(live.join("marker")).expect("read fresh"),
            b"fresh"
        );

        let upgrade = root.join("upgrade");
        std::fs::create_dir_all(&upgrade).expect("create upgrade candidate");
        std::fs::write(upgrade.join("marker"), b"upgrade").expect("write upgrade marker");
        PackInstallTransaction::activate(&upgrade, &live)
            .expect("activate upgrade")
            .commit()
            .await;
        assert_eq!(
            std::fs::read(live.join("marker")).expect("read upgrade"),
            b"upgrade"
        );
        assert_eq!(
            std::fs::read_dir(live.parent().expect("pack parent"))
                .expect("read pack parent")
                .count(),
            1,
            "committed upgrade removes old backup"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_upgrade_restores_previous_pack() {
        let root = unique_temp_root("pack-transaction-rollback");
        let live = root.join("pack").join("platform");
        let candidate = root.join("candidate");
        std::fs::create_dir_all(&live).expect("create live pack");
        std::fs::create_dir_all(&candidate).expect("create candidate");
        std::fs::write(live.join("marker"), b"old").expect("write old marker");
        std::fs::write(candidate.join("marker"), b"new").expect("write new marker");

        let transaction =
            PackInstallTransaction::activate(&candidate, &live).expect("activate candidate");
        assert_eq!(
            std::fs::read(live.join("marker")).expect("read new"),
            b"new"
        );
        drop(transaction);

        assert_eq!(
            std::fs::read(live.join("marker")).expect("read restored old"),
            b"old"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_replacement_rolls_back_until_committed() {
        let root = unique_temp_root("manifest-transaction");
        let manifest = root.join("installed.json");
        std::fs::create_dir_all(&root).expect("create manifest root");
        std::fs::write(&manifest, b"old").expect("write old manifest");

        let transaction = ManifestInstallTransaction::replace(&manifest, b"new")
            .expect("replace manifest transactionally");
        assert_eq!(std::fs::read(&manifest).expect("read new manifest"), b"new");
        drop(transaction);
        assert_eq!(
            std::fs::read(&manifest).expect("read restored manifest"),
            b"old"
        );

        ManifestInstallTransaction::replace(&manifest, b"committed")
            .expect("replace manifest for commit")
            .commit();
        assert_eq!(
            std::fs::read(&manifest).expect("read committed manifest"),
            b"committed"
        );
        assert_eq!(
            std::fs::read_dir(&root)
                .expect("read manifest root")
                .count(),
            1,
            "committed manifest leaves no backup or temporary file"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    struct CancellingReader {
        cursor: Cursor<Vec<u8>>,
        cancel: Arc<AtomicBool>,
        cancelled: bool,
    }

    impl Read for CancellingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.cursor.read(buffer)?;
            if read > 0 && !self.cancelled {
                self.cancel.store(true, Ordering::SeqCst);
                self.cancelled = true;
            }
            Ok(read)
        }
    }

    fn generous_test_limits() -> ArchiveLimits {
        ArchiveLimits {
            max_unpacked_bytes: 1024 * 1024,
            max_file_count: 100,
            max_symlink_count: 10,
            max_single_file_bytes: 1024 * 1024,
        }
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create zip parent");
        }
        let file = File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in files {
            writer.start_file(*name, options).expect("start zip file");
            writer.write_all(contents).expect("write zip file");
        }
        writer.finish().expect("finish zip");
    }

    fn unique_temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rosetta-pdf2zh-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
