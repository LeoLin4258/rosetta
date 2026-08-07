use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::read::GzDecoder;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tar::Archive;
use tokio::io::AsyncWriteExt;
use zip::ZipArchive;

use super::{
    layout::{InstalledManifest, PdfMarkdownLayout, MANIFEST_SCHEMA},
    profile::{PdfMarkdownProfile, PYMUPDF4LLM_VERSION, PYMUPDF_LAYOUT_VERSION, PYMUPDF_VERSION},
};

const MAX_ARCHIVE_BYTES: u64 = 400 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 160 * 1024 * 1024;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownInstallOptions {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownInstallResult {
    pub ready: bool,
    pub profile_id: String,
    pub archive_bytes: u64,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownInstallProgress {
    pub state: String,
    pub downloaded_bytes: u64,
    pub expected_bytes: u64,
}

pub struct PdfMarkdownInstallRegistry {
    cancel: Arc<AtomicBool>,
    state: tokio::sync::Mutex<PdfMarkdownInstallProgress>,
    operation: tokio::sync::Mutex<()>,
}

impl Default for PdfMarkdownInstallRegistry {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            state: tokio::sync::Mutex::new(PdfMarkdownInstallProgress {
                state: "idle".into(),
                downloaded_bytes: 0,
                expected_bytes: 0,
            }),
            operation: tokio::sync::Mutex::new(()),
        }
    }
}

impl PdfMarkdownInstallRegistry {
    pub async fn snapshot(&self) -> PdfMarkdownInstallProgress {
        self.state.lock().await.clone()
    }
    pub async fn request_cancel(&self) -> bool {
        let state = self.state.lock().await;
        if state.state == "installing" {
            self.cancel.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
    async fn set(&self, state: PdfMarkdownInstallProgress) {
        *self.state.lock().await = state;
    }
    fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }
}

pub async fn install_component(
    registry: &PdfMarkdownInstallRegistry,
    layout: &PdfMarkdownLayout,
    profile: &'static PdfMarkdownProfile,
    force: bool,
) -> Result<PdfMarkdownInstallResult, String> {
    let _operation = registry.operation.lock().await;
    layout.ensure_dirs()?;
    if !force && layout.validate_install(profile).is_ok() {
        return Ok(result(profile, "PDF Markdown 组件已就绪。"));
    }
    registry.reset_cancel();
    registry
        .set(PdfMarkdownInstallProgress {
            state: "installing".into(),
            downloaded_bytes: 0,
            expected_bytes: profile.archive_bytes,
        })
        .await;
    let archive = layout.downloads_dir.join(profile.archive_filename);
    let part = layout
        .downloads_dir
        .join(format!(".{}.part", profile.archive_filename));
    let outcome = async {
        if let Ok(path) = std::env::var("ROSETTA_PDF_MARKDOWN_COMPONENT_ARCHIVE") {
            copy_local_archive(
                Path::new(&path),
                &part,
                profile.archive_bytes,
                &registry.cancel,
            )
            .await?;
        } else {
            download_archive(&part, profile, registry).await?;
        }
        if registry.cancel.load(Ordering::SeqCst) {
            return Err("component installation cancelled".to_string());
        }
        verify_archive(&part, profile)?;
        replace_file(&part, &archive, "download")?;
        install_archive(&archive, layout, profile, &registry.cancel)?;
        Ok(result(profile, "PDF Markdown 组件安装完成。"))
    }
    .await;
    if outcome.is_err() {
        let _ = fs::remove_file(&part);
        registry
            .set(PdfMarkdownInstallProgress {
                state: if registry.cancel.load(Ordering::SeqCst) {
                    "cancelled".into()
                } else {
                    "failed".into()
                },
                downloaded_bytes: 0,
                expected_bytes: profile.archive_bytes,
            })
            .await;
    } else {
        registry
            .set(PdfMarkdownInstallProgress {
                state: "ready".into(),
                downloaded_bytes: profile.archive_bytes,
                expected_bytes: profile.archive_bytes,
            })
            .await;
    }
    outcome
}

fn result(profile: &'static PdfMarkdownProfile, message: &str) -> PdfMarkdownInstallResult {
    PdfMarkdownInstallResult {
        ready: true,
        profile_id: profile.id.to_string(),
        archive_bytes: profile.archive_bytes,
        message: message.to_string(),
    }
}

async fn copy_local_archive(
    source: &Path,
    destination: &Path,
    expected: u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if !source.is_file() {
        return Err("component archive is unavailable".into());
    }
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|_| "component archive is unavailable".to_string())?;
    let mut output = tokio::fs::File::create(destination)
        .await
        .map_err(|_| "unable to stage component archive".to_string())?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut total = 0_u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("component installation cancelled".into());
        }
        let read = tokio::io::AsyncReadExt::read(&mut input, &mut buffer)
            .await
            .map_err(|_| "unable to read component archive".to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "component archive is too large".to_string())?;
        if total > MAX_ARCHIVE_BYTES || (expected > 0 && total > expected) {
            return Err("component archive exceeds its size limit".into());
        }
        output
            .write_all(&buffer[..read])
            .await
            .map_err(|_| "unable to stage component archive".to_string())?;
    }
    output
        .flush()
        .await
        .map_err(|_| "unable to stage component archive".to_string())?;
    if total != expected {
        return Err("component archive size does not match its profile".into());
    }
    Ok(())
}

