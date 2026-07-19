use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use lopdf::{Document, Object};
use windows_sys::Win32::System::{
    ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX},
    Threading::GetCurrentProcess,
};

use super::{
    document::DocumentHandle,
    font::{stage_document_translation_font_registry, TranslationFontAsset, TranslationFontWeight},
    font_plan::TranslationFontCharacterPlan,
    incremental_export::IncrementalExportCancellation,
    ownership::PdfStreamOwnershipIndex,
    page_graph_store::PageGraphStore,
    page_index::PdfPageIndex,
    page_set::PageSet,
    patch_renderer::{
        stage_translation_patch_with_font_registry, TranslationPatchRenderPolicy,
        TRANSLATION_PATCH_RENDERER_VERSION,
    },
    patch_store::TranslationPatchStore,
    pipeline::{
        PdfV3ExtractionBatchOutcome, PdfV3ExtractionWorker, PdfV3TranslationBatchOutcome,
        PdfV3TranslationPageProcessor, PdfV3TranslationPageResult, PdfV3TranslationProcessFailure,
        PdfV3TranslationWorker,
    },
    scheduler::{
        DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3RunState, PdfV3SchedulerCapacity,
        PdfV3TranslationBinding,
    },
    source_object::{PdfObjectOverlay, PdfSourceObjectStore},
    translation_export::{
        export_translation_pdf_atomic, PdfV3TranslationExportCommitKind,
        PdfV3TranslationExportRequest,
    },
    translation_plan::{
        build_translation_page_plan, build_translation_patch_from_plan,
        TranslationPatchDraftMetadata, TranslationUnitResult,
    },
    types::{PageGraph, PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
};
use crate::rosetta_jobs::formats::pdf::test_helpers::{
    fixture_path, pdfium_test_lock, shared_pdfium,
};

const ENGINE_VERSION: &str = "pdf-v3-long-document-acceptance";
const OWNER_SESSION_ID: &str = "pdf-v3-long-document-acceptance-owner";
const TARGET_LANGUAGE: &str = "de";
const PRESERVE_EVERY: u32 = 100;
const PIPELINE_WINDOW: u32 = 4;
const TRANSLATION_REVISION: u64 = 1;
const TRANSLATION_FAILURE: &str = "pdf-v3-acceptance-scripted-translation-failed";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[tokio::test(flavor = "current_thread")]
#[ignore = "manual Windows 20-page durable translation/export smoke acceptance"]
async fn manual_windows_twenty_page_end_to_end_smoke_acceptance() {
    run_long_document_acceptance(20, 10).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "manual Windows 500-page durable translation/export acceptance"]
async fn manual_windows_five_hundred_page_end_to_end_acceptance() {
    run_long_document_acceptance(500, PRESERVE_EVERY).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "manual Windows 1000-page durable translation/export acceptance"]
async fn manual_windows_thousand_page_end_to_end_acceptance() {
    run_long_document_acceptance(1_000, PRESERVE_EVERY).await;
}

async fn run_long_document_acceptance(page_count: u32, preserve_every: u32) {
    assert!(page_count >= preserve_every);
    let _guard = pdfium_test_lock();
    let temp = AcceptanceDirectory::new(page_count);
    let source_path = temp.root.join("source.pdf");
    let output_path = temp.root.join("translated.pdf");

    let fixture_started = Instant::now();
    write_repeated_page_fixture(
        &fixture_path("002-trivial-libre-office-writer.pdf"),
        &source_path,
        page_count,
    );
    let fixture_elapsed = fixture_started.elapsed();
    let source_bytes = fs::metadata(&source_path).expect("source metadata").len();

    let baseline_memory = process_memory_sample();
    let pipeline_started = Instant::now();
    let handle = DocumentHandle::open(shared_pdfium(), &source_path).expect("document handle");
    let source_objects = PdfSourceObjectStore::open(&source_path).expect("source object store");
    assert_eq!(handle.page_count(), page_count);
    assert_eq!(source_objects.page_count(), page_count);

    let scheduler_dir = temp.root.join("scheduler");
    let page_graph_dir = temp.root.join("page-graphs");
    let patch_dir = temp.root.join("translation-patches");
    let requested_pages = PageSet::all(page_count).expect("all pages");
    let scheduler = DurablePdfV3Scheduler::create(
        &scheduler_dir,
        PdfV3RunSpec {
            run_id: format!("run-{page_count}-page-acceptance"),
            source_fingerprint: handle.source_fingerprint().to_string(),
            source_page_count: page_count,
            requested_pages: requested_pages.clone(),
            source_language: "en".to_string(),
            target_language: TARGET_LANGUAGE.to_string(),
            engine_version: ENGINE_VERSION.to_string(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
        },
        PdfV3SchedulerCapacity {
            max_extracting_pages: 2,
            max_extracted_pages: PIPELINE_WINDOW,
            max_translating_pages: 1,
        },
        OWNER_SESSION_ID,
        1,
    )
    .expect("scheduler");
    let page_graph_store = PageGraphStore::new(
        &page_graph_dir,
        handle.source_fingerprint(),
        page_count,
        ENGINE_VERSION,
    )
    .expect("PageGraph store");
    let patch_store =
        TranslationPatchStore::new(&patch_dir, handle.source_fingerprint(), TARGET_LANGUAGE)
            .expect("TranslationPatch store");
    let regular_font = TranslationFontAsset::open_weighted(
        "ArialRegular",
        TranslationFontWeight::Regular,
        Path::new(r"C:\Windows\Fonts\arial.ttf"),
        0,
    )
    .expect("Windows Arial font");

    let extraction_worker =
        PdfV3ExtractionWorker::new(&handle, &scheduler, &page_graph_store, OWNER_SESSION_ID)
            .expect("extraction worker");
    let binding = scheduler
        .translation_binding()
        .expect("translation binding");
    let mut processor = ScriptedPageProcessor::new(
        &source_objects,
        &binding,
        regular_font.clone(),
        preserve_every,
    );
    let translation_worker = PdfV3TranslationWorker::new(
        &scheduler,
        &page_graph_store,
        &patch_store,
        OWNER_SESSION_ID,
    )
    .expect("translation worker");

    let mut timestamp = 10u64;
    let mut extraction = PdfV3ExtractionBatchOutcome::default();
    let mut translation = PdfV3TranslationBatchOutcome::default();
    let mut extraction_elapsed = Duration::ZERO;
    let mut translation_elapsed = Duration::ZERO;
    let mut max_working_set_bytes = baseline_memory.working_set_bytes;
    let mut max_private_bytes = baseline_memory.private_bytes;

    for _ in 0..=page_count {
        let extraction_started = Instant::now();
        let extracted = extraction_worker
            .run_batch(PIPELINE_WINDOW, || next_timestamp(&mut timestamp))
            .expect("extraction batch");
        extraction_elapsed += extraction_started.elapsed();
        accumulate_extraction(&mut extraction, extracted);

        let translation_started = Instant::now();
        let translated = translation_worker
            .run_batch(
                PIPELINE_WINDOW,
                || next_timestamp(&mut timestamp),
                &mut processor,
            )
            .await
            .expect("translation batch");
        translation_elapsed += translation_started.elapsed();
        accumulate_translation(&mut translation, translated);

        let memory = process_memory_sample();
        max_working_set_bytes = max_working_set_bytes.max(memory.working_set_bytes);
        max_private_bytes = max_private_bytes.max(memory.private_bytes);
        let (run_state, summary) = scheduler.manifest_snapshot().expect("scheduler summary");
        if run_state == PdfV3RunState::Completed {
            assert_eq!(
                summary.completed_pages,
                page_count - page_count / preserve_every
            );
            assert_eq!(summary.preserved_pages, page_count / preserve_every);
            break;
        }
    }
    let pipeline_elapsed = pipeline_started.elapsed();
    let (run_state, summary) = scheduler.manifest_snapshot().expect("completed scheduler");
    assert_eq!(run_state, PdfV3RunState::Completed);
    assert_eq!(summary.failed_pages, 0);
    assert_eq!(extraction.committed_pages, page_count);
    assert_eq!(translation.failed_pages, 0);
    assert_eq!(translation.committed_pages, summary.completed_pages);
    assert_eq!(translation.preserved_pages, summary.preserved_pages);

    let expected_translations = std::mem::take(&mut processor.expected_translations);
    let renderer_decisions = std::mem::take(&mut processor.renderer_decisions);
    let processor_timing = processor.timing;
    drop(processor);
    println!("pdf-v3 acceptance renderer decisions: {renderer_decisions:?}");
    let page_snapshot = page_graph_store
        .validated_snapshot()
        .expect("PageGraph snapshot");
    let patch_snapshot = patch_store.snapshot().expect("patch snapshot");
    assert_eq!(page_snapshot.pages.len(), page_count as usize);
    assert_eq!(patch_snapshot.pages.len(), summary.completed_pages as usize);

    let patched_pages = PageSet::from_pages(
        (1..=page_count).filter(|page_number| page_number % preserve_every != 0),
    )
    .expect("patched pages");
    fs::write(&output_path, b"atomic-replacement-sentinel")
        .expect("pre-existing destination sentinel");
    let export_started = Instant::now();
    let export = export_translation_pdf_atomic(
        &source_objects,
        PdfV3TranslationExportRequest {
            source_fingerprint: handle.source_fingerprint(),
            destination_path: &output_path,
            pages: &patched_pages,
            page_graph_store: &page_graph_store,
            patch_store: &patch_store,
            regular_font: &regular_font,
            bold_font: None,
            render_policy: TranslationPatchRenderPolicy::default(),
            cancellation: &IncrementalExportCancellation::default(),
        },
    )
    .expect("incremental translation export");
    let export_elapsed = export_started.elapsed();
    let final_memory = process_memory_sample();

    assert_eq!(
        export.commit_kind,
        PdfV3TranslationExportCommitKind::Incremental
    );
    assert_eq!(export.selected_page_count, patched_pages.pages().len());
    assert_eq!(
        export.fitted_entry_count,
        expected_translations.values().map(Vec::len).sum::<usize>()
    );
    assert_eq!(export.preserved_entry_count, 0);
    assert_eq!(export.prepared_font_count, 1);
    assert_eq!(export.font_object_count, 6);
    assert!(export.font_subset_bytes > 0);
    assert!(export.appended_bytes > 0);
    assert_eq!(export.output_bytes, source_bytes + export.appended_bytes);

    let output_bytes = fs::read(&output_path).expect("translated output");
    let original_bytes = fs::read(&source_path).expect("source bytes");
    assert!(output_bytes.starts_with(&original_bytes));
    let output_document = Document::load_mem(&output_bytes).expect("output PDF");
    assert_eq!(output_document.get_pages().len(), page_count as usize);
    assert_no_atomic_residue(&temp.root, &output_path);
    verify_sampled_output(
        &original_bytes,
        &output_bytes,
        page_count,
        preserve_every,
        &expected_translations,
    );

    let page_graph_disk_bytes = directory_file_bytes(&page_graph_dir);
    let patch_disk_bytes = directory_file_bytes(&patch_dir);
    let scheduler_disk_bytes = directory_file_bytes(&scheduler_dir);
    println!(
        "pdf-v3 long-e2e pages={page_count} sourceBytes={source_bytes} fixtureMs={} pipelineMs={} extractionWallMs={} translationWallMs={} exportMs={} extractionReconciliationUs={} pageGraphStoreUs={} pageGraphSerializeCompressUs={} translationProcessorUs={} patchStoreUs={} processorPlanPatchUs={} processorFontPlanUs={} processorFontPrepareUs={} processorFontStageUs={} processorRenderStageUs={} pageGraphUncompressedBytes={} pageGraphCompressedBytes={} pageGraphDiskBytes={page_graph_disk_bytes} patchUncompressedBytes={} patchStoredBytes={} patchDiskBytes={patch_disk_bytes} schedulerDiskBytes={scheduler_disk_bytes} completedPatches={} preservedPages={} fittedEntries={} fontSubsetBytes={} deltaObjects={} appendedBytes={} outputBytes={} baselineWorkingSet={} maxPipelineWorkingSet={} finalWorkingSet={} processPeakWorkingSet={} baselinePrivate={} maxPipelinePrivate={} finalPrivate={}",
        fixture_elapsed.as_millis(),
        pipeline_elapsed.as_millis(),
        extraction_elapsed.as_millis(),
        translation_elapsed.as_millis(),
        export_elapsed.as_millis(),
        extraction.reconciliation_us,
        extraction.page_graph_store_us,
        extraction.page_graph_serialize_compress_us,
        translation.processor_us,
        translation.patch_store_us,
        processor_timing.plan_patch_us,
        processor_timing.font_plan_us,
        processor_timing.font_prepare_us,
        processor_timing.font_stage_us,
        processor_timing.render_stage_us,
        page_snapshot.uncompressed_bytes,
        page_snapshot.compressed_bytes,
        translation.uncompressed_patch_bytes,
        translation.patch_bytes,
        summary.completed_pages,
        summary.preserved_pages,
        export.fitted_entry_count,
        export.font_subset_bytes,
        export.delta_object_count,
        export.appended_bytes,
        export.output_bytes,
        baseline_memory.working_set_bytes,
        max_working_set_bytes,
        final_memory.working_set_bytes,
        final_memory.peak_working_set_bytes,
        baseline_memory.private_bytes,
        max_private_bytes,
        final_memory.private_bytes,
    );
}

struct ScriptedPageProcessor<'a> {
    source_objects: &'a PdfSourceObjectStore,
    page_index: PdfPageIndex,
    ownership_index: PdfStreamOwnershipIndex,
    regular_font: TranslationFontAsset,
    preserve_every: u32,
    expected_translations: BTreeMap<u32, Vec<String>>,
    renderer_decisions: BTreeMap<String, u32>,
    timing: ScriptedProcessorTiming,
}

#[derive(Debug, Clone, Copy, Default)]
struct ScriptedProcessorTiming {
    plan_patch_us: u64,
    font_plan_us: u64,
    font_prepare_us: u64,
    font_stage_us: u64,
    render_stage_us: u64,
}

impl<'a> ScriptedPageProcessor<'a> {
    fn new(
        source_objects: &'a PdfSourceObjectStore,
        binding: &PdfV3TranslationBinding,
        regular_font: TranslationFontAsset,
        preserve_every: u32,
    ) -> Self {
        let page_index = PdfPageIndex::resolve(source_objects, &binding.requested_pages)
            .expect("translation page index");
        let ownership_index = PdfStreamOwnershipIndex::resolve(
            source_objects,
            &page_index.selected_content_stream_ids(),
        )
        .expect("translation ownership index");
        Self {
            source_objects,
            page_index,
            ownership_index,
            regular_font,
            preserve_every,
            expected_translations: BTreeMap::new(),
            renderer_decisions: BTreeMap::new(),
            timing: ScriptedProcessorTiming::default(),
        }
    }

    fn process(
        &mut self,
        page: &PageGraph,
        binding: &PdfV3TranslationBinding,
    ) -> Result<PdfV3TranslationPageResult, PdfV3TranslationProcessFailure> {
        if page.page_number % self.preserve_every == 0 {
            return Ok(PdfV3TranslationPageResult::Preserved {
                reason_code: "pdf-v3-acceptance-preserved-page",
            });
        }
        let plan_started = Instant::now();
        let plan = build_translation_page_plan(page).map_err(|_| translation_failure())?;
        let results = plan
            .units
            .iter()
            .map(|unit| TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: scripted_translation(&unit.provider_text),
            })
            .collect::<Vec<_>>();
        let pending = build_translation_patch_from_plan(
            page,
            &plan,
            results,
            TranslationPatchDraftMetadata {
                target_language: binding.target_language.clone(),
                translation_revision: TRANSLATION_REVISION,
                provider_id: "deterministic-acceptance-provider".to_string(),
                model_id: "scripted-acceptance-model".to_string(),
                renderer_version: binding.renderer_version.clone(),
            },
        )
        .map_err(|_| translation_failure())?;
        self.timing.plan_patch_us = self
            .timing
            .plan_patch_us
            .saturating_add(elapsed_us(plan_started.elapsed()));
        let font_plan_started = Instant::now();
        let font_plan = TranslationFontCharacterPlan::for_pending_patch(page, &pending)
            .map_err(|_| translation_failure())?;
        self.timing.font_plan_us = self
            .timing
            .font_plan_us
            .saturating_add(elapsed_us(font_plan_started.elapsed()));
        let font_prepare_started = Instant::now();
        let prepared_fonts = font_plan
            .prepare_available_fonts(&self.regular_font, None)
            .map_err(|_| translation_failure())?;
        self.timing.font_prepare_us = self
            .timing
            .font_prepare_us
            .saturating_add(elapsed_us(font_prepare_started.elapsed()));
        let fonts = prepared_fonts.iter().collect::<Vec<_>>();
        let font_stage_started = Instant::now();
        let staged_fonts = stage_document_translation_font_registry(self.source_objects, &fonts)
            .map_err(|_| translation_failure())?;
        self.timing.font_stage_us = self
            .timing
            .font_stage_us
            .saturating_add(elapsed_us(font_stage_started.elapsed()));
        let overlay = PdfObjectOverlay::new(self.source_objects, &staged_fonts.object_delta);
        let render_stage_started = Instant::now();
        let staged = stage_translation_patch_with_font_registry(
            self.source_objects,
            &overlay,
            &self.page_index,
            &self.ownership_index,
            page,
            &pending,
            &fonts,
            TranslationPatchRenderPolicy::default(),
            &staged_fonts.registry,
        )
        .map_err(|_| translation_failure())?;
        self.timing.render_stage_us = self
            .timing
            .render_stage_us
            .saturating_add(elapsed_us(render_stage_started.elapsed()));
        for entry in &staged.render.resolved_patch.entries {
            let decision = match &entry.renderer_decision {
                super::types::TranslationPatchRendererDecision::Fitted { .. } => "fitted",
                super::types::TranslationPatchRendererDecision::Preserved { reason_code } => {
                    reason_code.as_str()
                }
                super::types::TranslationPatchRendererDecision::Pending => "pending",
            };
            *self
                .renderer_decisions
                .entry(decision.to_string())
                .or_default() += 1;
        }
        self.expected_translations.insert(
            page.page_number,
            staged
                .render
                .resolved_patch
                .entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.renderer_decision,
                        super::types::TranslationPatchRendererDecision::Fitted { .. }
                    )
                })
                .map(|entry| entry.translated_text.clone())
                .collect(),
        );
        Ok(PdfV3TranslationPageResult::Patch(
            staged.render.resolved_patch,
        ))
    }
}

