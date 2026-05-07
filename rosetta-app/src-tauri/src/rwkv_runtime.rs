use std::path::PathBuf;

use serde::Serialize;
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
    runtime_manifest_exists: bool,
    model_manifest_exists: bool,
    log_file: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeState {
    NotInstalled,
    Installed,
}

#[tauri::command]
pub fn get_rwkv_runtime_status(app: AppHandle) -> Result<RwkvRuntimeStatus, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    let runtime_dir = app_data_dir.join("runtime").join("rwkv-lightning");
    let model_dir = app_data_dir
        .join("models")
        .join("rwkv-v7-g1-translate")
        .join("1.5b");
    let log_file = app_data_dir.join("logs").join("rwkv-runtime.log");

    let runtime_manifest_exists = runtime_dir.join("runtime-manifest.json").is_file();
    let model_manifest_exists = model_dir.join("model-manifest.json").is_file();
    let state = if runtime_manifest_exists && model_manifest_exists {
        RwkvRuntimeState::Installed
    } else {
        RwkvRuntimeState::NotInstalled
    };

    let message = match state {
        RwkvRuntimeState::Installed => "本地 RWKV 运行时文件已就绪。".to_string(),
        RwkvRuntimeState::NotInstalled => "尚未安装托管 RWKV 运行时或 1.5B 翻译模型。".to_string(),
    };

    Ok(RwkvRuntimeStatus {
        state,
        api_url: format!("http://{DEFAULT_HOST}:{DEFAULT_PORT}"),
        runtime_dir: display_path(runtime_dir),
        model_dir: display_path(model_dir),
        runtime_manifest_exists,
        model_manifest_exists,
        log_file: display_path(log_file),
        message,
    })
}

fn display_path(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
