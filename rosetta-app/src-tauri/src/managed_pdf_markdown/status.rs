use serde::Serialize;
use tauri::AppHandle;

use super::{
    layout::PdfMarkdownLayout,
    profile::{current_profile, PdfMarkdownProfileSummary},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PdfMarkdownState {
    Unsupported,
    NotInstalled,
    Installed,
    NeedsRepair,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownStatus {
    pub state: PdfMarkdownState,
    pub message: String,
    pub profile: Option<PdfMarkdownProfileSummary>,
    pub cpu_only: bool,
    pub versions: PdfMarkdownVersions,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownVersions {
    pub pymupdf4llm: &'static str,
    pub pymupdf_layout: &'static str,
    pub pymupdf: &'static str,
    pub protocol: u32,
}

pub fn build_status(app: &AppHandle) -> Result<PdfMarkdownStatus, String> {
    let versions = PdfMarkdownVersions {
        pymupdf4llm: super::profile::PYMUPDF4LLM_VERSION,
        pymupdf_layout: super::profile::PYMUPDF_LAYOUT_VERSION,
        pymupdf: super::profile::PYMUPDF_VERSION,
        protocol: super::profile::PROTOCOL_VERSION,
    };
    let Some(profile) = current_profile() else {
        return Ok(PdfMarkdownStatus {
            state: PdfMarkdownState::Unsupported,
            message: "当前平台暂不支持 PDF Markdown 组件。".into(),
            profile: None,
            cpu_only: true,
            versions,
        });
    };
    let layout = PdfMarkdownLayout::from_app(app, profile)?;
    let state = classify_layout(
        &layout,
        profile,
        crate::managed_pdf2zh::layout::locate_managed_python_host(app).is_ok(),
    );
    let message = match state {
        PdfMarkdownState::Installed => "PDF Markdown 组件已就绪。",
        PdfMarkdownState::NeedsRepair => "PDF Markdown 组件需要修复。",
        PdfMarkdownState::NotInstalled => "尚未安装 PDF Markdown 组件。",
        PdfMarkdownState::Unsupported => "当前平台暂不支持 PDF Markdown 组件。",
    };
    Ok(PdfMarkdownStatus {
        state,
        message: message.into(),
        profile: Some(profile.into()),
        cpu_only: true,
        versions,
    })
}

fn classify_layout(
    layout: &PdfMarkdownLayout,
    profile: &super::profile::PdfMarkdownProfile,
    python_available: bool,
) -> PdfMarkdownState {
    if !layout.component_dir.exists() {
        PdfMarkdownState::NotInstalled
    } else if layout.validate_install(profile).is_ok() && python_available {
        PdfMarkdownState::Installed
    } else {
        PdfMarkdownState::NeedsRepair
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_pdf_markdown::{
        install::tests::install_fixture,
        profile::{PdfMarkdownProfile, WINDOWS_X64},
    };
    use std::path::PathBuf;

    #[test]
    fn status_never_serializes_paths() {
        let value = serde_json::to_string(&PdfMarkdownStatus {
            state: PdfMarkdownState::NotInstalled,
            message: "x".into(),
            profile: None,
            cpu_only: true,
            versions: PdfMarkdownVersions {
                pymupdf4llm: "1.28.0",
                pymupdf_layout: "1.28.0",
                pymupdf: "1.28.0",
                protocol: 1,
            },
        })
        .unwrap();
        assert!(!value.contains("path"));
        assert!(!value.contains("Users"));
    }

    #[test]
    fn status_distinguishes_missing_ready_and_repair_states() {
        let root = std::env::temp_dir().join(format!(
            "rosetta-pdf-markdown-status-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let profile = PdfMarkdownProfile {
            unpacked_bytes: 3,
            file_count: 3,
            ..WINDOWS_X64
        };
        let layout = PdfMarkdownLayout::resolve(PathBuf::from(&root), &profile);
        assert_eq!(
            classify_layout(&layout, &profile, true),
            PdfMarkdownState::NotInstalled
        );
        install_fixture(&layout, &profile);
        assert_eq!(
            classify_layout(&layout, &profile, true),
            PdfMarkdownState::Installed
        );
        assert_eq!(
            classify_layout(&layout, &profile, false),
            PdfMarkdownState::NeedsRepair
        );
        std::fs::remove_file(layout.component_dir.join("pymupdf/__init__.py")).unwrap();
        assert_eq!(
            classify_layout(&layout, &profile, true),
            PdfMarkdownState::NeedsRepair
        );
        std::fs::write(layout.component_dir.join("pymupdf/__init__.py"), b"x").unwrap();
        std::fs::remove_file(&layout.manifest_file).unwrap();
        assert_eq!(
            classify_layout(&layout, &profile, true),
            PdfMarkdownState::NeedsRepair
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
