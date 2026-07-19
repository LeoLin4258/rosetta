use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::{
    managed_pdf2zh::{layout::Pdf2zhLayout, profile as pdf_component_profile},
    managed_rwkv::{
        profile::RuntimeLaunchKind, resolve_verified_runtime_binding,
        verified_runtime_binding_is_current, Registry, VerifiedManagedRuntimeBinding,
    },
    pdf_v3::font::{
        recommended_translation_font_family, TranslationFontAsset, TranslationFontAssetCache,
        TranslationFontFamilySpec, TranslationFontWeight, GO_NOTO_KURRENT_REGULAR,
        SOURCE_HAN_SANS_CN_BOLD, SOURCE_HAN_SANS_CN_REGULAR,
    },
    rosetta_jobs::path::timestamp_ms_string,
};

use super::{
    unit_translation::{LightningPdfApiConfig, LlamaCppPdfApiConfig, PdfUnitProviderConfig},
    v3_runtime::{PdfV3TranslationComponentBinding, PdfV3TranslationFontBinding},
};
use crate::rwkv_providers::mobile_batch_chat::MobileBatchChatConfig;

const PDF_V3_COMPONENT_STATUS_SCHEMA: &str = "rosetta-pdf-v3-component-status/1";
const PDF_V3_COMPONENT_ID: &str = "rosetta-pdf-v3-native-translation";
const PDF_V3_COMPONENT_VERSION: &str = "1";
const PDF_V3_PROVIDER_TIMEOUT_MS: u64 = 120_000;

const SOURCE_HAN_SANS_CN_REGULAR_SHA256: &str =
    "dd4ae04ab7d33f43202750cf755b2ba47a2122ba41412954e562da459337bbf6";
const SOURCE_HAN_SANS_CN_BOLD_SHA256: &str =
    "2dcaecc0dcba896fdc48e32617633123d91fa80c7eb9f7ae7c836da70e45ce88";
const GO_NOTO_KURRENT_REGULAR_SHA256: &str =
    "2f2cee5fbb2403df352ca2005247f6c4faa70f3086ebd31b6c62308b5f2f9865";

#[derive(Default)]
pub struct PdfV3ComponentState {
    inner: Arc<Mutex<PdfV3ComponentCache>>,
}

#[derive(Default)]
struct PdfV3ComponentCache {
    digests: BTreeMap<PathBuf, CachedDigest>,
    fonts: TranslationFontAssetCache,
}

struct CachedDigest {
    stamp: FileStamp,
    sha256: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    byte_count: u64,
    modified_ns: u128,
}

#[derive(Clone)]
pub(crate) struct ResolvedPdfV3TranslationComponent {
    pub component: PdfV3TranslationComponentBinding,
    pub provider: PdfUnitProviderConfig,
    pub regular_font: TranslationFontAsset,
    pub bold_font: Option<TranslationFontAsset>,
    pub runtime_release_sha256: Option<String>,
    pub supported_directions: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3ComponentStatus {
    pub schema: &'static str,
    pub ready: bool,
    pub verified_at_ms: u64,
    pub component: PdfV3TranslationComponentBinding,
    pub runtime_release_sha256: Option<String>,
    pub regular_font: PdfV3TranslationFontBinding,
    pub bold_font: Option<PdfV3TranslationFontBinding>,
}

#[derive(Debug)]
pub(crate) enum PdfV3ComponentError {
    RuntimeUnavailable,
    FontPackUnavailable,
    ArtifactIntegrity(&'static str),
    LockPoisoned,
    Worker,
}

impl fmt::Display for PdfV3ComponentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeUnavailable => {
                formatter.write_str("PDF v3 local translation runtime is unavailable")
            }
            Self::FontPackUnavailable => {
                formatter.write_str("PDF v3 translation font assets are unavailable")
            }
            Self::ArtifactIntegrity(field) => {
                write!(
                    formatter,
                    "PDF v3 component {field} integrity verification failed"
                )
            }
            Self::LockPoisoned => formatter.write_str("PDF v3 component cache lock is poisoned"),
            Self::Worker => formatter.write_str("PDF v3 component verification worker failed"),
        }
    }
}

