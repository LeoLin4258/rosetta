#![allow(dead_code)]

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::pdf_v3::{
    font::{TranslationFontAsset, TranslationFontError, TranslationFontWeight},
    font_plan::{TranslationFontCharacterPlan, TranslationFontPlanError},
    ownership::{PdfStreamOwnershipError, PdfStreamOwnershipIndex},
    page_index::{PdfPageIndex, PdfPageIndexError},
    paragraph_translation_plan::{
        build_visual_paragraph_page_plan,
        resolve_visual_paragraph_results_preserving_invalid_containers,
    },
    patch_renderer::{TranslationPatchRenderError, TranslationPatchRenderPolicy},
    pipeline::{
        PdfV3TranslationPageProcessor, PdfV3TranslationPageResult, PdfV3TranslationProcessFailure,
    },
    region_layout::RegionLayoutPolicy,
    region_renderer::{
        stage_region_translation_patch_with_context, RegionTranslationRenderError,
        REGION_TRANSLATION_RENDERER_VERSION,
    },
    region_translation_patch::{
        build_region_translation_patch_preserving_containers, RegionTranslationPatchDraft,
    },
    replacement::TextShowReplacementError,
    scheduler::PdfV3TranslationBinding,
    source_object::PdfSourceObjectStore,
    translation_plan::TranslationPatchDraftMetadata,
    types::PageGraph,
};

use super::unit_translation::{
    translate_pdf_v3_visual_paragraph_plan, PdfUnitProviderConfig, PdfV3ProviderFailureKind,
};
use super::v3_runtime::BoundPdfV3TranslationRuntime;

const NO_SAFE_UNITS_REASON: &str = "pdf-v3-no-safe-translation-units";
const INVALID_RUNTIME_IDENTITY_REASON: &str = "pdf-v3-runtime-identity-mismatch";
const INVALID_TRANSLATION_PLAN_REASON: &str = "pdf-v3-translation-plan-invalid";
const PROVIDER_FAILURE_REASON: &str = "pdf-v3-translation-provider-failed";
const CANCELLED_REASON: &str = "pdf-v3-translation-cancelled";
const RENDERER_FONT_PLAN_FAILURE_REASON: &str = "pdf-v3-renderer-font-plan-failed";
const RENDERER_FONT_PREPARE_FAILURE_REASON: &str = "pdf-v3-renderer-font-prepare-failed";
const RENDERER_FONT_MISSING_GLYPHS_REASON: &str = "pdf-v3-renderer-font-missing-glyphs";
const RENDERER_FONT_CHARACTER_LIMIT_REASON: &str = "pdf-v3-renderer-font-character-limit";
const RENDERER_FONT_ASSET_FAILURE_REASON: &str = "pdf-v3-renderer-font-asset-failed";
const RENDERER_FONT_STAGE_FAILURE_REASON: &str = "pdf-v3-renderer-font-stage-failed";
const RENDERER_FONT_PREPARED_GLYPH_REASON: &str = "pdf-v3-renderer-font-prepared-glyph-missing";
const RENDERER_FONT_DUPLICATE_WEIGHT_REASON: &str = "pdf-v3-renderer-font-duplicate-weight";
const RENDERER_FONT_DOCUMENT_FACE_REASON: &str = "pdf-v3-renderer-font-document-face-missing";
const RENDERER_FONT_DOCUMENT_IDENTITY_REASON: &str =
    "pdf-v3-renderer-font-document-identity-mismatch";
const RENDERER_FONT_DOCUMENT_OBJECT_REASON: &str = "pdf-v3-renderer-font-document-object-invalid";
const RENDERER_FONT_PAGE_RESOURCES_REASON: &str = "pdf-v3-renderer-font-page-resources-invalid";
const RENDERER_GEOMETRY_FAILURE_REASON: &str = "pdf-v3-renderer-geometry-failed";
const RENDERER_STYLE_FAILURE_REASON: &str = "pdf-v3-renderer-style-failed";
const RENDERER_OWNERSHIP_FAILURE_REASON: &str = "pdf-v3-renderer-ownership-failed";
const RENDERER_CONTENT_FAILURE_REASON: &str = "pdf-v3-renderer-content-failed";
const RENDERER_TRANSACTION_FAILURE_REASON: &str = "pdf-v3-renderer-transaction-failed";
const RENDERER_PATCH_FAILURE_REASON: &str = "pdf-v3-renderer-patch-invalid";
const RENDERER_CACHE_FAILURE_REASON: &str = "pdf-v3-renderer-cache-failed";
const RENDERER_REGION_FAILURE_REASON: &str = "pdf-v3-region-renderer-failed";

