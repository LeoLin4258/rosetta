//! Managed local RWKV runtime (macOS ADR 0003 + Windows ADR 0006).
//!
//! Public surface is the seven Tauri commands at the bottom of this file.
//! Internals are split across:
//!
//! - `profile`   — platform runtime and artifact contracts
//! - `layout`    — app-data filesystem paths (models/runtime-state/logs)
//! - `status`    — compatibility / install-plan / static status snapshot
//! - `lifecycle` — port allocation, sidecar spawn / stop / probe, log tail
//!
//! Phase 3 ships the lifecycle commands wired to a running sidecar. Phase 4
//! will replace `install_managed_rwkv_runtime`'s stub with the real download
//! flow; the command exists now so the frontend can land the install button
//! UI in Phase 5 against a stable contract.

pub mod hardware;
pub mod install;
pub mod layout;
pub mod lifecycle;
pub mod migrate;
pub mod profile;
pub mod status;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

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

#[tauri::command]
pub fn get_managed_rwkv_hardware_support() -> Result<hardware::HardwareSupport, String> {
    let Some(profile) = profile::current_profile() else {
        return Ok(hardware::HardwareSupport {
            supported: false,
            gpu_name: None,
            compute_capability: None,
            message: "当前平台暂不支持本地 RWKV 运行时。".to_string(),
        });
    };
    Ok(hardware::inspect(profile))
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
pub fn get_managed_rwkv_install_plan(app: AppHandle) -> Result<ManagedRuntimeInstallPlan, String> {
    let static_status = build_static_status(&app)?;
    Ok(static_status.install_plan)
}

#[tauri::command]
pub async fn install_managed_rwkv_runtime(
    app: AppHandle,
    install_registry: State<'_, InstallStateRegistry>,
    runtime_registry: State<'_, Registry>,
    options: Option<InstallOptions>,
) -> Result<InstallResult, String> {
    let Some(profile) = profile::current_profile() else {
        return Err("当前平台不支持本地 RWKV 运行时。".to_string());
    };
    hardware::ensure_supported(profile)?;
    let layout = RuntimeLayout::from_app(&app, profile)?;
    let options = options.unwrap_or_default();

    let replacing_runtime_pack = profile.managed_runtime_directory_name.is_some()
        && (options.repair || !layout.is_runtime_installed(profile));
    if replacing_runtime_pack {
        let sidecar = layout.runtime_executable.as_deref();
        let tokenizer = layout
            .runtime_dir
            .as_ref()
            .map(|dir| dir.join(profile.tokenizer_filename));
        let model = layout
            .model_extracted_dir
            .as_deref()
            .unwrap_or(layout.model_file.as_path());
        eprintln!("[rwkv-install] stopping managed sidecar before replacing runtime pack");
        stop_sidecar(
            &runtime_registry,
            Some(profile),
            sidecar,
            tokenizer.as_deref(),
            Some(model),
        )
        .await?;
    }
    install_model(&app, install_registry.inner(), profile, &layout, options).await
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
    eprintln!("[rwkv-start] === start_managed_rwkv_runtime ===");
    let static_status = build_static_status(&app).map_err(|e| {
        eprintln!("[rwkv-start] build_static_status failed: {e}");
        e
    })?;
    eprintln!(
        "[rwkv-start]   initial_state: {:?}",
        static_status.initial_state
    );
    eprintln!(
        "[rwkv-start]   install_plan.ready: {}",
        static_status.install_plan.ready
    );
    for item in &static_status.install_plan.items {
        eprintln!(
            "[rwkv-start]   plan item: {:?} = {:?} ({})",
            item.kind, item.state, item.message
        );
    }
    eprintln!(
        "[rwkv-start]   sidecar_path: {:?}",
        static_status.sidecar_path
    );
    eprintln!(
        "[rwkv-start]   tokenizer_path: {:?}",
        static_status.tokenizer_path
    );
    eprintln!(
        "[rwkv-start]   runtime_dir: {:?}",
        static_status.layout.runtime_dir
    );
    eprintln!(
        "[rwkv-start]   model_file: {}",
        static_status.layout.model_file.display()
    );

    if matches!(
        static_status.initial_state,
        ManagedRuntimeState::Unsupported
    ) {
        let msg = static_status.hardware.message;
        eprintln!("[rwkv-start] ABORT: unsupported — {msg}");
        return Err(msg);
    }
    if !static_status.install_plan.ready {
        let msg = static_status.install_plan.message;
        eprintln!("[rwkv-start] ABORT: not ready — {msg}");
        return Err(msg);
    }

    let profile = static_status.profile;
    let sidecar = static_status.sidecar_path.ok_or_else(|| {
        let msg = "找不到 sidecar 二进制。".to_string();
        eprintln!("[rwkv-start] ABORT: {msg}");
        msg
    })?;
    let tokenizer = if profile.requires_tokenizer() {
        Some(static_status.tokenizer_path.ok_or_else(|| {
            let msg = "找不到分词表文件。".to_string();
            eprintln!("[rwkv-start] ABORT: {msg}");
            msg
        })?)
    } else {
        None
    };
    let model = static_status
        .layout
        .model_extracted_dir
        .clone()
        .unwrap_or_else(|| static_status.layout.model_file.clone());
    let log_file = static_status.layout.runtime_log_file.clone();
    let metallib = static_status.metallib_path.clone();

    static_status.layout.ensure_dirs()?;
    start_sidecar(
        &registry, profile, sidecar, tokenizer, model, log_file, metallib,
    )
    .await
}

#[tauri::command]
pub async fn stop_managed_rwkv_runtime(
    app: AppHandle,
    registry: State<'_, Registry>,
) -> Result<String, String> {
    let static_status = build_static_status(&app)?;
    let sidecar = static_status.sidecar_path.as_deref();
    let tokenizer = static_status.tokenizer_path.as_deref();
    let model = static_status.layout.model_file.as_path();
    stop_sidecar(
        &registry,
        Some(static_status.profile),
        sidecar,
        tokenizer,
        Some(model),
    )
    .await
}

pub async fn shutdown_managed_rwkv_runtime_for_exit(app: &AppHandle) {
    let Some(registry) = app.try_state::<Registry>() else {
        return;
    };

    let static_status = build_static_status(app);
    let result = match static_status {
        Ok(status) => {
            let sidecar = status.sidecar_path.as_deref();
            let tokenizer = status.tokenizer_path.as_deref();
            let model = status
                .layout
                .model_extracted_dir
                .as_deref()
                .unwrap_or(status.layout.model_file.as_path());
            stop_sidecar(
                &registry,
                Some(status.profile),
                sidecar,
                tokenizer,
                Some(model),
            )
            .await
        }
        Err(_) => stop_sidecar(&registry, None, None, None, None).await,
    };

    if let Err(error) = result {
        eprintln!("[managed-rwkv] app-exit cleanup failed: {error}");
    }
}

#[tauri::command]
pub async fn probe_managed_rwkv_runtime(
    app: AppHandle,
    registry: State<'_, Registry>,
) -> Result<ManagedRuntimeProbeResult, String> {
    let static_status = build_static_status(&app)?;
    if matches!(
        static_status.initial_state,
        ManagedRuntimeState::Unsupported
    ) {
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
