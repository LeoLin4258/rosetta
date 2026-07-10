use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use super::profile::Pdf2zhProfile;

pub const DOCLAYOUT_MODEL_FILENAME: &str = "doclayout_yolo_docstructbench_imgsz1024.onnx";
const REQUIRED_BABELDOC_FONTS: [&str; 3] = [
    "SourceHanSansCN-Regular.ttf",
    "SourceHanSansCN-Bold.ttf",
    "GoNotoKurrent-Regular.ttf",
];

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Pdf2zhPackManifest {
    profile_id: String,
    pack_filename: String,
    sha256: Option<String>,
    size_bytes: Option<u64>,
}

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

    pub fn python_path(&self, profile: &Pdf2zhProfile) -> PathBuf {
        if profile.platform_os == "windows" {
            self.pack_dir.join("python").join("python.exe")
        } else {
            self.pack_dir.join("python").join("bin").join("python")
        }
    }

    pub fn doclayout_model_path(&self) -> PathBuf {
        self.pack_dir.join("models").join(DOCLAYOUT_MODEL_FILENAME)
    }

    pub fn babeldoc_cache_dir(&self) -> PathBuf {
        self.pack_dir.join("assets").join("babeldoc")
    }

    pub fn has_required_babeldoc_fonts(&self) -> bool {
        REQUIRED_BABELDOC_FONTS
            .iter()
            .all(|font| self.babeldoc_cache_dir().join("fonts").join(font).is_file())
    }

    pub fn managed_pack_ready(&self, profile: &Pdf2zhProfile) -> bool {
        self.bin_path(profile).is_file()
            && self.doclayout_model_path().is_file()
            && self.has_required_babeldoc_fonts()
            && self.pack_manifest_matches_profile(profile)
    }

    pub fn pack_manifest_matches_profile(&self, profile: &Pdf2zhProfile) -> bool {
        if has_custom_pack_url_env() {
            return true;
        }
        if profile.pack_sha256.is_none() && profile.pack_size_bytes.is_none() {
            return true;
        }

        let Ok(contents) = std::fs::read_to_string(&self.manifest_file) else {
            return false;
        };
        let Ok(manifest) = serde_json::from_str::<Pdf2zhPackManifest>(&contents) else {
            return false;
        };

        if manifest.profile_id != profile.id || manifest.pack_filename != profile.pack_filename {
            return false;
        }
        if let Some(expected) = profile.pack_sha256 {
            if manifest.sha256.as_deref() != Some(expected) {
                return false;
            }
        }
        if let Some(expected) = profile.pack_size_bytes {
            if manifest.size_bytes != Some(expected) {
                return false;
            }
        }
        true
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        for dir in [&self.root_dir, &self.logs_dir, &self.downloads_dir] {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("无法创建 {}: {error}", dir.display()))?;
        }
        Ok(())
    }
}

