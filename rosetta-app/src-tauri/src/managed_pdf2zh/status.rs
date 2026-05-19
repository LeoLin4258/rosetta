use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use super::{
    layout::Pdf2zhLayout,
    profile::{current_profile, Pdf2zhProfile, Pdf2zhProfileSummary, MACOS_ARM64_PDF2ZH},
};

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
            state: Pdf2zhState::Unsupported,
            install_plan: Pdf2zhInstallPlan {
                ready: false,
                message: "当前平台暂不支持 PDFMathTranslate sidecar（v1 仅支持 macOS Apple Silicon）。".to_string(),
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
            "PDFMathTranslate 已就绪。".to_string()
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
    let ready = bin_path.as_ref().is_some_and(|path| path.is_file());
    let state = if ready {
        Pdf2zhState::Installed
    } else {
        Pdf2zhState::NotInstalled
    };
    let install_plan = Pdf2zhInstallPlan {
        ready,
        message: if ready {
            "PDFMathTranslate 可用。".to_string()
        } else {
            "未找到 pdf2zh。请先设置 ROSETTA_PDF2ZH_BIN 指向本地 pdf2zh，或安装 Rosetta pdf2zh sidecar pack。".to_string()
        },
    };
    Ok(StaticStatus {
        profile,
        layout,
        bin_path,
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
