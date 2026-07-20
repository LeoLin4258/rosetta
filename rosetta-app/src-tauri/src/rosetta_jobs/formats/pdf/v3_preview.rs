use std::{fmt, path::PathBuf};

use tauri::AppHandle;

use crate::pdf_v3::{
    font_plan::{TranslationFontCharacterPlan, TranslationFontPlanError},
    page_graph_store::PageGraphStore,
    patch_store::TranslationPatchStore,
    preview::{
        insert_translation_patch_preview_png_cache, open_region_translation_preview_png_cache,
        render_region_translation_preview_png, TranslationPatchPreviewError,
        MAX_PREVIEW_PIXEL_WIDTH, MIN_PREVIEW_PIXEL_WIDTH,
    },
    region_layout::RegionLayoutPolicy,
    region_renderer::{
        insert_region_translation_page_pdf_cache, open_region_translation_page_pdf_cache,
        render_resolved_region_translation_patch_page_pdf_from_view,
        restore_region_translation_page_pdf, RegionTranslationPagePdf,
        RegionTranslationRenderError,
    },
    region_translation_patch::RegionTranslationPatch,
    render_cache::{RenderCache, RenderCacheConfig, RenderCacheError},
    scheduler::{
        DurablePdfV3Scheduler, PdfV3ExtractionAuthority, PdfV3PageState, PdfV3PatchAuthority,
    },
    source_object::PdfSourceObjectStore,
    types::PageGraph,
};

use super::{
    runtime::{get_pdfium, lock_pdfium},
    v3_component::ResolvedPdfV3RenderAssets,
    v3_control::pdf_v3_run_directory,
    v3_runtime::{
        load_translation_runtime_manifest, validate_runtime_manifest_binding,
        validate_runtime_render_assets, PdfV3TranslationRuntimeManifest,
    },
};

pub(crate) struct PdfV3PreviewAuthority {
    source_path: PathBuf,
    binding: crate::pdf_v3::scheduler::PdfV3TranslationBinding,
    runtime: PdfV3TranslationRuntimeManifest,
    page: PageGraph,
    patch: RegionTranslationPatch,
    cache: RenderCache,
    pixel_width: u32,
    cached_png: Option<Vec<u8>>,
    cached_page_pdf: Option<RegionTranslationPagePdf>,
}

impl PdfV3PreviewAuthority {
    pub(crate) fn source_fingerprint(&self) -> &str {
        &self.binding.source_fingerprint
    }

    pub(crate) fn target_language(&self) -> &str {
        &self.binding.target_language
    }

    pub(crate) fn cached_png(&self) -> Option<&[u8]> {
        self.cached_png.as_deref()
    }

    pub(crate) fn has_cached_page_pdf(&self) -> bool {
        self.cached_page_pdf.is_some()
    }
}

#[derive(Debug)]
pub(crate) enum PdfV3PreviewError {
    InvalidJob,
    InvalidPageNumber,
    InvalidPixelWidth(u32),
    InvalidRun,
    PageNotRequested(u32),
    PageNotReady(u32),
    MissingPageGraph(u32),
    MissingTranslationPatch(u32),
    AuthorityMismatch(u32),
    Scheduler,
    Runtime,
    PageGraphStore,
    PatchStore,
    Cache(RenderCacheError),
    Source,
    FontPlan(TranslationFontPlanError),
    Render(RegionTranslationRenderError),
    Raster(TranslationPatchPreviewError),
    Pdfium,
}

