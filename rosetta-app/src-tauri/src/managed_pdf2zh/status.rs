use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use super::{
    layout::{Pdf2zhLayout, DOCLAYOUT_MODEL_FILENAME},
    profile::{current_profile, Pdf2zhProfile, Pdf2zhProfileSummary, MACOS_ARM64_PDF2ZH},
};

const REQUIRED_PYTHON_PACKAGES: &[&str] = &["tqdm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pdf2zhState {
    Unsupported,
    NotInstalled,
    Installed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhInstallPlan {
    pub ready: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhPaths {
    pub bin: Option<String>,
    pub pack_dir: String,
    pub logs_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhStatus {
    pub state: Pdf2zhState,
    pub message: String,
    pub profile: Option<Pdf2zhProfileSummary>,
    pub paths: Option<Pdf2zhPaths>,
    pub install_plan: Option<Pdf2zhInstallPlan>,
}

pub struct StaticStatus {
    pub profile: &'static Pdf2zhProfile,
    pub layout: Pdf2zhLayout,
    pub bin_path: Option<PathBuf>,
    pub doclayout_model_path: Option<PathBuf>,
    pub state: Pdf2zhState,
    pub install_plan: Pdf2zhInstallPlan,
}

impl StaticStatus {
    fn unsupported() -> Self {
        let layout = Pdf2zhLayout::resolve(PathBuf::new(), &MACOS_ARM64_PDF2ZH);
        Self {
            profile: &MACOS_ARM64_PDF2ZH,
            layout,
            bin_path: None,
            doclayout_model_path: None,
            state: Pdf2zhState::Unsupported,
            install_plan: Pdf2zhInstallPlan {
                ready: false,
                message: "当前平台暂不支持自动处理 PDF 版面。".to_string(),
            },
        }
    }

    pub fn into_status(self) -> Pdf2zhStatus {
        if matches!(self.state, Pdf2zhState::Unsupported) {
            return Pdf2zhStatus {
                state: self.state,
                message: self.install_plan.message,
                profile: None,
                paths: None,
                install_plan: None,
            };
        }

        let message = if self.install_plan.ready {
            "PDF 版面处理已就绪。".to_string()
        } else {
            self.install_plan.message.clone()
        };

        Pdf2zhStatus {
            state: self.state,
            message,
            profile: Some(Pdf2zhProfileSummary::from_profile(self.profile)),
            paths: Some(Pdf2zhPaths {
                bin: self.bin_path.map(|path| path.display().to_string()),
                pack_dir: self.layout.pack_dir.display().to_string(),
                logs_dir: self.layout.logs_dir.display().to_string(),
            }),
            install_plan: Some(self.install_plan),
        }
    }
}

pub fn build_static_status(app: &AppHandle) -> Result<StaticStatus, String> {
    let Some(profile) = current_profile() else {
        return Ok(StaticStatus::unsupported());
    };
    let layout = Pdf2zhLayout::from_app(app, profile)?;
    let bin_path = locate_pdf2zh_bin(&layout, profile);
    let doclayout_model_path = locate_doclayout_model(&layout);
    let missing_python_package = bin_path
        .as_ref()
        .and_then(|_| first_missing_python_package(&layout));
    let ready = bin_path.as_ref().is_some_and(|path| path.is_file())
        && doclayout_model_path
            .as_ref()
            .is_some_and(|path| path.is_file())
        && missing_python_package.is_none();
    let state = if ready {
        Pdf2zhState::Installed
    } else {
        Pdf2zhState::NotInstalled
    };
    let install_plan = Pdf2zhInstallPlan {
        ready,
        message: if ready {
            "PDF 版面处理可用。".to_string()
        } else if bin_path.is_some() && doclayout_model_path.is_none() {
            format!(
                "PDF 版面处理组件需要更新：缺少内置版面模型 models/{DOCLAYOUT_MODEL_FILENAME}。请重新安装 PDF 组件。"
            )
        } else if let Some(package) = missing_python_package {
            format!(
                "PDF 版面处理组件需要更新：缺少 Python 依赖 {package}。请重新安装 PDF 组件。"
            )
        } else {
            "尚未安装 PDF 版面处理组件。请先在设置中安装，或点击 PDF 翻译时自动准备。".to_string()
        },
    };
    Ok(StaticStatus {
        profile,
        layout,
        bin_path,
        doclayout_model_path,
        state,
        install_plan,
    })
}

fn locate_pdf2zh_bin(layout: &Pdf2zhLayout, profile: &Pdf2zhProfile) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ROSETTA_PDF2ZH_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    let packed = layout.bin_path(profile);
    if packed.is_file() {
        return Some(packed);
    }

    None
}

fn locate_doclayout_model(layout: &Pdf2zhLayout) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ROSETTA_DOCLAYOUT_MODEL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    let packed = layout.doclayout_model_path();
    if packed.is_file() {
        return Some(packed);
    }

    None
}

fn first_missing_python_package(layout: &Pdf2zhLayout) -> Option<&'static str> {
    REQUIRED_PYTHON_PACKAGES
        .iter()
        .copied()
        .find(|package| !python_package_installed(layout, package))
}

