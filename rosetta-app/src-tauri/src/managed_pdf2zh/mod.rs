pub mod install;
pub mod layout;
pub mod openai_shim;
pub mod profile;
pub mod status;
pub mod worker;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

pub use install::Pdf2zhInstallRegistry as InstallStateRegistry;
pub use status::build_static_status;
pub use worker::{Pdf2zhWorkerStatus, WorkerState as Pdf2zhWorkerState};

/// Warm up the persistent pdf2zh worker (heavy Python imports + layout model)
/// so the first translate click doesn't pay the ~13 s import. Fire-and-forget;
/// the worker stays warm for the rest of the session (no idle reaper).
#[tauri::command]
pub async fn prewarm_pdf2zh_worker(app: AppHandle) -> Result<bool, String> {
    worker::prewarm_worker(&app).await
}

/// Snapshot of the worker lifecycle for the header indicator. The frontend
/// also listens for `rosetta-pdf2zh-worker-status` events for live updates;
/// this command is the one-shot fetch the UI uses on mount before any event
/// has fired.
#[tauri::command]
pub fn get_pdf2zh_worker_status(
    state: State<'_, Pdf2zhWorkerState>,
) -> Result<Pdf2zhWorkerStatus, String> {
    Ok(state.status_snapshot())
}

/// Kick off prewarm in the background. Called once from lib.rs setup after
/// the main window is shown so the user never waits for the import.
pub fn prewarm_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = worker::prewarm_worker(&app).await {
            eprintln!("[pdf2zh-worker] background prewarm failed: {error}");
        }
    });
}

pub fn suspend_worker(app: &AppHandle) {
    if let Some(state) = app.try_state::<Pdf2zhWorkerState>() {
        state.request_shutdown();
    }
}

pub async fn shutdown_worker(app: &AppHandle) -> bool {
    worker::shutdown_worker(app).await
}

pub async fn shutdown_worker_for_exit(app: &AppHandle) {
    if shutdown_worker(app).await {
        eprintln!("[pdf2zh-worker] stopped during app exit");
    }
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