impl std::error::Error for PdfV3ComponentError {}

impl PdfV3ComponentState {
    pub(crate) async fn resolve(
        &self,
        app: &AppHandle,
        registry: &Registry,
        target_language: &str,
    ) -> Result<ResolvedPdfV3TranslationComponent, PdfV3ComponentError> {
        let runtime = resolve_verified_runtime_binding(app, registry)
            .await
            .map_err(|_| PdfV3ComponentError::RuntimeUnavailable)?;
        let pdf_profile = pdf_component_profile::current_profile()
            .ok_or(PdfV3ComponentError::FontPackUnavailable)?;
        let pdf_layout = Pdf2zhLayout::from_app(app, pdf_profile)
            .map_err(|_| PdfV3ComponentError::FontPackUnavailable)?;
        if !pdf_layout.has_required_babeldoc_fonts()
            || !pdf_layout.pack_manifest_matches_profile(pdf_profile)
        {
            return Err(PdfV3ComponentError::FontPackUnavailable);
        }
        let family = recommended_translation_font_family(target_language);
        let font_directory = pdf_layout.babeldoc_cache_dir().join("fonts");
        let cache = self.inner.clone();
        let worker_runtime = runtime.clone();
        let resolved = tokio::task::spawn_blocking(move || {
            resolve_blocking(cache, worker_runtime, family, font_directory, pdf_profile)
        })
        .await
        .map_err(|_| PdfV3ComponentError::Worker)??;
        if !verified_runtime_binding_is_current(registry, &runtime).await {
            return Err(PdfV3ComponentError::RuntimeUnavailable);
        }
        if resolved.provider.provider_id() != resolved.component.provider_id {
            return Err(PdfV3ComponentError::ArtifactIntegrity("provider"));
        }
        Ok(resolved)
    }

    pub(crate) async fn status(
        &self,
        app: &AppHandle,
        registry: &Registry,
        target_language: &str,
    ) -> Result<PdfV3ComponentStatus, PdfV3ComponentError> {
        let resolved = self.resolve(app, registry, target_language).await?;
        let verified_at_ms = timestamp_ms_string()
            .parse::<u64>()
            .map_err(|_| PdfV3ComponentError::Worker)?;
        Ok(PdfV3ComponentStatus {
            schema: PDF_V3_COMPONENT_STATUS_SCHEMA,
            ready: true,
            verified_at_ms,
            component: resolved.component,
            runtime_release_sha256: resolved.runtime_release_sha256,
            regular_font: PdfV3TranslationFontBinding::from_asset(&resolved.regular_font)
                .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("regularFont"))?,
            bold_font: resolved
                .bold_font
                .as_ref()
                .map(PdfV3TranslationFontBinding::from_asset)
                .transpose()
                .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("boldFont"))?,
        })
    }
}

