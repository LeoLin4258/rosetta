use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use super::{capabilities::validate_installed_capabilities, profile::Pdf2zhProfile};

pub const DOCLAYOUT_MODEL_FILENAME: &str = "doclayout_yolo_docstructbench_imgsz1024.onnx";
const REQUIRED_BABELDOC_FONTS: [&str; 3] = [
    "SourceHanSansCN-Regular.ttf",
    "SourceHanSansCN-Bold.ttf",
    "GoNotoKurrent-Regular.ttf",
];

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Pdf2zhPackManifest {
    schema_version: u32,
    profile_id: String,
    pack_filename: String,
    sha256: Option<String>,
    size_bytes: Option<u64>,
    unpacked_size_bytes: Option<u64>,
    file_count: Option<u64>,
    engine_capability_schema_version: Option<u32>,
    engine_contract_version: Option<u32>,
    engine_revision: Option<u32>,
    capabilities: Vec<String>,
    #[serde(default)]
    custom_pack: bool,
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
        self.pack_manifest_compatibility(profile).is_ok()
    }

    pub fn pack_manifest_compatibility(&self, profile: &Pdf2zhProfile) -> Result<(), String> {
        let Ok(contents) = std::fs::read_to_string(&self.manifest_file) else {
            return Err("组件缺少安装记录".to_string());
        };
        let Ok(manifest) = serde_json::from_str::<Pdf2zhPackManifest>(&contents) else {
            return Err("组件安装记录无法解析".to_string());
        };

        if manifest.profile_id != profile.id || manifest.pack_filename != profile.pack_filename {
            return Err("组件平台或文件身份与当前 profile 不匹配".to_string());
        }
        if manifest.custom_pack || has_custom_pack_url_env() {
            if !manifest.sha256.as_deref().is_some_and(is_lowercase_sha256) {
                return Err("自定义组件安装记录缺少有效 SHA-256".to_string());
            }
        } else if profile.pack_sha256.is_some() || profile.pack_size_bytes.is_some() {
            if let Some(expected) = profile.pack_sha256 {
                if manifest.sha256.as_deref() != Some(expected) {
                    return Err("组件 SHA-256 与当前 profile 不匹配".to_string());
                }
            }
            if let Some(expected) = profile.pack_size_bytes {
                if manifest.size_bytes != Some(expected) {
                    return Err("组件大小与当前 profile 不匹配".to_string());
                }
            }
        }
        if manifest.schema_version >= 2 {
            let unpacked_size_bytes = manifest
                .unpacked_size_bytes
                .ok_or_else(|| "组件安装记录缺少解压体积".to_string())?;
            let file_count = manifest
                .file_count
                .ok_or_else(|| "组件安装记录缺少文件数".to_string())?;
            if let Some(expected) = profile.pack_unpacked_size_bytes {
                if unpacked_size_bytes != expected {
                    return Err("组件解压体积与当前 profile 不匹配".to_string());
                }
            }
            if let Some(expected) = profile.pack_file_count {
                if file_count != expected {
                    return Err("组件文件数与当前 profile 不匹配".to_string());
                }
            }
        }
        validate_installed_capabilities(
            manifest.engine_capability_schema_version,
            manifest.engine_contract_version,
            manifest.engine_revision,
            &manifest.capabilities,
        )
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        for dir in [&self.root_dir, &self.logs_dir, &self.downloads_dir] {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("无法创建 {}: {error}", dir.display()))?;
        }
        Ok(())
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
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
    use crate::managed_pdf2zh::profile::{
        LINUX_X64_PDF2ZH, MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH,
    };

    #[test]
    fn pdf_pack_python_path_matches_platform_layout() {
        let root = PathBuf::from("/tmp/rosetta-app-data");
        let mac_layout = Pdf2zhLayout::resolve(root.clone(), &MACOS_ARM64_PDF2ZH);
        let windows_layout = Pdf2zhLayout::resolve(root.clone(), &WINDOWS_AMD64_PDF2ZH);
        let linux_layout = Pdf2zhLayout::resolve(root, &LINUX_X64_PDF2ZH);

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
        assert!(linux_layout.python_path(&LINUX_X64_PDF2ZH).ends_with(
            PathBuf::from("linux-x64")
                .join("python")
                .join("bin")
                .join("python")
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
  "schemaVersion": 2,
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

        std::fs::write(
            &layout.manifest_file,
            r#"{
  "schemaVersion": 1,
  "profileId": "windows-amd64-pdf2zh",
  "packFilename": "rosetta-pdf2zh-windows-amd64.zip",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "sizeBytes": null,
  "sourceUrl": "file:///tmp/local-test.zip",
  "installedAt": "0",
  "customPack": true
}"#,
        )
        .expect("write local test manifest");
        assert!(
            !layout.managed_pack_ready(&WINDOWS_AMD64_PDF2ZH),
            "a local pack without the required engine capabilities must fail closed"
        );

        std::fs::write(
            &layout.manifest_file,
            format!(
                r#"{{
  "schemaVersion": 1,
  "profileId": "windows-amd64-pdf2zh",
  "packFilename": "rosetta-pdf2zh-windows-amd64.zip",
  "sha256": "{}",
  "sizeBytes": {},
  "engineCapabilitySchemaVersion": 1,
  "engineContractVersion": 2,
  "engineRevision": 1,
  "capabilities": ["authoritative-render-slots", "durable-layout-cache", "partial-page-accounting", "reusable-prepared-run"]
}}"#,
                WINDOWS_AMD64_PDF2ZH.pack_sha256.unwrap_or_default(),
                WINDOWS_AMD64_PDF2ZH.pack_size_bytes.unwrap_or_default()
            ),
        )
        .expect("write compatible schema 1 manifest");
        assert!(
            layout.managed_pack_ready(&WINDOWS_AMD64_PDF2ZH),
            "schema 1 manifests remain compatible when release identity and capabilities match"
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
  "unpackedSizeBytes": {},
  "fileCount": {},
  "engineCapabilitySchemaVersion": 1,
  "engineContractVersion": 2,
  "engineRevision": 1,
  "capabilities": ["authoritative-render-slots", "durable-layout-cache", "partial-page-accounting", "reusable-prepared-run"],
  "sourceUrl": "file:///tmp/{}",
  "installedAt": "0"
}}"#,
            profile.id,
            profile.pack_filename,
            profile.pack_sha256.unwrap_or_default(),
            profile.pack_size_bytes.unwrap_or_default(),
            profile.pack_unpacked_size_bytes.unwrap_or(1),
            profile.pack_file_count.unwrap_or(1),
            profile.pack_filename
        );
        std::fs::write(&layout.manifest_file, contents).expect("write manifest");
    }
}
