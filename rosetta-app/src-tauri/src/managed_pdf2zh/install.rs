use std::{
    path::Path,
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
    layout::{Pdf2zhLayout, DOCLAYOUT_MODEL_FILENAME},
    profile::Pdf2zhProfile,
};
use crate::windows_process::HideConsole;

const PROGRESS_EVENT_NAME: &str = "managed-pdf2zh://install-progress";
const PROGRESS_EMIT_INTERVAL_MS: u128 = 100;
const STREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
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
    source_url: String,
    installed_at: String,
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
    let archive_path = layout.downloads_dir.join(profile.pack_filename);
    let part_path = layout
        .downloads_dir
        .join(format!("{}.part", profile.pack_filename));

    if options.repair {
        let _ = std::fs::remove_file(&archive_path);
        let _ = std::fs::remove_file(&part_path);
    }

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

    extract_pack(app, &archive_path, layout, profile, cancel).await?;
    scrub_python_bytecode(&layout.pack_dir)?;
    write_manifest(
        layout,
        profile,
        &source_url,
        expected_size,
        Some(actual_sha),
    )?;

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
    stream_response_to_file(app, registry, response, target, expected_size, cancel).await
}

async fn stream_response_to_file(
    app: &AppHandle,
    registry: &Pdf2zhInstallRegistry,
    response: reqwest::Response,
    target: &Path,
    expected_size: Option<u64>,
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
        file.write_all(&bytes)
            .await
            .map_err(|error| format!("写入 pack 文件失败: {error}"))?;
        bytes_done += bytes.len() as u64;
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
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        set_cancelled(registry).await;
        emit_progress(app, registry).await;
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("无法创建下载目录: {error}"))?;
    }
    std::fs::copy(source, target)
        .map_err(|error| format!("复制本地 PDF 版面处理组件失败: {error}"))?;
    let bytes = std::fs::metadata(target)
        .map_err(|error| format!("无法读取本地 PDF 版面处理组件: {error}"))?
        .len();
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

async fn extract_pack(
    app: &AppHandle,
    archive: &Path,
    layout: &Pdf2zhLayout,
    profile: &Pdf2zhProfile,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("PDF 版面处理组件安装已取消。".to_string());
    }
    let staging = layout.root_dir.join("extract-staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| format!("无法创建解压目录: {error}"))?;
    if profile.pack_filename.ends_with(".zip") {
        extract_zip(archive, &staging)
            .map_err(|error| format!("解压 PDF 版面处理 ZIP 失败: {error}"))?;
    } else {
        let status = tokio::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .hide_console_on_windows()
            .status()
            .await
            .map_err(|error| format!("启动 tar 解压失败: {error}"))?;
        if !status.success() {
            return Err(format!("解压 PDF 版面处理组件失败: {status}"));
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
        remove_pack_dir(&layout.pack_dir).await?;
    }
    if let Some(parent) = layout.pack_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("无法创建组件目录: {error}"))?;
    }
    match std::fs::rename(&candidate, &layout.pack_dir) {
        Ok(()) => {}
        Err(_) => {
            copy_dir_all(&candidate, &layout.pack_dir)?;
            std::fs::remove_dir_all(&candidate).ok();
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
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

fn extract_zip(archive_path: &Path, destination: &Path) -> std::io::Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let output = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(output)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

fn copy_dir_all(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|error| format!("无法创建目录: {error}"))?;
    for entry in std::fs::read_dir(source).map_err(|error| format!("无法读取目录: {error}"))?
    {
        let entry = entry.map_err(|error| format!("无法读取目录项: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法读取目录项类型: {error}"))?;
        let dst = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dst)?;
        } else {
            let source_path = entry.path();
            std::fs::copy(&source_path, &dst).map_err(|error| format!("复制文件失败: {error}"))?;
            if let Ok(metadata) = std::fs::metadata(&source_path) {
                let _ = std::fs::set_permissions(&dst, metadata.permissions());
            }
        }
    }
    Ok(())
}

fn scrub_python_bytecode(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    scrub_python_bytecode_inner(root)?;
    Ok(())
}

fn scrub_python_bytecode_inner(dir: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|error| format!("无法扫描目录: {error}"))? {
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
                scrub_python_bytecode_inner(&path)?;
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
) -> Result<(), String> {
    let manifest = Pdf2zhPackManifest {
        schema_version: 1,
        profile_id: profile.id.to_string(),
        pack_filename: profile.pack_filename.to_string(),
        sha256,
        size_bytes,
        source_url: source_url.to_string(),
        installed_at: timestamp_ms_string(),
    };
    let contents = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("无法序列化 pdf2zh manifest: {error}"))?;
    std::fs::write(&layout.manifest_file, contents)
        .map_err(|error| format!("无法写入 pdf2zh manifest: {error}"))
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
    use super::{effective_sha, effective_size, Pdf2zhInstallOptions};
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
}