async fn download_archive(
    destination: &Path,
    profile: &'static PdfMarkdownProfile,
    registry: &PdfMarkdownInstallRegistry,
) -> Result<(), String> {
    let client = Client::builder()
        .no_proxy()
        .build()
        .map_err(|_| "unable to prepare component download".to_string())?;
    let mut last_error = "component download failed".to_string();
    for url in profile.download_urls {
        let response = match client.get(*url).send().await {
            Ok(response) => response,
            Err(_) => {
                last_error = "component download failed".into();
                continue;
            }
        };
        if !response.status().is_success() {
            last_error = "component download failed".into();
            continue;
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_ARCHIVE_BYTES || n != profile.archive_bytes)
        {
            last_error = "component archive exceeds its size limit".into();
            continue;
        }
        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|_| "unable to stage component archive".to_string())?;
        let mut total = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
            if registry.cancel.load(Ordering::SeqCst) {
                return Err("component installation cancelled".into());
            }
            let chunk = chunk.map_err(|_| "component download failed".to_string())?;
            total = total
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "component archive is too large".to_string())?;
            if total > MAX_ARCHIVE_BYTES || total > profile.archive_bytes {
                return Err("component archive exceeds its size limit".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| "unable to stage component archive".to_string())?;
            registry
                .set(PdfMarkdownInstallProgress {
                    state: "installing".into(),
                    downloaded_bytes: total,
                    expected_bytes: profile.archive_bytes,
                })
                .await;
        }
        file.flush()
            .await
            .map_err(|_| "unable to stage component archive".to_string())?;
        if total == profile.archive_bytes {
            return Ok(());
        }
        last_error = "component archive size does not match its profile".into();
    }
    Err(last_error)
}

fn verify_archive(path: &Path, profile: &PdfMarkdownProfile) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|_| "component archive is unavailable".to_string())?;
    if metadata.len() != profile.archive_bytes || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("component archive size does not match its profile".into());
    }
    let mut file = File::open(path).map_err(|_| "component archive is unavailable".to_string())?;
    let mut digest = sha2::Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "unable to verify component archive".to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if format!("{:x}", digest.finalize()) != profile.archive_sha256 {
        return Err("component archive checksum does not match its profile".into());
    }
    Ok(())
}