impl PdfV3TranslationPageProcessor for ScriptedPageProcessor<'_> {
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
        Box::pin(async move { self.process(page, binding) })
    }
}

fn translation_failure() -> PdfV3TranslationProcessFailure {
    PdfV3TranslationProcessFailure {
        reason_code: TRANSLATION_FAILURE,
        retryable: false,
    }
}

fn scripted_translation(text: &str) -> String {
    let mut translated = String::from("Translated");
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'{' && cursor + 3 < bytes.len() && bytes[cursor + 1] == b'v' {
            let mut end = cursor + 2;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > cursor + 2 && end < bytes.len() && bytes[end] == b'}' {
                translated.push(' ');
                translated.push_str(&text[cursor..=end]);
                cursor = end + 1;
                continue;
            }
        }
        cursor += 1;
    }
    translated
}

fn write_repeated_page_fixture(template_path: &Path, destination: &Path, page_count: u32) {
    assert!(page_count > 0);
    let mut document = Document::load(template_path).expect("load one-page fixture");
    let pages = document.get_pages();
    assert_eq!(pages.len(), 1, "acceptance fixture must have one page");
    let template_page_id = *pages.values().next().expect("template page id");
    let template_page = document
        .get_object(template_page_id)
        .and_then(Object::as_dict)
        .expect("template page dictionary")
        .clone();
    let catalog_id = document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog reference");
    let pages_id = document
        .get_object(catalog_id)
        .and_then(Object::as_dict)
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference)
        .expect("root Pages reference");

    let mut page_ids = Vec::with_capacity(page_count as usize);
    for page_index in 0..page_count {
        let mut page = template_page.clone();
        page.set("Parent", Object::Reference(pages_id));
        let page_id = if page_index == 0 {
            document
                .objects
                .insert(template_page_id, Object::Dictionary(page));
            template_page_id
        } else {
            document.add_object(Object::Dictionary(page))
        };
        page_ids.push(Object::Reference(page_id));
    }
    let pages_dictionary = document
        .get_object_mut(pages_id)
        .and_then(Object::as_dict_mut)
        .expect("root Pages dictionary");
    pages_dictionary.set("Count", Object::Integer(i64::from(page_count)));
    pages_dictionary.set("Kids", Object::Array(page_ids));
    document
        .save(destination)
        .expect("save repeated-page fixture");
}