fn python_package_installed(layout: &Pdf2zhLayout, package: &str) -> bool {
    for site_packages in python_site_package_dirs(layout) {
        if site_packages.join(package).is_dir() {
            return true;
        }
    }
    false
}

fn python_site_package_dirs(layout: &Pdf2zhLayout) -> Vec<PathBuf> {
    let python_dir = layout.pack_dir.join("python");
    let mut dirs = Vec::new();
    dirs.push(python_dir.join("Lib").join("site-packages"));

    let lib_dir = python_dir.join("lib");
    if let Ok(entries) = std::fs::read_dir(&lib_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("python"))
            {
                dirs.push(path.join("site-packages"));
            }
        }
    }

    dirs
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::managed_pdf2zh::profile::WINDOWS_AMD64_PDF2ZH;

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_root(name: &str) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "rosetta-pdf2zh-status-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root should be created");
        root
    }

    fn create_pdf2zh_bin(layout: &Pdf2zhLayout, profile: &Pdf2zhProfile) {
        let bin = layout.bin_path(profile);
        fs::create_dir_all(bin.parent().expect("bin should have parent"))
            .expect("bin parent should be created");
        fs::write(bin, b"#!/usr/bin/env bash\n").expect("bin should be written");
    }

    #[test]
    fn installed_pack_without_bundled_layout_model_is_not_ready() {
        let root = temp_root("missing-layout-model");
        let profile = &MACOS_ARM64_PDF2ZH;
        let layout = Pdf2zhLayout::resolve(root, profile);
        create_pdf2zh_bin(&layout, profile);

        let ready = layout.managed_pack_ready(profile);

        assert!(!ready);
    }

    #[test]
    fn installed_pack_with_bundled_layout_model_is_ready() {
        let root = temp_root("with-layout-model");
        let profile = &MACOS_ARM64_PDF2ZH;
        let layout = Pdf2zhLayout::resolve(root, profile);
        create_pdf2zh_bin(&layout, profile);
        let model = layout.doclayout_model_path();
        fs::create_dir_all(model.parent().expect("model should have parent"))
            .expect("model parent should be created");
        fs::write(model, b"model bytes").expect("model should be written");

        let ready = layout.managed_pack_ready(profile);

        assert!(ready);
    }

    #[test]
    fn windows_pack_without_tqdm_dependency_is_not_ready() {
        let root = temp_root("missing-tqdm");
        let profile = &WINDOWS_AMD64_PDF2ZH;
        let layout = Pdf2zhLayout::resolve(root, profile);
        create_pdf2zh_bin(&layout, profile);

        let model = layout.doclayout_model_path();
        fs::create_dir_all(model.parent().expect("model should have parent"))
            .expect("model parent should be created");
        fs::write(model, b"model bytes").expect("model should be written");

        assert_eq!(first_missing_python_package(&layout), Some("tqdm"));
    }

    #[test]
    fn windows_pack_with_tqdm_dependency_is_ready_for_dependency_check() {
        let root = temp_root("with-tqdm");
        let profile = &WINDOWS_AMD64_PDF2ZH;
        let layout = Pdf2zhLayout::resolve(root, profile);
        let site_packages = layout
            .pack_dir
            .join("python")
            .join("Lib")
            .join("site-packages")
            .join("tqdm");
        fs::create_dir_all(&site_packages).expect("tqdm package should be created");

        assert_eq!(first_missing_python_package(&layout), None);
    }
}