fn resolve_blocking(
    cache: Arc<Mutex<PdfV3ComponentCache>>,
    runtime: VerifiedManagedRuntimeBinding,
    family: TranslationFontFamilySpec,
    font_directory: PathBuf,
    pdf_profile: &'static pdf_component_profile::Pdf2zhProfile,
) -> Result<ResolvedPdfV3TranslationComponent, PdfV3ComponentError> {
    let mut cache = cache
        .lock()
        .map_err(|_| PdfV3ComponentError::LockPoisoned)?;
    let sidecar_sha256 = cached_file_sha256(&mut cache, &runtime.sidecar_path)?;
    let model_sha256 = if runtime.model_path.is_file() {
        let actual = cached_file_sha256(&mut cache, &runtime.model_path)?;
        if actual != runtime.profile.model_sha256 {
            return Err(PdfV3ComponentError::ArtifactIntegrity("model"));
        }
        actual
    } else if runtime.model_path.is_dir() {
        directory_sha256(&runtime.model_path)?
    } else {
        return Err(PdfV3ComponentError::ArtifactIntegrity("model"));
    };

    let regular_path = font_directory.join(family.regular_filename);
    let regular_font = cache
        .fonts
        .load_weighted(
            format!("{}-regular", family.family_id),
            TranslationFontWeight::Regular,
            &regular_path,
            0,
        )
        .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("regularFont"))?;
    verify_font_fingerprint(&regular_font, family.regular_filename)?;
    let bold_font = family
        .bold_filename
        .map(|filename| {
            let asset = cache
                .fonts
                .load_weighted(
                    format!("{}-bold", family.family_id),
                    TranslationFontWeight::Bold,
                    &font_directory.join(filename),
                    0,
                )
                .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("boldFont"))?;
            verify_font_fingerprint(&asset, filename)?;
            Ok(asset)
        })
        .transpose()?;

    let runtime_release_sha256 = runtime.runtime_release_sha256.map(str::to_string);
    let component_manifest_id = component_manifest_id(&ComponentManifestIdentity {
        schema_version: 1,
        component_id: PDF_V3_COMPONENT_ID,
        component_version: PDF_V3_COMPONENT_VERSION,
        runtime_profile_id: runtime.profile.id,
        runtime_release_sha256: runtime.runtime_release_sha256,
        sidecar_sha256: &sidecar_sha256,
        provider_id: runtime.profile.provider_id,
        model_id: runtime.profile.model_filename,
        model_distribution_sha256: runtime.profile.model_sha256,
        model_sha256: &model_sha256,
        pdf_asset_profile_id: pdf_profile.id,
        pdf_asset_release_sha256: pdf_profile.pack_sha256,
        regular_font_sha256: regular_font.fingerprint(),
        bold_font_sha256: bold_font.as_ref().map(TranslationFontAsset::fingerprint),
    })?;
    let provider = provider_config(&runtime)?;
    Ok(ResolvedPdfV3TranslationComponent {
        component: PdfV3TranslationComponentBinding {
            component_id: PDF_V3_COMPONENT_ID.to_string(),
            component_version: PDF_V3_COMPONENT_VERSION.to_string(),
            component_manifest_id,
            component_build_sha256: sidecar_sha256,
            platform_os: runtime.profile.platform_os.to_string(),
            platform_arch: runtime.profile.platform_arch.to_string(),
            provider_id: runtime.profile.provider_id.to_string(),
            model_id: runtime.profile.model_filename.to_string(),
            model_sha256,
        },
        provider,
        regular_font,
        bold_font,
        runtime_release_sha256,
        supported_directions: runtime.profile.supported_directions,
    })
}

fn provider_config(
    runtime: &VerifiedManagedRuntimeBinding,
) -> Result<PdfUnitProviderConfig, PdfV3ComponentError> {
    match runtime.profile.launch_kind {
        RuntimeLaunchKind::RwkvMobile => {
            Ok(PdfUnitProviderConfig::MobileBatch(MobileBatchChatConfig {
                base_url: runtime.base_url.clone(),
                timeout_ms: PDF_V3_PROVIDER_TIMEOUT_MS,
            }))
        }
        RuntimeLaunchKind::LlamaCppServer => {
            Ok(PdfUnitProviderConfig::LlamaCpp(LlamaCppPdfApiConfig {
                base_url: runtime.base_url.clone(),
                timeout_ms: PDF_V3_PROVIDER_TIMEOUT_MS,
            }))
        }
        RuntimeLaunchKind::LightningCuda => {
            Ok(PdfUnitProviderConfig::Lightning(LightningPdfApiConfig {
                base_url: runtime.base_url.clone(),
                endpoint: runtime.profile.batch_chat_path.to_string(),
                internal_token: String::new(),
                body_password: String::new(),
                timeout_ms: PDF_V3_PROVIDER_TIMEOUT_MS,
            }))
        }
    }
}