fn verify_sampled_output(
    source_bytes: &[u8],
    output_bytes: &[u8],
    page_count: u32,
    preserve_every: u32,
    expected_translations: &BTreeMap<u32, Vec<String>>,
) {
    let pdfium = shared_pdfium();
    let source = pdfium
        .load_pdf_from_byte_slice(source_bytes, None)
        .expect("source PDFium document");
    let output = pdfium
        .load_pdf_from_byte_slice(output_bytes, None)
        .expect("output PDFium document");
    let mut translated_samples = vec![1, page_count / 2 + 1, page_count - 1];
    translated_samples.sort_unstable();
    translated_samples.dedup();
    for page_number in translated_samples {
        if page_number % preserve_every == 0 {
            continue;
        }
        let expected = expected_translations
            .get(&page_number)
            .expect("sampled translated page");
        let output_text = output
            .pages()
            .get((page_number - 1) as i32)
            .expect("sampled output page")
            .text()
            .expect("sampled output text")
            .all();
        for translation in expected {
            assert!(
                output_text.contains(translation),
                "page {page_number} is missing scripted translation"
            );
        }
    }
    let preserved_page = preserve_every;
    let source_text = source
        .pages()
        .get((preserved_page - 1) as i32)
        .expect("preserved source page")
        .text()
        .expect("preserved source text")
        .all();
    let output_text = output
        .pages()
        .get((preserved_page - 1) as i32)
        .expect("preserved output page")
        .text()
        .expect("preserved output text")
        .all();
    assert_eq!(output_text, source_text);
}

