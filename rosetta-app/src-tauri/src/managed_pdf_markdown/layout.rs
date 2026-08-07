use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::profile::{
    PdfMarkdownProfile, PYMUPDF4LLM_VERSION, PYMUPDF_LAYOUT_VERSION, PYMUPDF_VERSION,
};

pub const MANIFEST_SCHEMA: &str = "rosetta-pdf-markdown-component/1";

#[derive(Debug, Clone)]
pub struct PdfMarkdownLayout {
    pub root_dir: PathBuf,
    pub component_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub worker_dir: PathBuf,
    pub manifest_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledManifest {
    pub schema: String,
    pub profile_id: String,
    pub archive_filename: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub unpacked_bytes: u64,
    pub file_count: u64,
    pub pymupdf4llm: String,
    pub pymupdf_layout: String,
    pub pymupdf: String,
    pub cpu_only: bool,
    pub integration_boundary: String,
}

impl PdfMarkdownLayout {
    pub fn from_app(app: &AppHandle, profile: &PdfMarkdownProfile) -> Result<Self, String> {
        let app_data = app
            .path()
            .app_local_data_dir()
            .map_err(|_| "unable to locate app-local component storage".to_string())?;
        Ok(Self::resolve(app_data, profile))
    }

    pub fn resolve(app_data: PathBuf, profile: &PdfMarkdownProfile) -> Self {
        let root_dir = app_data.join("pdf-markdown-component");
        let component_dir = root_dir.join("overlay").join(profile.directory_name);
        Self {
            downloads_dir: root_dir.join("downloads"),
            worker_dir: root_dir.join("worker"),
            manifest_file: component_dir.join("manifest.json"),
            component_dir,
            root_dir,
        }
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        for path in [&self.root_dir, &self.downloads_dir, &self.worker_dir] {
            std::fs::create_dir_all(path)
                .map_err(|_| "unable to create managed component storage".to_string())?;
        }
        Ok(())
    }

    pub fn installed_manifest(&self) -> Result<InstalledManifest, String> {
        let bytes = std::fs::read(&self.manifest_file)
            .map_err(|_| "component install manifest is missing".to_string())?;
        if bytes.len() > 64 * 1024 {
            return Err("component install manifest exceeds its size limit".to_string());
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| "component install manifest is invalid".to_string())
    }

    pub fn validate_install(&self, profile: &PdfMarkdownProfile) -> Result<(), String> {
        let manifest = self.installed_manifest()?;
        if manifest.schema != MANIFEST_SCHEMA
            || manifest.profile_id != profile.id
            || manifest.archive_filename != profile.archive_filename
            || manifest.archive_sha256 != profile.archive_sha256
            || manifest.archive_bytes != profile.archive_bytes
            || manifest.unpacked_bytes != profile.unpacked_bytes
            || manifest.file_count != profile.file_count
            || manifest.pymupdf4llm != PYMUPDF4LLM_VERSION
            || manifest.pymupdf_layout != PYMUPDF_LAYOUT_VERSION
            || manifest.pymupdf != PYMUPDF_VERSION
            || !manifest.cpu_only
            || manifest.integration_boundary != "to_json"
        {
            return Err("component identity does not match the current release".to_string());
        }
        for relative in ["pymupdf4llm", "pymupdf"] {
            if !self.component_dir.join(relative).is_dir() {
                return Err("component package files are incomplete".to_string());
            }
        }
        if !self
            .component_dir
            .join("pymupdf_layout-1.28.0.dist-info")
            .is_dir()
        {
            return Err("component package metadata is incomplete".to_string());
        }
        let (unpacked_bytes, file_count) = self.installed_inventory()?;
        if unpacked_bytes != profile.unpacked_bytes || file_count != profile.file_count {
            return Err("component installed inventory does not match its manifest".to_string());
        }
        Ok(())
    }

    fn installed_inventory(&self) -> Result<(u64, u64), String> {
        let mut bytes = 0_u64;
        let mut count = 0_u64;
        let mut stack = vec![self.component_dir.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir)
                .map_err(|_| "unable to inspect component install".to_string())?
            {
                let entry = entry.map_err(|_| "unable to inspect component install".to_string())?;
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .map_err(|_| "unable to inspect component install".to_string())?;
                if file_type.is_symlink() {
                    return Err("component install contains a link".into());
                }
                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() && path != self.manifest_file {
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| "component install is too large".to_string())?;
                    bytes = bytes
                        .checked_add(
                            entry
                                .metadata()
                                .map_err(|_| "unable to inspect component install".to_string())?
                                .len(),
                        )
                        .ok_or_else(|| "component install is too large".to_string())?;
                }
            }
        }
        Ok((bytes, count))
    }
}

#[allow(dead_code)] // Checkpoint 3 consumes this internal path gate.
pub fn canonical_is_within(path: &Path, root: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.starts_with(&root) && path != root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_pdf_markdown::profile::WINDOWS_X64;

    #[test]
    fn component_storage_does_not_overlap_pdf2zh_pack() {
        let layout = PdfMarkdownLayout::resolve(PathBuf::from("C:/app-data"), &WINDOWS_X64);
        assert!(layout
            .component_dir
            .ends_with("pdf-markdown-component/overlay/windows-x64"));
        assert!(!layout
            .component_dir
            .to_string_lossy()
            .contains("pdf2zh-sidecar"));
    }

    #[test]
    fn containment_requires_existing_descendant() {
        let root =
            std::env::temp_dir().join(format!("rosetta-pdf-markdown-path-{}", std::process::id()));
        let child = root.join("child");
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("file"), b"ok").unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(canonical_is_within(&child.join("file"), &root));
        assert!(!canonical_is_within(&outside, &root));
        assert!(!canonical_is_within(&root, &root));
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
