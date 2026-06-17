use std::{
    fs,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8000;
const EXPECTED_RUNTIME_ID_PREFIX: &str = "rwkv-lightning-";
const RUNTIME_ARTIFACT_ID: &str = "rwkv-lightning-libtorch-cu132-windows-amd64";
const RUNTIME_ARTIFACT_FILENAME: &str =
    "rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip";
const RUNTIME_ARTIFACT_SIZE_BYTES: u64 = 1_321_825_122;
const RUNTIME_ARTIFACT_SHA256: &str =
    "e4957c0dc771ea949d24f1d15123848dc2243546db62f4928c695c799c99e881";
const RUNTIME_ARTIFACT_DOWNLOAD_URL: &str = "https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip";
const RUNTIME_ARTIFACT_SOURCE_PAGE: &str =
    "https://www.modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/files";
const RUNTIME_EXTRACTED_DIR: &str = "runtime-bundle";
const RUNTIME_EXECUTABLE_FILENAME: &str = "rwkv_lightning.exe";
const RUNTIME_VOCAB_FILENAME: &str = "rwkv_vocab_v20230424.txt";
const EXPECTED_MODEL_ID: &str = "rwkv-v7-g1-translate-1.5b";
const EXPECTED_CONTEXT_TOKENS: u32 = 4096;
const EXPECTED_DIRECTIONS: [&str; 2] = ["en-zh", "zh-en"];
const MODEL_ARTIFACT_FILENAME: &str = "RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth";
const MODEL_ARTIFACT_SIZE_BYTES: u64 = 3_055_445_546;
const MODEL_ARTIFACT_SHA256: &str =
    "b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa";
const MODEL_ARTIFACT_DOWNLOAD_URL: &str = "https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth";
const MODEL_ARTIFACT_SOURCE_PAGE: &str =
    "https://www.modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/files";
const RUNTIME_PID_FILENAME: &str = "rwkv-runtime.pid";
const RUNTIME_PASSWORD_FILENAME: &str = "rwkv-runtime.token";
const RUNTIME_LOG_TAIL_BYTES: u64 = 8 * 1024;
const RUNTIME_PROBE_TIMEOUT: Duration = Duration::from_millis(800);
const RUNTIME_TRANSLATION_PROBE_TEXT: &str = "Hello world!";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeStatus {
    state: RwkvRuntimeState,
    api_url: String,
    compatibility: RwkvRuntimeCompatibility,
    runtime_dir: String,
    model_dir: String,
    logs_dir: String,
    runtime_dir_exists: bool,
    model_dir_exists: bool,
    logs_dir_exists: bool,
    runtime_bundle_dir: String,
    runtime_bundle_exists: bool,
    runtime_executable_path: String,
    runtime_executable_exists: bool,
    runtime_manifest_exists: bool,
    model_manifest_exists: bool,
    runtime_manifest: Option<RwkvArtifactManifest>,
    model_manifest: Option<RwkvArtifactManifest>,
    manifest_error: Option<String>,
    log_file: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeInstallPlan {
    ready: bool,
    items: Vec<RwkvRuntimeInstallPlanItem>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeInstallPlanItem {
    id: String,
    kind: RwkvRuntimeInstallItemKind,
    state: RwkvRuntimeInstallItemState,
    label: String,
    target_dir: String,
    manifest_path: String,
    artifact_path: Option<String>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeInstallProgress {
    state: RwkvRuntimeInstallProgressState,
    items: Vec<RwkvRuntimeInstallProgressItem>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeInstallProgressItem {
    id: String,
    kind: RwkvRuntimeInstallItemKind,
    state: RwkvRuntimeInstallProgressItemState,
    label: String,
    bytes_total: Option<u64>,
    bytes_done: u64,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeArtifactCatalog {
    ready_for_download: bool,
    items: Vec<RwkvRuntimeArtifactCatalogItem>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeArtifactCatalogItem {
    id: String,
    kind: RwkvRuntimeInstallItemKind,
    state: RwkvRuntimeArtifactCatalogItemState,
    label: String,
    target_dir: String,
    manifest_path: String,
    artifact_filename: Option<String>,
    download_url: Option<String>,
    source_page: Option<String>,
    size_bytes: Option<u64>,
    sha256: Option<String>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeArtifactScanResult {
    scanned: bool,
    installed_manifests: Vec<String>,
    errors: Vec<String>,
    plan: RwkvRuntimeInstallPlan,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeExtractionResult {
    extracted: bool,
    target_dir: String,
    executable_path: String,
    files_extracted: usize,
    bytes_extracted: u64,
    plan: RwkvRuntimeInstallPlan,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeCompatibility {
    runtime_backend: String,
    hardware_requirement: String,
    detected_display_adapters: Vec<String>,
    compatible: bool,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeProcessStatus {
    state: RwkvRuntimeProcessState,
    pid: Option<u32>,
    process_running: Option<bool>,
    pid_file: String,
    api_url: String,
    port: u16,
    port_open: bool,
    http_ready: bool,
    http_status_code: Option<u16>,
    log_file: String,
    log_tail: Vec<String>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeStartResult {
    started: bool,
    command: Vec<String>,
    process: RwkvRuntimeProcessStatus,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeTranslationProbeResult {
    ok: bool,
    status_code: Option<u16>,
    response_body_preview: String,
    process: RwkvRuntimeProcessStatus,
    message: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeState {
    NotInstalled,
    Partial,
    Installed,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallItemKind {
    Runtime,
    Model,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallItemState {
    Missing,
    Ready,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallProgressState {
    NotStarted,
    Queued,
    Ready,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallProgressItemState {
    Pending,
    Ready,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeArtifactCatalogItemState {
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeProcessState {
    Stopped,
    Starting,
    Ready,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvArtifactManifest {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    context_tokens: Option<u32>,
    #[serde(default)]
    supported_directions: Option<Vec<String>>,
    #[serde(default)]
    installed_at: Option<String>,
}

struct ExpectedArtifact<'a> {
    kind: RwkvRuntimeInstallItemKind,
    id: &'a str,
    label: &'a str,
    filename: &'a str,
    sha256: &'a str,
    size_bytes: u64,
    base_dir: &'a Path,
    manifest_path: PathBuf,
}

#[tauri::command]
pub fn get_rwkv_runtime_status(app: AppHandle) -> Result<RwkvRuntimeStatus, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_status(runtime_paths(app_data_dir)))
}

#[tauri::command]
pub fn initialize_rwkv_runtime_layout(app: AppHandle) -> Result<RwkvRuntimeStatus, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    let paths = runtime_paths(app_data_dir);

    fs::create_dir_all(&paths.runtime_dir)
        .map_err(|error| format!("Could not create RWKV runtime directory: {error}"))?;
    fs::create_dir_all(&paths.model_dir)
        .map_err(|error| format!("Could not create RWKV model directory: {error}"))?;
    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;

    Ok(build_status(paths))
}

#[tauri::command]
pub fn get_rwkv_runtime_install_plan(app: AppHandle) -> Result<RwkvRuntimeInstallPlan, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_install_plan(runtime_paths(app_data_dir)))
}

#[tauri::command]
pub fn get_rwkv_runtime_artifact_catalog(
    app: AppHandle,
) -> Result<RwkvRuntimeArtifactCatalog, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_artifact_catalog(runtime_paths(app_data_dir)))
}

#[tauri::command]
pub fn prepare_rwkv_runtime_install(app: AppHandle) -> Result<RwkvRuntimeInstallProgress, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    let paths = runtime_paths(app_data_dir);

    fs::create_dir_all(&paths.runtime_dir)
        .map_err(|error| format!("Could not create RWKV runtime directory: {error}"))?;
    fs::create_dir_all(&paths.model_dir)
        .map_err(|error| format!("Could not create RWKV model directory: {error}"))?;
    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;

    Ok(build_install_progress(build_install_plan(paths)))
}

#[tauri::command]
pub fn get_rwkv_runtime_install_progress(
    app: AppHandle,
) -> Result<RwkvRuntimeInstallProgress, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_install_progress(build_install_plan(runtime_paths(
        app_data_dir,
    ))))
}

#[tauri::command]
pub fn scan_rwkv_runtime_artifacts(
    app: AppHandle,
) -> Result<RwkvRuntimeArtifactScanResult, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    let paths = runtime_paths(app_data_dir);

    fs::create_dir_all(&paths.runtime_dir)
        .map_err(|error| format!("Could not create RWKV runtime directory: {error}"))?;
    fs::create_dir_all(&paths.model_dir)
        .map_err(|error| format!("Could not create RWKV model directory: {error}"))?;
    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;

    scan_staged_artifacts(paths)
}

#[tauri::command]
pub fn extract_rwkv_runtime_artifact(
    app: AppHandle,
) -> Result<RwkvRuntimeExtractionResult, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    let paths = runtime_paths(app_data_dir);

    fs::create_dir_all(&paths.runtime_dir)
        .map_err(|error| format!("Could not create RWKV runtime directory: {error}"))?;
    fs::create_dir_all(&paths.model_dir)
        .map_err(|error| format!("Could not create RWKV model directory: {error}"))?;
    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;

    extract_runtime_zip(paths)
}

#[tauri::command]
pub fn get_rwkv_runtime_process_status(app: AppHandle) -> Result<RwkvRuntimeProcessStatus, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_process_status(&runtime_paths(app_data_dir)))
}

#[tauri::command]
pub fn start_rwkv_runtime(app: AppHandle) -> Result<RwkvRuntimeStartResult, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;
    let paths = runtime_paths(app_data_dir);

    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;

    start_runtime_process(paths)
}

#[tauri::command]
pub fn probe_rwkv_runtime_translation(
    app: AppHandle,
) -> Result<RwkvRuntimeTranslationProbeResult, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;
    let paths = runtime_paths(app_data_dir);

    probe_runtime_translation(paths)
}

struct RwkvRuntimePaths {
    runtime_dir: PathBuf,
    model_dir: PathBuf,
    logs_dir: PathBuf,
    log_file: PathBuf,
    pid_file: PathBuf,
    password_file: PathBuf,
}

fn runtime_paths(app_data_dir: PathBuf) -> RwkvRuntimePaths {
    let runtime_dir = app_data_dir.join("runtime").join("rwkv-lightning");
    let model_dir = app_data_dir
        .join("models")
        .join("rwkv-v7-g1-translate")
        .join("1.5b");
    let logs_dir = app_data_dir.join("logs");
    let log_file = logs_dir.join("rwkv-runtime.log");
    let pid_file = logs_dir.join(RUNTIME_PID_FILENAME);
    let password_file = logs_dir.join(RUNTIME_PASSWORD_FILENAME);

    RwkvRuntimePaths {
        runtime_dir,
        model_dir,
        logs_dir,
        log_file,
        pid_file,
        password_file,
    }
}

fn build_status(paths: RwkvRuntimePaths) -> RwkvRuntimeStatus {
    let runtime_manifest_path = paths.runtime_dir.join("runtime-manifest.json");
    let model_manifest_path = paths.model_dir.join("model-manifest.json");

    let runtime_dir_exists = paths.runtime_dir.is_dir();
    let model_dir_exists = paths.model_dir.is_dir();
    let logs_dir_exists = paths.logs_dir.is_dir();
    let runtime_bundle_dir = paths.runtime_dir.join(RUNTIME_EXTRACTED_DIR);
    let runtime_executable_path = runtime_bundle_dir.join(RUNTIME_EXECUTABLE_FILENAME);
    let runtime_bundle_exists = runtime_bundle_dir.is_dir();
    let runtime_executable_exists = runtime_executable_path.is_file();
    let runtime_manifest_exists = runtime_manifest_path.is_file();
    let model_manifest_exists = model_manifest_path.is_file();
    let runtime_manifest = read_manifest(&runtime_manifest_path);
    let model_manifest = read_manifest(&model_manifest_path);

    let parse_error = runtime_manifest
        .as_ref()
        .err()
        .or_else(|| model_manifest.as_ref().err())
        .map(ToString::to_string);
    let runtime_manifest = runtime_manifest.ok().flatten();
    let model_manifest = model_manifest.ok().flatten();
    let validation_error =
        validate_available_manifests(runtime_manifest.as_ref(), model_manifest.as_ref(), &paths)
            .err();
    let manifest_error = parse_error.or(validation_error);

    let has_any_layout = runtime_dir_exists
        || model_dir_exists
        || logs_dir_exists
        || runtime_manifest_exists
        || model_manifest_exists;

    let state = match (
        manifest_error.is_some(),
        runtime_manifest.is_some(),
        model_manifest.is_some(),
        has_any_layout,
    ) {
        (true, _, _, _) => RwkvRuntimeState::Invalid,
        (false, true, true, _) => RwkvRuntimeState::Installed,
        (false, _, _, true) => RwkvRuntimeState::Partial,
        (false, _, _, false) => RwkvRuntimeState::NotInstalled,
    };

    let message = match state {
        RwkvRuntimeState::Invalid => "本地 RWKV manifest 无法读取，请检查运行时文件。".to_string(),
        RwkvRuntimeState::Installed => "本地 RWKV 运行时文件已就绪。".to_string(),
        RwkvRuntimeState::Partial => "本地 RWKV 目录已准备，但运行时或模型尚未安装。".to_string(),
        RwkvRuntimeState::NotInstalled => "尚未准备托管 RWKV 运行时目录。".to_string(),
    };

    RwkvRuntimeStatus {
        state,
        api_url: format!("http://{DEFAULT_HOST}:{DEFAULT_PORT}"),
        compatibility: runtime_compatibility(runtime_manifest.as_ref()),
        runtime_dir: display_path(&paths.runtime_dir),
        model_dir: display_path(&paths.model_dir),
        logs_dir: display_path(&paths.logs_dir),
        runtime_dir_exists,
        model_dir_exists,
        logs_dir_exists,
        runtime_bundle_dir: display_path(&runtime_bundle_dir),
        runtime_bundle_exists,
        runtime_executable_path: display_path(&runtime_executable_path),
        runtime_executable_exists,
        runtime_manifest_exists,
        model_manifest_exists,
        runtime_manifest,
        model_manifest,
        manifest_error,
        log_file: display_path(&paths.log_file),
        message,
    }
}

fn build_install_plan(paths: RwkvRuntimePaths) -> RwkvRuntimeInstallPlan {
    let runtime_item = build_install_item(
        RwkvRuntimeInstallItemKind::Runtime,
        "RWKV Lightning runtime",
        "rwkv-lightning",
        &paths.runtime_dir,
        "runtime-manifest.json",
        |manifest| validate_runtime_manifest(manifest, &paths),
    );
    let model_item = build_install_item(
        RwkvRuntimeInstallItemKind::Model,
        "RWKV v7 G1 Translate 1.5B",
        EXPECTED_MODEL_ID,
        &paths.model_dir,
        "model-manifest.json",
        |manifest| validate_model_manifest(manifest, &paths),
    );
    let items = vec![runtime_item, model_item];
    let ready = items
        .iter()
        .all(|item| item.state == RwkvRuntimeInstallItemState::Ready);
    let message = if ready {
        "本地 RWKV 安装计划已满足。".to_string()
    } else {
        "本地 RWKV 仍需要准备运行时或模型文件。".to_string()
    };

    RwkvRuntimeInstallPlan {
        ready,
        items,
        message,
    }
}

fn build_artifact_catalog(paths: RwkvRuntimePaths) -> RwkvRuntimeArtifactCatalog {
    let items = vec![
        RwkvRuntimeArtifactCatalogItem {
            id: RUNTIME_ARTIFACT_ID.to_string(),
            kind: RwkvRuntimeInstallItemKind::Runtime,
            state: RwkvRuntimeArtifactCatalogItemState::Ready,
            label: "RWKV Lightning runtime".to_string(),
            target_dir: display_path(&paths.runtime_dir),
            manifest_path: display_path(&paths.runtime_dir.join("runtime-manifest.json")),
            artifact_filename: Some(RUNTIME_ARTIFACT_FILENAME.to_string()),
            download_url: Some(RUNTIME_ARTIFACT_DOWNLOAD_URL.to_string()),
            source_page: Some(RUNTIME_ARTIFACT_SOURCE_PAGE.to_string()),
            size_bytes: Some(RUNTIME_ARTIFACT_SIZE_BYTES),
            sha256: Some(RUNTIME_ARTIFACT_SHA256.to_string()),
            message: "已通过 ModelScope metadata 确认 Windows amd64 runtime 包。".to_string(),
        },
        RwkvRuntimeArtifactCatalogItem {
            id: EXPECTED_MODEL_ID.to_string(),
            kind: RwkvRuntimeInstallItemKind::Model,
            state: RwkvRuntimeArtifactCatalogItemState::Ready,
            label: "RWKV v7 G1 Translate 1.5B".to_string(),
            target_dir: display_path(&paths.model_dir),
            manifest_path: display_path(&paths.model_dir.join("model-manifest.json")),
            artifact_filename: Some(MODEL_ARTIFACT_FILENAME.to_string()),
            download_url: Some(MODEL_ARTIFACT_DOWNLOAD_URL.to_string()),
            source_page: Some(MODEL_ARTIFACT_SOURCE_PAGE.to_string()),
            size_bytes: Some(MODEL_ARTIFACT_SIZE_BYTES),
            sha256: Some(MODEL_ARTIFACT_SHA256.to_string()),
            message: "已通过 ModelScope metadata 确认，且与 Hugging Face mirror hash 一致。"
                .to_string(),
        },
    ];
    let ready_for_download = items
        .iter()
        .all(|item| item.state == RwkvRuntimeArtifactCatalogItemState::Ready);
    let message = if ready_for_download {
        "本地 RWKV artifact catalog 已可用于下载。".to_string()
    } else {
        "本地 RWKV artifact catalog 仍缺少可下载 metadata。".to_string()
    };

    RwkvRuntimeArtifactCatalog {
        ready_for_download,
        items,
        message,
    }
}

fn scan_staged_artifacts(paths: RwkvRuntimePaths) -> Result<RwkvRuntimeArtifactScanResult, String> {
    let runtime_manifest_path = paths.runtime_dir.join("runtime-manifest.json");
    let model_manifest_path = paths.model_dir.join("model-manifest.json");
    let expected_artifacts = [
        ExpectedArtifact {
            kind: RwkvRuntimeInstallItemKind::Runtime,
            id: RUNTIME_ARTIFACT_ID,
            label: "RWKV Lightning runtime",
            filename: RUNTIME_ARTIFACT_FILENAME,
            sha256: RUNTIME_ARTIFACT_SHA256,
            size_bytes: RUNTIME_ARTIFACT_SIZE_BYTES,
            base_dir: &paths.runtime_dir,
            manifest_path: runtime_manifest_path,
        },
        ExpectedArtifact {
            kind: RwkvRuntimeInstallItemKind::Model,
            id: EXPECTED_MODEL_ID,
            label: "RWKV v7 G1 Translate 1.5B",
            filename: MODEL_ARTIFACT_FILENAME,
            sha256: MODEL_ARTIFACT_SHA256,
            size_bytes: MODEL_ARTIFACT_SIZE_BYTES,
            base_dir: &paths.model_dir,
            manifest_path: model_manifest_path,
        },
    ];
    let mut installed_manifests = Vec::new();
    let mut errors = Vec::new();

    for artifact in expected_artifacts {
        match scan_expected_artifact(&artifact) {
            Ok(Some(manifest_path)) => installed_manifests.push(display_path(&manifest_path)),
            Ok(None) => {}
            Err(error) => errors.push(error),
        }
    }

    let plan = build_install_plan(paths);
    let message = match (installed_manifests.is_empty(), errors.is_empty()) {
        (true, true) => "未发现已放入管理目录的 RWKV artifact。".to_string(),
        (false, true) => "已为通过校验的 RWKV artifact 写入 manifest。".to_string(),
        (true, false) => "发现 RWKV artifact，但校验未通过。".to_string(),
        (false, false) => "部分 RWKV artifact 已写入 manifest，部分校验失败。".to_string(),
    };

    Ok(RwkvRuntimeArtifactScanResult {
        scanned: true,
        installed_manifests,
        errors,
        plan,
        message,
    })
}

fn scan_expected_artifact(artifact: &ExpectedArtifact<'_>) -> Result<Option<PathBuf>, String> {
    let artifact_path = artifact.base_dir.join(artifact.filename);

    if !artifact_path.exists() {
        return Ok(None);
    }

    verify_expected_artifact_file(
        artifact.label,
        &artifact_path,
        artifact.size_bytes,
        artifact.sha256,
    )?;

    let manifest = manifest_for_expected_artifact(artifact);
    let manifest_contents = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Could not serialize {} manifest: {error}", artifact.label))?;

    fs::write(&artifact.manifest_path, manifest_contents)
        .map_err(|error| format!("Could not write {} manifest: {error}", artifact.label))?;

    Ok(Some(artifact.manifest_path.clone()))
}

fn manifest_for_expected_artifact(artifact: &ExpectedArtifact<'_>) -> RwkvArtifactManifest {
    let (context_tokens, supported_directions) = match artifact.kind {
        RwkvRuntimeInstallItemKind::Runtime => (None, None),
        RwkvRuntimeInstallItemKind::Model => (
            Some(EXPECTED_CONTEXT_TOKENS),
            Some(
                EXPECTED_DIRECTIONS
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
        ),
    };

    RwkvArtifactManifest {
        id: artifact.id.to_string(),
        version: None,
        source: Some("modelscope".to_string()),
        filename: Some(artifact.filename.to_string()),
        sha256: Some(artifact.sha256.to_string()),
        size_bytes: Some(artifact.size_bytes),
        context_tokens,
        supported_directions,
        installed_at: None,
    }
}

fn verify_expected_artifact_file(
    label: &str,
    artifact_path: &Path,
    expected_size_bytes: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    let metadata = fs::metadata(artifact_path)
        .map_err(|error| format!("Could not read {label} artifact metadata: {error}"))?;

    if !metadata.is_file() {
        return Err(format!("{label} artifact must be a file."));
    }

    if metadata.len() != expected_size_bytes {
        return Err(format!(
            "{label} artifact size mismatch: expected {expected_size_bytes}, got {}.",
            metadata.len()
        ));
    }

    let actual_sha256 = sha256_file(artifact_path)?;

    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "{label} artifact sha256 mismatch: expected {expected_sha256}, got {actual_sha256}."
        ));
    }

    Ok(())
}

fn extract_runtime_zip(paths: RwkvRuntimePaths) -> Result<RwkvRuntimeExtractionResult, String> {
    extract_runtime_zip_from_artifact(
        paths,
        RUNTIME_ARTIFACT_FILENAME,
        RUNTIME_ARTIFACT_SIZE_BYTES,
    )
}

fn extract_runtime_zip_from_artifact(
    paths: RwkvRuntimePaths,
    runtime_artifact_filename: &str,
    runtime_artifact_size_bytes: u64,
) -> Result<RwkvRuntimeExtractionResult, String> {
    let runtime_zip_path = paths.runtime_dir.join(runtime_artifact_filename);
    let target_dir = paths.runtime_dir.join(RUNTIME_EXTRACTED_DIR);
    let executable_path = target_dir.join(RUNTIME_EXECUTABLE_FILENAME);

    if executable_path.is_file() {
        return Ok(RwkvRuntimeExtractionResult {
            extracted: true,
            target_dir: display_path(&target_dir),
            executable_path: display_path(&executable_path),
            files_extracted: 0,
            bytes_extracted: 0,
            plan: build_install_plan(paths),
            message: "RWKV Lightning runtime 已存在，无需重复解压。".to_string(),
        });
    }

    ensure_runtime_artifact_ready_for_extraction(
        &paths,
        runtime_artifact_filename,
        runtime_artifact_size_bytes,
    )?;

    fs::create_dir_all(&target_dir)
        .map_err(|error| format!("Could not create RWKV runtime extraction directory: {error}"))?;

    let runtime_zip = File::open(&runtime_zip_path)
        .map_err(|error| format!("Could not open RWKV runtime zip: {error}"))?;
    let mut archive = zip::ZipArchive::new(runtime_zip)
        .map_err(|error| format!("Could not read RWKV runtime zip: {error}"))?;
    let mut files_extracted = 0;
    let mut bytes_extracted = 0;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not read RWKV runtime zip entry: {error}"))?;
        let Some(entry_name) = entry.enclosed_name().map(PathBuf::from) else {
            return Err("RWKV runtime zip contains an unsafe entry path.".to_string());
        };
        let Some(output_path) = safe_zip_entry_path(&target_dir, &entry_name) else {
            return Err("RWKV runtime zip contains an unsupported entry path.".to_string());
        };

        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|error| format!("Could not create runtime directory from zip: {error}"))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Could not create runtime file parent directory from zip: {error}")
            })?;
        }

        let mut output_file = File::create(&output_path)
            .map_err(|error| format!("Could not create runtime file from zip: {error}"))?;
        let bytes_written = io::copy(&mut entry, &mut output_file)
            .map_err(|error| format!("Could not extract runtime file from zip: {error}"))?;
        files_extracted += 1;
        bytes_extracted += bytes_written;
    }

    if !executable_path.is_file() {
        return Err(format!(
            "Extracted RWKV runtime is missing `{RUNTIME_EXECUTABLE_FILENAME}`."
        ));
    }

    Ok(RwkvRuntimeExtractionResult {
        extracted: true,
        target_dir: display_path(&target_dir),
        executable_path: display_path(&executable_path),
        files_extracted,
        bytes_extracted,
        plan: build_install_plan(paths),
        message: "RWKV Lightning runtime 已解压。".to_string(),
    })
}