fn accumulate_extraction(
    total: &mut PdfV3ExtractionBatchOutcome,
    batch: PdfV3ExtractionBatchOutcome,
) {
    total.claimed_pages = total.claimed_pages.saturating_add(batch.claimed_pages);
    total.committed_pages = total.committed_pages.saturating_add(batch.committed_pages);
    total.failed_pages = total.failed_pages.saturating_add(batch.failed_pages);
    total.claim_us = total.claim_us.saturating_add(batch.claim_us);
    total.reconciliation_us = total
        .reconciliation_us
        .saturating_add(batch.reconciliation_us);
    total.page_graph_store_us = total
        .page_graph_store_us
        .saturating_add(batch.page_graph_store_us);
    total.page_graph_serialize_compress_us = total
        .page_graph_serialize_compress_us
        .saturating_add(batch.page_graph_serialize_compress_us);
    total.scheduler_commit_us = total
        .scheduler_commit_us
        .saturating_add(batch.scheduler_commit_us);
}

fn accumulate_translation(
    total: &mut PdfV3TranslationBatchOutcome,
    batch: PdfV3TranslationBatchOutcome,
) {
    total.claimed_pages = total.claimed_pages.saturating_add(batch.claimed_pages);
    total.committed_pages = total.committed_pages.saturating_add(batch.committed_pages);
    total.preserved_pages = total.preserved_pages.saturating_add(batch.preserved_pages);
    total.failed_pages = total.failed_pages.saturating_add(batch.failed_pages);
    total.claim_us = total.claim_us.saturating_add(batch.claim_us);
    total.page_graph_load_us = total
        .page_graph_load_us
        .saturating_add(batch.page_graph_load_us);
    total.processor_us = total.processor_us.saturating_add(batch.processor_us);
    total.patch_store_us = total.patch_store_us.saturating_add(batch.patch_store_us);
    total.scheduler_commit_us = total
        .scheduler_commit_us
        .saturating_add(batch.scheduler_commit_us);
    total.patch_bytes = total.patch_bytes.saturating_add(batch.patch_bytes);
    total.uncompressed_patch_bytes = total
        .uncompressed_patch_bytes
        .saturating_add(batch.uncompressed_patch_bytes);
}