#[derive(Clone)]
pub(crate) struct PdfV3LocalPageProcessorConfig {
    source_fingerprint: String,
    provider: PdfUnitProviderConfig,
    provider_id: String,
    model_id: String,
    source_language: String,
    target_language: String,
    translation_revision: u64,
    renderer_version: String,
    render_policy: TranslationPatchRenderPolicy,
    cancel: Arc<AtomicBool>,
    regular_font: TranslationFontAsset,
    bold_font: Option<TranslationFontAsset>,
}

impl PdfV3LocalPageProcessorConfig {
    pub(crate) fn from_runtime(
        runtime: &BoundPdfV3TranslationRuntime,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        let manifest = runtime.manifest();
        Self {
            source_fingerprint: manifest.source_fingerprint.clone(),
            provider: runtime.provider().clone(),
            provider_id: manifest.component.provider_id.clone(),
            model_id: manifest.component.model_id.clone(),
            source_language: manifest.source_language.clone(),
            target_language: manifest.target_language.clone(),
            translation_revision: manifest.translation_revision,
            renderer_version: manifest.renderer_version.clone(),
            render_policy: runtime.render_policy(),
            cancel,
            regular_font: runtime.regular_font().clone(),
            bold_font: runtime.bold_font().cloned(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum PdfV3LocalPageProcessorConfigError {
    InvalidIdentity(&'static str),
    BindingMismatch(&'static str),
    SourcePageCountMismatch {
        expected: u32,
        actual: u32,
    },
    FontWeightMismatch {
        field: &'static str,
        expected: TranslationFontWeight,
        actual: TranslationFontWeight,
    },
    PageIndex(PdfPageIndexError),
    Ownership(PdfStreamOwnershipError),
}

impl fmt::Display for PdfV3LocalPageProcessorConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(field) => {
                write!(formatter, "PDF v3 page processor {field} is invalid")
            }
            Self::BindingMismatch(field) => {
                write!(
                    formatter,
                    "PDF v3 page processor {field} does not match the run"
                )
            }
            Self::SourcePageCountMismatch { expected, actual } => write!(
                formatter,
                "PDF v3 page processor source has {actual} pages; the run requires {expected}"
            ),
            Self::FontWeightMismatch {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "PDF v3 page processor {field} has weight {actual:?}; expected {expected:?}"
            ),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::Ownership(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PdfV3LocalPageProcessorConfigError {}

impl From<PdfPageIndexError> for PdfV3LocalPageProcessorConfigError {
    fn from(value: PdfPageIndexError) -> Self {
        Self::PageIndex(value)
    }
}

impl From<PdfStreamOwnershipError> for PdfV3LocalPageProcessorConfigError {
    fn from(value: PdfStreamOwnershipError) -> Self {
        Self::Ownership(value)
    }
}

pub(crate) struct PdfV3LocalPageProcessor<'a> {
    source_objects: &'a PdfSourceObjectStore,
    config: PdfV3LocalPageProcessorConfig,
    page_index: PdfPageIndex,
    ownership_index: PdfStreamOwnershipIndex,
}

impl<'a> PdfV3LocalPageProcessor<'a> {
    pub(crate) fn new(
        source_objects: &'a PdfSourceObjectStore,
        binding: &PdfV3TranslationBinding,
        config: PdfV3LocalPageProcessorConfig,
    ) -> Result<Self, PdfV3LocalPageProcessorConfigError> {
        validate_config(source_objects, binding, &config)?;
        let page_index = PdfPageIndex::resolve(source_objects, &binding.requested_pages)?;
        let ownership_index = PdfStreamOwnershipIndex::resolve(
            source_objects,
            &page_index.selected_content_stream_ids(),
        )?;
        Ok(Self {
            source_objects,
            config,
            page_index,
            ownership_index,
        })
    }

    async fn process(
        &mut self,
        page: &PageGraph,
        binding: &PdfV3TranslationBinding,
    ) -> Result<PdfV3TranslationPageResult, PdfV3TranslationProcessFailure> {
        if !self.matches_binding(binding) {
            return Err(process_failure(INVALID_RUNTIME_IDENTITY_REASON, false));
        }
        if self.is_cancelled() {
            return Err(process_failure(CANCELLED_REASON, true));
        }

        let plan = build_visual_paragraph_page_plan(page).map_err(|error| {
            log_page_stage_failure(page.page_number, "plan", &error);
            process_failure(INVALID_TRANSLATION_PLAN_REASON, false)
        })?;
        if plan.units.is_empty() {
            return Ok(PdfV3TranslationPageResult::Preserved {
                reason_code: NO_SAFE_UNITS_REASON,
            });
        }

        let translated = translate_pdf_v3_visual_paragraph_plan(
            &self.config.provider,
            &self.config.source_language,
            &self.config.target_language,
            &plan,
            Some(Arc::clone(&self.config.cancel)),
        )
        .await
        .map_err(|failure| match failure.kind {
            PdfV3ProviderFailureKind::NoTranslatableUnits => {
                process_failure(INVALID_TRANSLATION_PLAN_REASON, false)
            }
            PdfV3ProviderFailureKind::InvalidPlan => {
                process_failure(INVALID_TRANSLATION_PLAN_REASON, false)
            }
            PdfV3ProviderFailureKind::Cancelled => process_failure(CANCELLED_REASON, true),
            PdfV3ProviderFailureKind::Provider => {
                process_failure(PROVIDER_FAILURE_REASON, failure.retryable)
            }
        })?;
        if self.is_cancelled() {
            return Err(process_failure(CANCELLED_REASON, true));
        }

        let resolved = resolve_visual_paragraph_results_preserving_invalid_containers(
            &plan,
            translated.results,
            &self.config.target_language,
        )
        .map_err(|error| {
            log_page_stage_failure(page.page_number, "provider-result-resolution", &error);
            process_failure(INVALID_TRANSLATION_PLAN_REASON, false)
        })?;
        let pending_patch = build_region_translation_patch_preserving_containers(
            page,
            RegionTranslationPatchDraft {
                plan,
                translations: resolved.translations,
                metadata: TranslationPatchDraftMetadata {
                    target_language: self.config.target_language.clone(),
                    translation_revision: self.config.translation_revision,
                    provider_id: self.config.provider_id.clone(),
                    model_id: self.config.model_id.clone(),
                    renderer_version: self.config.renderer_version.clone(),
                },
            },
            &resolved.preserved_containers,
        )
        .map_err(|error| {
            log_page_stage_failure(page.page_number, "region-patch", &error);
            process_failure(INVALID_TRANSLATION_PLAN_REASON, false)
        })?;

        let font_plan =
            TranslationFontCharacterPlan::for_pending_region_patch(page, &pending_patch)
                .map_err(|_| process_failure(RENDERER_FONT_PLAN_FAILURE_REASON, false))?;
        let prepared_fonts = font_plan
            .prepare_available_fonts(&self.config.regular_font, self.config.bold_font.as_ref())
            .map_err(|error| {
                log_font_prepare_failure(&error);
                process_failure(font_prepare_failure_reason(&error), false)
            })?;
        let fonts = prepared_fonts.iter().collect::<Vec<_>>();
        let staged = stage_region_translation_patch_with_context(
            self.source_objects,
            &self.page_index,
            &self.ownership_index,
            page,
            &pending_patch,
            &fonts,
            RegionLayoutPolicy::default(),
        )
        .map_err(|error| {
            log_page_stage_failure(page.page_number, "region-renderer", &error);
            process_failure(region_renderer_failure_reason(&error), false)
        })?;
        if self.is_cancelled() {
            return Err(process_failure(CANCELLED_REASON, true));
        }
        Ok(PdfV3TranslationPageResult::RegionPatch(
            staged.resolved_patch,
        ))
    }

    fn matches_binding(&self, binding: &PdfV3TranslationBinding) -> bool {
        self.config.source_fingerprint == binding.source_fingerprint
            && self.config.source_language == binding.source_language
            && self.config.target_language == binding.target_language
            && self.config.renderer_version == binding.renderer_version
            && binding.renderer_version == REGION_TRANSLATION_RENDERER_VERSION
            && self.source_objects.page_count() == binding.source_page_count
    }

    fn is_cancelled(&self) -> bool {
        self.config.cancel.load(Ordering::SeqCst)
    }
}

impl PdfV3TranslationPageProcessor for PdfV3LocalPageProcessor<'_> {
    fn process_page<'a>(
        &'a mut self,
        page: &'a PageGraph,
        binding: &'a PdfV3TranslationBinding,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PdfV3TranslationPageResult, PdfV3TranslationProcessFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(self.process(page, binding))
    }

    fn is_cancelled(&self) -> bool {
        PdfV3LocalPageProcessor::is_cancelled(self)
    }
}

fn validate_config(
    source_objects: &PdfSourceObjectStore,
    binding: &PdfV3TranslationBinding,
    config: &PdfV3LocalPageProcessorConfig,
) -> Result<(), PdfV3LocalPageProcessorConfigError> {
    for (value, field) in [
        (&config.source_fingerprint, "sourceFingerprint"),
        (&config.provider_id, "providerId"),
        (&config.model_id, "modelId"),
        (&config.source_language, "sourceLanguage"),
        (&config.target_language, "targetLanguage"),
        (&config.renderer_version, "rendererVersion"),
    ] {
        validate_identity(value, field)?;
    }
    if config.translation_revision == 0 {
        return Err(PdfV3LocalPageProcessorConfigError::InvalidIdentity(
            "translationRevision",
        ));
    }
    let mut fonts = vec![(
        config.regular_font.weight(),
        TranslationFontWeight::Regular,
        "regularFont",
    )];
    if let Some(bold) = &config.bold_font {
        fonts.push((bold.weight(), TranslationFontWeight::Bold, "boldFont"));
    }
    for (actual, expected, field) in fonts {
        if actual != expected {
            return Err(PdfV3LocalPageProcessorConfigError::FontWeightMismatch {
                field,
                expected,
                actual,
            });
        }
    }
    for (matches, field) in [
        (
            config.source_fingerprint == binding.source_fingerprint,
            "sourceFingerprint",
        ),
        (
            config.source_language == binding.source_language,
            "sourceLanguage",
        ),
        (
            config.target_language == binding.target_language,
            "targetLanguage",
        ),
        (
            config.renderer_version == binding.renderer_version
                && config.renderer_version == REGION_TRANSLATION_RENDERER_VERSION,
            "rendererVersion",
        ),
    ] {
        if !matches {
            return Err(PdfV3LocalPageProcessorConfigError::BindingMismatch(field));
        }
    }
    if source_objects.page_count() != binding.source_page_count {
        return Err(
            PdfV3LocalPageProcessorConfigError::SourcePageCountMismatch {
                expected: binding.source_page_count,
                actual: source_objects.page_count(),
            },
        );
    }
    Ok(())
}

fn validate_identity(
    value: &str,
    field: &'static str,
) -> Result<(), PdfV3LocalPageProcessorConfigError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3LocalPageProcessorConfigError::InvalidIdentity(field));
    }
    Ok(())
}

fn process_failure(reason_code: &'static str, retryable: bool) -> PdfV3TranslationProcessFailure {
    PdfV3TranslationProcessFailure {
        reason_code,
        retryable,
    }
}

fn log_page_stage_failure(page_number: u32, stage: &str, error: &impl fmt::Display) {
    eprintln!("[pdf-v3-processor] page={page_number} stage={stage} error={error}");
}

fn renderer_failure_reason(error: &TranslationPatchRenderError) -> &'static str {
    match error {
        TranslationPatchRenderError::InvalidPolicy
        | TranslationPatchRenderError::RendererVersionMismatch { .. }
        | TranslationPatchRenderError::MixedRendererDecisionState
        | TranslationPatchRenderError::ResolvedRendererDecisionMismatch(_)
        | TranslationPatchRenderError::Patch(_) => RENDERER_PATCH_FAILURE_REASON,
        TranslationPatchRenderError::PageOutOfBounds { .. }
        | TranslationPatchRenderError::SinglePage(_)
        | TranslationPatchRenderError::PagePdfSerialization(_)
        | TranslationPatchRenderError::InvalidPagePdf(_) => RENDERER_CONTENT_FAILURE_REASON,
        TranslationPatchRenderError::Replacement(error) => replacement_failure_reason(error),
        TranslationPatchRenderError::Cache(_) => RENDERER_CACHE_FAILURE_REASON,
    }
}

