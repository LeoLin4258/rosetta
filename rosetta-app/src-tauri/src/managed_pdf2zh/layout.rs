use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use super::profile::Pdf2zhProfile;

pub const DOCLAYOUT_MODEL_FILENAME: &str = "doclayout_yolo_docstructbench_imgsz1024.onnx";

#[derive(Debug, Clone)]
pub struct Pdf2zhLayout {
    pub root_dir: PathBuf,
    pub pack_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub manifest_file: PathBuf,
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
            downloads_dir: root_dir.join("downloads"),
            manifest_file: root_dir
                .join("pack")
                .join(profile.pack_directory_name)
                .join("manifest.json"),
            root_dir,
        }
    }

    pub fn bin_path(&self, profile: &Pdf2zhProfile) -> PathBuf {
        self.pack_dir.join(profile.bin_relative_path)
    }

    pub fn doclayout_model_path(&self) -> PathBuf {
        self.pack_dir.join("models").join(DOCLAYOUT_MODEL_FILENAME)
    }

    pub fn managed_pack_ready(&self, profile: &Pdf2zhProfile) -> bool {
        self.bin_path(profile).is_file() && self.doclayout_model_path().is_file()
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        for dir in [&self.root_dir, &self.logs_dir, &self.downloads_dir] {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("无法创建 {}: {error}", dir.display()))?;
        }
        Ok(())
    }
}
