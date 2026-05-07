use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8000;

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
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeState {
    NotInstalled,
    Partial,
    Installed,
    Invalid,
}

#[derive(Deserialize, Serialize)]
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

    let manifest_error = runtime_manifest
        .as_ref()
        .err()
        .or_else(|| model_manifest.as_ref().err())
        .map(ToString::to_string);
    let runtime_manifest = runtime_manifest.ok().flatten();
    let model_manifest = model_manifest.ok().flatten();

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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