fn has_custom_pack_url_env() -> bool {
    std::env::var("ROSETTA_PDF2ZH_PACK_URL")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{Pdf2zhLayout, REQUIRED_BABELDOC_FONTS};
    use crate::managed_pdf2zh::profile::{MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH};

    #[test]
    fn pdf_pack_python_path_matches_platform_layout() {
        let root = PathBuf::from("/tmp/rosetta-app-data");
        let mac_layout = Pdf2zhLayout::resolve(root.clone(), &MACOS_ARM64_PDF2ZH);
        let windows_layout = Pdf2zhLayout::resolve(root, &WINDOWS_AMD64_PDF2ZH);

        assert!(mac_layout.python_path(&MACOS_ARM64_PDF2ZH).ends_with(
            PathBuf::from("macos-arm64")
                .join("python")
                .join("bin")
                .join("python")
        ));
        assert!(windows_layout.python_path(&WINDOWS_AMD64_PDF2ZH).ends_with(
            PathBuf::from("windows-amd64")
                .join("python")
                .join("python.exe")
        ));
    }

    #[test]
    fn pdf_pack_ready_requires_bundled_babeldoc_fonts() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let temp = std::env::temp_dir().join(format!(
            "rosetta-pdf2zh-layout-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let layout = Pdf2zhLayout::resolve(temp.clone(), &MACOS_ARM64_PDF2ZH);
        std::fs::create_dir_all(layout.bin_path(&MACOS_ARM64_PDF2ZH).parent().unwrap())
            .expect("create bin dir");
        std::fs::write(layout.bin_path(&MACOS_ARM64_PDF2ZH), b"bin").expect("write bin");
        std::fs::create_dir_all(layout.doclayout_model_path().parent().unwrap())
            .expect("create model dir");
        std::fs::write(layout.doclayout_model_path(), b"model").expect("write model");

        assert!(
            !layout.managed_pack_ready(&MACOS_ARM64_PDF2ZH),
            "a pack that still depends on runtime BabelDOC font downloads is incomplete"
        );

        write_matching_manifest(&layout, &MACOS_ARM64_PDF2ZH);
        assert!(
            !layout.managed_pack_ready(&MACOS_ARM64_PDF2ZH),
            "the manifest cannot make a pack ready when required fonts are missing"
        );

        let fonts_dir = layout.babeldoc_cache_dir().join("fonts");
        std::fs::create_dir_all(&fonts_dir).expect("create fonts dir");
        for font in REQUIRED_BABELDOC_FONTS {
            std::fs::write(fonts_dir.join(font), b"font").expect("write font");
        }

        assert!(layout.managed_pack_ready(&MACOS_ARM64_PDF2ZH));
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn pdf_pack_ready_requires_current_profile_manifest() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let temp = std::env::temp_dir().join(format!(
            "rosetta-pdf2zh-layout-manifest-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let layout = Pdf2zhLayout::resolve(temp.clone(), &WINDOWS_AMD64_PDF2ZH);
        std::fs::create_dir_all(layout.bin_path(&WINDOWS_AMD64_PDF2ZH).parent().unwrap())
            .expect("create bin dir");
        std::fs::write(layout.bin_path(&WINDOWS_AMD64_PDF2ZH), b"bin").expect("write bin");
        std::fs::create_dir_all(layout.doclayout_model_path().parent().unwrap())
            .expect("create model dir");
        std::fs::write(layout.doclayout_model_path(), b"model").expect("write model");
        let fonts_dir = layout.babeldoc_cache_dir().join("fonts");
        std::fs::create_dir_all(&fonts_dir).expect("create fonts dir");
        for font in REQUIRED_BABELDOC_FONTS {
            std::fs::write(fonts_dir.join(font), b"font").expect("write font");
        }

        assert!(
            !layout.managed_pack_ready(&WINDOWS_AMD64_PDF2ZH),
            "an old pack without release identity metadata must be reinstalled"
        );

        std::fs::write(
            &layout.manifest_file,
            r#"{
  "schemaVersion": 1,
  "profileId": "windows-amd64-pdf2zh",
  "packFilename": "rosetta-pdf2zh-windows-amd64.zip",
  "sha256": "old-pack",
  "sizeBytes": 1,
  "sourceUrl": "file:///tmp/old.zip",
  "installedAt": "0"
}"#,
        )
        .expect("write stale manifest");
        assert!(
            !layout.managed_pack_ready(&WINDOWS_AMD64_PDF2ZH),
            "an installed pack from a previous release must not satisfy the current profile"
        );

        write_matching_manifest(&layout, &WINDOWS_AMD64_PDF2ZH);
        assert!(layout.managed_pack_ready(&WINDOWS_AMD64_PDF2ZH));
        let _ = std::fs::remove_dir_all(temp);
    }

    fn write_matching_manifest(
        layout: &Pdf2zhLayout,
        profile: &crate::managed_pdf2zh::profile::Pdf2zhProfile,
    ) {
        let contents = format!(
            r#"{{
  "schemaVersion": 1,
  "profileId": "{}",
  "packFilename": "{}",
  "sha256": "{}",
  "sizeBytes": {},
  "sourceUrl": "file:///tmp/{}",
  "installedAt": "0"
}}"#,
            profile.id,
            profile.pack_filename,
            profile.pack_sha256.unwrap_or_default(),
            profile.pack_size_bytes.unwrap_or_default(),
            profile.pack_filename
        );
        std::fs::write(&layout.manifest_file, contents).expect("write manifest");
    }
}
