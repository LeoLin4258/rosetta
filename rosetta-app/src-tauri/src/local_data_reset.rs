use std::{fs, path::Path};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{managed_pdf2zh, managed_rwkv, rosetta_jobs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDataResetItem {
    pub label: String,
    pub path: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDataResetResult {
    pub items: Vec<LocalDataResetItem>,
    pub stopped_runtime: bool,
    pub cancelled_rwkv_install: bool,
    pub cancelled_pdf2zh_install: bool,
    pub cancelled_pdf_translation: bool,
    pub runtime_stop_error: Option<String>,
}

#[tauri::command]
pub async fn clear_rosetta_local_data(
    app: AppHandle,
    rwkv_registry: State<'_, managed_rwkv::Registry>,
    rwkv_install_registry: State<'_, managed_rwkv::InstallStateRegistry>,
    pdf2zh_install_registry: State<'_, managed_pdf2zh::InstallStateRegistry>,
    pdf_translation_cancel_state: State<'_, rosetta_jobs::PdfTranslationCancelState>,
) -> Result<LocalDataResetResult, String> {
    let cancelled_rwkv_install = managed_rwkv::cancel_managed_rwkv_install(rwkv_install_registry)
        .await
        .map(|result| result.cancelled)
        .unwrap_or(false);
    let cancelled_pdf2zh_install = managed_pdf2zh::cancel_pdf2zh_install(pdf2zh_install_registry)
        .await
        .map(|result| result.cancelled)
        .unwrap_or(false);
    let cancelled_pdf_translation = cancel_pdf_translation(pdf_translation_cancel_state);

    let stop_result = managed_rwkv::stop_managed_rwkv_runtime(app.clone(), rwkv_registry).await;
    let (stopped_runtime, runtime_stop_error) = match stop_result {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error)),
    };

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法读取 Rosetta app data 目录: {error}"))?;
    let app_local_data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法读取 Rosetta app local data 目录: {error}"))?;
    let items = remove_rosetta_data_dirs(&app_data_dir, &app_local_data_dir)?;

    Ok(LocalDataResetResult {
        items,
        stopped_runtime,
        cancelled_rwkv_install,
        cancelled_pdf2zh_install,
        cancelled_pdf_translation,
        runtime_stop_error,
    })
}

pub(crate) fn remove_rosetta_data_dirs(
    app_data_dir: &Path,
    app_local_data_dir: &Path,
) -> Result<Vec<LocalDataResetItem>, String> {
    let targets = [
        ("任务历史与缓存", app_data_dir.join("jobs")),
        ("本地模型", app_local_data_dir.join("managed-rwkv")),
        ("PDF 处理组件", app_local_data_dir.join("pdf2zh-sidecar")),
        ("初始设置状态", app_local_data_dir.join("onboarding.json")),
    ];

    let mut items = Vec::with_capacity(targets.len());
    for (label, path) in targets {
        let deleted = remove_path_if_exists(&path)?;
        items.push(LocalDataResetItem {
            label: label.to_string(),
            path: path.display().to_string(),
            deleted,
        });
    }

    Ok(items)
}

fn remove_path_if_exists(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    if path.is_file() {
        fs::remove_file(path).map_err(|error| format!("无法删除 {}: {error}", path.display()))?;
        return Ok(true);
    }
    if !path.is_dir() {
        return Err(format!("目标不是目录，未删除: {}", path.display()));
    }
    fs::remove_dir_all(path).map_err(|error| format!("无法删除 {}: {error}", path.display()))?;
    Ok(true)
}

fn cancel_pdf_translation(
    cancel_state: State<'_, rosetta_jobs::PdfTranslationCancelState>,
) -> bool {
    cancel_state.request_cancel();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn removes_only_known_rosetta_data_dirs() {
        let root = unique_temp_dir("reset-known-dirs");
        let app_data = root.join("app-data");
        let app_local = root.join("app-local-data");
        fs::create_dir_all(app_data.join("jobs/job-1")).expect("create jobs");
        fs::create_dir_all(app_data.join("keep-me")).expect("create app data sibling");
        fs::create_dir_all(app_local.join("managed-rwkv/models")).expect("create rwkv");
        fs::create_dir_all(app_local.join("pdf2zh-sidecar/pack")).expect("create pdf2zh");
        fs::create_dir_all(app_local.join("keep-me")).expect("create local sibling");
        fs::write(app_local.join("onboarding.json"), "{}").expect("create onboarding state");

        let result = remove_rosetta_data_dirs(&app_data, &app_local).expect("reset dirs");

        assert_eq!(result.len(), 4);
        assert!(result.iter().all(|item| item.deleted));
        assert!(!app_data.join("jobs").exists());
        assert!(!app_local.join("managed-rwkv").exists());
        assert!(!app_local.join("pdf2zh-sidecar").exists());
        assert!(!app_local.join("onboarding.json").exists());
        assert!(app_data.join("keep-me").is_dir());
        assert!(app_local.join("keep-me").is_dir());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_rosetta_data_dirs_are_reported_without_error() {
        let root = unique_temp_dir("reset-missing-dirs");
        let app_data = root.join("app-data");
        let app_local = root.join("app-local-data");
        fs::create_dir_all(&app_data).expect("create app data");
        fs::create_dir_all(&app_local).expect("create local data");

        let result = remove_rosetta_data_dirs(&app_data, &app_local).expect("reset dirs");

        assert_eq!(result.len(), 4);
        assert!(result.iter().all(|item| !item.deleted));

        fs::remove_dir_all(root).ok();
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("rosetta-{name}-{nanos}"))
    }
}
