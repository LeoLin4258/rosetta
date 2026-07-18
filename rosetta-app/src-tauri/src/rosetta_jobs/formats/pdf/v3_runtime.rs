#![allow(dead_code)]

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pdf_v3::{
    font::{TranslationFontAsset, TranslationFontWeight},
    page_set::{PageSet, PageSetError},
    patch_renderer::{TranslationPatchRenderPolicy, TRANSLATION_PATCH_RENDERER_VERSION},
    scheduler::PdfV3TranslationBinding,
};

use super::unit_translation::PdfUnitProviderConfig;

const RUNTIME_MANIFEST_SCHEMA_VERSION: u32 = 1;
const RUNTIME_MANIFEST_FILENAME: &str = "runtime-manifest.json";
const MAX_RUNTIME_MANIFEST_BYTES: u64 = 64 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PdfV3TranslationComponentBinding {
    pub component_id: String,
    pub component_version: String,
    pub component_manifest_id: String,
    pub component_build_sha256: String,
    pub platform_os: String,
    pub platform_arch: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PdfV3TranslationFontBinding {
    pub asset_id: String,
    pub weight: TranslationFontWeight,
    pub face_index: u32,
    pub fingerprint_sha256: String,
    pub byte_count: u64,
}

impl PdfV3TranslationFontBinding {
    fn from_asset(asset: &TranslationFontAsset) -> Result<Self, PdfV3RuntimeManifestError> {
        Ok(Self {
            asset_id: asset.asset_id().to_string(),
            weight: asset.weight(),
            face_index: asset.face_index(),
            fingerprint_sha256: asset.fingerprint().to_string(),
            byte_count: u64::try_from(asset.byte_count())
                .map_err(|_| PdfV3RuntimeManifestError::InvalidIdentity("fontByteCount"))?,
        })
    }

    fn matches_asset(&self, asset: &TranslationFontAsset) -> bool {
        self.asset_id == asset.asset_id()
            && self.weight == asset.weight()
            && self.face_index == asset.face_index()
            && self.fingerprint_sha256 == asset.fingerprint()
            && usize::try_from(self.byte_count).ok() == Some(asset.byte_count())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PdfV3TranslationRenderPolicyBinding {
    pub minimum_fit_scale: f32,
}

impl From<TranslationPatchRenderPolicy> for PdfV3TranslationRenderPolicyBinding {
    fn from(value: TranslationPatchRenderPolicy) -> Self {
        Self {
            minimum_fit_scale: value.minimum_fit_scale,
        }
    }
}

impl From<PdfV3TranslationRenderPolicyBinding> for TranslationPatchRenderPolicy {
    fn from(value: PdfV3TranslationRenderPolicyBinding) -> Self {
        Self {
            minimum_fit_scale: value.minimum_fit_scale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PdfV3TranslationRuntimeManifest {
    pub schema_version: u32,
    pub manifest_id: String,
    pub source_fingerprint: String,
    pub source_page_count: u32,
    pub requested_page_set: String,
    pub source_language: String,
    pub target_language: String,
    pub engine_version: String,
    pub page_graph_schema_version: u32,
    pub translation_patch_schema_version: u32,
    pub renderer_version: String,
    pub translation_revision: u64,
    pub component: PdfV3TranslationComponentBinding,
    pub render_policy: PdfV3TranslationRenderPolicyBinding,
    pub regular_font: PdfV3TranslationFontBinding,
    pub bold_font: Option<PdfV3TranslationFontBinding>,
}

pub(crate) struct PdfV3TranslationRuntimeSpec<'a> {
    pub binding: &'a PdfV3TranslationBinding,
    pub translation_revision: u64,
    pub component: PdfV3TranslationComponentBinding,
    pub render_policy: TranslationPatchRenderPolicy,
    pub regular_font: &'a TranslationFontAsset,
    pub bold_font: Option<&'a TranslationFontAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfV3RuntimeManifestCommitKind {
    Created,
    Existing,
}

#[derive(Debug)]
pub(crate) enum PdfV3RuntimeManifestError {
    InvalidPath,
    Missing,
    InvalidIdentity(&'static str),
    BindingMismatch(&'static str),
    ProviderMismatch {
        expected: String,
        actual: String,
    },
    FontMismatch(TranslationFontWeight),
    Conflict,
    TooLarge {
        bytes: u64,
        maximum: u64,
    },
    PageSet(PageSetError),
    Serialization(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for PdfV3RuntimeManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("PDF v3 runtime manifest path is invalid"),
            Self::Missing => formatter.write_str("PDF v3 runtime manifest is missing"),
            Self::InvalidIdentity(field) => {
                write!(formatter, "PDF v3 runtime manifest {field} is invalid")
            }
            Self::BindingMismatch(field) => write!(
                formatter,
                "PDF v3 runtime manifest {field} does not match the translation run"
            ),
            Self::ProviderMismatch { expected, actual } => write!(
                formatter,
                "PDF v3 runtime manifest requires provider {expected}; live provider is {actual}"
            ),
            Self::FontMismatch(weight) => write!(
                formatter,
                "PDF v3 runtime manifest {weight:?} font does not match the live asset"
            ),
            Self::Conflict => formatter.write_str(
                "PDF v3 runtime manifest already exists with a different immutable identity",
            ),
            Self::TooLarge { bytes, maximum } => write!(
                formatter,
                "PDF v3 runtime manifest has {bytes} bytes, above maximum {maximum}"
            ),
            Self::PageSet(error) => error.fmt(formatter),
            Self::Serialization(message) => {
                write!(
                    formatter,
                    "PDF v3 runtime manifest serialization failed: {message}"
                )
            }
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} PDF v3 runtime manifest {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PdfV3RuntimeManifestError {}

impl From<PageSetError> for PdfV3RuntimeManifestError {
    fn from(value: PageSetError) -> Self {
        Self::PageSet(value)
    }
}

#[derive(Clone)]
pub(crate) struct BoundPdfV3TranslationRuntime {
    manifest: PdfV3TranslationRuntimeManifest,
    provider: PdfUnitProviderConfig,
    regular_font: TranslationFontAsset,
    bold_font: Option<TranslationFontAsset>,
}

impl BoundPdfV3TranslationRuntime {
    pub(crate) fn new(
        binding: &PdfV3TranslationBinding,
        manifest: PdfV3TranslationRuntimeManifest,
        provider: PdfUnitProviderConfig,
        regular_font: TranslationFontAsset,
        bold_font: Option<TranslationFontAsset>,
    ) -> Result<Self, PdfV3RuntimeManifestError> {
        validate_translation_runtime_manifest(&manifest)?;
        validate_runtime_manifest_binding(&manifest, binding)?;
        if manifest.component.platform_os != std::env::consts::OS {
            return Err(PdfV3RuntimeManifestError::BindingMismatch("platformOs"));
        }
        if manifest.component.platform_arch != std::env::consts::ARCH {
            return Err(PdfV3RuntimeManifestError::BindingMismatch("platformArch"));
        }
        let live_provider_id = provider.provider_id();
        if manifest.component.provider_id != live_provider_id {
            return Err(PdfV3RuntimeManifestError::ProviderMismatch {
                expected: manifest.component.provider_id.clone(),
                actual: live_provider_id.to_string(),
            });
        }
        if !manifest.regular_font.matches_asset(&regular_font) {
            return Err(PdfV3RuntimeManifestError::FontMismatch(
                TranslationFontWeight::Regular,
            ));
        }
        match (&manifest.bold_font, &bold_font) {
            (Some(expected), Some(actual)) if expected.matches_asset(actual) => {}
            (None, None) => {}
            _ => {
                return Err(PdfV3RuntimeManifestError::FontMismatch(
                    TranslationFontWeight::Bold,
                ));
            }
        }
        Ok(Self {
            manifest,
            provider,
            regular_font,
            bold_font,
        })
    }

    pub(crate) fn manifest(&self) -> &PdfV3TranslationRuntimeManifest {
        &self.manifest
    }

    pub(crate) fn provider(&self) -> &PdfUnitProviderConfig {
        &self.provider
    }

    pub(crate) fn regular_font(&self) -> &TranslationFontAsset {
        &self.regular_font
    }

    pub(crate) fn bold_font(&self) -> Option<&TranslationFontAsset> {
        self.bold_font.as_ref()
    }

    pub(crate) fn render_policy(&self) -> TranslationPatchRenderPolicy {
        self.manifest.render_policy.into()
    }
}

pub(crate) fn build_translation_runtime_manifest(
    spec: PdfV3TranslationRuntimeSpec<'_>,
) -> Result<PdfV3TranslationRuntimeManifest, PdfV3RuntimeManifestError> {
    let mut manifest = PdfV3TranslationRuntimeManifest {
        schema_version: RUNTIME_MANIFEST_SCHEMA_VERSION,
        manifest_id: String::new(),
        source_fingerprint: spec.binding.source_fingerprint.clone(),
        source_page_count: spec.binding.source_page_count,
        requested_page_set: spec.binding.requested_pages.canonical_string(),
        source_language: spec.binding.source_language.clone(),
        target_language: spec.binding.target_language.clone(),
        engine_version: spec.binding.engine_version.clone(),
        page_graph_schema_version: spec.binding.page_graph_schema_version,
        translation_patch_schema_version: spec.binding.translation_patch_schema_version,
        renderer_version: spec.binding.renderer_version.clone(),
        translation_revision: spec.translation_revision,
        component: spec.component,
        render_policy: spec.render_policy.into(),
        regular_font: PdfV3TranslationFontBinding::from_asset(spec.regular_font)?,
        bold_font: spec
            .bold_font
            .map(PdfV3TranslationFontBinding::from_asset)
            .transpose()?,
    };
    manifest.manifest_id = runtime_manifest_id(&manifest)?;
    validate_translation_runtime_manifest(&manifest)?;
    validate_runtime_manifest_binding(&manifest, spec.binding)?;
    Ok(manifest)
}

pub(crate) fn commit_translation_runtime_manifest(
    run_directory: &Path,
    manifest: &PdfV3TranslationRuntimeManifest,
) -> Result<PdfV3RuntimeManifestCommitKind, PdfV3RuntimeManifestError> {
    validate_run_directory(run_directory)?;
    validate_translation_runtime_manifest(manifest)?;
    let path = run_directory.join(RUNTIME_MANIFEST_FILENAME);
    if path
        .try_exists()
        .map_err(|error| io_error("inspect", &path, error))?
    {
        let existing = load_translation_runtime_manifest(run_directory)?;
        return if existing == *manifest {
            Ok(PdfV3RuntimeManifestCommitKind::Existing)
        } else {
            Err(PdfV3RuntimeManifestError::Conflict)
        };
    }

    let bytes = encode_manifest(manifest)?;
    let temp = unique_temp_path(&path);
    write_new_synced_file(&temp, &bytes)?;
    match fs::rename(&temp, &path) {
        Ok(()) => {
            sync_parent_directory(run_directory)?;
            Ok(PdfV3RuntimeManifestCommitKind::Created)
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if path.exists() {
                let existing = load_translation_runtime_manifest(run_directory)?;
                if existing == *manifest {
                    return Ok(PdfV3RuntimeManifestCommitKind::Existing);
                }
                return Err(PdfV3RuntimeManifestError::Conflict);
            }
            Err(io_error("commit", &path, error))
        }
    }
}

pub(crate) fn load_translation_runtime_manifest(
    run_directory: &Path,
) -> Result<PdfV3TranslationRuntimeManifest, PdfV3RuntimeManifestError> {
    validate_run_directory(run_directory)?;
    let path = run_directory.join(RUNTIME_MANIFEST_FILENAME);
    if !path
        .try_exists()
        .map_err(|error| io_error("inspect", &path, error))?
    {
        return Err(PdfV3RuntimeManifestError::Missing);
    }
    let bytes = read_limited(&path)?;
    let manifest = serde_json::from_slice::<PdfV3TranslationRuntimeManifest>(&bytes)
        .map_err(|error| PdfV3RuntimeManifestError::Serialization(error.to_string()))?;
    validate_translation_runtime_manifest(&manifest)?;
    Ok(manifest)
}

pub(crate) fn validate_runtime_manifest_binding(
    manifest: &PdfV3TranslationRuntimeManifest,
    binding: &PdfV3TranslationBinding,
) -> Result<(), PdfV3RuntimeManifestError> {
    let requested_page_set = binding.requested_pages.canonical_string();
    for (matches, field) in [
        (
            manifest.source_fingerprint == binding.source_fingerprint,
            "sourceFingerprint",
        ),
        (
            manifest.source_page_count == binding.source_page_count,
            "sourcePageCount",
        ),
        (
            manifest.requested_page_set == requested_page_set,
            "requestedPageSet",
        ),
        (
            manifest.source_language == binding.source_language,
            "sourceLanguage",
        ),
        (
            manifest.target_language == binding.target_language,
            "targetLanguage",
        ),
        (
            manifest.engine_version == binding.engine_version,
            "engineVersion",
        ),
        (
            manifest.page_graph_schema_version == binding.page_graph_schema_version,
            "pageGraphSchemaVersion",
        ),
        (
            manifest.translation_patch_schema_version == binding.translation_patch_schema_version,
            "translationPatchSchemaVersion",
        ),
        (
            manifest.renderer_version == binding.renderer_version,
            "rendererVersion",
        ),
    ] {
        if !matches {
            return Err(PdfV3RuntimeManifestError::BindingMismatch(field));
        }
    }
    Ok(())
}

fn validate_translation_runtime_manifest(
    manifest: &PdfV3TranslationRuntimeManifest,
) -> Result<(), PdfV3RuntimeManifestError> {
    if manifest.schema_version != RUNTIME_MANIFEST_SCHEMA_VERSION {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity("schemaVersion"));
    }
    for (value, field) in [
        (&manifest.manifest_id, "manifestId"),
        (&manifest.source_fingerprint, "sourceFingerprint"),
        (&manifest.source_language, "sourceLanguage"),
        (&manifest.target_language, "targetLanguage"),
        (&manifest.engine_version, "engineVersion"),
        (&manifest.renderer_version, "rendererVersion"),
        (&manifest.component.component_id, "componentId"),
        (&manifest.component.component_version, "componentVersion"),
        (
            &manifest.component.component_manifest_id,
            "componentManifestId",
        ),
        (&manifest.component.platform_os, "platformOs"),
        (&manifest.component.platform_arch, "platformArch"),
        (&manifest.component.provider_id, "providerId"),
        (&manifest.component.model_id, "modelId"),
        (&manifest.regular_font.asset_id, "regularFontAssetId"),
    ] {
        validate_identifier(value, field)?;
    }
    if manifest.source_page_count == 0 {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "sourcePageCount",
        ));
    }
    let pages = PageSet::parse(&manifest.requested_page_set, manifest.source_page_count)?;
    if pages.is_empty() || pages.canonical_string() != manifest.requested_page_set {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "requestedPageSet",
        ));
    }
    if manifest.page_graph_schema_version == 0 {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "pageGraphSchemaVersion",
        ));
    }
    if manifest.translation_patch_schema_version == 0 {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "translationPatchSchemaVersion",
        ));
    }
    if manifest.renderer_version != TRANSLATION_PATCH_RENDERER_VERSION {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "rendererVersion",
        ));
    }
    if manifest.translation_revision == 0 {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "translationRevision",
        ));
    }
    if !manifest.render_policy.minimum_fit_scale.is_finite()
        || !(0.0..=1.0).contains(&manifest.render_policy.minimum_fit_scale)
        || manifest.render_policy.minimum_fit_scale == 0.0
    {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(
            "minimumFitScale",
        ));
    }
    validate_sha256(
        &manifest.component.component_build_sha256,
        "componentBuildSha256",
    )?;
    validate_sha256(&manifest.component.model_sha256, "modelSha256")?;
    validate_font_binding(
        &manifest.regular_font,
        TranslationFontWeight::Regular,
        "regularFont",
    )?;
    if let Some(font) = &manifest.bold_font {
        validate_font_binding(font, TranslationFontWeight::Bold, "boldFont")?;
    }
    if runtime_manifest_id(manifest)? != manifest.manifest_id {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity("manifestId"));
    }
    encode_manifest(manifest)?;
    Ok(())
}