fn ensure_runtime_artifact_ready_for_extraction(
    paths: &RwkvRuntimePaths,
    runtime_artifact_filename: &str,
    runtime_artifact_size_bytes: u64,
) -> Result<(), String> {
    let runtime_manifest_path = paths.runtime_dir.join("runtime-manifest.json");
    let runtime_manifest = read_manifest(&runtime_manifest_path)?;

    match runtime_manifest {
        Some(runtime_manifest) => validate_runtime_manifest(&runtime_manifest, paths),
        None => {
            let runtime_zip_path = paths.runtime_dir.join(runtime_artifact_filename);
            let metadata = fs::metadata(&runtime_zip_path)
                .map_err(|error| format!("Could not read RWKV runtime artifact: {error}"))?;

            if !metadata.is_file() {
                return Err("RWKV runtime artifact must be a file.".to_string());
            }

            if metadata.len() != runtime_artifact_size_bytes {
                return Err(format!(
                    "RWKV runtime artifact size mismatch: expected {runtime_artifact_size_bytes}, got {}.",
                    metadata.len()
                ));
            }

            let expected_artifact = ExpectedArtifact {
                kind: RwkvRuntimeInstallItemKind::Runtime,
                id: RUNTIME_ARTIFACT_ID,
                label: "RWKV Lightning runtime",
                filename: runtime_artifact_filename,
                sha256: RUNTIME_ARTIFACT_SHA256,
                size_bytes: runtime_artifact_size_bytes,
                base_dir: &paths.runtime_dir,
                manifest_path: runtime_manifest_path,
            };
            let manifest = manifest_for_expected_artifact(&expected_artifact);
            let manifest_contents = serde_json::to_string_pretty(&manifest)
                .map_err(|error| format!("Could not serialize RWKV runtime manifest: {error}"))?;

            fs::write(&expected_artifact.manifest_path, manifest_contents)
                .map_err(|error| format!("Could not write RWKV runtime manifest: {error}"))?;

            Ok(())
        }
    }
}