impl fmt::Display for PdfV3PreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJob => formatter.write_str("PDF v3 preview job is unavailable"),
            Self::InvalidPageNumber => {
                formatter.write_str("PDF v3 preview page number must be positive")
            }
            Self::InvalidPixelWidth(width) => write!(
                formatter,
                "PDF v3 preview width {width} is outside {MIN_PREVIEW_PIXEL_WIDTH}..={MAX_PREVIEW_PIXEL_WIDTH}"
            ),
            Self::InvalidRun => formatter.write_str("PDF v3 preview run is invalid"),
            Self::PageNotRequested(page) => {
                write!(formatter, "PDF v3 preview page {page} is not part of this run")
            }
            Self::PageNotReady(page) => {
                write!(formatter, "PDF v3 preview page {page} has no completed translation")
            }
            Self::MissingPageGraph(page) => {
                write!(formatter, "PDF v3 preview page {page} is missing extraction authority")
            }
            Self::MissingTranslationPatch(page) => {
                write!(formatter, "PDF v3 preview page {page} is missing translation authority")
            }
            Self::AuthorityMismatch(page) => {
                write!(formatter, "PDF v3 preview page {page} authority is stale or invalid")
            }
            Self::Scheduler => formatter.write_str("PDF v3 preview scheduler state is invalid"),
            Self::Runtime => formatter.write_str("PDF v3 preview runtime binding is invalid"),
            Self::PageGraphStore => {
                formatter.write_str("PDF v3 preview extraction store is unavailable")
            }
            Self::PatchStore => {
                formatter.write_str("PDF v3 preview translation store is unavailable")
            }
            Self::Cache(_) => formatter.write_str("PDF v3 preview cache is unavailable"),
            Self::Source => formatter.write_str("PDF v3 preview source is invalid"),
            Self::FontPlan(_) => {
                formatter.write_str("PDF v3 preview font preparation failed")
            }
            Self::Render(error) => {
                write!(formatter, "PDF v3 preview page rendering failed: {error}")
            }
            Self::Raster(error) => {
                write!(formatter, "PDF v3 preview rasterization failed: {error}")
            }
            Self::Pdfium => formatter.write_str("PDF v3 preview rasterizer is unavailable"),
        }
    }
}