fn validate_font_binding(
    font: &PdfV3TranslationFontBinding,
    expected_weight: TranslationFontWeight,
    field: &'static str,
) -> Result<(), PdfV3RuntimeManifestError> {
    validate_identifier(&font.asset_id, field)?;
    if font.weight != expected_weight || font.byte_count == 0 {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(field));
    }
    validate_sha256(&font.fingerprint_sha256, field)
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), PdfV3RuntimeManifestError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(field));
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &'static str) -> Result<(), PdfV3RuntimeManifestError> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(PdfV3RuntimeManifestError::InvalidIdentity(field));
    }
    Ok(())
}

fn runtime_manifest_id(
    manifest: &PdfV3TranslationRuntimeManifest,
) -> Result<String, PdfV3RuntimeManifestError> {
    let mut canonical = manifest.clone();
    canonical.manifest_id.clear();
    Ok(format!("runtime-{}", sha256(&encode_manifest(&canonical)?)))
}

fn encode_manifest(
    manifest: &PdfV3TranslationRuntimeManifest,
) -> Result<Vec<u8>, PdfV3RuntimeManifestError> {
    let bytes = serde_json::to_vec(manifest)
        .map_err(|error| PdfV3RuntimeManifestError::Serialization(error.to_string()))?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_RUNTIME_MANIFEST_BYTES {
        return Err(PdfV3RuntimeManifestError::TooLarge {
            bytes: byte_count,
            maximum: MAX_RUNTIME_MANIFEST_BYTES,
        });
    }
    Ok(bytes)
}

