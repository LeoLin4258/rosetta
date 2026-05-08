use std::{
    fs,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RwkvRuntimeStatus {
    state: RwkvRuntimeState,
    api_url: String,
    runtime_dir: String,
    model_dir: String,
    logs_dir: String,
    runtime_dir_exists: bool,
    model_dir_exists: bool,
    logs_dir_exists: bool,
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

struct RwkvRuntimePaths {
    runtime_dir: PathBuf,
    model_dir: PathBuf,
    logs_dir: PathBuf,
    log_file: PathBuf,
}

fn runtime_paths(app_data_dir: PathBuf) -> RwkvRuntimePaths {
    let runtime_dir = app_data_dir.join("runtime").join("rwkv-lightning");
    let model_dir = app_data_dir
        .join("models")
        .join("rwkv-v7-g1-translate")
        .join("1.5b");
    let logs_dir = app_data_dir.join("logs");
    let log_file = logs_dir.join("rwkv-runtime.log");

    RwkvRuntimePaths {
        runtime_dir,
        model_dir,
        logs_dir,
        log_file,
    }
}

fn build_status(paths: RwkvRuntimePaths) -> RwkvRuntimeStatus {
    let runtime_manifest_path = paths.runtime_dir.join("runtime-manifest.json");
    let model_manifest_path = paths.model_dir.join("model-manifest.json");

    let runtime_dir_exists = paths.runtime_dir.is_dir();
    let model_dir_exists = paths.model_dir.is_dir();
    let logs_dir_exists = paths.logs_dir.is_dir();
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
        runtime_dir: display_path(&paths.runtime_dir),
        model_dir: display_path(&paths.model_dir),
        logs_dir: display_path(&paths.logs_dir),
        runtime_dir_exists,
        model_dir_exists,
        logs_dir_exists,
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
    let manifest = serde_json::from_str(&contents)
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

    verify_optional_artifact("Runtime", runtime_manifest, &paths.runtime_dir)?;

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

    verify_required_artifact("Model", model_manifest, &paths.model_dir)?;

    Ok(())
}

fn invalid_sha256(value: &str) -> bool {
    value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_optional_artifact(
    label: &str,
    manifest: &RwkvArtifactManifest,
    base_dir: &Path,
) -> Result<(), String> {
    if manifest.filename.is_none() && manifest.sha256.is_none() && manifest.size_bytes.is_none() {
        return Ok(());
    }

    verify_required_artifact(label, manifest, base_dir)
}

fn verify_required_artifact(
    label: &str,
    manifest: &RwkvArtifactManifest,
    base_dir: &Path,
) -> Result<(), String> {
    let filename = manifest
        .filename
        .as_deref()
        .ok_or_else(|| format!("{label} manifest filename is required."))?;
    let expected_sha256 = manifest
        .sha256
        .as_deref()
        .ok_or_else(|| format!("{label} manifest sha256 is required."))?;

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

    let actual_sha256 = sha256_file(&artifact_path)?;

    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "{label} artifact sha256 mismatch: expected {expected_sha256}, got {actual_sha256}."
        ));
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
    fn status_is_invalid_when_model_artifact_hash_mismatches() {
        let root = unique_temp_root("hash-mismatch");
        let paths = runtime_paths(root.clone());
        write_valid_runtime_manifest(&paths);
        write_model_artifact(&paths, "model.pth", b"model bytes");
        write_model_manifest(
            &paths,
            r#"{"id":"rwkv-v7-g1-translate-1.5b","filename":"model.pth","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","contextTokens":4096,"supportedDirections":["en-zh","zh-en"]}"#,
        );

        let status = build_status(paths);

        assert_eq!(status.state, RwkvRuntimeState::Invalid);
        assert!(status
            .manifest_error
            .as_deref()
            .is_some_and(|error| error.contains("sha256 mismatch")));

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