fn region_renderer_failure_reason(error: &RegionTranslationRenderError) -> &'static str {
    match error {
        RegionTranslationRenderError::MissingRegularFont
        | RegionTranslationRenderError::MissingFontWeight(_) => RENDERER_FONT_STAGE_FAILURE_REASON,
        RegionTranslationRenderError::DuplicateFontWeight(_) => {
            RENDERER_FONT_DUPLICATE_WEIGHT_REASON
        }
        RegionTranslationRenderError::Font(error) => translation_font_failure_reason(error),
        RegionTranslationRenderError::InvalidProvenance(_)
        | RegionTranslationRenderError::ContainerOwnershipIncomplete(_)
        | RegionTranslationRenderError::SharedTextShow(_)
        | RegionTranslationRenderError::Ownership(_) => RENDERER_OWNERSHIP_FAILURE_REASON,
        RegionTranslationRenderError::Layout(_) => RENDERER_GEOMETRY_FAILURE_REASON,
        RegionTranslationRenderError::Patch(_) => RENDERER_PATCH_FAILURE_REASON,
        RegionTranslationRenderError::Replacement(error) => replacement_failure_reason(error),
        RegionTranslationRenderError::PageContentsInvalid
        | RegionTranslationRenderError::ResourceConflict(_)
        | RegionTranslationRenderError::ObjectNumberOverflow
        | RegionTranslationRenderError::ContentEncode(_)
        | RegionTranslationRenderError::StreamWrite(_)
        | RegionTranslationRenderError::ObjectDelta(_)
        | RegionTranslationRenderError::PageIndex(_)
        | RegionTranslationRenderError::PageContext(_)
        | RegionTranslationRenderError::SinglePage(_)
        | RegionTranslationRenderError::Cache(_)
        | RegionTranslationRenderError::InvalidPagePdf(_)
        | RegionTranslationRenderError::PagePdfSerialization(_)
        | RegionTranslationRenderError::RendererVersionMismatch => RENDERER_REGION_FAILURE_REASON,
    }
}

