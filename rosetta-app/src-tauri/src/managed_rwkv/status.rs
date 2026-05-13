//! Compatibility detection + install plan + status snapshot.
//!
//! This module owns the *static* parts of the runtime story: "is this host
//! supported?", "do the bundled / downloaded artifacts exist where we expect
//! them?", and "what overall state should the UI show?". It deliberately
//! does NOT spawn or probe the sidecar; that's `lifecycle.rs`.
//!
//! Two paths are resolved here that don't live under app data:
//!
//! - The sidecar binary itself, which ships in the Tauri bundle under
//!   `Contents/MacOS/<sidecar>` and is staged at
//!   `<src-tauri>/binaries/<sidecar>` during local dev.
//! - The tokenizer file, bundled at `Contents/Resources/_up_/resources/rwkv-sidecar/<file>`
//!   and staged at `<src-tauri>/resources/rwkv-sidecar/<file>` during dev.
//!
//! Phase 4 will fill in the model download path; until then the model file
//! check just reports "missing".

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::layout::RuntimeLayout;
use super::profile::{current_profile, RuntimeProfile, RuntimeProfileSummary};

/// Overall lifecycle state surfaced to the UI.
///
/// Note this is intentionally *higher level* than the lifecycle's
/// "is the process actually running right now?" — it's the "what button
/// should the Settings panel show?" question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedRuntimeState {
    /// Platform / architecture combination has no enabled profile.
    Unsupported,
    /// Profile exists; some artifact (sidecar / tokenizer / model) is missing.
    NotInstalled,
    /// All artifacts present, sidecar idle.
    Installed,
    /// Sidecar starting up; `/health` not yet responding.
    Starting,
    /// Sidecar running and `/health` returns 200.
    Ready,
    /// Sidecar was running but crashed / exited unexpectedly.
    Failed,
    /// User explicitly stopped the sidecar.
    Stopped,
}

