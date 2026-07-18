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
    font::{
        stage_document_translation_font_registry, DocumentTranslationFontRegistry,
        PreparedTranslationFont, TranslationFontError, TranslationFontWeight,
    },
    object_delta::PdfObjectDelta,
    ownership::{PdfStreamOwnershipError, PdfStreamOwnershipIndex},
    page_index::{PdfPageIndex, PdfPageIndexError},
    patch_renderer::{
        stage_translation_patch_with_font_registry, TranslationPatchRenderPolicy,
        TRANSLATION_PATCH_RENDERER_VERSION,
    },
    pipeline::{
        PdfV3TranslationPageProcessor, PdfV3TranslationPageResult, PdfV3TranslationProcessFailure,
    },
    scheduler::PdfV3TranslationBinding,
    source_object::{PdfObjectOverlay, PdfSourceObjectStore},
    translation_plan::{
        build_translation_page_plan, build_translation_patch_from_plan,
        TranslationPatchDraftMetadata, TranslationPlanError,
    },
    types::PageGraph,
};

use super::unit_translation::{
    translate_pdf_v3_page_plan, PdfUnitProviderConfig, PdfV3ProviderFailureKind,
};

const NO_SAFE_UNITS_REASON: &str = "pdf-v3-no-safe-translation-units";
const INVALID_RUNTIME_IDENTITY_REASON: &str = "pdf-v3-runtime-identity-mismatch";
const INVALID_TRANSLATION_PLAN_REASON: &str = "pdf-v3-translation-plan-invalid";
const PROVIDER_FAILURE_REASON: &str = "pdf-v3-translation-provider-failed";
const CANCELLED_REASON: &str = "pdf-v3-translation-cancelled";
const RENDERER_FAILURE_REASON: &str = "pdf-v3-translation-renderer-failed";

#[derive(Debug, Clone)]
pub(crate) struct PdfV3LocalPageProcessorConfig {
    pub source_fingerprint: String,
    pub provider: PdfUnitProviderConfig,
    pub provider_id: String,
    pub model_id: String,
    pub source_language: String,
    pub target_language: String,
    pub translation_revision: u64,
    pub renderer_version: String,
    pub render_policy: TranslationPatchRenderPolicy,
    pub cancel: Arc<AtomicBool>,
    pub regular_font: PreparedTranslationFont,
    pub bold_font: PreparedTranslationFont,
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
    Font(TranslationFontError),
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
            Self::Font(error) => error.fmt(formatter),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::Ownership(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PdfV3LocalPageProcessorConfigError {}

impl From<TranslationFontError> for PdfV3LocalPageProcessorConfigError {
    fn from(value: TranslationFontError) -> Self {
        Self::Font(value)
    }
}

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
    font_registry: DocumentTranslationFontRegistry,
    accumulated_delta: PdfObjectDelta,
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
        let staged_fonts = stage_document_translation_font_registry(
            source_objects,
            &[&config.regular_font, &config.bold_font],
        )?;
        Ok(Self {
            source_objects,
            config,
            page_index,
            ownership_index,
            font_registry: staged_fonts.registry,
            accumulated_delta: staged_fonts.object_delta,
        })
    }