fn next_timestamp(timestamp: &mut u64) -> u64 {
    *timestamp = timestamp.saturating_add(1);
    *timestamp
}

fn elapsed_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn directory_file_bytes(root: &Path) -> u64 {
    let mut pending = vec![root.to_path_buf()];
    let mut total = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("list acceptance directory") {
            let entry = entry.expect("acceptance directory entry");
            let metadata = entry.metadata().expect("acceptance entry metadata");
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

fn assert_no_atomic_residue(directory: &Path, destination: &Path) {
    let destination_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .expect("destination filename");
    let residues = fs::read_dir(directory)
        .expect("output directory")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != destination_name && name.contains(destination_name))
        .collect::<Vec<_>>();
    assert!(residues.is_empty(), "atomic export residue: {residues:?}");
}

#[derive(Debug, Clone, Copy)]
struct ProcessMemorySample {
    working_set_bytes: usize,
    peak_working_set_bytes: usize,
    private_bytes: usize,
}

fn process_memory_sample() -> ProcessMemorySample {
    let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS_EX>() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let result = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    assert_ne!(result, 0, "read Windows process memory counters");
    ProcessMemorySample {
        working_set_bytes: counters.WorkingSetSize,
        peak_working_set_bytes: counters.PeakWorkingSetSize,
        private_bytes: counters.PrivateUsage,
    }
}

struct AcceptanceDirectory {
    root: PathBuf,
}

impl AcceptanceDirectory {
    fn new(page_count: u32) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rosetta-pdf-v3-long-e2e-{}-{sequence}-{page_count}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("acceptance root");
        Self { root }
    }
}

impl Drop for AcceptanceDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn scripted_translation_keeps_protected_tokens_exact() {
    assert_eq!(
        scripted_translation("Hello {v0}, World {v12}!"),
        "Translated {v0} {v12}"
    );
}