fn install_archive(
    archive: &Path,
    layout: &PdfMarkdownLayout,
    profile: &PdfMarkdownProfile,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = layout
        .component_dir
        .with_file_name(format!(".{}-install-{stamp}", profile.directory_name));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)
        .map_err(|_| "unable to create component staging directory".to_string())?;
    let extraction = if archive.extension().and_then(|s| s.to_str()) == Some("zip") {
        extract_zip(archive, &staging, cancel)
    } else {
        extract_tar(archive, &staging, cancel)
    };
    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let (unpacked, count) = tree_stats(&staging)?;
    if unpacked != profile.unpacked_bytes || count != profile.file_count {
        let _ = fs::remove_dir_all(&staging);
        return Err("component unpacked inventory does not match its profile".into());
    }
    for relative in ["pymupdf4llm", "pymupdf"] {
        if !staging.join(relative).is_dir() {
            let _ = fs::remove_dir_all(&staging);
            return Err("component package files are incomplete".into());
        }
    }
    if !staging.join("pymupdf_layout-1.28.0.dist-info").is_dir() {
        let _ = fs::remove_dir_all(&staging);
        return Err("component package metadata is incomplete".into());
    }
    let manifest = InstalledManifest {
        schema: MANIFEST_SCHEMA.into(),
        profile_id: profile.id.into(),
        archive_filename: profile.archive_filename.into(),
        archive_sha256: profile.archive_sha256.into(),
        archive_bytes: profile.archive_bytes,
        unpacked_bytes: unpacked,
        file_count: count,
        pymupdf4llm: PYMUPDF4LLM_VERSION.into(),
        pymupdf_layout: PYMUPDF_LAYOUT_VERSION.into(),
        pymupdf: PYMUPDF_VERSION.into(),
        cpu_only: true,
        integration_boundary: "to_json".into(),
    };
    atomic_json(&staging.join("manifest.json"), &manifest)?;
    let backup = layout
        .component_dir
        .with_file_name(format!(".{}-previous", profile.directory_name));
    let _ = fs::remove_dir_all(&backup);
    if layout.component_dir.exists() {
        fs::rename(&layout.component_dir, &backup)
            .map_err(|_| "unable to stage component replacement".to_string())?;
    }
    if let Err(error) = fs::rename(&staging, &layout.component_dir) {
        if backup.exists() {
            let _ = fs::rename(&backup, &layout.component_dir);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("unable to commit component: {error}"));
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let temp = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("manifest"),
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "unable to serialize component manifest".to_string())?;
    fs::write(&temp, bytes).map_err(|_| "unable to write component manifest".to_string())?;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&temp)
        .map_err(|_| "unable to flush component manifest".to_string())?;
    file.sync_all()
        .map_err(|_| "unable to flush component manifest".to_string())?;
    replace_file(&temp, path, "manifest")
}

fn replace_file(staged: &Path, destination: &Path, label: &str) -> Result<(), String> {
    let backup = destination.with_file_name(format!(
        ".{}-previous",
        destination
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(label)
    ));
    let _ = fs::remove_file(&backup);
    if destination.exists() {
        fs::rename(destination, &backup)
            .map_err(|_| format!("unable to stage {label} replacement"))?;
    }
    if let Err(error) = fs::rename(staged, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        return Err(format!("unable to commit {label}: {error}"));
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

fn safe_member(path: &Path) -> bool {
    path.is_relative() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn stripped_member(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(root)) if root == "pdf-markdown-overlay" => {
            let rest = components.collect::<PathBuf>();
            (!rest.as_os_str().is_empty()).then_some(rest)
        }
        _ => Some(path.to_path_buf()),
    }
}

fn extract_zip(archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let file = File::open(archive).map_err(|_| "unable to open component archive".to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|_| "component archive is invalid".to_string())?;
    for index in 0..zip.len() {
        if cancel.load(Ordering::SeqCst) {
            return Err("component installation cancelled".into());
        }
        let mut entry = zip
            .by_index(index)
            .map_err(|_| "component archive is invalid".to_string())?;
        let raw_name = Path::new(entry.name());
        if !safe_member(raw_name) {
            return Err("component archive contains an unsafe path".into());
        }
        let Some(name) = stripped_member(raw_name) else {
            if entry.is_dir() {
                continue;
            }
            return Err("component archive contains an invalid root".into());
        };
        let target = destination.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|_| "unable to unpack component".to_string())?;
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("component archive contains a link".into());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|_| "unable to unpack component".to_string())?;
        }
        let mut output =
            File::create(&target).map_err(|_| "unable to unpack component".to_string())?;
        copy_with_cancel(&mut entry, &mut output, cancel)?;
    }
    Ok(())
}