fn verify_font_fingerprint(
    asset: &TranslationFontAsset,
    filename: &str,
) -> Result<(), PdfV3ComponentError> {
    let expected = match filename {
        SOURCE_HAN_SANS_CN_REGULAR => SOURCE_HAN_SANS_CN_REGULAR_SHA256,
        SOURCE_HAN_SANS_CN_BOLD => SOURCE_HAN_SANS_CN_BOLD_SHA256,
        GO_NOTO_KURRENT_REGULAR => GO_NOTO_KURRENT_REGULAR_SHA256,
        _ => return Err(PdfV3ComponentError::ArtifactIntegrity("fontProfile")),
    };
    if asset.fingerprint() != expected {
        return Err(PdfV3ComponentError::ArtifactIntegrity("font"));
    }
    Ok(())
}

fn cached_file_sha256(
    cache: &mut PdfV3ComponentCache,
    path: &Path,
) -> Result<String, PdfV3ComponentError> {
    let stamp = file_stamp(path)?;
    if let Some(cached) = cache
        .digests
        .get(path)
        .filter(|cached| cached.stamp == stamp)
    {
        return Ok(cached.sha256.clone());
    }
    let sha256 = file_sha256(path)?;
    cache.digests.insert(
        path.to_path_buf(),
        CachedDigest {
            stamp,
            sha256: sha256.clone(),
        },
    );
    Ok(sha256)
}

fn file_stamp(path: &Path) -> Result<FileStamp, PdfV3ComponentError> {
    let metadata = fs::metadata(path)
        .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactMetadata"))?;
    if !metadata.is_file() {
        return Err(PdfV3ComponentError::ArtifactIntegrity("artifactType"));
    }
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    Ok(FileStamp {
        byte_count: metadata.len(),
        modified_ns,
    })
}

