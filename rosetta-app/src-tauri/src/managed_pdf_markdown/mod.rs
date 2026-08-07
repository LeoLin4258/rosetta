mod install;
mod layout;
mod profile;
mod status;
mod worker;

use tauri::{AppHandle, State};

pub use install::{
    PdfMarkdownInstallOptions, PdfMarkdownInstallProgress, PdfMarkdownInstallRegistry,
    PdfMarkdownInstallResult,
};
pub use status::PdfMarkdownStatus;
pub use worker::{PdfMarkdownWorkerState, PdfMarkdownWorkerStatus};

#[tauri::command]
pub fn get_pdf_markdown_status(app: AppHandle) -> Result<PdfMarkdownStatus, String> {
    status::build_status(&app)
}

#[tauri::command]
pub async fn get_pdf_markdown_install_progress(
    state: State<'_, PdfMarkdownInstallRegistry>,
) -> Result<PdfMarkdownInstallProgress, String> {
    Ok(state.snapshot().await)
}

#[tauri::command]
pub async fn cancel_pdf_markdown_install(
    state: State<'_, PdfMarkdownInstallRegistry>,
) -> Result<bool, String> {
    Ok(state.request_cancel().await)
}

#[tauri::command]
pub async fn install_pdf_markdown_component(
    app: AppHandle,
    state: State<'_, PdfMarkdownInstallRegistry>,
    options: Option<PdfMarkdownInstallOptions>,
) -> Result<PdfMarkdownInstallResult, String> {
    let profile = profile::current_profile()
        .ok_or_else(|| "当前平台暂不支持 PDF Markdown 组件。".to_string())?;
    let layout = layout::PdfMarkdownLayout::from_app(&app, profile)?;
    let _ = crate::managed_pdf2zh::layout::locate_managed_python_host(&app)?;
    let force = options.unwrap_or_default().force;
    if force || layout.validate_install(profile).is_err() {
        let _ = worker::shutdown(&app).await;
    }
    install::install_component(&state, &layout, profile, force).await
}

#[tauri::command]
pub async fn repair_pdf_markdown_component(
    app: AppHandle,
    state: State<'_, PdfMarkdownInstallRegistry>,
) -> Result<PdfMarkdownInstallResult, String> {
    let profile = profile::current_profile()
        .ok_or_else(|| "当前平台暂不支持 PDF Markdown 组件。".to_string())?;
    let layout = layout::PdfMarkdownLayout::from_app(&app, profile)?;
    let _ = crate::managed_pdf2zh::layout::locate_managed_python_host(&app)?;
    let _ = worker::shutdown(&app).await;
    install::install_component(&state, &layout, profile, true).await
}

#[tauri::command]
pub async fn prewarm_pdf_markdown_worker(app: AppHandle) -> Result<bool, String> {
    worker::prewarm(&app).await
}

#[tauri::command]
pub fn get_pdf_markdown_worker_status(
    state: State<'_, PdfMarkdownWorkerState>,
) -> Result<PdfMarkdownWorkerStatus, String> {
    Ok(state.status_snapshot())
}

#[tauri::command]
pub async fn cancel_pdf_markdown_worker(app: AppHandle) -> Result<bool, String> {
    Ok(worker::cancel(&app).await)
}

pub async fn shutdown_worker_for_exit(app: &AppHandle) {
    let _ = worker::shutdown(app).await;
}
