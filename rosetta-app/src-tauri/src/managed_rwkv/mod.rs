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
pub mod migrate;
pub mod profile;
pub mod status;

use serde::Serialize;
use std::io::Write;
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
pub struct ManagedRuntimeDebugBundle {
    pub archive_path: String,
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
pub fn get_managed_rwkv_install_plan(app: AppHandle) -> Result<ManagedRuntimeInstallPlan, String> {
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
    if matches!(
        static_status.initial_state,
        ManagedRuntimeState::Unsupported
    ) {
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
    let model = static_status
        .layout
        .model_extracted_dir
        .as_deref()
        .unwrap_or(static_status.layout.model_file.as_path());
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

#[tauri::command]
pub async fn export_managed_rwkv_debug_bundle(
    app: AppHandle,
    registry: State<'_, Registry>,
    install_registry: State<'_, InstallStateRegistry>,
) -> Result<ManagedRuntimeDebugBundle, String> {
    let static_status = build_static_status(&app)?;
    static_status.layout.ensure_dirs()?;

    let timestamp = debug_timestamp();
    let archive_path = static_status
        .layout
        .logs_dir
        .join(format!("rosetta-rwkv-debug-{timestamp}.zip"));

    let file = std::fs::File::create(&archive_path)
        .map_err(|error| format!("无法创建调试日志压缩包: {error}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let (process_snapshot, lifecycle_state) = current_process_snapshot(&registry).await;
    let layout = static_status.layout.clone();
    let mut status = static_status.into_status(process_snapshot);
    if let Some(live) = lifecycle_state {
        if !matches!(status.state, ManagedRuntimeState::Unsupported) {
            status.state = live;
        }
    }
    let progress = install_registry.snapshot().await;

    add_json_entry(&mut zip, options, "managed-rwkv-status.json", &status)?;
    add_json_entry(
        &mut zip,
        options,
        "managed-rwkv-install-progress.json",
        &progress,
    )?;
    add_text_entry(
        &mut zip,
        options,
        "README.txt",
        "Rosetta managed RWKV debug bundle.\nUser document contents are not included.\n",
    )?;

    let profile = profile::current_profile()
        .map(|p| format!("{p:#?}\n"))
        .unwrap_or_else(|| "unsupported platform\n".to_string());
    add_text_entry(&mut zip, options, "managed-rwkv-profile.txt", &profile)?;

    add_file_if_exists(
        &mut zip,
        options,
        &layout.runtime_log_file,
        "managed-rwkv/runtime.log",
    )?;
    add_file_if_exists(
        &mut zip,
        options,
        &layout.model_manifest_file,
        "managed-rwkv/model-manifest.json",
    )?;
    if let Some(manifest) = layout.runtime_manifest_file.as_ref() {
        add_file_if_exists(
            &mut zip,
            options,
            manifest,
            "managed-rwkv/runtime-manifest.json",
        )?;
    }

    zip.finish()
        .map_err(|error| format!("写入调试日志压缩包失败: {error}"))?;

    Ok(ManagedRuntimeDebugBundle {
        archive_path: archive_path.display().to_string(),
        message: "已导出本地 RWKV 调试日志。".to_string(),
    })
}

fn add_json_entry<T: Serialize>(
    zip: &mut zip::ZipWriter<std::fs::File>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    value: &T,
) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|error| format!("序列化 {name} 失败: {error}"))?;
    add_text_entry(zip, options, name, &body)
}

fn add_text_entry(
    zip: &mut zip::ZipWriter<std::fs::File>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    body: &str,
) -> Result<(), String> {
    zip.start_file(name, options)
        .map_err(|error| format!("创建 zip 条目 {name} 失败: {error}"))?;
    zip.write_all(body.as_bytes())
        .map_err(|error| format!("写入 zip 条目 {name} 失败: {error}"))
}

fn add_file_if_exists(
    zip: &mut zip::ZipWriter<std::fs::File>,
    options: zip::write::SimpleFileOptions,
    path: &std::path::Path,
    name: &str,
) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("读取日志文件 {} 失败: {error}", path.display()))?;
    zip.start_file(name, options)
        .map_err(|error| format!("创建 zip 条目 {name} 失败: {error}"))?;
    zip.write_all(&bytes)
        .map_err(|error| format!("写入 zip 条目 {name} 失败: {error}"))
}

fn debug_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}
