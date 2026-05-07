use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8000;
const EXPECTED_RUNTIME_ID_PREFIX: &str = "rwkv-lightning-";
const EXPECTED_MODEL_ID: &str = "rwkv-v7-g1-translate-1.5b";
const EXPECTED_CONTEXT_TOKENS: u32 = 4096;
const EXPECTED_DIRECTIONS: [&str; 2] = ["en-zh", "zh-en"];

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

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeState {
    NotInstalled,
    Partial,
    Installed,
    Invalid,
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
    let validation_error = match (&runtime_manifest, &model_manifest) {
        (Some(runtime_manifest), Some(model_manifest)) => {
            validate_manifests(runtime_manifest, model_manifest).err()
        }
        _ => None,
    };
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

    Ok(())
}

fn invalid_sha256(value: &str) -> bool {
    value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
            r#"{"id":"rwkv-lightning-windows-x64-cpu","version":"2026.05.07","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        )
        .expect("runtime manifest should be written");
        fs::write(
            paths.model_dir.join("model-manifest.json"),
            r#"{"id":"rwkv-v7-g1-translate-1.5b","contextTokens":4096,"supportedDirections":["en-zh","zh-en"],"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#,
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
