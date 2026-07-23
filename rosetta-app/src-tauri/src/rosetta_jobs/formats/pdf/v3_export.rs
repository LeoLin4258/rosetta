use std::{fmt, path::Path};

use serde::Serialize;

use crate::pdf_v3::{
    document::VerifiedDocumentIdentity,
    incremental_export::IncrementalExportCancellation,
    page_graph_store::PageGraphStore,
    page_set::PageSet,
    patch_store::TranslationPatchStore,
    region_layout::RegionLayoutPolicy,
    scheduler::{DurablePdfV3Scheduler, PdfV3PageState, PdfV3RunState},
    source_object::PdfSourceObjectStore,
    translation_export::{
        export_region_translation_pdf_atomic, PdfV3RegionTranslationExportRequest,
        PdfV3RegionTranslationExportResult,
    },
};

use super::{
    v3_component::ResolvedPdfV3RenderAssets,
    v3_control::pdf_v3_run_directory,
    v3_runtime::{
        load_translation_runtime_manifest, validate_runtime_manifest_binding,
        validate_runtime_render_assets,
    },
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RunExportResult {
    pub schema: &'static str,
    pub run_id: String,
    pub target_language: String,
    pub requested_page_count: usize,
    pub translated_page_count: usize,
    pub preserved_page_count: usize,
    pub export: PdfV3RegionTranslationExportResult,
}

#[derive(Debug)]
pub(crate) enum PdfV3RunExportError {
    InvalidRun,
    InvalidDestination,
    RunNotCompleted,
    ActiveLeases,
    Authority,
    Source,
    Scheduler,
    Runtime,
    PageGraph,
    Patch,
    Export,
}

impl fmt::Display for PdfV3RunExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRun => "PDF v3 导出运行无效。",
            Self::InvalidDestination => "PDF v3 导出目标无效。",
            Self::RunNotCompleted => "PDF v3 运行尚未完成。",
            Self::ActiveLeases => "PDF v3 运行仍有活动页面。",
            Self::Authority => "PDF v3 持久化权威不一致。",
            Self::Source => "PDF v3 源文件校验失败。",
            Self::Scheduler => "PDF v3 调度状态不可用。",
            Self::Runtime => "PDF v3 运行时绑定不可用。",
            Self::PageGraph => "PDF v3 页面提取结果不可用。",
            Self::Patch => "PDF v3 翻译结果不可用。",
            Self::Export => "PDF v3 原子导出失败。",
        })
    }
}

impl std::error::Error for PdfV3RunExportError {}

pub(crate) fn export_target_language(
    job_directory: &Path,
    run_id: &str,
) -> Result<String, PdfV3RunExportError> {
    let run_directory =
        pdf_v3_run_directory(job_directory, run_id).map_err(|_| PdfV3RunExportError::InvalidRun)?;
    let scheduler =
        DurablePdfV3Scheduler::open(&run_directory).map_err(|_| PdfV3RunExportError::Scheduler)?;
    let snapshot = scheduler
        .status_snapshot()
        .map_err(|_| PdfV3RunExportError::Scheduler)?;
    if snapshot.run_state != PdfV3RunState::Completed {
        return Err(PdfV3RunExportError::RunNotCompleted);
    }
    Ok(snapshot.target_language)
}

