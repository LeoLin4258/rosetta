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

pub mod layout;
pub mod lifecycle;
pub mod profile;
pub mod status;

use serde::Serialize;
use tauri::{AppHandle, State};

use layout::RuntimeLayout;
use lifecycle::{
    current_process_snapshot, probe_sidecar, read_log_tail, start_sidecar, stop_sidecar,
    ManagedRuntimeProbeResult, ManagedRuntimeStartResult,
};
use status::{
    build_static_status, ManagedRuntimeInstallPlan, ManagedRuntimeState, ManagedRuntimeStatus,
};

/// Re-export so `lib.rs` can manage the registry as Tauri state.
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
pub struct ManagedRuntimeInstallStubResult {
    pub ready: bool,
    pub message: String,
    pub install_plan: ManagedRuntimeInstallPlan,
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
) -> Result<ManagedRuntimeInstallStubResult, String> {
    // Phase 4 will wire the actual download here. For Phase 3 we ensure the
    // app-data layout exists (so Phase 4's downloader has somewhere to write)
    // and report current readiness so the UI can show a coherent state.
    let static_status = build_static_status(&app)?;
    if let Some(profile) = profile::current_profile() {
        let layout = RuntimeLayout::from_app(&app, profile)?;
        layout.ensure_dirs()?;
    }

    Ok(ManagedRuntimeInstallStubResult {
        ready: static_status.install_plan.ready,
        message: "Phase 4 将接入自动下载；当前请等待 Phase 4 落地或手动放置模型文件。"
            .to_string(),
        install_plan: static_status.install_plan,
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