    #[cfg(test)]
    pub(crate) fn accumulated_object_count(&self) -> usize {
        self.accumulated_delta.object_count()
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

        let plan = match build_translation_page_plan(page) {
            Ok(plan) => plan,
            Err(TranslationPlanError::NoTranslatableUnits) => {
                return Ok(PdfV3TranslationPageResult::Preserved {
                    reason_code: NO_SAFE_UNITS_REASON,
                });
            }
            Err(_) => return Err(process_failure(INVALID_TRANSLATION_PLAN_REASON, false)),
        };
        if plan.units.is_empty() {
            return Ok(PdfV3TranslationPageResult::Preserved {
                reason_code: NO_SAFE_UNITS_REASON,
            });
        }

        let translated = translate_pdf_v3_page_plan(
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

        let pending_patch = build_translation_patch_from_plan(
            page,
            &plan,
            translated.results,
            TranslationPatchDraftMetadata {
                target_language: self.config.target_language.clone(),
                translation_revision: self.config.translation_revision,
                provider_id: self.config.provider_id.clone(),
                model_id: self.config.model_id.clone(),
                renderer_version: self.config.renderer_version.clone(),
            },
        )
        .map_err(|_| process_failure(INVALID_TRANSLATION_PLAN_REASON, false))?;

        let accumulated = PdfObjectOverlay::new(self.source_objects, &self.accumulated_delta);
        let staged = stage_translation_patch_with_font_registry(
            self.source_objects,
            &accumulated,
            &self.page_index,
            &self.ownership_index,
            page,
            &pending_patch,
            &[&self.config.regular_font, &self.config.bold_font],
            self.config.render_policy,
            &self.font_registry,
        )
        .map_err(|_| process_failure(RENDERER_FAILURE_REASON, false))?;
        if self.is_cancelled() {
            return Err(process_failure(CANCELLED_REASON, true));
        }
        self.accumulated_delta
            .merge(staged.object_delta)
            .map_err(|_| process_failure(RENDERER_FAILURE_REASON, false))?;
        Ok(PdfV3TranslationPageResult::Patch(
            staged.render.resolved_patch,
        ))
    }

    fn matches_binding(&self, binding: &PdfV3TranslationBinding) -> bool {
        self.config.source_fingerprint == binding.source_fingerprint
            && self.config.source_language == binding.source_language
            && self.config.target_language == binding.target_language
            && self.config.renderer_version == binding.renderer_version
            && binding.renderer_version == TRANSLATION_PATCH_RENDERER_VERSION
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
    for (actual, expected, field) in [
        (
            config.regular_font.weight(),
            TranslationFontWeight::Regular,
            "regularFont",
        ),
        (
            config.bold_font.weight(),
            TranslationFontWeight::Bold,
            "boldFont",
        ),
    ] {
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
                && config.renderer_version == TRANSLATION_PATCH_RENDERER_VERSION,
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
            font::{TranslationFontAsset, TranslationFontWeight, UnifiedTranslationFontPlan},
            page_set::PageSet,
            patch_renderer::{TranslationPatchRenderPolicy, TRANSLATION_PATCH_RENDERER_VERSION},
            pipeline::PdfV3TranslationPageResult,
            reconcile::build_reconciled_page_graph_from_handle,
            scheduler::PdfV3TranslationBinding,
            source_object::PdfSourceObjectStore,
            translation_plan::build_translation_page_plan,
            types::{
                TranslationPatchRendererDecision, PAGE_GRAPH_SCHEMA_VERSION,
                TRANSLATION_PATCH_SCHEMA_VERSION,
            },
        },
        rosetta_jobs::formats::pdf::{
            test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
            unit_translation::PdfUnitProviderConfig,
        },
        rwkv_providers::ProviderTranslateResult,
    };

    use super::{
        PdfV3LocalPageProcessor, PdfV3LocalPageProcessorConfig, PdfV3LocalPageProcessorConfigError,
        CANCELLED_REASON, NO_SAFE_UNITS_REASON, RENDERER_FAILURE_REASON,
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
            translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
        }
    }

