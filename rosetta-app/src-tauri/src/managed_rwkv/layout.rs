//! Filesystem layout for the managed RWKV runtime.
//!
//! All managed-runtime state lives under `<app-local-data>/managed-rwkv/`:
//!
//! ```text
//! managed-rwkv/
//!   models/
//!     <profile.model_directory_name>/
//!       <profile.model_filename>          ← Phase 4 downloads, never committed
//!       manifest.json                     ← Phase 4 writes after verify
//!   runtime-state/
//!     active-runtime.json                 ← live port + pid snapshot
//!   logs/
//!     runtime.log                         ← redacted sidecar stdout/stderr
//! ```
//!
//! Sidecar binary + tokenizer are *not* here — they ship inside the app
//! bundle (`<App>.app/Contents/MacOS/<sidecar>` and
//! `<App>.app/Contents/Resources/_up_/resources/rwkv-sidecar/<tokenizer>`)
//! so they participate in app codesigning and stay tightly coupled to the
//! Rosetta version that ships them.
//!
//! Path resolution is deliberately *infallible by construction*: we compute
//! paths from the AppHandle (or a passed-in root for tests) without touching
//! the filesystem. Creating the directories is a separate explicit step
//! (`ensure_layout`) so a dev box that never starts the runtime doesn't pay
//! for empty dirs being scattered around.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use super::profile::RuntimeProfile;

const MANAGED_ROOT_DIR: &str = "managed-rwkv";
const MODELS_DIR: &str = "models";
const RUNTIME_STATE_DIR: &str = "runtime-state";
const LOGS_DIR: &str = "logs";

const ACTIVE_RUNTIME_FILE: &str = "active-runtime.json";
const RUNTIME_LOG_FILE: &str = "runtime.log";
const MODEL_MANIFEST_FILE: &str = "manifest.json";

/// Resolved paths for one profile's data on a specific install.
///
/// Some fields are not read in Phase 3 but exist as part of the documented
/// layout — Phase 4 writes `model_manifest_file` after download, lifecycle's
/// future "active-runtime.json" snapshot uses `active_runtime_file`, and the
/// download/status code uses `root` / `models_dir` for housekeeping. They
/// stay in the struct so the layout is a single source of truth.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RuntimeLayout {
    pub root: PathBuf,
    pub models_dir: PathBuf,
    pub model_dir: PathBuf,
    pub model_file: PathBuf,
    pub model_manifest_file: PathBuf,
    pub runtime_state_dir: PathBuf,
    pub active_runtime_file: PathBuf,
    pub logs_dir: PathBuf,
    pub runtime_log_file: PathBuf,
}

impl RuntimeLayout {
    /// Compute the layout under a base directory (e.g. AppHandle's
    /// `app_local_data_dir`).
    pub fn resolve(base: &Path, profile: &RuntimeProfile) -> Self {
        let root = base.join(MANAGED_ROOT_DIR);
        let models_dir = root.join(MODELS_DIR);
        let model_dir = models_dir.join(profile.model_directory_name);
        let model_file = model_dir.join(profile.model_filename);
        let model_manifest_file = model_dir.join(MODEL_MANIFEST_FILE);
        let runtime_state_dir = root.join(RUNTIME_STATE_DIR);
        let active_runtime_file = runtime_state_dir.join(ACTIVE_RUNTIME_FILE);
        let logs_dir = root.join(LOGS_DIR);
        let runtime_log_file = logs_dir.join(RUNTIME_LOG_FILE);

        Self {
            root,
            models_dir,
            model_dir,
            model_file,
            model_manifest_file,
            runtime_state_dir,
            active_runtime_file,
            logs_dir,
            runtime_log_file,
        }
    }

    /// Resolve from an `AppHandle`. Equivalent to `resolve(app_local_data, ..)`.
    pub fn from_app(app: &AppHandle, profile: &RuntimeProfile) -> Result<Self, String> {
        let base = app.path().app_local_data_dir().map_err(|error| {
            format!("无法解析 Rosetta 应用本地数据目录: {error}")
        })?;
        Ok(Self::resolve(&base, profile))
    }

    /// Create model / state / logs directories. The model file itself is
    /// produced by Phase 4 download; here we only ensure its parent exists.
    pub fn ensure_dirs(&self) -> Result<(), String> {
        for dir in [&self.model_dir, &self.runtime_state_dir, &self.logs_dir] {
            std::fs::create_dir_all(dir).map_err(|error| {
                format!("无法创建 {}: {error}", dir.display())
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_rwkv::profile::MACOS_ARM64_WEBRWKV;

    #[test]
    fn layout_paths_nest_under_managed_rwkv_root() {
        let base = Path::new("/tmp/rosetta-test");
        let layout = RuntimeLayout::resolve(base, &MACOS_ARM64_WEBRWKV);

        assert!(layout.root.ends_with("managed-rwkv"));
        assert!(layout.model_dir.starts_with(&layout.models_dir));
        assert!(layout.model_file.starts_with(&layout.model_dir));
        assert!(layout.runtime_state_dir.starts_with(&layout.root));
        assert!(layout.logs_dir.starts_with(&layout.root));
        assert_eq!(
            layout.model_file.file_name().unwrap().to_string_lossy(),
            MACOS_ARM64_WEBRWKV.model_filename,
        );
    }

    #[test]
    fn ensure_dirs_creates_only_data_directories() {
        let tmp = tempdir_path();
        let layout = RuntimeLayout::resolve(&tmp, &MACOS_ARM64_WEBRWKV);
        layout.ensure_dirs().expect("ensure_dirs");

        assert!(layout.model_dir.is_dir());
        assert!(layout.runtime_state_dir.is_dir());
        assert!(layout.logs_dir.is_dir());
        // Model file is NOT created — that's Phase 4's job.
        assert!(!layout.model_file.exists());
        std::fs::remove_dir_all(&tmp).ok();
    }

    fn tempdir_path() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rosetta-managed-rwkv-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        p
    }
}