fn extract_tar(archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let file = File::open(archive).map_err(|_| "unable to open component archive".to_string())?;
    let decoder = GzDecoder::new(file);
    let mut tar = Archive::new(decoder);
    for entry in tar
        .entries()
        .map_err(|_| "component archive is invalid".to_string())?
    {
        if cancel.load(Ordering::SeqCst) {
            return Err("component installation cancelled".into());
        }
        let mut entry = entry.map_err(|_| "component archive is invalid".to_string())?;
        let raw_path = entry
            .path()
            .map_err(|_| "component archive is invalid".to_string())?
            .into_owned();
        if !safe_member(&raw_path) {
            return Err("component archive contains an unsafe entry".into());
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err("component archive contains an unsafe entry".into());
        }
        let path = stripped_member(&raw_path)
            .ok_or_else(|| "component archive contains an invalid root".to_string())?;
        let target = destination.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|_| "unable to unpack component".to_string())?;
        }
        let mut output =
            File::create(&target).map_err(|_| "unable to unpack component".to_string())?;
        copy_with_cancel(&mut entry, &mut output, cancel)?;
    }
    Ok(())
}

fn copy_with_cancel(
    input: &mut impl Read,
    output: &mut File,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("component installation cancelled".into());
        }
        let read = input
            .read(&mut buffer)
            .map_err(|_| "unable to unpack component".to_string())?;
        if read == 0 {
            return Ok(());
        }
        std::io::Write::write_all(output, &buffer[..read])
            .map_err(|_| "unable to unpack component".to_string())?;
    }
}