pub(crate) fn export_pdf_v3_run(
    job_directory: &Path,
    source_path: &Path,
    run_id: &str,
    destination_path: &Path,
    assets: ResolvedPdfV3RenderAssets,
) -> Result<PdfV3RunExportResult, PdfV3RunExportError> {
    if !job_directory.is_absolute() || !job_directory.is_dir() || !source_path.is_file() {
        return Err(PdfV3RunExportError::InvalidRun);
    }
    if !destination_path.is_absolute()
        || destination_path.file_name().is_none()
        || destination_path
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("pdf"))
    {
        return Err(PdfV3RunExportError::InvalidDestination);
    }

    let run_directory =
        pdf_v3_run_directory(job_directory, run_id).map_err(|_| PdfV3RunExportError::InvalidRun)?;
    let scheduler =
        DurablePdfV3Scheduler::open(&run_directory).map_err(|_| PdfV3RunExportError::Scheduler)?;
    let snapshot = scheduler
        .status_snapshot()
        .map_err(|_| PdfV3RunExportError::Scheduler)?;
    if snapshot.run_id != run_id {
        return Err(PdfV3RunExportError::Authority);
    }
    if snapshot.run_state != PdfV3RunState::Completed {
        return Err(PdfV3RunExportError::RunNotCompleted);
    }
    if snapshot.summary.extracting_pages != 0 || snapshot.summary.translating_pages != 0 {
        return Err(PdfV3RunExportError::ActiveLeases);
    }

    let binding = scheduler
        .translation_binding()
        .map_err(|_| PdfV3RunExportError::Scheduler)?;
    let runtime = load_translation_runtime_manifest(&run_directory)
        .map_err(|_| PdfV3RunExportError::Runtime)?;
    validate_runtime_manifest_binding(&runtime, &binding)
        .map_err(|_| PdfV3RunExportError::Runtime)?;
    validate_runtime_render_assets(
        &binding,
        &runtime,
        &assets.regular_font,
        assets.bold_font.as_ref(),
    )
    .map_err(|_| PdfV3RunExportError::Runtime)?;

    let identity =
        VerifiedDocumentIdentity::verify(source_path).map_err(|_| PdfV3RunExportError::Source)?;
    if identity.source_fingerprint() != binding.source_fingerprint {
        return Err(PdfV3RunExportError::Source);
    }
    let source_metadata = super::source_state::read_pdf_source_metadata(job_directory)
        .map_err(|_| PdfV3RunExportError::Source)?
        .ok_or(PdfV3RunExportError::Source)?;
    if source_metadata.schema_version != crate::rosetta_jobs::model::SCHEMA_VERSION
        || source_metadata.source_fingerprint != binding.source_fingerprint
        || source_metadata.page_count != binding.source_page_count
    {
        return Err(PdfV3RunExportError::Source);
    }
    let source =
        PdfSourceObjectStore::open(source_path).map_err(|_| PdfV3RunExportError::Source)?;
    if source.page_count() != binding.source_page_count {
        return Err(PdfV3RunExportError::Source);
    }

    let pdf_v3_directory = job_directory.join("pdf-v3");
    let page_graph_store = PageGraphStore::new(
        &pdf_v3_directory.join("extraction"),
        binding.source_fingerprint.clone(),
        binding.source_page_count,
        binding.engine_version.clone(),
    )
    .map_err(|_| PdfV3RunExportError::PageGraph)?;
    let patch_store = TranslationPatchStore::new(
        &pdf_v3_directory.join("translations"),
        binding.source_fingerprint.clone(),
        binding.target_language.clone(),
    )
    .map_err(|_| PdfV3RunExportError::Patch)?;

    let mut translated_pages = Vec::new();
    let mut preserved_page_count = 0usize;
    let mut seen_pages = 0usize;
    let mut start_after = None;
    loop {
        let records = scheduler
            .page_window(start_after, 256)
            .map_err(|_| PdfV3RunExportError::Scheduler)?;
        if records.is_empty() {
            break;
        }
        for record in records.iter() {
            if record.lease.is_some() {
                return Err(PdfV3RunExportError::ActiveLeases);
            }
            seen_pages = seen_pages.saturating_add(1);
            match &record.state {
                PdfV3PageState::Completed { extraction, patch } => {
                    let stored_page = page_graph_store
                        .load(record.page_number)
                        .map_err(|_| PdfV3RunExportError::PageGraph)?
                        .ok_or(PdfV3RunExportError::PageGraph)?;
                    if stored_page.authority.artifact_id != extraction.artifact_id
                        || stored_page.authority.source_page_hash != extraction.source_page_hash
                    {
                        return Err(PdfV3RunExportError::Authority);
                    }
                    let stored_patch = patch_store
                        .load_region(&stored_page.page)
                        .map_err(|_| PdfV3RunExportError::Patch)?
                        .ok_or(PdfV3RunExportError::Patch)?;
                    if stored_patch.patch.patch_id != patch.patch_id
                        || stored_patch.patch.translation_revision != patch.translation_revision
                        || stored_patch.patch.translation_revision != runtime.translation_revision
                    {
                        return Err(PdfV3RunExportError::Authority);
                    }
                    translated_pages.push(record.page_number);
                }
                PdfV3PageState::Preserved { .. } => {
                    preserved_page_count = preserved_page_count.saturating_add(1);
                }
                _ => return Err(PdfV3RunExportError::RunNotCompleted),
            }
        }
        let last_page = records.last().map(|record| record.page_number);
        if !last_page.is_some_and(|page| {
            snapshot
                .requested_pages
                .pages()
                .iter()
                .any(|next| *next > page)
        }) {
            break;
        }
        start_after = last_page;
    }
    if seen_pages != snapshot.requested_pages.pages().len()
        || seen_pages
            != snapshot.summary.completed_pages as usize + snapshot.summary.preserved_pages as usize
    {
        return Err(PdfV3RunExportError::Authority);
    }

    let pages =
        PageSet::from_pages(translated_pages).map_err(|_| PdfV3RunExportError::Authority)?;
    let export = export_region_translation_pdf_atomic(
        &source,
        PdfV3RegionTranslationExportRequest {
            source_fingerprint: &binding.source_fingerprint,
            destination_path,
            pages: &pages,
            page_graph_store: &page_graph_store,
            patch_store: &patch_store,
            regular_font: &assets.regular_font,
            bold_font: assets.bold_font.as_ref(),
            layout_policy: RegionLayoutPolicy::default(),
            cancellation: &IncrementalExportCancellation::default(),
        },
    )
    .map_err(|_| PdfV3RunExportError::Export)?;

    Ok(PdfV3RunExportResult {
        schema: "rosetta-pdf-v3-run-export/1",
        run_id: run_id.to_string(),
        target_language: binding.target_language,
        requested_page_count: snapshot.requested_pages.pages().len(),
        translated_page_count: snapshot.summary.completed_pages as usize,
        preserved_page_count,
        export,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::pdf_v3::{
        page_set::PageSet,
        region_renderer::REGION_TRANSLATION_RENDERER_VERSION,
        region_translation_patch::REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
        scheduler::{DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3SchedulerCapacity},
        types::{PAGE_GRAPH_SCHEMA_VERSION, PDF_V3_ENGINE_VERSION},
    };

    use super::{export_target_language, PdfV3RunExportError};

    #[test]
    fn target_language_rejects_nonterminal_runs() {
        let test = TestDirectory::new("nonterminal");
        create_scheduler(test.path(), PageSet::from_pages([1]).expect("page set"));

        assert!(matches!(
            export_target_language(test.path(), "run-test"),
            Err(PdfV3RunExportError::RunNotCompleted)
        ));
    }

    #[test]
    fn target_language_accepts_completed_empty_run_and_rejects_unsafe_id() {
        let test = TestDirectory::new("completed");
        create_scheduler(test.path(), PageSet::empty());

        assert_eq!(
            export_target_language(test.path(), "run-test").expect("target language"),
            "zh-CN"
        );
        assert!(matches!(
            export_target_language(test.path(), "../private"),
            Err(PdfV3RunExportError::InvalidRun)
        ));
    }

    #[test]
    fn public_errors_do_not_expose_internal_values() {
        let secret = r"C:\private\customer-contract.pdf";
        for error in [
            PdfV3RunExportError::InvalidRun,
            PdfV3RunExportError::InvalidDestination,
            PdfV3RunExportError::Authority,
            PdfV3RunExportError::Source,
            PdfV3RunExportError::Export,
        ] {
            assert!(!error.to_string().contains(secret));
        }
    }

    fn create_scheduler(job_directory: &Path, requested_pages: PageSet) {
        let run_directory = job_directory.join("pdf-v3").join("runs").join("run-test");
        DurablePdfV3Scheduler::create(
            &run_directory,
            PdfV3RunSpec {
                run_id: "run-test".to_string(),
                source_fingerprint: format!("sha256:{}", "a".repeat(64)),
                source_page_count: 1,
                requested_pages,
                source_language: "en".to_string(),
                target_language: "zh-CN".to_string(),
                engine_version: PDF_V3_ENGINE_VERSION.to_string(),
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
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-run-export-{label}-{}-{nanos}",
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
