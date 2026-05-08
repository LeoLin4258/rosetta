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

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeState {
    NotInstalled,
    Partial,
    Installed,
    Invalid,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallItemKind {
    Runtime,
    Model,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RwkvRuntimeInstallItemState {
    Missing,
    Ready,
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

#[tauri::command]
pub fn get_rwkv_runtime_install_plan(app: AppHandle) -> Result<RwkvRuntimeInstallPlan, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve Rosetta app data directory: {error}"))?;

    Ok(build_install_plan(runtime_paths(app_data_dir)))
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