fn safe_zip_entry_path(base_dir: &Path, entry_name: &Path) -> Option<PathBuf> {
    if entry_name.is_absolute()
        || entry_name
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    Some(base_dir.join(entry_name))
}

fn runtime_compatibility(
    runtime_manifest: Option<&RwkvArtifactManifest>,
) -> RwkvRuntimeCompatibility {
    let runtime_backend = runtime_backend(runtime_manifest);
    let detected_display_adapters = detect_display_adapters();
    let has_nvidia = detected_display_adapters
        .iter()
        .any(|adapter| adapter.to_lowercase().contains("nvidia"));

    if runtime_backend == "cuda-nvidia" && !detected_display_adapters.is_empty() && !has_nvidia {
        return RwkvRuntimeCompatibility {
            runtime_backend,
            hardware_requirement: "NVIDIA GPU with CUDA support".to_string(),
            detected_display_adapters,
            compatible: false,
            message: "当前已安装的是 CUDA/NVIDIA RWKV runtime，但这台机器没有检测到 NVIDIA 显卡。请改用 Vulkan/CPU runtime 后再启动。".to_string(),
        };
    }

    if runtime_backend == "cuda-nvidia" && detected_display_adapters.is_empty() {
        return RwkvRuntimeCompatibility {
            runtime_backend,
            hardware_requirement: "NVIDIA GPU with CUDA support".to_string(),
            detected_display_adapters,
            compatible: true,
            message: "当前 runtime 是 CUDA/NVIDIA 包，但 Rosetta 未能读取显示设备信息，启动前请确认本机有 NVIDIA CUDA 显卡。".to_string(),
        };
    }

    let hardware_requirement = runtime_hardware_requirement(&runtime_backend);

    RwkvRuntimeCompatibility {
        runtime_backend,
        hardware_requirement,
        detected_display_adapters,
        compatible: true,
        message: "当前 runtime 与已检测硬件未发现明确冲突。".to_string(),
    }
}