fn validate_run_directory(path: &Path) -> Result<(), PdfV3RuntimeManifestError> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(PdfV3RuntimeManifestError::InvalidPath);
    }
    Ok(())
}

fn read_limited(path: &Path) -> Result<Vec<u8>, PdfV3RuntimeManifestError> {
    let metadata = fs::metadata(path).map_err(|error| io_error("inspect", path, error))?;
    if metadata.len() > MAX_RUNTIME_MANIFEST_BYTES {
        return Err(PdfV3RuntimeManifestError::TooLarge {
            bytes: metadata.len(),
            maximum: MAX_RUNTIME_MANIFEST_BYTES,
        });
    }
    let bytes = fs::read(path).map_err(|error| io_error("read", path, error))?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_RUNTIME_MANIFEST_BYTES {
        return Err(PdfV3RuntimeManifestError::TooLarge {
            bytes: byte_count,
            maximum: MAX_RUNTIME_MANIFEST_BYTES,
        });
    }
    Ok(bytes)
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), PdfV3RuntimeManifestError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| io_error("create", path, error))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(io_error("write", path, error));
    }
    Ok(())
}

fn unique_temp_path(target: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("runtime-manifest.json");
    target.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), counter))
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn io_error(
    operation: &'static str,
    path: &Path,
    error: std::io::Error,
) -> PdfV3RuntimeManifestError {
    PdfV3RuntimeManifestError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), PdfV3RuntimeManifestError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync directory", path, error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), PdfV3RuntimeManifestError> {
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        pdf_v3::{
            font::{TranslationFontAsset, TranslationFontWeight},
            page_set::PageSet,
            patch_renderer::{TranslationPatchRenderPolicy, TRANSLATION_PATCH_RENDERER_VERSION},
            scheduler::PdfV3TranslationBinding,
            types::{PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
        },
        rosetta_jobs::formats::pdf::unit_translation::PdfUnitProviderConfig,
    };

    use super::{
        build_translation_runtime_manifest, commit_translation_runtime_manifest,
        load_translation_runtime_manifest, BoundPdfV3TranslationRuntime,
        PdfV3RuntimeManifestCommitKind, PdfV3RuntimeManifestError,
        PdfV3TranslationComponentBinding, PdfV3TranslationRuntimeSpec,
    };

    #[test]
    fn immutable_manifest_round_trips_and_binds_live_assets() {
        let directory = TestDirectory::new("round-trip");
        let binding = binding();
        let regular = font_asset(TranslationFontWeight::Regular);
        let bold = font_asset(TranslationFontWeight::Bold);
        let manifest = manifest(&binding, &regular, Some(&bold));

        assert_eq!(
            commit_translation_runtime_manifest(directory.path(), &manifest).expect("first commit"),
            PdfV3RuntimeManifestCommitKind::Created
        );
        assert_eq!(
            commit_translation_runtime_manifest(directory.path(), &manifest)
                .expect("idempotent commit"),
            PdfV3RuntimeManifestCommitKind::Existing
        );
        let loaded = load_translation_runtime_manifest(directory.path()).expect("loaded manifest");
        assert_eq!(loaded, manifest);
        assert!(
            !fs::read_to_string(directory.path().join("runtime-manifest.json"))
                .expect("manifest JSON")
                .contains("C:\\Windows\\Fonts")
        );

        let live = BoundPdfV3TranslationRuntime::new(
            &binding,
            loaded,
            scripted_provider(),
            regular,
            Some(bold),
        )
        .expect("bound runtime");
        assert_eq!(live.manifest().component.model_id, "model-test");
    }

    #[test]
    fn conflicting_identity_and_live_asset_drift_are_rejected() {
        let directory = TestDirectory::new("conflict");
        let binding = binding();
        let regular = font_asset(TranslationFontWeight::Regular);
        let manifest = manifest(&binding, &regular, None);
        commit_translation_runtime_manifest(directory.path(), &manifest).expect("manifest commit");

        let mut conflict = manifest.clone();
        conflict.translation_revision = 2;
        conflict.manifest_id.clear();
        conflict = build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
            binding: &binding,
            translation_revision: 2,
            component: conflict.component,
            render_policy: TranslationPatchRenderPolicy::default(),
            regular_font: &regular,
            bold_font: None,
        })
        .expect("conflicting manifest");
        assert!(matches!(
            commit_translation_runtime_manifest(directory.path(), &conflict),
            Err(PdfV3RuntimeManifestError::Conflict)
        ));

        let other_font = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            Path::new(r"C:\Windows\Fonts\calibri.ttf"),
            0,
        )
        .expect("different live font");
        assert!(matches!(
            BoundPdfV3TranslationRuntime::new(
                &binding,
                manifest.clone(),
                scripted_provider(),
                other_font,
                None,
            ),
            Err(PdfV3RuntimeManifestError::FontMismatch(
                TranslationFontWeight::Regular
            ))
        ));

        let mut other_component = manifest.component.clone();
        other_component.provider_id = "rwkv-lightning-contents".to_string();
        let other_provider_manifest =
            build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
                binding: &binding,
                translation_revision: 1,
                component: other_component,
                render_policy: TranslationPatchRenderPolicy::default(),
                regular_font: &regular,
                bold_font: None,
            })
            .expect("other provider manifest");
        assert!(matches!(
            BoundPdfV3TranslationRuntime::new(
                &binding,
                other_provider_manifest,
                scripted_provider(),
                regular,
                None,
            ),
            Err(PdfV3RuntimeManifestError::ProviderMismatch { .. })
        ));
    }

    fn binding() -> PdfV3TranslationBinding {
        PdfV3TranslationBinding {
            source_fingerprint: format!("sha256:{}", "a".repeat(64)),
            source_page_count: 30,
            requested_pages: PageSet::parse("1-2,17", 30).expect("PageSet"),
            source_language: "en".to_string(),
            target_language: "zh-CN".to_string(),
            engine_version: "pdf-v3-test".to_string(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
        }
    }

    fn manifest(
        binding: &PdfV3TranslationBinding,
        regular: &TranslationFontAsset,
        bold: Option<&TranslationFontAsset>,
    ) -> super::PdfV3TranslationRuntimeManifest {
        build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
            binding,
            translation_revision: 1,
            component: PdfV3TranslationComponentBinding {
                component_id: "pdf-v3-runtime-test".to_string(),
                component_version: "1.0.0".to_string(),
                component_manifest_id: "component-manifest-test".to_string(),
                component_build_sha256: "b".repeat(64),
                platform_os: std::env::consts::OS.to_string(),
                platform_arch: std::env::consts::ARCH.to_string(),
                provider_id: "scripted-test-provider".to_string(),
                model_id: "model-test".to_string(),
                model_sha256: "c".repeat(64),
            },
            render_policy: TranslationPatchRenderPolicy::default(),
            regular_font: regular,
            bold_font: bold,
        })
        .expect("runtime manifest")
    }

    fn font_asset(weight: TranslationFontWeight) -> TranslationFontAsset {
        let path = match weight {
            TranslationFontWeight::Regular => Path::new(r"C:\Windows\Fonts\arial.ttf"),
            TranslationFontWeight::Bold => Path::new(r"C:\Windows\Fonts\arialbd.ttf"),
        };
        TranslationFontAsset::open_weighted(format!("Arial{weight:?}"), weight, path, 0)
            .expect("Windows Arial font")
    }

    fn scripted_provider() -> PdfUnitProviderConfig {
        PdfUnitProviderConfig::Scripted {
            results: Arc::new(Mutex::new(Default::default())),
            max_batch_size: 1,
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-runtime-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
