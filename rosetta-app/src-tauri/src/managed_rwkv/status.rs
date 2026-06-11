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
    Metallib,
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
pub fn build_static_status(app: &AppHandle) -> Result<StaticStatus, String> {
    let Some(profile) = current_profile() else {
        return Ok(StaticStatus::unsupported());
    };

    let layout = RuntimeLayout::from_app(app, profile)?;
    let sidecar_path = locate_sidecar(app, profile);
    let tokenizer_path = locate_tokenizer(app, profile);
    let metallib_path = if profile.backend == "mlx" {
        locate_metallib(app, sidecar_path.as_deref())
    } else {
        None
    };

    let install_plan = build_install_plan(
        profile,
        &layout,
        &sidecar_path,
        &tokenizer_path,
        &metallib_path,
    );
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
        metallib_path,
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
    /// `default.metallib` source path. Lifecycle copies it next to the sidecar
    /// at start time (or falls back to setting cwd) so the MLX backend can find
    /// it. `None` on non-MLX profiles.
    pub metallib_path: Option<PathBuf>,
    pub install_plan: ManagedRuntimeInstallPlan,
    pub initial_state: ManagedRuntimeState,
}

impl StaticStatus {
    fn unsupported() -> Self {
        Self {
            profile: &super::profile::MACOS_ARM64_WEBRWKV,
            layout: RuntimeLayout::resolve(Path::new(""), &super::profile::MACOS_ARM64_WEBRWKV),
            sidecar_path: None,
            tokenizer_path: None,
            metallib_path: None,
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

        // For zip profiles, after install the zip is deleted and the model is
        // the extracted directory. Prefer that as the "model path" the user
        // sees when it exists, so the Settings panel doesn't keep showing a
        // path to a file that's been deleted.
        let model_display_path = self
            .layout
            .model_extracted_dir
            .as_ref()
            .filter(|d| d.exists())
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| self.layout.model_file.display().to_string());
        let paths = ManagedRuntimePaths {
            sidecar: self.sidecar_path.map(|p| p.display().to_string()),
            tokenizer: self.tokenizer_path.map(|p| p.display().to_string()),
            model_file: model_display_path,
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
    metallib_path: &Option<PathBuf>,
) -> ManagedRuntimeInstallPlan {
    let mut items = Vec::with_capacity(4);

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

    if profile.backend == "mlx" {
        items.push(make_item(
            InstallItemKind::Metallib,
            metallib_path.as_deref(),
            "MLX 运行所需的 default.metallib 未找到。请重新运行 src-tauri/scripts/fetch-rwkv-sidecar.sh（已在 2026-06-10 后更新为同时 stage metallib）。".to_string(),
            "MLX default.metallib 已就绪。".to_string(),
        ));
    }

    let model_ready_path = layout
        .model_extracted_dir
        .as_deref()
        .filter(|p| p.exists())
        .or_else(|| {
            if layout.model_file.is_file() {
                Some(layout.model_file.as_path())
            } else {
                None
            }
        });
    items.push(make_item(
        InstallItemKind::Model,
        model_ready_path,
        format!(
            "翻译模型尚未下载 ({})。下次进入 Phase 4 后会从 UI 一键下载。",
            profile.model_filename
        ),
        format!("翻译模型 {} 已就绪。", profile.model_filename),
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
/// Tauri 2 sidecar layout discovered via A2 bundle test (2026-05-14):
///
/// - **Dev**: `<src-tauri>/binaries/<name>-<target-triple>` —
///   `scripts/fetch-rwkv-sidecar.sh` stages with the triple suffix so multiple
///   platforms can coexist; the dev runtime keeps the full name.
/// - **Bundle**: `<App>.app/Contents/MacOS/<name>` — Tauri 2 strips the
///   `-<target-triple>` suffix when packaging the bundle (each `.app` is
///   architecture-specific so the suffix would be redundant).
///
/// We probe both names in each location so the resolver doesn't care which
/// mode we're running under.
fn locate_sidecar(app: &AppHandle, profile: &RuntimeProfile) -> Option<PathBuf> {
    let full_name = profile.sidecar_binary_name;
    // Strip the `-aarch64-apple-darwin` (or whichever) suffix that Tauri's
    // bundler removes in production. Look up dynamically rather than
    // hard-coding the triple so the same logic works on future Intel /
    // Windows bundles too.
    let bundle_name = strip_target_triple_suffix(full_name);

    // Bundle: derive from resource_dir().parent()/MacOS/<bundle_name>.
    if let Ok(resource_dir) = app.path().resource_dir() {
        if let Some(contents_dir) = resource_dir.parent() {
            let macos_dir = contents_dir.join("MacOS");
            for candidate in [macos_dir.join(bundle_name), macos_dir.join(full_name)] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // Dev: <CARGO_MANIFEST_DIR>/binaries/<full_name> (with triple). Some
    // future dev flows might also stage the trimmed name; try both.
    let bin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    for candidate in [bin_dir.join(full_name), bin_dir.join(bundle_name)] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Strip a `-<target-triple>` suffix if present. Mirrors Tauri 2's bundler
/// renaming. Triples we care about right now: `aarch64-apple-darwin`,
/// `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`. Pattern: trailing
/// `-<arch>-<vendor>-<sys>(-<env>)?` separated by dashes. Conservatively we
/// just match by known suffixes to avoid false positives.
fn strip_target_triple_suffix(name: &str) -> &str {
    const SUFFIXES: &[&str] = &[
        "-aarch64-apple-darwin",
        "-x86_64-apple-darwin",
        "-x86_64-pc-windows-msvc.exe",
        "-x86_64-pc-windows-gnu.exe",
    ];
    for suffix in SUFFIXES {
        if let Some(prefix) = name.strip_suffix(suffix) {
            return prefix;
        }
    }
    name
}

/// Locate `default.metallib` across dev / bundle contexts.
///
/// MLX needs this file at runtime; the easiest place to find it is next to
/// the sidecar (which is where `fetch-rwkv-sidecar.sh` stages it in dev). For
/// the bundled `.app` the file ships as a Tauri resource under
/// `Contents/Resources/resources/rwkv-sidecar/default.metallib`. Lifecycle
/// will copy it next to the sidecar at start time if it's not already there.
fn locate_metallib(app: &AppHandle, sidecar_path: Option<&Path>) -> Option<PathBuf> {
    const METALLIB_NAME: &str = "default.metallib";

    if let Some(sidecar) = sidecar_path {
        if let Some(parent) = sidecar.parent() {
            let candidate = parent.join(METALLIB_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in [
            resource_dir
                .join("resources")
                .join("rwkv-sidecar")
                .join(METALLIB_NAME),
            resource_dir
                .join("_up_")
                .join("resources")
                .join("rwkv-sidecar")
                .join(METALLIB_NAME),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(METALLIB_NAME);
    if dev_path.is_file() {
        return Some(dev_path);
    }

    None
}

/// Locate the tokenizer across dev / bundle contexts.
///
/// Tauri 2 resource layout discovered via A2 bundle test (2026-05-14):
///
/// - **Bundle**: `<App>.app/Contents/Resources/<rel-path>` — Tauri 2 places
///   `bundle.resources` entries directly under `Contents/Resources/`. (Tauri 1
///   used to interpose an `_up_/` directory; Tauri 2 does not.)
/// - **Dev**: `<src-tauri>/<rel-path>`.
///
/// Our `tauri.macos.conf.json` declares `resources/rwkv-sidecar/*`, so the
/// tokenizer lands at `Contents/Resources/resources/rwkv-sidecar/<name>` in
/// bundle and `src-tauri/resources/rwkv-sidecar/<name>` in dev.
fn locate_tokenizer(app: &AppHandle, profile: &RuntimeProfile) -> Option<PathBuf> {
    let name = profile.tokenizer_filename;

    // Bundle path (Tauri 2): Contents/Resources/resources/rwkv-sidecar/<name>.
    // Also probe the legacy `_up_/` path in case a future Tauri release
    // reintroduces it; harmless when the file isn't there.
    if let Ok(resource_dir) = app.path().resource_dir() {
        let base = resource_dir.join("resources").join("rwkv-sidecar");
        for candidate in [
            base.join(name),
            resource_dir
                .join("_up_")
                .join("resources")
                .join("rwkv-sidecar")
                .join(name),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // Dev: <CARGO_MANIFEST_DIR>/resources/rwkv-sidecar/<name>.
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("rwkv-sidecar")
        .join(name);
    if dev_path.is_file() {
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
        let layout = RuntimeLayout::resolve(Path::new("/tmp/rosetta-fake"), &MACOS_ARM64_WEBRWKV);
        let plan = build_install_plan(&MACOS_ARM64_WEBRWKV, &layout, &None, &None, &None);
        assert!(!plan.ready);
        // WebRWKV profile: no metallib item, so still 3 items.
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
            &None,
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
