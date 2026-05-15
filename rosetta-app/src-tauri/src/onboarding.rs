//! First-launch onboarding state + window orchestration.
//!
//! Rosetta launches into **one of two windows**, chosen by `lib.rs::setup`:
//!
//! - `onboarding` (640×480, non-resizable, no decorations sidebar): shown
//!   when this is a fresh user / model file is missing. Walks them through
//!   downloading the local engine before they ever see the Workspace.
//! - `main` (the Workspace): shown when onboarding has been marked complete
//!   AND the local model file is present (or the user opted in to "use my
//!   own external API" path).
//!
//! Both windows are declared in `tauri.conf.json` with `visible: false`; this
//! module decides which one to flip on at startup, and during the
//! "完成" transition closes the onboarding window + shows main atomically.
//!
//! ### State persistence
//!
//! Source of truth: `<app_local_data_dir>/onboarding.json`. We keep state
//! out of zustand-localStorage on purpose — users can clear browser-side
//! data (DevTools, OS-level cleanup), and we want a fresh user to see
//! onboarding even if main-window webview storage was wiped. The Rust-side
//! file survives WebKit cache resets.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::managed_rwkv;

const STATE_FILENAME: &str = "onboarding.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    /// User completed onboarding (either downloaded local model or explicitly
    /// chose "use my own external API"). Persists across launches.
    pub completed: bool,
    /// ISO-8601 timestamp of completion. Useful for change-log debugging,
    /// not user-visible.
    pub completed_at: Option<String>,
    /// User chose to skip local install in favour of external API. We
    /// remember this so the Workspace can route the user to Settings
    /// instead of nagging about local engine state.
    pub skipped_local_install: bool,
}

/// Snapshot delivered to the frontend at boot time and after each state
/// transition. `needs_onboarding` is the single boolean the setup hook /
/// frontend should branch on — derived from `completed` + actual on-disk
/// model presence so a deleted model auto-rewinds the user to onboarding.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingDecision {
    pub state: OnboardingState,
    pub model_installed: bool,
    pub needs_onboarding: bool,
}

fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("无法解析 app 数据目录: {e}"))?;
    Ok(dir.join(STATE_FILENAME))
}

pub fn load(app: &AppHandle) -> OnboardingState {
    let Ok(path) = state_path(app) else {
        return OnboardingState::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return OnboardingState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(app: &AppHandle, state: &OnboardingState) -> Result<(), String> {
    let path = state_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建 app 数据目录: {e}"))?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("无法序列化 onboarding state: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("无法写入 onboarding state: {e}"))?;
    Ok(())
}

/// Probe the runtime profile + filesystem to decide whether the user still
/// needs to see onboarding. `completed=true` AND model present → skip
/// onboarding. Either missing → show onboarding (with resume support).
pub fn decide(app: &AppHandle) -> OnboardingDecision {
    let state = load(app);
    let model_installed = match managed_rwkv::profile::current_profile() {
        Some(profile) => managed_rwkv::layout::RuntimeLayout::from_app(app, profile)
            .map(|layout| layout.model_file.is_file())
            .unwrap_or(false),
        None => false, // unsupported platform — onboarding will route to "use external API"
    };
    // User who opted out of local install doesn't need a model — but we still
    // need them to have completed onboarding once. `completed` alone wins
    // for the "skipped_local" path.
    let needs_onboarding = if state.skipped_local_install {
        !state.completed
    } else {
        !state.completed || !model_installed
    };
    OnboardingDecision {
        state,
        model_installed,
        needs_onboarding,
    }
}

// -----------------------------------------------------------------------------
// Tauri commands
// -----------------------------------------------------------------------------

#[tauri::command]
pub async fn get_onboarding_decision(app: AppHandle) -> Result<OnboardingDecision, String> {
    Ok(decide(&app))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteOnboardingRequest {
    /// `true` when the user explicitly chose "use my own API" instead of
    /// downloading the local engine.
    pub skipped_local_install: bool,
}

#[tauri::command]
pub async fn complete_onboarding_and_open_main(
    app: AppHandle,
    request: CompleteOnboardingRequest,
) -> Result<OnboardingDecision, String> {
    let state = OnboardingState {
        completed: true,
        completed_at: Some(iso_now()),
        skipped_local_install: request.skipped_local_install,
    };
    save(&app, &state)?;

    // Atomic-ish window swap: show main first (so the user always sees a
    // window) then close onboarding. If show-main fails, we deliberately
    // leave onboarding open so the user can retry.
    if let Some(main) = app.get_webview_window("main") {
        main.show().map_err(|e| format!("无法显示主窗口: {e}"))?;
        main.set_focus().ok();
    } else {
        return Err("主窗口未声明 (期望 label = 'main')。".to_string());
    }
    if let Some(onb) = app.get_webview_window("onboarding") {
        onb.close().ok();
    }

    // Signal the main window to discard any stale job history from a
    // previous session so the workspace opens to a clean welcome screen.
    app.emit("rosetta-onboarding-completed", ()).ok();

    Ok(decide(&app))
}

/// Open the onboarding window again. Useful for "Repair" / "Reinstall" entry
/// points from Settings, or future "Reset Rosetta" flows. Not used in P1
/// happy path but cheap to expose.
#[tauri::command]
pub async fn reopen_onboarding_window(app: AppHandle) -> Result<(), String> {
    if let Some(onb) = app.get_webview_window("onboarding") {
        onb.show().map_err(|e| format!("无法显示 onboarding 窗口: {e}"))?;
        onb.set_focus().ok();
        return Ok(());
    }
    Err("onboarding 窗口未声明。".to_string())
}

fn iso_now() -> String {
    // Reuse the same ISO format helper logic as managed_rwkv::install. We
    // duplicate here intentionally to keep onboarding.rs zero-dep on
    // managed_rwkv internals.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day, hour, min, sec) = secs_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn secs_to_ymdhms(mut secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    secs /= 60;
    let min = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    secs /= 24;
    let (year, month, day) = days_since_epoch_to_ymd(secs as i64);
    (year, month, day, hour, min, sec)
}

fn days_since_epoch_to_ymd(mut days: i64) -> (u32, u32, u32) {
    days += 719468;
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = (days - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as u32;
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_not_completed() {
        let s = OnboardingState::default();
        assert!(!s.completed);
        assert!(!s.skipped_local_install);
        assert!(s.completed_at.is_none());
    }

    #[test]
    fn iso_now_format_is_zulu() {
        let s = iso_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(s.chars().filter(|c| *c == 'T').count(), 1);
    }
}