impl std::error::Error for PdfV3PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            Self::FontPlan(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Raster(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TranslationFontPlanError> for PdfV3PreviewError {
    fn from(value: TranslationFontPlanError) -> Self {
        Self::FontPlan(value)
    }
}

impl From<RegionTranslationRenderError> for PdfV3PreviewError {
    fn from(value: RegionTranslationRenderError) -> Self {
        Self::Render(value)
    }
}

impl From<TranslationPatchPreviewError> for PdfV3PreviewError {
    fn from(value: TranslationPatchPreviewError) -> Self {
        Self::Raster(value)
    }
}

impl From<RenderCacheError> for PdfV3PreviewError {
    fn from(value: RenderCacheError) -> Self {
        Self::Cache(value)
    }
}

pub(crate) fn load_pdf_v3_preview_authority(
    job_directory: &std::path::Path,
    source_path: PathBuf,
    run_id: &str,
    page_number: u32,
    pixel_width: u32,
) -> Result<PdfV3PreviewAuthority, PdfV3PreviewError> {
    if !job_directory.is_absolute() || !job_directory.is_dir() || !source_path.is_file() {
        return Err(PdfV3PreviewError::InvalidJob);
    }
    if page_number == 0 {
        return Err(PdfV3PreviewError::InvalidPageNumber);
    }
    if !(MIN_PREVIEW_PIXEL_WIDTH..=MAX_PREVIEW_PIXEL_WIDTH).contains(&pixel_width) {
        return Err(PdfV3PreviewError::InvalidPixelWidth(pixel_width));
    }
    let run_directory =
        pdf_v3_run_directory(job_directory, run_id).map_err(|_| PdfV3PreviewError::InvalidRun)?;
    let scheduler =
        DurablePdfV3Scheduler::open(&run_directory).map_err(|_| PdfV3PreviewError::Scheduler)?;
    let binding = scheduler
        .translation_binding()
        .map_err(|_| PdfV3PreviewError::Scheduler)?;
    if !binding.requested_pages.contains(page_number) {
        return Err(PdfV3PreviewError::PageNotRequested(page_number));
    }
    let runtime = load_translation_runtime_manifest(&run_directory)
        .map_err(|_| PdfV3PreviewError::Runtime)?;
    validate_runtime_manifest_binding(&runtime, &binding)
        .map_err(|_| PdfV3PreviewError::Runtime)?;
    let record = scheduler
        .page_window(Some(page_number.saturating_sub(1)), 1)
        .map_err(|_| PdfV3PreviewError::Scheduler)?
        .into_iter()
        .find(|record| record.page_number == page_number)
        .ok_or(PdfV3PreviewError::PageNotRequested(page_number))?;
    let (extraction, patch_authority) =
        completed_page_authorities(page_number, record.state, record.lease.is_some())?;

    let pdf_v3_directory = job_directory.join("pdf-v3");
    let page_store = PageGraphStore::new(
        &pdf_v3_directory.join("extraction"),
        binding.source_fingerprint.clone(),
        binding.source_page_count,
        binding.engine_version.clone(),
    )
    .map_err(|_| PdfV3PreviewError::PageGraphStore)?;
    let stored_page = page_store
        .load(page_number)
        .map_err(|_| PdfV3PreviewError::PageGraphStore)?
        .ok_or(PdfV3PreviewError::MissingPageGraph(page_number))?;
    if stored_page.authority.artifact_id != extraction.artifact_id
        || stored_page.authority.source_page_hash != extraction.source_page_hash
    {
        return Err(PdfV3PreviewError::AuthorityMismatch(page_number));
    }

    let patch_store = TranslationPatchStore::new(
        &pdf_v3_directory.join("translations"),
        binding.source_fingerprint.clone(),
        binding.target_language.clone(),
    )
    .map_err(|_| PdfV3PreviewError::PatchStore)?;
    let stored_patch = patch_store
        .load_region(&stored_page.page)
        .map_err(|_| PdfV3PreviewError::PatchStore)?
        .ok_or(PdfV3PreviewError::MissingTranslationPatch(page_number))?;
    if stored_patch.patch.patch_id != patch_authority.patch_id
        || stored_patch.patch.translation_revision != patch_authority.translation_revision
        || stored_patch.patch.translation_revision != runtime.translation_revision
    {
        return Err(PdfV3PreviewError::AuthorityMismatch(page_number));
    }

    let cache = RenderCache::new(
        &pdf_v3_directory.join("render-cache"),
        RenderCacheConfig::default(),
    )?;
    let cached_png = open_region_translation_preview_png_cache(
        &cache,
        &binding.source_fingerprint,
        &stored_patch.patch,
        pixel_width,
    )?;
    let cached_page_pdf = if cached_png.is_none() {
        open_region_translation_page_pdf_cache(
            &cache,
            &binding.source_fingerprint,
            &stored_patch.patch,
        )?
        .map(|bytes| {
            restore_region_translation_page_pdf(
                &binding.source_fingerprint,
                &stored_patch.patch,
                bytes,
            )
        })
        .transpose()?
    } else {
        None
    };

    Ok(PdfV3PreviewAuthority {
        source_path,
        binding,
        runtime,
        page: stored_page.page,
        patch: stored_patch.patch,
        cache,
        pixel_width,
        cached_png,
        cached_page_pdf,
    })
}

fn completed_page_authorities(
    page_number: u32,
    state: PdfV3PageState,
    has_active_lease: bool,
) -> Result<(PdfV3ExtractionAuthority, PdfV3PatchAuthority), PdfV3PreviewError> {
    match state {
        PdfV3PageState::Completed { extraction, patch } if !has_active_lease => {
            Ok((extraction, patch))
        }
        _ => Err(PdfV3PreviewError::PageNotReady(page_number)),
    }
}

pub(crate) fn rasterize_cached_pdf_v3_preview(
    app: &AppHandle,
    authority: PdfV3PreviewAuthority,
) -> Result<Vec<u8>, PdfV3PreviewError> {
    let page_pdf =
        authority
            .cached_page_pdf
            .as_ref()
            .ok_or(PdfV3PreviewError::MissingTranslationPatch(
                authority.page.page_number,
            ))?;
    rasterize_and_cache(app, &authority, page_pdf)
}

pub(crate) fn render_pdf_v3_preview(
    app: &AppHandle,
    authority: PdfV3PreviewAuthority,
    assets: ResolvedPdfV3RenderAssets,
) -> Result<Vec<u8>, PdfV3PreviewError> {
    validate_runtime_render_assets(
        &authority.binding,
        &authority.runtime,
        &assets.regular_font,
        assets.bold_font.as_ref(),
    )
    .map_err(|_| PdfV3PreviewError::Runtime)?;
    let source = PdfSourceObjectStore::open(&authority.source_path)
        .map_err(|_| PdfV3PreviewError::Source)?;
    if source.page_count() != authority.binding.source_page_count {
        return Err(PdfV3PreviewError::Source);
    }
    let font_plan = TranslationFontCharacterPlan::for_resolved_region_replay(
        &authority.page,
        &authority.patch,
    )?;
    let prepared =
        font_plan.prepare_available_fonts(&assets.regular_font, assets.bold_font.as_ref())?;
    let fonts = prepared.iter().collect::<Vec<_>>();
    let page_pdf = render_resolved_region_translation_patch_page_pdf_from_view(
        &source,
        &authority.binding.source_fingerprint,
        &authority.page,
        &authority.patch,
        &fonts,
        RegionLayoutPolicy::default(),
    )?;
    if page_pdf.patch() != &authority.patch {
        return Err(PdfV3PreviewError::AuthorityMismatch(
            authority.page.page_number,
        ));
    }
    let _ = insert_region_translation_page_pdf_cache(&authority.cache, &page_pdf);
    rasterize_and_cache(app, &authority, &page_pdf)
}

fn rasterize_and_cache(
    app: &AppHandle,
    authority: &PdfV3PreviewAuthority,
    page_pdf: &RegionTranslationPagePdf,
) -> Result<Vec<u8>, PdfV3PreviewError> {
    let preview = {
        let _guard = lock_pdfium();
        let pdfium = get_pdfium(app).map_err(|_| PdfV3PreviewError::Pdfium)?;
        render_region_translation_preview_png(pdfium, page_pdf, authority.pixel_width)?
    };
    let _ = insert_translation_patch_preview_png_cache(&authority.cache, &preview);
    Ok(preview.into_png_bytes())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        pdf_v3::{
            document::VerifiedDocumentIdentity,
            extract::page_source_hash,
            font::{TranslationFontAsset, TranslationFontWeight},
            page_graph_store::PageGraphStore,
            page_set::PageSet,
            paragraph_translation_plan::{
                build_visual_paragraph_page_plan, resolve_visual_paragraph_results,
            },
            patch_renderer::TranslationPatchRenderPolicy,
            patch_store::TranslationPatchStore,
            preview::region_translation_preview_png_cache_key,
            region_renderer::{
                region_translation_page_pdf_cache_key, REGION_TRANSLATION_RENDERER_VERSION,
            },
            region_translation_patch::{
                build_region_translation_patch, RegionTranslationPatch,
                RegionTranslationPatchDraft, REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
            },
            render_cache::{RenderCache, RenderCacheConfig},
            scheduler::{
                DurablePdfV3Scheduler, PdfV3ExtractionAuthority, PdfV3PageState,
                PdfV3PatchAuthority, PdfV3RunSpec, PdfV3SchedulerCapacity, PdfV3SchedulerStage,
                PdfV3TranslationCommit,
            },
            translation_plan::TranslationPatchDraftMetadata,
            types::{
                PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind,
                PageReconciliationStatus, PageReconciliationSummary, PAGE_GRAPH_SCHEMA_VERSION,
            },
        },
        rosetta_jobs::formats::pdf::{
            test_helpers::fixture_path,
            v3_runtime::{
                build_translation_runtime_manifest, commit_translation_runtime_manifest,
                PdfV3TranslationComponentBinding, PdfV3TranslationRuntimeSpec,
            },
        },
    };

    use super::{completed_page_authorities, load_pdf_v3_preview_authority, PdfV3PreviewError};

    const RUN_ID: &str = "run-preview-test";
    const TARGET_LANGUAGE: &str = "zh-CN";

    #[test]
    fn preview_error_display_preserves_render_failure_detail() {
        let error = PdfV3PreviewError::Render(
            crate::pdf_v3::region_renderer::RegionTranslationRenderError::MissingRegularFont,
        );

        assert_eq!(
            error.to_string(),
            "PDF v3 preview page rendering failed: region renderer requires a regular translation font"
        );
    }

    #[test]
    fn preview_state_gate_accepts_only_unleased_completed_pages() {
        let extraction = extraction_authority();
        let patch = patch_authority("patch-test");
        assert!(completed_page_authorities(
            1,
            PdfV3PageState::Completed {
                extraction: extraction.clone(),
                patch: patch.clone(),
            },
            false,
        )
        .is_ok());

        let unavailable = [
            PdfV3PageState::Pending,
            PdfV3PageState::Extracted {
                extraction: extraction.clone(),
            },
            PdfV3PageState::Preserved {
                extraction: extraction.clone(),
                reason_code: "unsupported-page".to_string(),
            },
            PdfV3PageState::Failed {
                stage: PdfV3SchedulerStage::Translation,
                extraction: Some(extraction.clone()),
                reason_code: "provider-failed".to_string(),
                retryable: true,
            },
        ];
        for state in unavailable {
            assert!(matches!(
                completed_page_authorities(1, state, false),
                Err(PdfV3PreviewError::PageNotReady(1))
            ));
        }
        assert!(matches!(
            completed_page_authorities(1, PdfV3PageState::Completed { extraction, patch }, true,),
            Err(PdfV3PreviewError::PageNotReady(1))
        ));
    }

    #[test]
    fn completed_authority_prefers_cached_png() {
        let run = TestPreviewRun::new(false);
        let png = b"\x89PNG\r\n\x1a\npreview-cache".to_vec();
        let cache = run.cache();
        let key =
            region_translation_preview_png_cache_key(&run.source_fingerprint, &run.patch, 900)
                .expect("preview cache key");
        cache.insert(&key, &png).expect("cached PNG");

        let authority = run.load(900).expect("preview authority");

        assert_eq!(authority.cached_png(), Some(png.as_slice()));
        assert!(!authority.has_cached_page_pdf());
    }

    #[test]
    fn completed_authority_falls_back_to_cached_single_page_pdf() {
        let run = TestPreviewRun::new(false);
        let cache = run.cache();
        let key = region_translation_page_pdf_cache_key(&run.source_fingerprint, &run.patch)
            .expect("page PDF cache key");
        cache
            .insert(&key, &fs::read(&run.source_path).expect("source PDF"))
            .expect("cached page PDF");

        let authority = run.load(900).expect("preview authority");

        assert!(authority.cached_png().is_none());
        assert!(authority.has_cached_page_pdf());
    }

    #[test]
    fn scheduler_patch_authority_mismatch_is_rejected() {
        let run = TestPreviewRun::new(true);

        assert!(matches!(
            run.load(900),
            Err(PdfV3PreviewError::AuthorityMismatch(1))
        ));
    }

    #[test]
    fn invalid_or_unrequested_pages_are_rejected_before_store_access() {
        let run = TestPreviewRun::new(false);
        assert!(matches!(
            load_pdf_v3_preview_authority(
                &run.job_directory,
                run.source_path.clone(),
                RUN_ID,
                0,
                900,
            ),
            Err(PdfV3PreviewError::InvalidPageNumber)
        ));
        assert!(matches!(
            load_pdf_v3_preview_authority(
                &run.job_directory,
                run.source_path.clone(),
                RUN_ID,
                2,
                900,
            ),
            Err(PdfV3PreviewError::PageNotRequested(2))
        ));
    }

    #[test]
    fn public_preview_errors_do_not_expose_internal_paths() {
        let secret_path = PathBuf::from(r"C:\private\customer-contract.pdf");
        let error = PdfV3PreviewError::Cache(crate::pdf_v3::render_cache::RenderCacheError::Io {
            operation: "read",
            path: secret_path.clone(),
            message: "private storage failure".to_string(),
        });
        let public = error.to_string();

        assert!(!public.contains(secret_path.to_string_lossy().as_ref()));
        assert!(!public.contains("private storage failure"));
    }

    struct TestPreviewRun {
        root: PathBuf,
        job_directory: PathBuf,
        source_path: PathBuf,
        source_fingerprint: String,
        patch: RegionTranslationPatch,
    }

    impl TestPreviewRun {
        fn new(mismatched_scheduler_patch: bool) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-preview-{}-{nanos}",
                std::process::id()
            ));
            let job_directory = root.join("job-test");
            let source_path = job_directory.join("source.pdf");
            fs::create_dir_all(&job_directory).expect("job directory");
            fs::copy(fixture_path("simple-one-page.pdf"), &source_path).expect("source fixture");
            let identity = VerifiedDocumentIdentity::verify(&source_path).expect("source identity");
            let source_fingerprint = identity.source_fingerprint().to_string();
            let run_directory = job_directory.join("pdf-v3").join("runs").join(RUN_ID);
            let scheduler = DurablePdfV3Scheduler::create(
                &run_directory,
                PdfV3RunSpec {
                    run_id: RUN_ID.to_string(),
                    source_fingerprint: source_fingerprint.clone(),
                    source_page_count: 1,
                    requested_pages: PageSet::from_pages([1]).expect("page set"),
                    source_language: "en".to_string(),
                    target_language: TARGET_LANGUAGE.to_string(),
                    engine_version: "pdf-v3-preview-test".to_string(),
                    page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
                    translation_patch_schema_version: REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
                    renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
                },
                PdfV3SchedulerCapacity {
                    max_extracting_pages: 1,
                    max_extracted_pages: 1,
                    max_translating_pages: 1,
                },
                "owner-test",
                1,
            )
            .expect("scheduler");
            commit_runtime(&run_directory, &scheduler);

            let page = empty_page(&source_fingerprint);
            let page_store = PageGraphStore::new(
                &job_directory.join("pdf-v3").join("extraction"),
                &source_fingerprint,
                1,
                "pdf-v3-preview-test",
            )
            .expect("page store");
            let page_commit = page_store.commit(&page).expect("page commit");
            let plan = build_visual_paragraph_page_plan(&page).expect("empty paragraph plan");
            let translations = resolve_visual_paragraph_results(&plan, Vec::new(), TARGET_LANGUAGE)
                .expect("empty translations");
            let patch = build_region_translation_patch(
                &page,
                RegionTranslationPatchDraft {
                    plan,
                    translations,
                    metadata: TranslationPatchDraftMetadata {
                        target_language: TARGET_LANGUAGE.to_string(),
                        translation_revision: 1,
                        provider_id: "provider-test".to_string(),
                        model_id: "model-test".to_string(),
                        renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
                    },
                },
            )
            .expect("empty resolved patch");
            TranslationPatchStore::new(
                &job_directory.join("pdf-v3").join("translations"),
                &source_fingerprint,
                TARGET_LANGUAGE,
            )
            .expect("patch store")
            .commit_region(&page, &patch)
            .expect("patch commit");

            let extraction_claim = scheduler
                .claim_extraction("owner-test", 1, 2)
                .expect("extraction claim")
                .remove(0);
            scheduler
                .commit_extraction(
                    "owner-test",
                    &extraction_claim,
                    PdfV3ExtractionAuthority {
                        artifact_id: page_commit.authority.artifact_id,
                        source_page_hash: page_commit.authority.source_page_hash,
                    },
                    3,
                )
                .expect("extraction authority");
            let translation_claim = scheduler
                .claim_translation("owner-test", 1, 4)
                .expect("translation claim")
                .remove(0);
            let patch_id = if mismatched_scheduler_patch {
                format!("sha256:{}", "f".repeat(64))
            } else {
                patch.patch_id.clone()
            };
            scheduler
                .commit_translation(
                    "owner-test",
                    &translation_claim,
                    PdfV3TranslationCommit::Patch(patch_authority(&patch_id)),
                    5,
                )
                .expect("translation authority");

            Self {
                root,
                job_directory,
                source_path,
                source_fingerprint,
                patch,
            }
        }

        fn cache(&self) -> RenderCache {
            RenderCache::new(
                &self.job_directory.join("pdf-v3").join("render-cache"),
                RenderCacheConfig::default(),
            )
            .expect("render cache")
        }

        fn load(&self, width: u32) -> Result<super::PdfV3PreviewAuthority, PdfV3PreviewError> {
            load_pdf_v3_preview_authority(
                &self.job_directory,
                self.source_path.clone(),
                RUN_ID,
                1,
                width,
            )
        }
    }

    impl Drop for TestPreviewRun {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn commit_runtime(run_directory: &Path, scheduler: &DurablePdfV3Scheduler) {
        let binding = scheduler.translation_binding().expect("binding");
        let regular = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            Path::new(r"C:\Windows\Fonts\arial.ttf"),
            0,
        )
        .expect("Windows Arial");
        let manifest = build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
            binding: &binding,
            translation_revision: 1,
            component: PdfV3TranslationComponentBinding {
                component_id: "pdf-v3-preview-test".to_string(),
                component_version: "1.0.0".to_string(),
                component_manifest_id: "component-manifest-test".to_string(),
                component_build_sha256: "b".repeat(64),
                platform_os: std::env::consts::OS.to_string(),
                platform_arch: std::env::consts::ARCH.to_string(),
                provider_id: "provider-test".to_string(),
                model_id: "model-test".to_string(),
                model_sha256: "c".repeat(64),
            },
            render_policy: TranslationPatchRenderPolicy::default(),
            regular_font: &regular,
            bold_font: None,
        })
        .expect("runtime manifest");
        commit_translation_runtime_manifest(run_directory, &manifest).expect("runtime commit");
    }

    fn empty_page(source_fingerprint: &str) -> PageGraph {
        let atom_id = "atom-preserved".to_string();
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: page_source_hash(source_fingerprint, 1),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms: vec![PageAtom {
                atom_id: atom_id.clone(),
                source_text: "Preserved source".to_string(),
                source_object_id: None,
                kind: PageAtomKind::Body,
                style_id: None,
                bounds: [40.0, 700.0, 140.0, 720.0],
                loose_bounds: None,
                origin: Some([40.0, 700.0]),
                text_matrix: None,
                angle_degrees: Some(0.0),
                order: 0,
                generated: false,
                hyphen: false,
                requires_translation: true,
                source_kind: PageAtomSourceKind::PreservedUnmapped,
                source_provenance: None,
            }],
            styles: Vec::new(),
            groups: vec![
                PageGroup {
                    group_id: "line-preserved".to_string(),
                    kind: PageGroupKind::Line,
                    atom_ids: vec![atom_id.clone()],
                    bounds: [40.0, 700.0, 140.0, 720.0],
                    confidence: 0.99,
                },
                PageGroup {
                    group_id: "paragraph-preserved".to_string(),
                    kind: PageGroupKind::Paragraph,
                    atom_ids: vec![atom_id.clone()],
                    bounds: [40.0, 700.0, 140.0, 720.0],
                    confidence: 0.99,
                },
                PageGroup {
                    group_id: "container-preserved".to_string(),
                    kind: PageGroupKind::FlowContainer,
                    atom_ids: vec![atom_id],
                    bounds: [40.0, 700.0, 140.0, 720.0],
                    confidence: 0.99,
                },
            ],
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary {
                status: PageReconciliationStatus::Complete,
                mapped_object_count: 0,
                preserved_object_count: 0,
                verified_atom_count: 0,
                corrected_atom_count: 0,
                synthetic_whitespace_atom_count: 0,
                unrepresented_source_whitespace_count: 0,
                preserved_atom_count: 0,
                fallback_reasons: Vec::new(),
            },
            warnings: Vec::new(),
        }
    }

    fn extraction_authority() -> PdfV3ExtractionAuthority {
        PdfV3ExtractionAuthority {
            artifact_id: format!("sha256:{}", "a".repeat(64)),
            source_page_hash: format!("sha256:{}", "b".repeat(64)),
        }
    }

    fn patch_authority(patch_id: &str) -> PdfV3PatchAuthority {
        PdfV3PatchAuthority {
            patch_id: patch_id.to_string(),
            translation_revision: 1,
        }
    }
}