/// Per-artifact resolution for the install plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallItemKind {
    Sidecar,
    Tokenizer,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallItemState {
    Missing,
    Present,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallItem {
    pub kind: InstallItemKind,
    pub state: InstallItemState,
    pub path: String,
    pub size_bytes: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeInstallPlan {
    pub ready: bool,
    pub items: Vec<InstallItem>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimePaths {
    pub sidecar: Option<String>,
    pub tokenizer: Option<String>,
    pub model_file: String,
    pub logs_dir: String,
}

/// Live-status fields the lifecycle owns. `None` when the sidecar is idle.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeProcessSnapshot {
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub base_url: Option<String>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeStatus {
    pub state: ManagedRuntimeState,
    pub message: String,
    pub profile: Option<RuntimeProfileSummary>,
    pub paths: Option<ManagedRuntimePaths>,
    pub install_plan: Option<ManagedRuntimeInstallPlan>,
    pub process: ManagedRuntimeProcessSnapshot,
}

/// Compute everything but the live process snapshot. Lifecycle layers the
/// process snapshot on top before returning the status to the frontend.
pub fn build_static_status(
    app: &AppHandle,
) -> Result<StaticStatus, String> {
    let Some(profile) = current_profile() else {
        return Ok(StaticStatus::unsupported());
    };

    let layout = RuntimeLayout::from_app(app, profile)?;
    let sidecar_path = locate_sidecar(app, profile);
    let tokenizer_path = locate_tokenizer(app, profile);

    let install_plan = build_install_plan(profile, &layout, &sidecar_path, &tokenizer_path);
    let initial_state = if install_plan.ready {
        ManagedRuntimeState::Installed
    } else {
        ManagedRuntimeState::NotInstalled
    };

    Ok(StaticStatus {
        profile,
        layout,
        sidecar_path,
        tokenizer_path,
        install_plan,
        initial_state,
    })
}

/// Intermediate value handed to the lifecycle layer so it can decorate with
/// live process info before serializing back to the frontend.
pub struct StaticStatus {
    pub profile: &'static RuntimeProfile,
    pub layout: RuntimeLayout,
    pub sidecar_path: Option<PathBuf>,
    pub tokenizer_path: Option<PathBuf>,
    pub install_plan: ManagedRuntimeInstallPlan,
    pub initial_state: ManagedRuntimeState,
}

impl StaticStatus {
    fn unsupported() -> Self {
        Self {
            profile: &super::profile::MACOS_ARM64_WEBRWKV,
            layout: RuntimeLayout::resolve(
                Path::new(""),
                &super::profile::MACOS_ARM64_WEBRWKV,
            ),
            sidecar_path: None,
            tokenizer_path: None,
            install_plan: ManagedRuntimeInstallPlan {
                ready: false,
                items: Vec::new(),
                message: "当前平台暂不支持本地 RWKV 运行时（仅支持 macOS Apple Silicon）。"
                    .to_string(),
            },
            initial_state: ManagedRuntimeState::Unsupported,
        }
    }

    pub fn into_status(self, process: ManagedRuntimeProcessSnapshot) -> ManagedRuntimeStatus {
        if matches!(self.initial_state, ManagedRuntimeState::Unsupported) {
            return ManagedRuntimeStatus {
                state: ManagedRuntimeState::Unsupported,
                message: self.install_plan.message,
                profile: None,
                paths: None,
                install_plan: None,
                process,
            };
        }

        let paths = ManagedRuntimePaths {
            sidecar: self.sidecar_path.map(|p| p.display().to_string()),
            tokenizer: self.tokenizer_path.map(|p| p.display().to_string()),
            model_file: self.layout.model_file.display().to_string(),
            logs_dir: self.layout.logs_dir.display().to_string(),
        };
        let message = if self.install_plan.ready {
            "本地 RWKV 运行时已安装，可启动。".to_string()
        } else {
            self.install_plan.message.clone()
        };

        ManagedRuntimeStatus {
            state: self.initial_state,
            message,
            profile: Some(RuntimeProfileSummary::from_profile(self.profile)),
            paths: Some(paths),
            install_plan: Some(self.install_plan),
            process,
        }
    }
}

fn build_install_plan(
    profile: &RuntimeProfile,
    layout: &RuntimeLayout,
    sidecar_path: &Option<PathBuf>,
    tokenizer_path: &Option<PathBuf>,
) -> ManagedRuntimeInstallPlan {
    let mut items = Vec::with_capacity(3);

    items.push(make_item(
        InstallItemKind::Sidecar,
        sidecar_path.as_deref(),
        format!(
            "Sidecar 二进制 ({}) 未在应用包内找到。请运行 src-tauri/scripts/fetch-rwkv-sidecar.sh。",
            profile.sidecar_binary_name
        ),
        "Sidecar 二进制已就绪。".to_string(),
    ));

    items.push(make_item(
        InstallItemKind::Tokenizer,
        tokenizer_path.as_deref(),
        format!("分词表 ({}) 未在应用包内找到。", profile.tokenizer_filename),
        "分词表已就绪。".to_string(),
    ));

    items.push(make_item(
        InstallItemKind::Model,
        if layout.model_file.is_file() {
            Some(layout.model_file.as_path())
        } else {
            None
        },
        format!(
            "翻译模型尚未下载 ({})。下次进入 Phase 4 后会从 UI 一键下载。",
            profile.model_filename
        ),
        format!(
            "翻译模型 {} 已就绪。",
            profile.model_filename
        ),
    ));

    let ready = items
        .iter()
        .all(|item| item.state == InstallItemState::Present);
    let message = if ready {
        "本地 RWKV 运行时所需文件全部就绪。".to_string()
    } else {
        "本地 RWKV 运行时尚未就绪，请检查缺失项。".to_string()
    };

    ManagedRuntimeInstallPlan {
        ready,
        items,
        message,
    }
}

fn make_item(
    kind: InstallItemKind,
    path: Option<&Path>,
    missing_message: String,
    present_message: String,
) -> InstallItem {
    if let Some(p) = path {
        let size_bytes = std::fs::metadata(p).ok().map(|meta| meta.len());
        InstallItem {
            kind,
            state: InstallItemState::Present,
            path: p.display().to_string(),
            size_bytes,
            message: present_message,
        }
    } else {
        InstallItem {
            kind,
            state: InstallItemState::Missing,
            path: String::new(),
            size_bytes: None,
            message: missing_message,
        }
    }
}

/// Locate the sidecar binary across dev / bundle contexts.
///
/// - **Bundle**: `<App>.app/Contents/MacOS/<sidecar-name>` (sibling of the
///   main Rosetta binary; this is where Tauri's externalBin places it).
/// - **Dev**: `<src-tauri>/binaries/<sidecar-name>` (where
///   `scripts/fetch-rwkv-sidecar.sh` stages it).
fn locate_sidecar(app: &AppHandle, profile: &RuntimeProfile) -> Option<PathBuf> {
    let name = profile.sidecar_binary_name;

    // Bundle: derive from resource_dir().parent()/MacOS/.
    if let Ok(resource_dir) = app.path().resource_dir() {
        if let Some(contents_dir) = resource_dir.parent() {
            let candidate = contents_dir.join("MacOS").join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // Dev: <CARGO_MANIFEST_DIR>/binaries/<name>. CARGO_MANIFEST_DIR is the
    // crate root (src-tauri/); the fetch script stages binaries/ there.
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(name);
    if dev_path.exists() {
        return Some(dev_path);
    }

    None
}

/// Locate the tokenizer across dev / bundle contexts.
fn locate_tokenizer(app: &AppHandle, profile: &RuntimeProfile) -> Option<PathBuf> {
    let name = profile.tokenizer_filename;

    // Bundle: Tauri places `bundle.resources` entries under
    // Contents/Resources/_up_/<rel-path>. Our tauri.macos.conf.json declares
    // `resources/rwkv-sidecar/*`, so the file ends up at
    // Contents/Resources/_up_/resources/rwkv-sidecar/<name>.
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundle_candidate = resource_dir
            .join("_up_")
            .join("resources")
            .join("rwkv-sidecar")
            .join(name);
        if bundle_candidate.exists() {
            return Some(bundle_candidate);
        }
    }

    // Dev: <CARGO_MANIFEST_DIR>/resources/rwkv-sidecar/<name>.
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("rwkv-sidecar")
        .join(name);
    if dev_path.exists() {
        return Some(dev_path);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_rwkv::profile::MACOS_ARM64_WEBRWKV;

    #[test]
    fn install_plan_missing_all_artifacts_when_none_present() {
        let layout =
            RuntimeLayout::resolve(Path::new("/tmp/rosetta-fake"), &MACOS_ARM64_WEBRWKV);
        let plan = build_install_plan(&MACOS_ARM64_WEBRWKV, &layout, &None, &None);
        assert!(!plan.ready);
        assert_eq!(plan.items.len(), 3);
        for item in &plan.items {
            assert_eq!(item.state, InstallItemState::Missing);
        }
    }

    #[test]
    fn install_plan_ready_when_all_artifacts_present() {
        let tmp = std::env::temp_dir().join(format!(
            "rosetta-install-plan-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).expect("create tmp");
        let layout = RuntimeLayout::resolve(&tmp, &MACOS_ARM64_WEBRWKV);
        layout.ensure_dirs().expect("ensure_dirs");

        // Fake sidecar + tokenizer + model files.
        let sidecar = tmp.join("fake-sidecar");
        let tokenizer = tmp.join("fake-tokenizer");
        std::fs::write(&sidecar, b"binary").expect("write sidecar");
        std::fs::write(&tokenizer, b"vocab").expect("write tokenizer");
        std::fs::write(&layout.model_file, b"weights").expect("write model");

        let plan = build_install_plan(
            &MACOS_ARM64_WEBRWKV,
            &layout,
            &Some(sidecar),
            &Some(tokenizer),
        );
        assert!(plan.ready, "plan should be ready when all artifacts exist");
        for item in &plan.items {
            assert_eq!(item.state, InstallItemState::Present);
        }

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn into_status_returns_unsupported_when_initial_state_unsupported() {
        let status =
            StaticStatus::unsupported().into_status(ManagedRuntimeProcessSnapshot::default());
        assert_eq!(status.state, ManagedRuntimeState::Unsupported);
        assert!(status.profile.is_none());
        assert!(status.paths.is_none());
    }
}
