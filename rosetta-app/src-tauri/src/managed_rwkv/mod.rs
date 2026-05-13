//! Managed local RWKV runtime (Phase 3 — macOS-first per ADR 0003).
//!
//! Public surface is the seven Tauri commands at the bottom of this file.
//! Internals are split across:
//!
//! - `profile`   — RuntimeProfile + the macOS / (disabled) Windows constants
//! - `layout`    — app-data filesystem paths (models/runtime-state/logs)
//! - `status`    — compatibility / install-plan / static status snapshot
//! - `lifecycle` — port allocation, sidecar spawn / stop / probe, log tail
//!
//! Phase 3 ships the lifecycle commands wired to a running sidecar. Phase 4
//! will replace `install_managed_rwkv_runtime`'s stub with the real download
//! flow; the command exists now so the frontend can land the install button
//! UI in Phase 5 against a stable contract.

pub mod install;
pub mod layout;
pub mod lifecycle;
pub mod profile;
pub mod status;

use serde::Serialize;
use tauri::{AppHandle, State};

use install::{install_model, InstallOptions, InstallProgress, InstallResult};
use layout::RuntimeLayout;
use lifecycle::{
    current_process_snapshot, probe_sidecar, read_log_tail, start_sidecar, stop_sidecar,
    ManagedRuntimeProbeResult, ManagedRuntimeStartResult,
};
use status::{
    build_static_status, ManagedRuntimeInstallPlan, ManagedRuntimeState, ManagedRuntimeStatus,
};

/// Re-export so `lib.rs` can manage the registry as Tauri state.
pub use install::InstallRegistry as InstallStateRegistry;
pub use lifecycle::ManagedRwkvRuntimeRegistry as Registry;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeLogsSummary {
    pub log_file: String,
    pub log_tail: Vec<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeCancelInstallResult {
    pub cancelled: bool,
    pub message: String,
}

// =============================================================================
// Tauri commands
// =============================================================================

#[tauri::command]
pub async fn get_managed_rwkv_runtime_status(
    app: AppHandle,
    registry: State<'_, Registry>,
) -> Result<ManagedRuntimeStatus, String> {
    let static_status = build_static_status(&app)?;
    let (process_snapshot, lifecycle_state) = current_process_snapshot(&registry).await;

    let mut status = static_status.into_status(process_snapshot);
    // Lifecycle state (Starting / Ready / Failed / Stopped) overrides the
    // static "Installed / NotInstalled" if the sidecar is alive or just was.
    if let Some(live) = lifecycle_state {
        if !matches!(status.state, ManagedRuntimeState::Unsupported) {
            status.state = live;
        }
    }
    Ok(status)
}

#[tauri::command]
pub fn get_managed_rwkv_install_plan(
    app: AppHandle,
) -> Result<ManagedRuntimeInstallPlan, String> {
    let static_status = build_static_status(&app)?;
    Ok(static_status.install_plan)
}

#[tauri::command]
pub async fn install_managed_rwkv_runtime(
    app: AppHandle,
    install_registry: State<'_, InstallStateRegistry>,
    options: Option<InstallOptions>,
) -> Result<InstallResult, String> {
    let Some(profile) = profile::current_profile() else {
        return Err("当前平台不支持本地 RWKV 运行时。".to_string());
    };
    let layout = RuntimeLayout::from_app(&app, profile)?;
    install_model(
        &app,
        install_registry.inner(),
        profile,
        &layout,
        options.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn get_managed_rwkv_install_progress(
    install_registry: State<'_, InstallStateRegistry>,
) -> Result<InstallProgress, String> {
    Ok(install_registry.snapshot().await)
}

#[tauri::command]
pub async fn cancel_managed_rwkv_install(
    install_registry: State<'_, InstallStateRegistry>,
) -> Result<ManagedRuntimeCancelInstallResult, String> {
    let cancelled = install_registry.request_cancel().await;
    Ok(ManagedRuntimeCancelInstallResult {
        cancelled,
        message: if cancelled {
            "已请求取消，当前批次完成后退出。".to_string()
        } else {
            "当前没有正在进行的安装任务。".to_string()
        },
    })
}

#[tauri::command]
pub async fn start_managed_rwkv_runtime(
    app: AppHandle,
    registry: State<'_, Registry>,
) -> Result<ManagedRuntimeStartResult, String> {
    let static_status = build_static_status(&app)?;
    if matches!(static_status.initial_state, ManagedRuntimeState::Unsupported) {
        return Err("当前平台不支持本地 RWKV 运行时。".to_string());
    }
    if !static_status.install_plan.ready {
        return Err(static_status.install_plan.message);
    }

    let profile = static_status.profile;
    let sidecar = static_status
        .sidecar_path
        .ok_or_else(|| "找不到 sidecar 二进制。".to_string())?;
    let tokenizer = static_status
        .tokenizer_path
        .ok_or_else(|| "找不到分词表文件。".to_string())?;
    let model = static_status.layout.model_file.clone();
    let log_file = static_status.layout.runtime_log_file.clone();

    static_status.layout.ensure_dirs()?;
    start_sidecar(&registry, profile, sidecar, tokenizer, model, log_file).await
}

#[tauri::command]
pub async fn stop_managed_rwkv_runtime(
    registry: State<'_, Registry>,
) -> Result<&'static str, String> {
    stop_sidecar(&registry).await
}

#[tauri::command]
pub async fn probe_managed_rwkv_runtime(
    app: AppHandle,
    registry: State<'_, Registry>,
) -> Result<ManagedRuntimeProbeResult, String> {
    let static_status = build_static_status(&app)?;
    if matches!(static_status.initial_state, ManagedRuntimeState::Unsupported) {
        return Err("当前平台不支持本地 RWKV 运行时。".to_string());
    }
    Ok(probe_sidecar(&registry, static_status.profile).await)
}

#[tauri::command]
pub fn get_managed_rwkv_runtime_logs_summary(
    app: AppHandle,
) -> Result<ManagedRuntimeLogsSummary, String> {
    let static_status = build_static_status(&app)?;
    let log_path = static_status.layout.runtime_log_file.clone();
    let tail = read_log_tail(&log_path)?;
    let message = if tail.is_empty() {
        "运行时日志为空或尚未生成。".to_string()
    } else {
        format!("已读取 {} 行运行时日志。", tail.len())
    };
    Ok(ManagedRuntimeLogsSummary {
        log_file: log_path.display().to_string(),
        log_tail: tail,
        message,
    })
}