fn font_prepare_failure_reason(error: &TranslationFontPlanError) -> &'static str {
    match error {
        TranslationFontPlanError::CharacterLimit { .. } => RENDERER_FONT_CHARACTER_LIMIT_REASON,
        TranslationFontPlanError::MissingFontAsset(_)
        | TranslationFontPlanError::FontAssetWeightMismatch { .. } => {
            RENDERER_FONT_ASSET_FAILURE_REASON
        }
        TranslationFontPlanError::Font(TranslationFontError::MissingGlyphs(_)) => {
            RENDERER_FONT_MISSING_GLYPHS_REASON
        }
        _ => RENDERER_FONT_PREPARE_FAILURE_REASON,
    }
}

fn log_font_prepare_failure(error: &TranslationFontPlanError) {
    let TranslationFontPlanError::Font(TranslationFontError::MissingGlyphs(codepoints)) = error
    else {
        return;
    };
    let diagnostic = codepoints
        .iter()
        .take(32)
        .map(|codepoint| format!("U+{codepoint:04X}"))
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "[pdf-v3-renderer] font prepare missingGlyphCount={} codepoints={}",
        codepoints.len(),
        diagnostic,
    );
}

fn replacement_failure_reason(error: &TextShowReplacementError) -> &'static str {
    match error {
        TextShowReplacementError::Font(error) => translation_font_failure_reason(error),
        TextShowReplacementError::MissingTranslationFontFace(_) => {
            RENDERER_FONT_DOCUMENT_FACE_REASON
        }
        TextShowReplacementError::DuplicateTranslationFontFace(_) => {
            RENDERER_FONT_DUPLICATE_WEIGHT_REASON
        }
        TextShowReplacementError::FitBounds(_)
        | TextShowReplacementError::InvalidFitBounds
        | TextShowReplacementError::Overflow { .. } => RENDERER_GEOMETRY_FAILURE_REASON,
        TextShowReplacementError::Style(_)
        | TextShowReplacementError::MissingSourceFontState
        | TextShowReplacementError::SourcePaintStateUnsupported
        | TextShowReplacementError::SourceStyleMismatch => RENDERER_STYLE_FAILURE_REASON,
        TextShowReplacementError::PageContext(_)
        | TextShowReplacementError::PageIndex(_)
        | TextShowReplacementError::Ownership(_)
        | TextShowReplacementError::PageOutOfBounds { .. }
        | TextShowReplacementError::StreamOutsidePage
        | TextShowReplacementError::RepeatedPageStream => RENDERER_OWNERSHIP_FAILURE_REASON,
        TextShowReplacementError::EmptyBatch
        | TextShowReplacementError::BatchPageMismatch
        | TextShowReplacementError::DuplicateBatchTarget
        | TextShowReplacementError::EmptyTransaction
        | TextShowReplacementError::TransactionTargetMismatch
        | TextShowReplacementError::DuplicateTransactionOperation
        | TextShowReplacementError::CrossTextObjectTransaction
        | TextShowReplacementError::EmptyTranslation
        | TextShowReplacementError::LaterTextShowInTextObject
        | TextShowReplacementError::InvalidTextPositionReset => RENDERER_TRANSACTION_FAILURE_REASON,
        TextShowReplacementError::Patch(_)
        | TextShowReplacementError::OperationMissing
        | TextShowReplacementError::UnsupportedOperator(_)
        | TextShowReplacementError::SourceIdentityMismatch(_) => RENDERER_PATCH_FAILURE_REASON,
        TextShowReplacementError::ObjectDelta(_)
        | TextShowReplacementError::SourceObject(_)
        | TextShowReplacementError::StreamRead(_)
        | TextShowReplacementError::ContentDecode(_)
        | TextShowReplacementError::ContentEncode(_)
        | TextShowReplacementError::StreamWrite(_) => RENDERER_CONTENT_FAILURE_REASON,
    }
}

