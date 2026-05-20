pub mod install;
pub mod layout;
pub mod openai_shim;
pub mod profile;
pub mod status;

use serde::Serialize;
use tauri::{AppHandle, State};

pub use install::Pdf2zhInstallRegistry as InstallStateRegistry;
pub use status::build_static_status;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhCancelInstallResult {
    pub cancelled: bool,
    pub message: String,
}

#[tauri::command]
pub fn get_pdf2zh_status(app: AppHandle) -> Result<status::Pdf2zhStatus, String> {
    Ok(status::build_static_status(&app)?.into_status())
}

#[tauri::command]
pub async fn install_pdf2zh_pack(
    app: AppHandle,
    install_registry: State<'_, InstallStateRegistry>,
    options: Option<install::Pdf2zhInstallOptions>,
) -> Result<install::Pdf2zhInstallResult, String> {
    let profile = profile::current_profile()
        .ok_or_else(|| "当前平台尚未支持托管 pdf2zh pack。".to_string())?;
    let layout = layout::Pdf2zhLayout::from_app(&app, profile)?;
    install::install_pack(
        &app,
        install_registry.inner(),
        profile,
        &layout,
        options.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn cancel_pdf2zh_install(
    install_registry: State<'_, InstallStateRegistry>,
) -> Result<Pdf2zhCancelInstallResult, String> {
    let cancelled = install_registry.request_cancel().await;
    let message = if cancelled {
        "已请求取消 pdf2zh 安装。"
    } else {
        "当前没有正在进行的 pdf2zh 安装任务。"
    };
    Ok(Pdf2zhCancelInstallResult {
        cancelled,
        message: message.to_string(),
    })
}

#[tauri::command]
pub async fn get_pdf2zh_install_progress(
    install_registry: State<'_, InstallStateRegistry>,
) -> Result<install::Pdf2zhInstallProgress, String> {
    Ok(install_registry.snapshot().await)
}