fn tree_stats(root: &Path) -> Result<(u64, u64), String> {
    let mut bytes = 0_u64;
    let mut count = 0_u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).map_err(|_| "unable to inspect component".to_string())? {
            let entry = entry.map_err(|_| "unable to inspect component".to_string())?;
            let ty = entry
                .file_type()
                .map_err(|_| "unable to inspect component".to_string())?;
            if ty.is_symlink() {
                return Err("component contains a link".into());
            }
            if ty.is_dir() {
                stack.push(entry.path());
            } else if ty.is_file() {
                count += 1;
                bytes = bytes
                    .checked_add(
                        entry
                            .metadata()
                            .map_err(|_| "unable to inspect component".to_string())?
                            .len(),
                    )
                    .ok_or_else(|| "component is too large".to_string())?;
                if bytes > MAX_UNPACKED_BYTES {
                    return Err("component exceeds its unpacked size limit".into());
                }
            }
        }
    }
    Ok((bytes, count))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rosetta-pdf-markdown-install-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixture_archive(root: &Path, unsafe_name: bool) -> (PathBuf, PdfMarkdownProfile) {
        let archive = root.join("fixture.zip");
        let file = File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let files: [(&str, &[u8]); 3] = if unsafe_name {
            [
                ("../escape", b"x"),
                ("pdf-markdown-overlay/pymupdf/__init__.py", b"p"),
                (
                    "pdf-markdown-overlay/pymupdf_layout-1.28.0.dist-info/METADATA",
                    b"m",
                ),
            ]
        } else {
            [
                ("pdf-markdown-overlay/pymupdf4llm/__init__.py", b"l"),
                ("pdf-markdown-overlay/pymupdf/__init__.py", b"p"),
                (
                    "pdf-markdown-overlay/pymupdf_layout-1.28.0.dist-info/METADATA",
                    b"m",
                ),
            ]
        };
        for (name, contents) in files {
            zip.start_file(name, options).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
        let archive_bytes = fs::metadata(&archive).unwrap().len();
        let mut digest = sha2::Sha256::new();
        digest.update(fs::read(&archive).unwrap());
        let sha: &'static str = Box::leak(format!("{:x}", digest.finalize()).into_boxed_str());
        (
            archive,
            PdfMarkdownProfile {
                id: "fixture",
                platform_os: "windows",
                platform_arch: "x86_64",
                directory_name: "fixture",
                archive_filename: "fixture.zip",
                archive_bytes,
                unpacked_bytes: 3,
                file_count: 3,
                archive_sha256: sha,
                download_urls: &[],
            },
        )
    }

    pub(crate) fn install_fixture(layout: &PdfMarkdownLayout, profile: &PdfMarkdownProfile) {
        let files = [
            layout.component_dir.join("pymupdf4llm/__init__.py"),
            layout.component_dir.join("pymupdf/__init__.py"),
            layout
                .component_dir
                .join("pymupdf_layout-1.28.0.dist-info/METADATA"),
        ];
        for file in files {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"x").unwrap();
        }
        let manifest = InstalledManifest {
            schema: MANIFEST_SCHEMA.into(),
            profile_id: profile.id.into(),
            archive_filename: profile.archive_filename.into(),
            archive_sha256: profile.archive_sha256.into(),
            archive_bytes: profile.archive_bytes,
            unpacked_bytes: profile.unpacked_bytes,
            file_count: profile.file_count,
            pymupdf4llm: PYMUPDF4LLM_VERSION.into(),
            pymupdf_layout: PYMUPDF_LAYOUT_VERSION.into(),
            pymupdf: PYMUPDF_VERSION.into(),
            cpu_only: true,
            integration_boundary: "to_json".into(),
        };
        fs::create_dir_all(&layout.component_dir).unwrap();
        atomic_json(&layout.manifest_file, &manifest).unwrap();
    }

    #[test]
    fn archive_member_validation_rejects_traversal() {
        assert!(!safe_member(Path::new("../escape")));
        assert!(!safe_member(Path::new("/absolute")));
        assert!(safe_member(Path::new("pymupdf4llm/__init__.py")));
    }

    #[test]
    fn profile_inventory_is_below_defensive_limits() {
        assert!(
            crate::managed_pdf_markdown::profile::WINDOWS_X64.unpacked_bytes < MAX_UNPACKED_BYTES
        );
    }

    #[test]
    fn install_and_repair_commit_only_complete_overlay() {
        let root = temp_root("commit-repair");
        let (archive, profile) = fixture_archive(&root, false);
        let layout = PdfMarkdownLayout::resolve(root.join("app-data"), &profile);
        layout.ensure_dirs().unwrap();
        install_archive(&archive, &layout, &profile, &AtomicBool::new(false)).unwrap();
        layout.validate_install(&profile).unwrap();
        fs::remove_dir_all(layout.component_dir.join("pymupdf4llm")).unwrap();
        assert!(layout.validate_install(&profile).is_err());
        install_archive(&archive, &layout, &profile, &AtomicBool::new(false)).unwrap();
        layout.validate_install(&profile).unwrap();
        let manifest = fs::read_to_string(&layout.manifest_file).unwrap();
        assert!(!manifest.contains(root.to_string_lossy().as_ref()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_archive_does_not_replace_last_good_install() {
        let root = temp_root("rollback");
        let (archive, profile) = fixture_archive(&root, false);
        let layout = PdfMarkdownLayout::resolve(root.join("app-data"), &profile);
        layout.ensure_dirs().unwrap();
        install_archive(&archive, &layout, &profile, &AtomicBool::new(false)).unwrap();
        let previous = fs::read(&layout.manifest_file).unwrap();
        let unsafe_root = root.join("unsafe");
        fs::create_dir_all(&unsafe_root).unwrap();
        let (unsafe_archive, unsafe_profile) = fixture_archive(&unsafe_root, true);
        assert!(install_archive(
            &unsafe_archive,
            &layout,
            &unsafe_profile,
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(fs::read(&layout.manifest_file).unwrap(), previous);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_extraction_does_not_replace_last_good_install() {
        let root = temp_root("cancel-rollback");
        let (archive, profile) = fixture_archive(&root, false);
        let layout = PdfMarkdownLayout::resolve(root.join("app-data"), &profile);
        layout.ensure_dirs().unwrap();
        install_archive(&archive, &layout, &profile, &AtomicBool::new(false)).unwrap();
        let previous = fs::read(&layout.manifest_file).unwrap();
        assert!(install_archive(&archive, &layout, &profile, &AtomicBool::new(true)).is_err());
        assert_eq!(fs::read(&layout.manifest_file).unwrap(), previous);
        layout.validate_install(&profile).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires ROSETTA_PDF_MARKDOWN_TEST_ARCHIVE with the exact native release artifact"]
    fn exact_native_release_archive_installs_and_reopens_offline() {
        let archive = PathBuf::from(
            std::env::var("ROSETTA_PDF_MARKDOWN_TEST_ARCHIVE").expect("release archive path"),
        );
        let profile =
            crate::managed_pdf_markdown::profile::current_profile().expect("native profile");
        verify_archive(&archive, profile).unwrap();
        let root = temp_root("native-release");
        let layout = PdfMarkdownLayout::resolve(root.join("app-data"), profile);
        layout.ensure_dirs().unwrap();
        install_archive(&archive, &layout, profile, &AtomicBool::new(false)).unwrap();
        layout.validate_install(profile).unwrap();
        let reopened = PdfMarkdownLayout::resolve(root.join("app-data"), profile);
        reopened.validate_install(profile).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