fn translation_font_failure_reason(error: &TranslationFontError) -> &'static str {
    match error {
        TranslationFontError::MissingGlyphs(_) => RENDERER_FONT_MISSING_GLYPHS_REASON,
        TranslationFontError::MissingPreparedGlyph(_) => RENDERER_FONT_PREPARED_GLYPH_REASON,
        TranslationFontError::DuplicatePreparedWeight(_) => RENDERER_FONT_DUPLICATE_WEIGHT_REASON,
        TranslationFontError::MissingDocumentFont(_) => RENDERER_FONT_DOCUMENT_FACE_REASON,
        TranslationFontError::DocumentFontIdentityMismatch(_) => {
            RENDERER_FONT_DOCUMENT_IDENTITY_REASON
        }
        TranslationFontError::DocumentFontObjectInvalid(_) => RENDERER_FONT_DOCUMENT_OBJECT_REASON,
        TranslationFontError::PageResources(_) => RENDERER_FONT_PAGE_RESOURCES_REASON,
        TranslationFontError::Read(_)
        | TranslationFontError::Parse(_)
        | TranslationFontError::EmbeddingRestricted
        | TranslationFontError::SubsettingRestricted
        | TranslationFontError::UnsupportedOutline
        | TranslationFontError::Subset(_) => RENDERER_FONT_ASSET_FAILURE_REASON,
        TranslationFontError::ObjectIdOverflow
        | TranslationFontError::Content(_)
        | TranslationFontError::ObjectDelta(_)
        | TranslationFontError::SourceObject(_) => RENDERER_FONT_STAGE_FAILURE_REASON,
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    use crate::{
        pdf_v3::{
            document::DocumentHandle,
            font::{TranslationFontAsset, TranslationFontWeight},
            page_set::PageSet,
            paragraph_translation_plan::build_visual_paragraph_page_plan,
            patch_renderer::TranslationPatchRenderPolicy,
            pipeline::PdfV3TranslationPageResult,
            reconcile::build_reconciled_page_graph_from_handle,
            region_renderer::REGION_TRANSLATION_RENDERER_VERSION,
            region_translation_patch::{
                RegionTranslationPatchRendererDecision, REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
            },
            scheduler::PdfV3TranslationBinding,
            source_object::PdfSourceObjectStore,
            types::{PageGroupKind, PAGE_GRAPH_SCHEMA_VERSION},
        },
        rosetta_jobs::formats::pdf::{
            test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
            unit_translation::PdfUnitProviderConfig,
            v3_runtime::{
                build_translation_runtime_manifest, BoundPdfV3TranslationRuntime,
                PdfV3TranslationComponentBinding, PdfV3TranslationRuntimeSpec,
            },
        },
        rwkv_providers::ProviderTranslateResult,
    };

    use super::{
        PdfV3LocalPageProcessor, PdfV3LocalPageProcessorConfig, PdfV3LocalPageProcessorConfigError,
        CANCELLED_REASON, NO_SAFE_UNITS_REASON,
    };

    fn binding(handle: &DocumentHandle<'_>) -> PdfV3TranslationBinding {
        PdfV3TranslationBinding {
            source_fingerprint: handle.source_fingerprint().to_string(),
            source_page_count: handle.page_count(),
            requested_pages: PageSet::all(handle.page_count()).expect("all pages"),
            source_language: "en".to_string(),
            target_language: "zh-CN".to_string(),
            engine_version: "pdf-v3-test".to_string(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            translation_patch_schema_version: REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
        }
    }

    fn font_asset(weight: TranslationFontWeight) -> TranslationFontAsset {
        let path = match weight {
            TranslationFontWeight::Regular => PathBuf::from(r"C:\Windows\Fonts\msyh.ttc"),
            TranslationFontWeight::Bold => PathBuf::from(r"C:\Windows\Fonts\msyhbd.ttc"),
        };
        TranslationFontAsset::open_weighted(format!("Arial{weight:?}"), weight, &path, 0)
            .expect("Windows Arial font")
    }

    fn provider_success(translations: Vec<String>) -> ProviderTranslateResult {
        ProviderTranslateResult {
            ok: true,
            status_code: Some(200),
            translations,
            raw_response_preview: String::new(),
            message: "ok".to_string(),
            latency_ms: 0,
        }
    }

    fn config(
        handle: &DocumentHandle<'_>,
        translations: Vec<String>,
    ) -> (
        PdfV3LocalPageProcessorConfig,
        Arc<Mutex<VecDeque<ProviderTranslateResult>>>,
    ) {
        let scripted = Arc::new(Mutex::new(VecDeque::from([provider_success(translations)])));
        let binding = binding(handle);
        let provider = PdfUnitProviderConfig::Scripted {
            results: Arc::clone(&scripted),
            max_batch_size: 256,
        };
        let regular = font_asset(TranslationFontWeight::Regular);
        let bold = font_asset(TranslationFontWeight::Bold);
        let manifest = build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
            binding: &binding,
            translation_revision: 1,
            component: PdfV3TranslationComponentBinding {
                component_id: "pdf-v3-processor-test".to_string(),
                component_version: "1.0.0".to_string(),
                component_manifest_id: "component-manifest-test".to_string(),
                component_build_sha256: "b".repeat(64),
                platform_os: std::env::consts::OS.to_string(),
                platform_arch: std::env::consts::ARCH.to_string(),
                provider_id: provider.provider_id().to_string(),
                model_id: "rwkv-model-test".to_string(),
                model_sha256: "c".repeat(64),
            },
            render_policy: TranslationPatchRenderPolicy::default(),
            regular_font: &regular,
            bold_font: Some(&bold),
        })
        .expect("processor runtime manifest");
        let runtime =
            BoundPdfV3TranslationRuntime::new(&binding, manifest, provider, regular, Some(bold))
                .expect("bound processor runtime");
        (
            PdfV3LocalPageProcessorConfig::from_runtime(&runtime, Arc::new(AtomicBool::new(false))),
            scripted,
        )
    }

    #[tokio::test]
    async fn provider_result_resolves_renderer_patch_with_page_local_fonts() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let plan = build_visual_paragraph_page_plan(&page).expect("translation plan");
        assert!(!plan.units.is_empty());
        let translations = visual_translations(&plan, "");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let result = processor
            .process(&page, &binding)
            .await
            .expect("processed page");
        let PdfV3TranslationPageResult::RegionPatch(patch) = result else {
            panic!("safe page must produce a patch");
        };
        assert!(!patch.containers.is_empty());
        assert!(patch.containers.iter().all(|container| !matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Pending
        )));
        assert!(patch
            .containers
            .iter()
            .flat_map(|container| container.paragraphs.iter())
            .all(|paragraph| paragraph.translated_text.contains("这是完整中文译文")));
    }

    #[tokio::test]
    async fn no_safe_unit_preserves_page_without_provider_io() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let mut page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        for atom in &mut page.atoms {
            atom.requires_translation = false;
        }
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, scripted) = config(&handle, Vec::new());
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let result = processor
            .process(&page, &binding)
            .await
            .expect("preserved page");
        assert_eq!(
            result,
            PdfV3TranslationPageResult::Preserved {
                reason_code: NO_SAFE_UNITS_REASON,
            }
        );
        assert_eq!(scripted.lock().expect("scripted provider").len(), 1);
    }

    #[tokio::test]
    async fn cancellation_stops_before_provider_and_renderer() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, scripted) = config(&handle, Vec::new());
        config.cancel.store(true, Ordering::SeqCst);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let failure = processor
            .process(&page, &binding)
            .await
            .expect_err("cancelled processor");
        assert_eq!(failure.reason_code, CANCELLED_REASON);
        assert!(failure.retryable);
        assert_eq!(scripted.lock().expect("scripted provider").len(), 1);
    }

    #[tokio::test]
    async fn overflow_is_resolved_as_container_preservation() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let mut page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        for group in &mut page.groups {
            if group.kind == PageGroupKind::FlowContainer {
                group.bounds[2] = group.bounds[0] + 8.0;
                group.bounds[3] = group.bounds[1] + 8.0;
            }
        }
        let plan = build_visual_paragraph_page_plan(&page).expect("translation plan");
        let translations = visual_translations(&plan, "");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let result = processor
            .process(&page, &binding)
            .await
            .expect("processed page");
        let PdfV3TranslationPageResult::RegionPatch(patch) = result else {
            panic!("overflow is represented by resolved patch decisions");
        };
        assert!(patch.containers.iter().all(|container| matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { .. }
        )));
        assert!(patch.containers.iter().any(|container| matches!(
            &container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "region-layout-overflow"
        )));
    }

    #[tokio::test]
    async fn missing_asset_glyph_preserves_affected_entries() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let plan = build_visual_paragraph_page_plan(&page).expect("translation plan");
        let translations = visual_translations(&plan, "🧪");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let result = processor
            .process(&page, &binding)
            .await
            .expect("missing asset glyph must resolve conservatively");
        let PdfV3TranslationPageResult::RegionPatch(patch) = result else {
            panic!("missing asset glyph must produce resolved preservation decisions");
        };
        assert!(patch.containers.iter().all(|container| matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { .. }
        )));
        assert!(patch.containers.iter().any(|container| matches!(
            &container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "region-font-glyph-unavailable"
        )));
    }

    #[test]
    fn runtime_identity_mismatch_is_rejected_before_page_processing() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (mut config, _) = config(&handle, Vec::new());
        config.model_id = " model-with-invalid-boundary ".to_string();

        let error = match PdfV3LocalPageProcessor::new(&source_objects, &binding, config) {
            Ok(_) => panic!("invalid runtime identity must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            PdfV3LocalPageProcessorConfigError::InvalidIdentity("modelId")
        ));
    }

    fn visual_translations(
        plan: &crate::pdf_v3::paragraph_translation_plan::VisualParagraphPagePlan,
        suffix: &str,
    ) -> Vec<String> {
        plan.units
            .iter()
            .map(|unit| {
                format!(
                    "这是完整中文译文。{}{}",
                    "译".repeat((unit.source_text.chars().count() * 45 / 100).max(8)),
                    suffix,
                )
            })
            .collect()
    }
}