fn runtime_hardware_requirement(runtime_backend: &str) -> String {
    match runtime_backend {
        "cuda-nvidia" => "NVIDIA GPU with CUDA support".to_string(),
        _ => "Unknown runtime package requirements".to_string(),
    }
}

fn runtime_backend(runtime_manifest: Option<&RwkvArtifactManifest>) -> String {
    let Some(runtime_manifest) = runtime_manifest else {
        return "unknown".to_string();
    };

    let id = runtime_manifest.id.to_lowercase();
    let filename = runtime_manifest
        .filename
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();

    if id.contains("cuda") || id.contains("cu132") || filename.contains("+cu") {
        "cuda-nvidia".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(windows)]
fn detect_display_adapters() -> Vec<String> {
    let mut command = Command::new("pnputil");
    command.args(["/enum-devices", "/class", "Display"]);
    configure_runtime_command(&mut command);
    let output = command.output();

    match output {
        Ok(output) if output.status.success() => {
            parse_display_adapters(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
fn detect_display_adapters() -> Vec<String> {
    Vec::new()
}

fn parse_display_adapters(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_once("Device Description:"))
        .map(|(_, name)| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn start_runtime_process(paths: RwkvRuntimePaths) -> Result<RwkvRuntimeStartResult, String> {
    let plan = build_install_plan(runtime_paths_from_paths(&paths));

    if !plan.ready {
        return Err("本地 RWKV runtime/model 尚未就绪，不能启动。".to_string());
    }

    let runtime_manifest_path = paths.runtime_dir.join("runtime-manifest.json");
    let runtime_manifest = read_manifest(&runtime_manifest_path)?;
    let compatibility = runtime_compatibility(runtime_manifest.as_ref());
    if !compatibility.compatible {
        return Err(compatibility.message);
    }

    let process_status = build_process_status(&paths);

    if process_status.port_open {
        return Ok(RwkvRuntimeStartResult {
            started: false,
            command: runtime_start_command_preview(&paths)?,
            process: process_status,
            message: "RWKV runtime 端口已可连接，无需重复启动。".to_string(),
        });
    }

    if matches!(process_status.process_running, Some(false)) && paths.pid_file.exists() {
        let _ = fs::remove_file(&paths.pid_file);
    }

    let executable_path = runtime_executable_path(&paths);
    if !executable_path.is_file() {
        return Err(format!(
            "RWKV runtime executable is missing: {}",
            display_path(&executable_path)
        ));
    }

    let vocab_path = runtime_vocab_path(&paths);
    if !vocab_path.is_file() {
        return Err(format!(
            "RWKV runtime vocab is missing: {}",
            display_path(&vocab_path)
        ));
    }

    let model_path = installed_model_path(&paths)?;
    let password = read_or_create_runtime_password(&paths)?;
    let port = DEFAULT_PORT.to_string();
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_file)
        .map_err(|error| format!("Could not open RWKV runtime log file: {error}"))?;
    let log_file_for_stderr = log_file
        .try_clone()
        .map_err(|error| format!("Could not clone RWKV runtime log file handle: {error}"))?;

    let mut command = Command::new(&executable_path);
    command
        .arg("--model-path")
        .arg(&model_path)
        .arg("--vocab-path")
        .arg(&vocab_path)
        .arg("--port")
        .arg(&port)
        .arg("--password")
        .arg(&password)
        .current_dir(paths.runtime_dir.join(RUNTIME_EXTRACTED_DIR))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_for_stderr));
    configure_runtime_command(&mut command);

    let child = command
        .spawn()
        .map_err(|error| format!("Could not start RWKV runtime process: {error}"))?;
    let pid = child.id();

    fs::write(&paths.pid_file, pid.to_string())
        .map_err(|error| format!("Could not write RWKV runtime pid file: {error}"))?;

    Ok(RwkvRuntimeStartResult {
        started: true,
        command: runtime_start_command_preview(&paths)?,
        process: build_process_status(&paths),
        message: "RWKV runtime 已启动，模型加载可能还需要一段时间。".to_string(),
    })
}

fn runtime_paths_from_paths(paths: &RwkvRuntimePaths) -> RwkvRuntimePaths {
    RwkvRuntimePaths {
        runtime_dir: paths.runtime_dir.clone(),
        model_dir: paths.model_dir.clone(),
        logs_dir: paths.logs_dir.clone(),
        log_file: paths.log_file.clone(),
        pid_file: paths.pid_file.clone(),
        password_file: paths.password_file.clone(),
    }
}

fn runtime_start_command_preview(paths: &RwkvRuntimePaths) -> Result<Vec<String>, String> {
    Ok(vec![
        display_path(&runtime_executable_path(paths)),
        "--model-path".to_string(),
        display_path(&installed_model_path(paths)?),
        "--vocab-path".to_string(),
        display_path(&runtime_vocab_path(paths)),
        "--port".to_string(),
        DEFAULT_PORT.to_string(),
        "--password".to_string(),
        "<redacted>".to_string(),
    ])
}

fn runtime_executable_path(paths: &RwkvRuntimePaths) -> PathBuf {
    paths
        .runtime_dir
        .join(RUNTIME_EXTRACTED_DIR)
        .join(RUNTIME_EXECUTABLE_FILENAME)
}

fn runtime_vocab_path(paths: &RwkvRuntimePaths) -> PathBuf {
    paths
        .runtime_dir
        .join(RUNTIME_EXTRACTED_DIR)
        .join(RUNTIME_VOCAB_FILENAME)
}

fn installed_model_path(paths: &RwkvRuntimePaths) -> Result<PathBuf, String> {
    let manifest_path = paths.model_dir.join("model-manifest.json");
    let manifest = read_manifest(&manifest_path)?
        .ok_or_else(|| "RWKV model manifest is missing.".to_string())?;

    validate_model_manifest(&manifest, paths)?;

    let filename = manifest
        .filename
        .as_deref()
        .ok_or_else(|| "RWKV model manifest filename is missing.".to_string())?;

    safe_artifact_path(&paths.model_dir, filename)
        .ok_or_else(|| "RWKV model manifest filename must stay inside model directory.".to_string())
}

fn build_process_status(paths: &RwkvRuntimePaths) -> RwkvRuntimeProcessStatus {
    let pid = read_runtime_pid(&paths.pid_file);
    let port_open = is_tcp_port_open(DEFAULT_HOST, DEFAULT_PORT, Duration::from_millis(200));
    let process_running = pid.and_then(is_process_running);
    let password = read_runtime_password(paths);
    let http_probe = probe_runtime_http(
        "GET",
        "/v1/models",
        None,
        password.as_deref(),
        RUNTIME_PROBE_TIMEOUT,
    );
    let http_ready = http_probe.status_code.is_some();
    let state = process_state_from_signals(pid, process_running, port_open, http_ready);
    let message = match state {
        RwkvRuntimeProcessState::Stopped if matches!(process_running, Some(false)) => {
            "RWKV runtime PID 已失效，进程未运行。".to_string()
        }
        RwkvRuntimeProcessState::Stopped => "RWKV runtime 尚未启动。".to_string(),
        RwkvRuntimeProcessState::Starting if port_open => {
            "RWKV runtime 端口已打开，等待 HTTP API 就绪。".to_string()
        }
        RwkvRuntimeProcessState::Starting => "RWKV runtime 进程已启动，等待端口就绪。".to_string(),
        RwkvRuntimeProcessState::Ready => "RWKV runtime HTTP API 已就绪。".to_string(),
    };

    RwkvRuntimeProcessStatus {
        state,
        pid,
        process_running,
        pid_file: display_path(&paths.pid_file),
        api_url: format!("http://{DEFAULT_HOST}:{DEFAULT_PORT}"),
        port: DEFAULT_PORT,
        port_open,
        http_ready,
        http_status_code: http_probe.status_code,
        log_file: display_path(&paths.log_file),
        log_tail: read_log_tail(&paths.log_file),
        message,
    }
}

fn process_state_from_signals(
    pid: Option<u32>,
    process_running: Option<bool>,
    port_open: bool,
    http_ready: bool,
) -> RwkvRuntimeProcessState {
    if port_open && http_ready {
        RwkvRuntimeProcessState::Ready
    } else if matches!(process_running, Some(false)) {
        RwkvRuntimeProcessState::Stopped
    } else if pid.is_some() || port_open {
        RwkvRuntimeProcessState::Starting
    } else {
        RwkvRuntimeProcessState::Stopped
    }
}

fn read_runtime_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| contents.trim().parse::<u32>().ok())
}

fn is_tcp_port_open(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(mut addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    let Some(address) = addresses.next() else {
        return false;
    };

    TcpStream::connect_timeout(&address, timeout).is_ok()
}

fn read_or_create_runtime_password(paths: &RwkvRuntimePaths) -> Result<String, String> {
    if let Some(password) = read_runtime_password(paths) {
        return Ok(password);
    }

    fs::create_dir_all(&paths.logs_dir)
        .map_err(|error| format!("Could not create RWKV logs directory: {error}"))?;
    let password = generate_runtime_password();
    fs::write(&paths.password_file, &password)
        .map_err(|error| format!("Could not write RWKV runtime token file: {error}"))?;

    Ok(password)
}

fn read_runtime_password(paths: &RwkvRuntimePaths) -> Option<String> {
    fs::read_to_string(&paths.password_file)
        .ok()
        .map(|contents| contents.trim().to_string())
        .filter(|password| !password.is_empty())
}

fn generate_runtime_password() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("rosetta-local-{timestamp}-{}", std::process::id())
}

#[cfg(windows)]
fn configure_runtime_command(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_runtime_command(_command: &mut Command) {}

#[cfg(windows)]
fn is_process_running(pid: u32) -> Option<bool> {
    let mut command = Command::new("tasklist");
    command.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    configure_runtime_command(&mut command);
    let output = command.output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.contains(&format!("\"{pid}\"")) || stdout.contains(&format!(",\"{pid}\",")))
}

#[cfg(not(windows))]
fn is_process_running(_pid: u32) -> Option<bool> {
    None
}

struct HttpProbeResult {
    status_code: Option<u16>,
    body: String,
}

fn probe_runtime_http(
    method: &str,
    path: &str,
    body: Option<&str>,
    password: Option<&str>,
    timeout: Duration,
) -> HttpProbeResult {
    let Ok(mut addresses) = (DEFAULT_HOST, DEFAULT_PORT).to_socket_addrs() else {
        return HttpProbeResult {
            status_code: None,
            body: String::new(),
        };
    };
    let Some(address) = addresses.next() else {
        return HttpProbeResult {
            status_code: None,
            body: String::new(),
        };
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        return HttpProbeResult {
            status_code: None,
            body: String::new(),
        };
    };

    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let body = body.unwrap_or("");
    let auth_header = password
        .map(|password| format!("Authorization: Bearer {password}\r\n"))
        .unwrap_or_default();
    let content_type_header = if body.is_empty() {
        String::new()
    } else {
        "Content-Type: application/json\r\n".to_string()
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {DEFAULT_HOST}:{DEFAULT_PORT}\r\n{auth_header}{content_type_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    if stream.write_all(request.as_bytes()).is_err() {
        return HttpProbeResult {
            status_code: None,
            body: String::new(),
        };
    }

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let status_code = parse_http_status_code(&response);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();

    HttpProbeResult { status_code, body }
}

fn parse_http_status_code(response: &str) -> Option<u16> {
    response
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()
}

fn probe_runtime_translation(
    paths: RwkvRuntimePaths,
) -> Result<RwkvRuntimeTranslationProbeResult, String> {
    let process = build_process_status(&paths);
    if !process.http_ready {
        return Ok(RwkvRuntimeTranslationProbeResult {
            ok: false,
            status_code: process.http_status_code,
            response_body_preview: String::new(),
            process,
            message: "RWKV runtime HTTP API 尚未就绪，无法测试翻译。".to_string(),
        });
    }

    let password = read_runtime_password(&paths);
    let request_body = format!(
        r#"{{"source_lang":"en","target_lang":"zh-CN","text_list":["{RUNTIME_TRANSLATION_PROBE_TEXT}"]}}"#
    );
    let probe = probe_runtime_http(
        "POST",
        "/translate/v1/batch-translate",
        Some(&request_body),
        password.as_deref(),
        Duration::from_secs(120),
    );
    let ok = probe
        .status_code
        .map(|status_code| (200..300).contains(&status_code))
        .unwrap_or(false);
    let response_body_preview = preview_text(&probe.body, 1200);
    let message = if ok {
        "RWKV runtime 最小翻译探测成功。".to_string()
    } else {
        "RWKV runtime 最小翻译探测失败，请查看状态和日志。".to_string()
    };

    Ok(RwkvRuntimeTranslationProbeResult {
        ok,
        status_code: probe.status_code,
        response_body_preview,
        process: build_process_status(&paths),
        message,
    })
}

fn read_log_tail(path: &Path) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let file_len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let start = file_len.saturating_sub(RUNTIME_LOG_TAIL_BYTES);

    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }

    let mut tail = String::new();
    if file.read_to_string(&mut tail).is_err() {
        return Vec::new();
    }

    tail.lines()
        .rev()
        .take(12)
        .map(|line| preview_text(line, 240))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = String::new();
    for character in text.chars().take(max_chars) {
        preview.push(character);
    }

    preview
}

fn build_install_progress(plan: RwkvRuntimeInstallPlan) -> RwkvRuntimeInstallProgress {
    let items: Vec<RwkvRuntimeInstallProgressItem> = plan
        .items
        .iter()
        .map(progress_item_from_plan_item)
        .collect();
    let state = if items
        .iter()
        .any(|item| item.state == RwkvRuntimeInstallProgressItemState::Blocked)
    {
        RwkvRuntimeInstallProgressState::Blocked
    } else if items
        .iter()
        .all(|item| item.state == RwkvRuntimeInstallProgressItemState::Ready)
    {
        RwkvRuntimeInstallProgressState::Ready
    } else if items
        .iter()
        .any(|item| item.state == RwkvRuntimeInstallProgressItemState::Pending)
    {
        RwkvRuntimeInstallProgressState::Queued
    } else {
        RwkvRuntimeInstallProgressState::NotStarted
    };
    let message = match state {
        RwkvRuntimeInstallProgressState::NotStarted => "本地 RWKV 安装尚未开始。".to_string(),
        RwkvRuntimeInstallProgressState::Queued => {
            "本地 RWKV 安装任务已准备，等待下载实现接入。".to_string()
        }
        RwkvRuntimeInstallProgressState::Ready => "本地 RWKV 已就绪。".to_string(),
        RwkvRuntimeInstallProgressState::Blocked => {
            "本地 RWKV 安装被无效 manifest 或文件状态阻塞。".to_string()
        }
    };

    RwkvRuntimeInstallProgress {
        state,
        items,
        message,
    }
}

fn progress_item_from_plan_item(
    item: &RwkvRuntimeInstallPlanItem,
) -> RwkvRuntimeInstallProgressItem {
    let state = match item.state {
        RwkvRuntimeInstallItemState::Missing => RwkvRuntimeInstallProgressItemState::Pending,
        RwkvRuntimeInstallItemState::Ready => RwkvRuntimeInstallProgressItemState::Ready,
        RwkvRuntimeInstallItemState::Invalid => RwkvRuntimeInstallProgressItemState::Blocked,
    };
    let bytes_total = if state == RwkvRuntimeInstallProgressItemState::Ready {
        None
    } else {
        expected_install_size_bytes(&item.kind)
    };
    let bytes_done = if state == RwkvRuntimeInstallProgressItemState::Ready {
        bytes_total.unwrap_or(0)
    } else {
        0
    };

    RwkvRuntimeInstallProgressItem {
        id: item.id.clone(),
        kind: item.kind.clone(),
        state,
        label: item.label.clone(),
        bytes_total,
        bytes_done,
        message: item.message.clone(),
    }
}

fn expected_install_size_bytes(kind: &RwkvRuntimeInstallItemKind) -> Option<u64> {
    match kind {
        RwkvRuntimeInstallItemKind::Runtime => Some(RUNTIME_ARTIFACT_SIZE_BYTES),
        RwkvRuntimeInstallItemKind::Model => Some(MODEL_ARTIFACT_SIZE_BYTES),
    }
}

fn build_install_item(
    kind: RwkvRuntimeInstallItemKind,
    label: &str,
    id: &str,
    target_dir: &Path,
    manifest_filename: &str,
    validate: impl FnOnce(&RwkvArtifactManifest) -> Result<(), String>,
) -> RwkvRuntimeInstallPlanItem {
    let manifest_path = target_dir.join(manifest_filename);
    let manifest = read_manifest(&manifest_path);

    match manifest {
        Ok(Some(manifest)) => {
            let artifact_path = manifest
                .filename
                .as_deref()
                .and_then(|filename| safe_artifact_path(target_dir, filename))
                .map(|path| display_path(&path));

            match validate(&manifest) {
                Ok(()) => RwkvRuntimeInstallPlanItem {
                    id: id.to_string(),
                    kind,
                    state: RwkvRuntimeInstallItemState::Ready,
                    label: label.to_string(),
                    target_dir: display_path(target_dir),
                    manifest_path: display_path(&manifest_path),
                    artifact_path,
                    message: "已就绪。".to_string(),
                },
                Err(error) => RwkvRuntimeInstallPlanItem {
                    id: id.to_string(),
                    kind,
                    state: RwkvRuntimeInstallItemState::Invalid,
                    label: label.to_string(),
                    target_dir: display_path(target_dir),
                    manifest_path: display_path(&manifest_path),
                    artifact_path,
                    message: error,
                },
            }
        }
        Ok(None) => RwkvRuntimeInstallPlanItem {
            id: id.to_string(),
            kind,
            state: RwkvRuntimeInstallItemState::Missing,
            label: label.to_string(),
            target_dir: display_path(target_dir),
            manifest_path: display_path(&manifest_path),
            artifact_path: None,
            message: "尚未安装。".to_string(),
        },
        Err(error) => RwkvRuntimeInstallPlanItem {
            id: id.to_string(),
            kind,
            state: RwkvRuntimeInstallItemState::Invalid,
            label: label.to_string(),
            target_dir: display_path(target_dir),
            manifest_path: display_path(&manifest_path),
            artifact_path: None,
            message: error,
        },
    }
}

fn read_manifest(path: &Path) -> Result<Option<RwkvArtifactManifest>, String> {
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Could not read {}: {error}", display_path(path)))?;
    let manifest = serde_json::from_str(contents.trim_start_matches('\u{feff}'))
        .map_err(|error| format!("Could not parse {}: {error}", display_path(path)))?;

    Ok(Some(manifest))
}

fn validate_manifests(
    runtime_manifest: &RwkvArtifactManifest,
    model_manifest: &RwkvArtifactManifest,
    paths: &RwkvRuntimePaths,
) -> Result<(), String> {
    validate_runtime_manifest(runtime_manifest, paths)?;
    validate_model_manifest(model_manifest, paths)?;

    Ok(())
}

fn validate_available_manifests(
    runtime_manifest: Option<&RwkvArtifactManifest>,
    model_manifest: Option<&RwkvArtifactManifest>,
    paths: &RwkvRuntimePaths,
) -> Result<(), String> {
    match (runtime_manifest, model_manifest) {
        (Some(runtime_manifest), Some(model_manifest)) => {
            validate_manifests(runtime_manifest, model_manifest, paths)
        }
        (Some(runtime_manifest), None) => validate_runtime_manifest(runtime_manifest, paths),
        (None, Some(model_manifest)) => validate_model_manifest(model_manifest, paths),
        (None, None) => Ok(()),
    }
}

fn validate_runtime_manifest(
    runtime_manifest: &RwkvArtifactManifest,
    paths: &RwkvRuntimePaths,
) -> Result<(), String> {
    if !runtime_manifest.id.starts_with(EXPECTED_RUNTIME_ID_PREFIX) {
        return Err(format!(
            "Runtime manifest id must start with `{EXPECTED_RUNTIME_ID_PREFIX}`."
        ));
    }

    if runtime_manifest
        .sha256
        .as_deref()
        .is_some_and(invalid_sha256)
    {
        return Err(
            "Runtime manifest sha256 must be a 64-character lowercase hex string.".to_string(),
        );
    }

    verify_optional_artifact_fast("Runtime", runtime_manifest, &paths.runtime_dir)?;

    Ok(())
}

fn validate_model_manifest(
    model_manifest: &RwkvArtifactManifest,
    paths: &RwkvRuntimePaths,
) -> Result<(), String> {
    if model_manifest.id != EXPECTED_MODEL_ID {
        return Err(format!("Model manifest id must be `{EXPECTED_MODEL_ID}`."));
    }

    if model_manifest.context_tokens != Some(EXPECTED_CONTEXT_TOKENS) {
        return Err(format!(
            "Model manifest contextTokens must be {EXPECTED_CONTEXT_TOKENS}."
        ));
    }

    let Some(supported_directions) = model_manifest.supported_directions.as_ref() else {
        return Err("Model manifest supportedDirections is required.".to_string());
    };

    for direction in EXPECTED_DIRECTIONS {
        if !supported_directions
            .iter()
            .any(|supported_direction| supported_direction == direction)
        {
            return Err(format!(
                "Model manifest supportedDirections must include `{direction}`."
            ));
        }
    }

    if model_manifest.sha256.as_deref().is_some_and(invalid_sha256) {
        return Err(
            "Model manifest sha256 must be a 64-character lowercase hex string.".to_string(),
        );
    }

    verify_required_artifact_fast("Model", model_manifest, &paths.model_dir)?;

    Ok(())
}

fn invalid_sha256(value: &str) -> bool {
    value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_optional_artifact_fast(
    label: &str,
    manifest: &RwkvArtifactManifest,
    base_dir: &Path,
) -> Result<(), String> {
    if manifest.filename.is_none() && manifest.sha256.is_none() && manifest.size_bytes.is_none() {
        return Ok(());
    }

    verify_required_artifact_fast(label, manifest, base_dir)
}

fn verify_required_artifact_fast(
    label: &str,
    manifest: &RwkvArtifactManifest,
    base_dir: &Path,
) -> Result<(), String> {
    let filename = manifest
        .filename
        .as_deref()
        .ok_or_else(|| format!("{label} manifest filename is required."))?;

    if manifest.sha256.is_none() {
        return Err(format!("{label} manifest sha256 is required."));
    }

    let artifact_path = safe_artifact_path(base_dir, filename).ok_or_else(|| {
        format!("{label} manifest filename must stay inside its managed directory.")
    })?;

    let metadata = fs::metadata(&artifact_path)
        .map_err(|error| format!("Could not read {label} artifact metadata: {error}"))?;

    if !metadata.is_file() {
        return Err(format!("{label} artifact must be a file."));
    }

    if let Some(expected_size_bytes) = manifest.size_bytes {
        if metadata.len() != expected_size_bytes {
            return Err(format!(
                "{label} artifact size mismatch: expected {expected_size_bytes}, got {}.",
                metadata.len()
            ));
        }
    }

    Ok(())
}

fn safe_artifact_path(base_dir: &Path, filename: &str) -> Option<PathBuf> {
    let filename_path = Path::new(filename);

    if filename_path.is_absolute()
        || filename_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }

    Some(base_dir.join(filename_path))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("Could not open artifact for sha256 verification: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read artifact for sha256 verification: {error}"))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn status_is_not_installed_when_layout_is_missing() {
        let root = unique_temp_root("missing-layout");
        let status = build_status(runtime_paths(root.clone()));

        assert_eq!(status.state, RwkvRuntimeState::NotInstalled);
        assert!(!status.runtime_dir_exists);
        assert!(!status.model_dir_exists);
        assert!(!status.logs_dir_exists);
        assert!(!status.runtime_manifest_exists);
        assert!(!status.model_manifest_exists);
        assert!(status.runtime_manifest.is_none());
        assert!(status.model_manifest.is_none());

        cleanup(root);
    }

    #[test]
    fn status_is_partial_when_layout_exists_without_manifests() {
        let root = unique_temp_root("partial-layout");
        let paths = runtime_paths(root.clone());
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::create_dir_all(&paths.logs_dir).expect("logs dir should be created");

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Partial);
        assert!(status.runtime_dir_exists);
        assert!(status.model_dir_exists);
        assert!(status.logs_dir_exists);
        assert!(!status.runtime_manifest_exists);
        assert!(!status.model_manifest_exists);

        cleanup(root);
    }

    #[test]
    fn status_is_installed_when_both_manifests_are_valid() {
        let root = unique_temp_root("installed-layout");
        let paths = runtime_paths(root.clone());
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::create_dir_all(&paths.logs_dir).expect("logs dir should be created");
        fs::write(
            paths.runtime_dir.join("runtime-manifest.json"),
            r#"{"id":"rwkv-lightning-windows-x64-cpu","version":"2026.05.07"}"#,
        )
        .expect("runtime manifest should be written");
        let model_filename = "model.pth";
        let model_contents = b"rwkv model bytes";
        fs::write(paths.model_dir.join(model_filename), model_contents)
            .expect("model artifact should be written");
        let model_sha256 =
            sha256_file(&paths.model_dir.join(model_filename)).expect("model sha should compute");
        fs::write(
            paths.model_dir.join("model-manifest.json"),
            format!(
                r#"{{"id":"rwkv-v7-g1-translate-1.5b","filename":"{model_filename}","sha256":"{model_sha256}","sizeBytes":{},"contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}}"#,
                model_contents.len()
            ),
        )
        .expect("model manifest should be written");

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Installed);
        assert_eq!(
            status
                .runtime_manifest
                .as_ref()
                .map(|manifest| manifest.id.as_str()),
            Some("rwkv-lightning-windows-x64-cpu")
        );
        assert_eq!(
            status
                .model_manifest
                .as_ref()
                .map(|manifest| manifest.id.as_str()),
            Some("rwkv-v7-g1-translate-1.5b")
        );
        assert!(status.manifest_error.is_none());

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_model_id_is_unexpected() {
        let root = unique_temp_root("unexpected-model-id");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"other-model","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("Model manifest id")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_only_model_manifest_is_invalid() {
        let root = unique_temp_root("only-bad-model");
        let paths = runtime_paths(root.clone());
        write_model_manifest(
            &paths,
            r#"{"id":"other-model","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("Model manifest id")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_model_direction_is_missing() {
        let root = unique_temp_root("missing-direction");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","contextTokens":4096,"supportedDirections":["en-zh"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("zh-en")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_sha256_is_malformed() {
        let root = unique_temp_root("bad-sha");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","contextTokens":4096,"supportedDirections":["en-zh","zh-en"],"sha256":"NOT-A-SHA"}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("sha256")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_model_artifact_is_missing() {
        let root = unique_temp_root("missing-artifact");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","filename":"model.pth","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("artifact metadata")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_model_artifact_size_mismatches() {
        let root = unique_temp_root("size-mismatch");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_artifact(&paths, "model.pth", b"model bytes");
        let model_sha256 =
            sha256_file(&paths.model_dir.join("model.pth")).expect("model sha should compute");
        write_model_manifest(
            &paths,
            &format!(
                r#"{{"id":"rwkv-v7-g1-translate-1.5b","filename":"model.pth","sha256":"{model_sha256}","sizeBytes":999,"contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}}"#
            ),
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("size mismatch")));

        cleanup(root);
    }

    #[test]
    fn status_does_not_hash_large_model_artifact_on_refresh() {
        let root = unique_temp_root("status-fast-hash");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_artifact(&paths, "model.pth", b"model bytes");
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","filename":"model.pth","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Installed);
        assert!(status.manifest_error.is_none());

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_model_filename_escapes_managed_directory() {
        let root = unique_temp_root("path-escape");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","filename":"../model.pth","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("managed directory")));

        cleanup(root);
    }

    #[test]
    fn status_is_invalid_when_manifest_json_is_invalid() {
        let root = unique_temp_root("invalid-layout");
        let paths = runtime_paths(root.clone());
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::write(paths.runtime_dir.join("runtime-manifest.json"), "{not-json")
            .expect("runtime manifest should be written");

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status.manifest_error.is_some());
        assert!(status.runtime_manifest.is_none());

        cleanup(root);
    }

    #[test]
    fn manifest_reader_accepts_utf8_bom() {
        let root = unique_temp_root("manifest-bom");
        let paths = runtime_paths(root.clone());
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::write(
            paths.model_dir.join("model-manifest.json"),
            "\u{feff}{\"id\":\"rwkv-v7-g1-translate-1.5b\"}",
        )
        .expect("model manifest should be written");

        let manifest = read_manifest(&paths.model_dir.join("model-manifest.json"))
            .expect("manifest should parse")
            .expect("manifest should exist");

        assert_eq!(manifest.id, EXPECTED_MODEL_ID);

        cleanup(root);
    }

    #[test]
    fn install_plan_reports_missing_items_when_no_manifests_exist() {
        let root = unique_temp_root("plan-missing");
        let plan = build_install_plan(runtime_paths(root.clone()));

        assert!(!plan.ready);
        assert_eq!(plan.items.len(), 2);
        assert!(plan
            .items
            .iter()
            .all(|item| item.state == RwkvRuntimeInstallItemState::Missing));

        cleanup(root);
    }

    #[test]
    fn install_plan_reports_ready_when_runtime_and_model_are_valid() {
        let root = unique_temp_root("plan-ready");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_valid_model_manifest_with_artifact(&paths, "model.pth", b"rwkv model bytes");

        let plan = build_install_plan(paths);

        assert!(plan.ready);
        assert_eq!(plan.items.len(), 2);
        assert!(plan
            .items
            .iter()
            .all(|item| item.state == RwkvRuntimeInstallItemState::Ready));
        assert!(plan.items.iter().any(|item| item.artifact_path.is_some()));

        cleanup(root);
    }

    #[test]
    fn install_plan_reports_invalid_item_when_manifest_is_bad() {
        let root = unique_temp_root("plan-invalid");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"other-model","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let plan = build_install_plan(paths);

        assert!(!plan.ready);
        assert_eq!(
            plan.items
                .iter()
                .find(|item| item.kind == RwkvRuntimeInstallItemKind::Runtime)
                .map(|item| &item.state),
            Some(&RwkvRuntimeInstallItemState::Ready)
        );
        assert_eq!(
            plan.items
                .iter()
                .find(|item| item.kind == RwkvRuntimeInstallItemKind::Model)
                .map(|item| &item.state),
            Some(&RwkvRuntimeInstallItemState::Invalid)
        );

        cleanup(root);
    }

    #[test]
    fn install_progress_is_queued_when_items_are_missing() {
        let root = unique_temp_root("progress-queued");
        let progress = build_install_progress(build_install_plan(runtime_paths(root.clone())));

        assert_eq!(progress.state, RwkvRuntimeInstallProgressState::Queued);
        assert_eq!(progress.items.len(), 2);
        assert!(progress.items.iter().all(|item| {
            item.state == RwkvRuntimeInstallProgressItemState::Pending && item.bytes_done == 0
        }));

        cleanup(root);
    }

    #[test]
    fn install_progress_is_ready_when_plan_is_ready() {
        let root = unique_temp_root("progress-ready");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_valid_model_manifest_with_artifact(&paths, "model.pth", b"rwkv model bytes");

        let progress = build_install_progress(build_install_plan(paths));

        assert_eq!(progress.state, RwkvRuntimeInstallProgressState::Ready);
        assert!(progress
            .items
            .iter()
            .all(|item| item.state == RwkvRuntimeInstallProgressItemState::Ready));

        cleanup(root);
    }

    #[test]
    fn install_progress_is_blocked_when_plan_has_invalid_item() {
        let root = unique_temp_root("progress-blocked");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_manifest(
            &paths,
            r#"{"id":"other-model","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let progress = build_install_progress(build_install_plan(paths));

        assert_eq!(progress.state, RwkvRuntimeInstallProgressState::Blocked);
        assert!(progress
            .items
            .iter()
            .any(|item| item.state == RwkvRuntimeInstallProgressItemState::Blocked));

        cleanup(root);
    }

    #[test]
    fn artifact_catalog_is_ready_when_modelscope_metadata_is_confirmed() {
        let root = unique_temp_root("catalog-ready");
        let catalog = build_artifact_catalog(runtime_paths(root.clone()));

        assert!(catalog.ready_for_download);
        assert_eq!(catalog.items.len(), 2);
        assert!(catalog
            .items
            .iter()
            .any(|item| item.kind == RwkvRuntimeInstallItemKind::Runtime
                && item.state == RwkvRuntimeArtifactCatalogItemState::Ready
                && item.download_url.as_deref() == Some(RUNTIME_ARTIFACT_DOWNLOAD_URL)
                && item.sha256.as_deref() == Some(RUNTIME_ARTIFACT_SHA256)
                && item.size_bytes == Some(RUNTIME_ARTIFACT_SIZE_BYTES)));
        assert!(catalog.items.iter().any(|item| {
            item.kind == RwkvRuntimeInstallItemKind::Model
                && item.state == RwkvRuntimeArtifactCatalogItemState::Ready
                && item.download_url.as_deref() == Some(MODEL_ARTIFACT_DOWNLOAD_URL)
                && item.sha256.as_deref() == Some(MODEL_ARTIFACT_SHA256)
                && item.size_bytes == Some(MODEL_ARTIFACT_SIZE_BYTES)
        }));

        cleanup(root);
    }

    #[test]
    fn artifact_catalog_points_to_managed_target_directories() {
        let root = unique_temp_root("catalog-paths");
        let paths = runtime_paths(root.clone());
        let runtime_dir = display_path(&paths.runtime_dir);
        let model_dir = display_path(&paths.model_dir);
        let catalog = build_artifact_catalog(paths);

        assert!(catalog
            .items
            .iter()
            .any(|item| item.kind == RwkvRuntimeInstallItemKind::Runtime
                && item.target_dir == runtime_dir
                && item.manifest_path.ends_with("runtime-manifest.json")));
        assert!(catalog
            .items
            .iter()
            .any(|item| item.kind == RwkvRuntimeInstallItemKind::Model
                && item.target_dir == model_dir
                && item.manifest_path.ends_with("model-manifest.json")));

        cleanup(root);
    }

    #[test]
    fn scan_staged_artifacts_reports_no_files_without_writing_manifests() {
        let root = unique_temp_root("scan-empty");
        let paths = runtime_paths(root.clone());
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");

        let scan = scan_staged_artifacts(paths).expect("scan should complete");

        assert!(scan.scanned);
        assert!(scan.installed_manifests.is_empty());
        assert!(scan.errors.is_empty());
        assert!(!scan.plan.ready);

        cleanup(root);
    }

    #[test]
    fn scan_expected_artifact_writes_manifest_for_valid_file() {
        let root = unique_temp_root("scan-valid");
        let paths = runtime_paths(root.clone());
        let filename = "tiny-model.pth";
        let contents = b"tiny model";
        write_model_artifact(&paths, filename, contents);
        let artifact_path = paths.model_dir.join(filename);
        let artifact_sha256 = sha256_file(&artifact_path).expect("sha should compute");
        let manifest_path = paths.model_dir.join("model-manifest.json");
        let expected_artifact = ExpectedArtifact {
            kind: RwkvRuntimeInstallItemKind::Model,
            id: EXPECTED_MODEL_ID,
            label: "Test model",
            filename,
            sha256: &artifact_sha256,
            size_bytes: contents.len() as u64,
            base_dir: &paths.model_dir,
            manifest_path: manifest_path.clone(),
        };

        let scanned_manifest_path =
            scan_expected_artifact(&expected_artifact).expect("scan should pass");

        assert_eq!(scanned_manifest_path, Some(manifest_path.clone()));
        assert!(manifest_path.is_file());

        let manifest = read_manifest(&manifest_path)
            .expect("manifest should read")
            .expect("manifest should exist");
        assert_eq!(manifest.id, EXPECTED_MODEL_ID);
        assert_eq!(manifest.filename.as_deref(), Some(filename));
        assert_eq!(manifest.sha256.as_deref(), Some(artifact_sha256.as_str()));

        cleanup(root);
    }

    #[test]
    fn scan_expected_artifact_rejects_hash_mismatch() {
        let root = unique_temp_root("scan-bad-hash");
        let paths = runtime_paths(root.clone());
        let filename = "tiny-model.pth";
        let contents = b"tiny model";
        write_model_artifact(&paths, filename, contents);
        let manifest_path = paths.model_dir.join("model-manifest.json");
        let expected_artifact = ExpectedArtifact {
            kind: RwkvRuntimeInstallItemKind::Model,
            id: EXPECTED_MODEL_ID,
            label: "Test model",
            filename,
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            size_bytes: contents.len() as u64,
            base_dir: &paths.model_dir,
            manifest_path,
        };

        let error = scan_expected_artifact(&expected_artifact).expect_err("scan should fail");

        assert!(error.contains("sha256 mismatch"));

        cleanup(root);
    }

    #[test]
    fn extract_runtime_zip_writes_bundle_and_reports_executable() {
        let root = unique_temp_root("extract-runtime");
        let paths = runtime_paths(root.clone());
        let zip_filename = "tiny-runtime.zip";
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        write_test_runtime_zip(
            &paths.runtime_dir.join(zip_filename),
            &[
                (RUNTIME_EXECUTABLE_FILENAME, b"exe bytes".as_slice()),
                ("rwkv_vocab_v20230424.txt", b"vocab bytes".as_slice()),
            ],
        );
        let zip_path = paths.runtime_dir.join(zip_filename);
        let zip_size = fs::metadata(&zip_path)
            .expect("zip metadata should read")
            .len();

        let result = extract_runtime_zip_from_artifact(paths, zip_filename, zip_size)
            .expect("runtime zip should extract");

        assert!(result.extracted);
        assert_eq!(result.files_extracted, 2);
        assert!(Path::new(&result.executable_path).is_file());

        cleanup(root);
    }

    #[test]
    fn extract_runtime_zip_rejects_missing_executable() {
        let root = unique_temp_root("extract-missing-exe");
        let paths = runtime_paths(root.clone());
        let zip_filename = "tiny-runtime.zip";
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        write_test_runtime_zip(
            &paths.runtime_dir.join(zip_filename),
            &[("rwkv_vocab_v20230424.txt", b"vocab bytes".as_slice())],
        );
        let zip_path = paths.runtime_dir.join(zip_filename);
        let zip_size = fs::metadata(&zip_path)
            .expect("zip metadata should read")
            .len();

        let error = match extract_runtime_zip_from_artifact(paths, zip_filename, zip_size) {
            Ok(_) => panic!("runtime zip should be rejected"),
            Err(error) => error,
        };

        assert!(error.contains(RUNTIME_EXECUTABLE_FILENAME));

        cleanup(root);
    }

    #[test]
    fn extract_runtime_zip_returns_fast_when_executable_exists() {
        let root = unique_temp_root("extract-existing");
        let paths = runtime_paths(root.clone());
        let bundle_dir = paths.runtime_dir.join(RUNTIME_EXTRACTED_DIR);
        fs::create_dir_all(&bundle_dir).expect("bundle dir should be created");
        fs::write(bundle_dir.join(RUNTIME_EXECUTABLE_FILENAME), b"exe bytes")
            .expect("executable should be written");

        let result = extract_runtime_zip_from_artifact(paths, "missing-runtime.zip", 999)
            .expect("existing executable should be reused");

        assert!(result.extracted);
        assert_eq!(result.files_extracted, 0);
        assert_eq!(result.bytes_extracted, 0);
        assert!(result.message.contains("无需重复解压"));

        cleanup(root);
    }

    #[test]
    fn extract_runtime_zip_rejects_size_mismatch_before_unzip() {
        let root = unique_temp_root("extract-size-mismatch");
        let paths = runtime_paths(root.clone());
        let zip_filename = "tiny-runtime.zip";
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        write_test_runtime_zip(
            &paths.runtime_dir.join(zip_filename),
            &[(RUNTIME_EXECUTABLE_FILENAME, b"exe bytes".as_slice())],
        );

        let error = match extract_runtime_zip_from_artifact(paths, zip_filename, 999) {
            Ok(_) => panic!("runtime zip should be rejected"),
            Err(error) => error,
        };

        assert!(error.contains("size mismatch"));

        cleanup(root);
    }

    #[test]
    fn safe_zip_entry_path_rejects_escape_components() {
        let base_dir = Path::new("runtime-bundle");

        assert!(safe_zip_entry_path(base_dir, Path::new("rwkv_lightning.exe")).is_some());
        assert!(safe_zip_entry_path(base_dir, Path::new("../rwkv_lightning.exe")).is_none());
        assert!(safe_zip_entry_path(base_dir, Path::new("nested/../rwkv_lightning.exe")).is_none());
    }

    #[test]
    fn missing_log_tail_is_empty() {
        let root = unique_temp_root("missing-log-tail");
        let paths = runtime_paths(root.clone());

        assert!(read_log_tail(&paths.log_file).is_empty());

        cleanup(root);
    }

    #[test]
    fn process_state_uses_port_and_http_readiness() {
        assert_eq!(
            process_state_from_signals(None, None, false, false),
            RwkvRuntimeProcessState::Stopped
        );
        assert_eq!(
            process_state_from_signals(Some(12345), None, false, false),
            RwkvRuntimeProcessState::Starting
        );
        assert_eq!(
            process_state_from_signals(Some(12345), Some(false), false, false),
            RwkvRuntimeProcessState::Stopped
        );
        assert_eq!(
            process_state_from_signals(Some(12345), Some(true), true, false),
            RwkvRuntimeProcessState::Starting
        );
        assert_eq!(
            process_state_from_signals(Some(12345), Some(true), true, true),
            RwkvRuntimeProcessState::Ready
        );
    }

    #[test]
    fn parse_display_adapters_reads_pnputil_device_descriptions() {
        let output = r#"Microsoft PnP Utility

Instance ID:                ROOT\DISPLAY\0000
Device Description:         Parsec Virtual Display Adapter
Class Name:                 Display

Instance ID:                PCI\VEN_1002&DEV_1900
Device Description:         AMD Radeon 780M Graphics
Class Name:                 Display
"#;

        assert_eq!(
            parse_display_adapters(output),
            vec![
                "Parsec Virtual Display Adapter".to_string(),
                "AMD Radeon 780M Graphics".to_string()
            ]
        );
    }

    #[test]
    fn cuda_runtime_is_incompatible_without_nvidia_display_adapter() {
        let runtime_manifest = RwkvArtifactManifest {
            id: RUNTIME_ARTIFACT_ID.to_string(),
            version: None,
            source: None,
            filename: Some(RUNTIME_ARTIFACT_FILENAME.to_string()),
            sha256: Some(RUNTIME_ARTIFACT_SHA256.to_string()),
            size_bytes: Some(RUNTIME_ARTIFACT_SIZE_BYTES),
            context_tokens: None,
            supported_directions: None,
            installed_at: None,
        };

        assert_eq!(runtime_backend(Some(&runtime_manifest)), "cuda-nvidia");
    }

    fn write_valid_runtime_manifest(paths: &RwkvRuntimePaths) {
        fs::create_dir_all(&paths.runtime_dir).expect("runtime dir should be created");
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::write(
            paths.runtime_dir.join("runtime-manifest.json"),
            r#"{"id":"rwkv-lightning-windows-x64-cpu"}"#,
        )
        .expect("runtime manifest should be written");
    }

    fn write_model_manifest(paths: &RwkvRuntimePaths, contents: &str) {
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::write(paths.model_dir.join("model-manifest.json"), contents)
            .expect("model manifest should be written");
    }

    fn write_valid_model_manifest_with_artifact(
        paths: &RwkvRuntimePaths,
        filename: &str,
        contents: &[u8],
    ) {
        write_model_artifact(paths, filename, contents);
        let model_sha256 =
            sha256_file(&paths.model_dir.join(filename)).expect("model sha should compute");
        write_model_manifest(
            paths,
            &format!(
                r#"{{"id":"rwkv-v7-g1-translate-1.5b","filename":"{filename}","sha256":"{model_sha256}","sizeBytes":{},"contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}}"#,
                contents.len()
            ),
        );
    }

    fn write_model_artifact(paths: &RwkvRuntimePaths, filename: &str, contents: &[u8]) {
        fs::create_dir_all(&paths.model_dir).expect("model dir should be created");
        fs::write(paths.model_dir.join(filename), contents)
            .expect("model artifact should be written");
    }

    fn write_test_runtime_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let zip_file = File::create(path).expect("test runtime zip should be created");
        let mut zip = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (name, contents) in entries {
            zip.start_file(name, options)
                .expect("test runtime zip entry should start");
            zip.write_all(contents)
                .expect("test runtime zip entry should write");
        }

        zip.finish().expect("test runtime zip should finish");
    }

    fn unique_temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("rosetta-rwkv-runtime-test-{name}-{nanos}"))
    }

    fn cleanup(root: PathBuf) {
        if root.exists() {
            fs::remove_dir_all(root).expect("test temp directory should be removed");
        }
    }
}