fn file_sha256(path: &Path) -> Result<String, PdfV3ComponentError> {
    let mut file =
        File::open(path).map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn directory_sha256(path: &Path) -> Result<String, PdfV3ComponentError> {
    let mut files = Vec::new();
    collect_directory_files(path, path, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    hasher.update(b"rosetta-pdf-v3-directory-artifact/1\0");
    for (relative, file) in files {
        let relative = relative
            .to_str()
            .ok_or(PdfV3ComponentError::ArtifactIntegrity("artifactPath"))?
            .replace('\\', "/");
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        let metadata = fs::metadata(&file)
            .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactMetadata"))?;
        hasher.update(metadata.len().to_le_bytes());
        let mut input =
            File::open(file).map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?;
        let mut buffer = [0u8; 1024 * 1024];
        loop {
            let count = input
                .read(&mut buffer)
                .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_directory_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PdfV3ComponentError> {
    for entry in fs::read_dir(directory)
        .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?
    {
        let entry = entry.map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactRead"))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactMetadata"))?;
        if metadata.file_type().is_symlink() {
            return Err(PdfV3ComponentError::ArtifactIntegrity("artifactSymlink"));
        }
        if metadata.is_dir() {
            collect_directory_files(root, &entry.path(), output)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("artifactPath"))?
                .to_path_buf();
            output.push((relative, entry.path()));
        } else {
            return Err(PdfV3ComponentError::ArtifactIntegrity("artifactType"));
        }
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ComponentManifestIdentity<'a> {
    schema_version: u32,
    component_id: &'a str,
    component_version: &'a str,
    runtime_profile_id: &'a str,
    runtime_release_sha256: Option<&'a str>,
    sidecar_sha256: &'a str,
    provider_id: &'a str,
    model_id: &'a str,
    model_distribution_sha256: &'a str,
    model_sha256: &'a str,
    pdf_asset_profile_id: &'a str,
    pdf_asset_release_sha256: Option<&'a str>,
    regular_font_sha256: &'a str,
    bold_font_sha256: Option<&'a str>,
}

fn component_manifest_id(
    identity: &ComponentManifestIdentity<'_>,
) -> Result<String, PdfV3ComponentError> {
    let bytes = serde_json::to_vec(identity)
        .map_err(|_| PdfV3ComponentError::ArtifactIntegrity("componentManifest"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use crate::{
        pdf_v3::font::TranslationFontWeight,
        rosetta_jobs::formats::pdf::v3_runtime::{
            PdfV3TranslationComponentBinding, PdfV3TranslationFontBinding,
        },
    };

    use super::{
        cached_file_sha256, component_manifest_id, directory_sha256, ComponentManifestIdentity,
        PdfV3ComponentCache, PdfV3ComponentStatus, PDF_V3_COMPONENT_STATUS_SCHEMA,
    };

    #[test]
    fn file_digest_cache_invalidates_when_artifact_stamp_changes() {
        let path = temp_path("digest-cache");
        fs::write(&path, b"first").expect("write first");
        let mut cache = PdfV3ComponentCache::default();
        let first = cached_file_sha256(&mut cache, &path).expect("first digest");
        assert_eq!(cache.digests.len(), 1);
        assert_eq!(
            cached_file_sha256(&mut cache, &path).expect("cached digest"),
            first
        );

        std::thread::sleep(Duration::from_millis(2));
        fs::write(&path, b"second-longer").expect("write second");
        let second = cached_file_sha256(&mut cache, &path).expect("second digest");
        assert_ne!(first, second);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn directory_digest_is_ordered_and_rejects_content_drift() {
        let root = temp_path("directory");
        fs::create_dir_all(root.join("nested")).expect("create directory");
        fs::write(root.join("b.bin"), b"b").expect("write b");
        fs::write(root.join("nested").join("a.bin"), b"a").expect("write a");
        let first = directory_sha256(&root).expect("first directory digest");
        assert_eq!(first, directory_sha256(&root).expect("stable digest"));
        fs::write(root.join("nested").join("a.bin"), b"changed").expect("change a");
        assert_ne!(first, directory_sha256(&root).expect("changed digest"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn component_manifest_identity_changes_with_any_runtime_identity() {
        let first = component_manifest_id(&identity("sidecar-a")).expect("first ID");
        let second = component_manifest_id(&identity("sidecar-b")).expect("second ID");
        assert_ne!(first, second);
        assert!(first.starts_with("sha256:"));
    }

    #[test]
    fn component_status_excludes_paths_endpoints_processes_and_credentials() {
        let font = PdfV3TranslationFontBinding {
            asset_id: "font-regular".to_string(),
            weight: TranslationFontWeight::Regular,
            face_index: 0,
            fingerprint_sha256: "a".repeat(64),
            byte_count: 100,
        };
        let status = PdfV3ComponentStatus {
            schema: PDF_V3_COMPONENT_STATUS_SCHEMA,
            ready: true,
            verified_at_ms: 42,
            component: PdfV3TranslationComponentBinding {
                component_id: "component".to_string(),
                component_version: "1".to_string(),
                component_manifest_id: "manifest".to_string(),
                component_build_sha256: "b".repeat(64),
                platform_os: "windows".to_string(),
                platform_arch: "x86_64".to_string(),
                provider_id: "provider".to_string(),
                model_id: "model".to_string(),
                model_sha256: "c".repeat(64),
            },
            runtime_release_sha256: Some("d".repeat(64)),
            regular_font: font,
            bold_font: None,
        };
        let encoded = serde_json::to_string(&status).expect("encode status");
        for forbidden in [
            "path",
            "baseUrl",
            "endpoint",
            "pid",
            "token",
            "password",
            "credential",
        ] {
            assert!(!encoded.contains(forbidden), "leaked field {forbidden}");
        }
    }

    fn identity(sidecar_sha256: &str) -> ComponentManifestIdentity<'_> {
        ComponentManifestIdentity {
            schema_version: 1,
            component_id: "component",
            component_version: "1",
            runtime_profile_id: "profile",
            runtime_release_sha256: Some("release"),
            sidecar_sha256,
            provider_id: "provider",
            model_id: "model",
            model_distribution_sha256: "distribution",
            model_sha256: "model-sha",
            pdf_asset_profile_id: "pdf-assets",
            pdf_asset_release_sha256: Some("pdf-release"),
            regular_font_sha256: "regular",
            bold_font_sha256: Some("bold"),
        }
    }

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rosetta-pdf-v3-component-{label}-{}-{}",
            std::process::id(),
            crate::rosetta_jobs::path::timestamp_ms_string()
        ))
    }
}
