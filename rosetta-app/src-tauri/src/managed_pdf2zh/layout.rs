use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use super::profile::Pdf2zhProfile;

#[derive(Debug, Clone)]
pub struct Pdf2zhLayout {
    pub pack_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl Pdf2zhLayout {
    pub fn from_app(app: &AppHandle, profile: &Pdf2zhProfile) -> Result<Self, String> {
        let app_data = app
            .path()
            .app_local_data_dir()
            .map_err(|error| format!("无法定位应用数据目录: {error}"))?;
        Ok(Self::resolve(app_data, profile))
    }

    pub fn resolve(app_data: PathBuf, profile: &Pdf2zhProfile) -> Self {
        let root_dir = app_data.join("pdf2zh-sidecar");
        Self {
            pack_dir: root_dir.join("pack").join(profile.pack_directory_name),
            logs_dir: root_dir.join("logs"),
        }
    }

    pub fn bin_path(&self, profile: &Pdf2zhProfile) -> PathBuf {
        self.pack_dir.join(profile.bin_relative_path)
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.logs_dir)
            .map_err(|error| format!("无法创建 pdf2zh 日志目录: {error}"))
    }
}
