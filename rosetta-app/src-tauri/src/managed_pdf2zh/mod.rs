pub mod install;
pub mod layout;
pub mod openai_shim;
pub mod profile;
pub mod status;
pub mod worker;

use serde::Serialize;
use tauri::{AppHandle, State};

pub use install::Pdf2zhInstallRegistry as InstallStateRegistry;
pub use status::build_static_status;
pub use worker::WorkerState as Pdf2zhWorkerState;

/// Warm up the persistent pdf2zh worker (heavy Python imports + layout model)
/// so the first translate click doesn't pay the ~13 s import. Fire-and-forget
/// from the frontend when a PDF document becomes active.
#[tauri::command]
pub async fn prewarm_pdf2zh_worker(app: AppHandle) -> Result<bool, String> {
    worker::prewarm_worker(&app).await
}

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
        .ok_or_else(|| "当前平台尚未支持自动安装 PDF 版面处理组件。".to_string())?;
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
        "已请求取消 PDF 版面处理组件安装。"
    } else {
        "当前没有正在进行的 PDF 版面处理组件安装任务。"
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