    fn prepared_font(
        weight: TranslationFontWeight,
        text: &str,
    ) -> crate::pdf_v3::font::PreparedTranslationFont {
        let path = match weight {
            TranslationFontWeight::Regular => PathBuf::from(r"C:\Windows\Fonts\arial.ttf"),
            TranslationFontWeight::Bold => PathBuf::from(r"C:\Windows\Fonts\arialbd.ttf"),
        };
        let asset =
            TranslationFontAsset::open_weighted(format!("Arial{weight:?}"), weight, &path, 0)
                .expect("Windows Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(text);
        asset.prepare(&plan).expect("prepared Arial subset")
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
        prepared_text: &str,
    ) -> (
        PdfV3LocalPageProcessorConfig,
        Arc<Mutex<VecDeque<ProviderTranslateResult>>>,
    ) {
        let scripted = Arc::new(Mutex::new(VecDeque::from([provider_success(translations)])));
        (
            PdfV3LocalPageProcessorConfig {
                source_fingerprint: handle.source_fingerprint().to_string(),
                provider: PdfUnitProviderConfig::Scripted {
                    results: Arc::clone(&scripted),
                    max_batch_size: 256,
                },
                provider_id: "rwkv-local-test".to_string(),
                model_id: "rwkv-model-test".to_string(),
                source_language: "en".to_string(),
                target_language: "zh-CN".to_string(),
                translation_revision: 1,
                renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                render_policy: TranslationPatchRenderPolicy::default(),
                cancel: Arc::new(AtomicBool::new(false)),
                regular_font: prepared_font(TranslationFontWeight::Regular, prepared_text),
                bold_font: prepared_font(TranslationFontWeight::Bold, prepared_text),
            },
            scripted,
        )
    }

    #[tokio::test]
    async fn provider_result_resolves_renderer_patch_and_accumulates_only_render_delta() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let plan = build_translation_page_plan(&page).expect("translation plan");
        assert!(!plan.units.is_empty());
        let translated = "Translated";
        let translations = vec![translated.to_string(); plan.units.len()];
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations, translated);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");
        let font_object_count = processor.accumulated_object_count();

        let result = processor
            .process(&page, &binding)
            .await
            .expect("processed page");
        let PdfV3TranslationPageResult::Patch(patch) = result else {
            panic!("safe page must produce a patch");
        };
        assert!(!patch.entries.is_empty());
        assert!(patch.entries.iter().all(|entry| !matches!(
            entry.renderer_decision,
            TranslationPatchRendererDecision::Pending
        )));
        assert!(patch.entries.iter().any(|entry| matches!(
            entry.renderer_decision,
            TranslationPatchRendererDecision::Fitted { .. }
        )));
        assert!(processor.accumulated_object_count() > font_object_count);
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
        let (config, scripted) = config(&handle, Vec::new(), "A");
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
        let (config, scripted) = config(&handle, Vec::new(), "A");
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
    async fn overflow_is_resolved_as_entry_preservation() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let plan = build_translation_page_plan(&page).expect("translation plan");
        let translated = "W".repeat(4_096);
        let translations = vec![translated.clone(); plan.units.len()];
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations, &translated);
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let result = processor
            .process(&page, &binding)
            .await
            .expect("processed page");
        let PdfV3TranslationPageResult::Patch(patch) = result else {
            panic!("overflow is represented by resolved patch decisions");
        };
        assert!(patch.entries.iter().all(|entry| matches!(
            entry.renderer_decision,
            TranslationPatchRendererDecision::Preserved { .. }
        )));
        assert!(patch.entries.iter().any(|entry| matches!(
            &entry.renderer_decision,
            TranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "translation-overflow"
        )));
    }

    #[tokio::test]
    async fn missing_prepared_glyph_is_renderer_failure_not_a_patch() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let plan = build_translation_page_plan(&page).expect("translation plan");
        let translations = vec!["Z".to_string(); plan.units.len()];
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (config, _) = config(&handle, translations, "A");
        let mut processor =
            PdfV3LocalPageProcessor::new(&source_objects, &binding, config).expect("processor");

        let failure = processor
            .process(&page, &binding)
            .await
            .expect_err("missing prepared glyph must fail rendering");
        assert_eq!(failure.reason_code, RENDERER_FAILURE_REASON);
        assert!(!failure.retryable);
    }

    #[test]
    fn runtime_identity_mismatch_is_rejected_before_page_processing() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let source_objects = PdfSourceObjectStore::open(&source).expect("lazy source");
        let binding = binding(&handle);
        let (mut config, _) = config(&handle, Vec::new(), "A");
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
}
