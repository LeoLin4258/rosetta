use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    pdf_v3::scheduler::{
        DurablePdfV3Scheduler, PdfV3Cancellation, PdfV3PageState, PdfV3RunState,
        PdfV3SchedulerError, PdfV3SchedulerStage, PdfV3SchedulerSummary,
    },
    rosetta_jobs::path::is_safe_job_id,
};

use super::v3_runtime::{
    load_translation_runtime_manifest, validate_runtime_manifest_binding,
    PdfV3RuntimeManifestError, PdfV3TranslationFontBinding,
};

pub(crate) const DEFAULT_PDF_V3_STATUS_WINDOW: usize = 64;
pub(crate) const MAX_PDF_V3_STATUS_WINDOW: usize = 256;
const USER_CANCEL_REASON: &str = "user-requested";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RuntimeIdentityStatus {
    pub manifest_id: String,
    pub component_id: String,
    pub component_version: String,
    pub component_manifest_id: String,
    pub component_build_sha256: String,
    pub platform_os: String,
    pub platform_arch: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_sha256: String,
    pub translation_revision: u64,
    pub renderer_version: String,
    pub minimum_fit_scale: f32,
    pub regular_font: PdfV3TranslationFontBinding,
    pub bold_font: Option<PdfV3TranslationFontBinding>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3PageLeaseStatus {
    pub stage: PdfV3SchedulerStage,
    pub leased_at_ms: u64,
    pub owned_by_current_session: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3PageControlStatus {
    pub page_number: u32,
    pub state: PdfV3PageState,
    pub active_lease: Option<PdfV3PageLeaseStatus>,
    pub extraction_attempts: u32,
    pub translation_attempts: u32,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RunControlStatus {
    pub schema: &'static str,
    pub run_id: String,
    pub state: PdfV3RunState,
    pub source_fingerprint: String,
    pub source_page_count: u32,
    pub requested_page_set: String,
    pub source_language: String,
    pub target_language: String,
    pub owned_by_current_session: bool,
    pub owner_lease_updated_at_ms: u64,
    pub cancellation: Option<PdfV3Cancellation>,
    pub summary: PdfV3SchedulerSummary,
    pub runtime: PdfV3RuntimeIdentityStatus,
    pub pages: Vec<PdfV3PageControlStatus>,
    pub next_start_after: Option<u32>,
    pub has_more: bool,
}

#[derive(Debug)]
pub(crate) enum PdfV3RunControlError {
    InvalidJobDirectory,
    InvalidRunId,
    InvalidStatusLimit(usize),
    Scheduler(PdfV3SchedulerError),
    Runtime(PdfV3RuntimeManifestError),
}

impl fmt::Display for PdfV3RunControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJobDirectory => formatter.write_str("PDF v3 job directory is invalid"),
            Self::InvalidRunId => formatter.write_str("PDF v3 run ID is invalid"),
            Self::InvalidStatusLimit(limit) => write!(
                formatter,
                "PDF v3 status window {limit} is outside 1..={MAX_PDF_V3_STATUS_WINDOW}"
            ),
            Self::Scheduler(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PdfV3RunControlError {}

impl From<PdfV3SchedulerError> for PdfV3RunControlError {
    fn from(value: PdfV3SchedulerError) -> Self {
        Self::Scheduler(value)
    }
}

impl From<PdfV3RuntimeManifestError> for PdfV3RunControlError {
    fn from(value: PdfV3RuntimeManifestError) -> Self {
        Self::Runtime(value)
    }
}

pub(crate) fn pdf_v3_run_directory(
    job_directory: &Path,
    run_id: &str,
) -> Result<PathBuf, PdfV3RunControlError> {
    if !job_directory.is_absolute() || !job_directory.is_dir() {
        return Err(PdfV3RunControlError::InvalidJobDirectory);
    }
    if !is_safe_job_id(run_id) {
        return Err(PdfV3RunControlError::InvalidRunId);
    }
    Ok(job_directory.join("pdf-v3").join("runs").join(run_id))
}

pub(crate) fn pdf_v3_run_status(
    job_directory: &Path,
    run_id: &str,
    current_session_id: &str,
    start_after: Option<u32>,
    limit: usize,
) -> Result<PdfV3RunControlStatus, PdfV3RunControlError> {
    if limit == 0 || limit > MAX_PDF_V3_STATUS_WINDOW {
        return Err(PdfV3RunControlError::InvalidStatusLimit(limit));
    }
    let run_directory = pdf_v3_run_directory(job_directory, run_id)?;
    let scheduler = DurablePdfV3Scheduler::open(&run_directory)?;
    build_status(
        &scheduler,
        &run_directory,
        current_session_id,
        start_after,
        limit,
    )
}

pub(crate) fn pause_pdf_v3_run(
    job_directory: &Path,
    run_id: &str,
    owner_session_id: &str,
    now_ms: u64,
) -> Result<PdfV3RunControlStatus, PdfV3RunControlError> {
    let run_directory = pdf_v3_run_directory(job_directory, run_id)?;
    let scheduler = DurablePdfV3Scheduler::open(&run_directory)?;
    scheduler.pause(owner_session_id, now_ms)?;
    build_status(
        &scheduler,
        &run_directory,
        owner_session_id,
        None,
        DEFAULT_PDF_V3_STATUS_WINDOW,
    )
}

pub(crate) fn resume_pdf_v3_run(
    job_directory: &Path,
    run_id: &str,
    owner_session_id: &str,
    now_ms: u64,
) -> Result<PdfV3RunControlStatus, PdfV3RunControlError> {
    let run_directory = pdf_v3_run_directory(job_directory, run_id)?;
    let scheduler = DurablePdfV3Scheduler::open(&run_directory)?;
    scheduler.resume(owner_session_id, now_ms)?;
    build_status(
        &scheduler,
        &run_directory,
        owner_session_id,
        None,
        DEFAULT_PDF_V3_STATUS_WINDOW,
    )
}

pub(crate) fn cancel_pdf_v3_run(
    job_directory: &Path,
    run_id: &str,
    owner_session_id: &str,
    now_ms: u64,
) -> Result<PdfV3RunControlStatus, PdfV3RunControlError> {
    let run_directory = pdf_v3_run_directory(job_directory, run_id)?;
    let scheduler = DurablePdfV3Scheduler::open(&run_directory)?;
    let initial = scheduler.status_snapshot()?;
    match initial.run_state {
        PdfV3RunState::Running | PdfV3RunState::Paused => {
            scheduler.request_cancel(owner_session_id, now_ms, USER_CANCEL_REASON)?;
        }
        PdfV3RunState::Cancelling => {}
        PdfV3RunState::Cancelled => {
            return build_status(
                &scheduler,
                &run_directory,
                owner_session_id,
                None,
                DEFAULT_PDF_V3_STATUS_WINDOW,
            );
        }
        PdfV3RunState::Completed => {
            scheduler.request_cancel(owner_session_id, now_ms, USER_CANCEL_REASON)?;
        }
    }
    let current = scheduler.status_snapshot()?;
    if current.summary.extracting_pages == 0 && current.summary.translating_pages == 0 {
        scheduler.finish_cancellation(owner_session_id, now_ms)?;
    }
    build_status(
        &scheduler,
        &run_directory,
        owner_session_id,
        None,
        DEFAULT_PDF_V3_STATUS_WINDOW,
    )
}

fn build_status(
    scheduler: &DurablePdfV3Scheduler,
    run_directory: &Path,
    current_session_id: &str,
    start_after: Option<u32>,
    limit: usize,
) -> Result<PdfV3RunControlStatus, PdfV3RunControlError> {
    let snapshot = scheduler.status_snapshot()?;
    let binding = scheduler.translation_binding()?;
    let manifest = load_translation_runtime_manifest(run_directory)?;
    validate_runtime_manifest_binding(&manifest, &binding)?;
    let page_records = scheduler.page_window(start_after, limit)?;
    let last_page = page_records.last().map(|page| page.page_number);
    let has_more = last_page.is_some_and(|last_page| {
        snapshot
            .requested_pages
            .pages()
            .iter()
            .any(|page| *page > last_page)
    });
    let next_start_after = has_more.then_some(last_page).flatten();
    let pages = page_records
        .into_iter()
        .map(|page| PdfV3PageControlStatus {
            page_number: page.page_number,
            state: page.state,
            active_lease: page.lease.map(|lease| PdfV3PageLeaseStatus {
                stage: lease.stage,
                leased_at_ms: lease.leased_at_ms,
                owned_by_current_session: lease.owner_session_id == current_session_id,
            }),
            extraction_attempts: page.extraction_attempts,
            translation_attempts: page.translation_attempts,
            updated_at_ms: page.updated_at_ms,
        })
        .collect();
    let component = &manifest.component;

    Ok(PdfV3RunControlStatus {
        schema: "rosetta-pdf-v3-run-control-status/1",
        run_id: snapshot.run_id,
        state: snapshot.run_state,
        source_fingerprint: manifest.source_fingerprint,
        source_page_count: snapshot.source_page_count,
        requested_page_set: snapshot.requested_pages.canonical_string(),
        source_language: snapshot.source_language,
        target_language: snapshot.target_language,
        owned_by_current_session: snapshot.owner_session_id == current_session_id,
        owner_lease_updated_at_ms: snapshot.owner_lease_updated_at_ms,
        cancellation: snapshot.cancellation,
        summary: snapshot.summary,
        runtime: PdfV3RuntimeIdentityStatus {
            manifest_id: manifest.manifest_id,
            component_id: component.component_id.clone(),
            component_version: component.component_version.clone(),
            component_manifest_id: component.component_manifest_id.clone(),
            component_build_sha256: component.component_build_sha256.clone(),
            platform_os: component.platform_os.clone(),
            platform_arch: component.platform_arch.clone(),
            provider_id: component.provider_id.clone(),
            model_id: component.model_id.clone(),
            model_sha256: component.model_sha256.clone(),
            translation_revision: manifest.translation_revision,
            renderer_version: manifest.renderer_version,
            minimum_fit_scale: manifest.render_policy.minimum_fit_scale,
            regular_font: manifest.regular_font,
            bold_font: manifest.bold_font,
        },
        pages,
        next_start_after,
        has_more,
    })
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
            font::{TranslationFontAsset, TranslationFontWeight},
            page_set::PageSet,
            patch_renderer::{TranslationPatchRenderPolicy, TRANSLATION_PATCH_RENDERER_VERSION},
            scheduler::{
                DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3RunState, PdfV3SchedulerCapacity,
            },
            types::{PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
        },
        rosetta_jobs::formats::pdf::v3_runtime::{
            build_translation_runtime_manifest, commit_translation_runtime_manifest,
            PdfV3TranslationComponentBinding, PdfV3TranslationRuntimeSpec,
        },
    };

    use super::{
        cancel_pdf_v3_run, pause_pdf_v3_run, pdf_v3_run_status, resume_pdf_v3_run,
        PdfV3RunControlError,
    };

    #[test]
    fn status_is_windowed_and_controls_keep_owner_and_runtime_identity_explicit() {
        let run = TestRun::new();

        let first = pdf_v3_run_status(run.job_dir(), "run-test", "owner-a", None, 2)
            .expect("first status window");
        assert_eq!(first.state, PdfV3RunState::Running);
        assert_eq!(first.requested_page_set, "1-3,9");
        assert_eq!(first.pages.len(), 2);
        assert_eq!(first.next_start_after, Some(2));
        assert!(first.has_more);
        assert!(first.owned_by_current_session);
        assert_eq!(first.runtime.provider_id, "scripted-test-provider");
        assert_eq!(
            first.runtime.regular_font.weight,
            TranslationFontWeight::Regular
        );

        let second = pdf_v3_run_status(
            run.job_dir(),
            "run-test",
            "owner-other",
            first.next_start_after,
            2,
        )
        .expect("second status window");
        assert_eq!(
            second
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![3, 9]
        );
        assert!(!second.has_more);
        assert!(!second.owned_by_current_session);

        assert_eq!(
            pause_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 2)
                .expect("pause")
                .state,
            PdfV3RunState::Paused
        );
        assert!(matches!(
            resume_pdf_v3_run(run.job_dir(), "run-test", "owner-other", 3),
            Err(PdfV3RunControlError::Scheduler(_))
        ));
        assert_eq!(
            resume_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 4)
                .expect("resume")
                .state,
            PdfV3RunState::Running
        );
        let cancelled =
            cancel_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 5).expect("cancel idle run");
        assert_eq!(cancelled.state, PdfV3RunState::Cancelled);
        assert_eq!(
            cancelled
                .cancellation
                .as_ref()
                .map(|value| value.reason_code.as_str()),
            Some("user-requested")
        );
    }

    #[test]
    fn status_rejects_unbounded_windows_and_unsafe_run_ids() {
        let run = TestRun::new();

        assert!(matches!(
            pdf_v3_run_status(run.job_dir(), "run-test", "owner-a", None, 257),
            Err(PdfV3RunControlError::InvalidStatusLimit(257))
        ));
        assert!(matches!(
            pdf_v3_run_status(run.job_dir(), "../run-test", "owner-a", None, 1),
            Err(PdfV3RunControlError::InvalidRunId)
        ));
    }

    #[test]
    fn cancellation_waits_for_active_page_leases() {
        let run = TestRun::new();
        let scheduler = DurablePdfV3Scheduler::open(&run.run_dir()).expect("scheduler");
        let claims = scheduler
            .claim_extraction("owner-a", 1, 2)
            .expect("extraction claim");
        assert_eq!(claims.len(), 1);

        let cancelled =
            cancel_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 3).expect("cancel active run");
        assert_eq!(cancelled.state, PdfV3RunState::Cancelling);
        assert_eq!(cancelled.summary.extracting_pages, 1);
        assert!(cancelled.pages[0]
            .active_lease
            .as_ref()
            .is_some_and(|lease| lease.owned_by_current_session));
        let encoded = serde_json::to_string(&cancelled).expect("encode status");
        assert!(!encoded.contains("owner-a"));
        assert!(!encoded.contains(&claims[0].lease.lease_id));

        scheduler
            .fail_claim("owner-a", &claims[0], "cancelled-active-work", true, 4)
            .expect("settle active claim");
        let finished = cancel_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 5)
            .expect("finish cancellation");
        assert_eq!(finished.state, PdfV3RunState::Cancelled);
        assert_eq!(
            cancel_pdf_v3_run(run.job_dir(), "run-test", "owner-a", 6)
                .expect("idempotent cancellation")
                .state,
            PdfV3RunState::Cancelled
        );
    }

    struct TestRun {
        root: PathBuf,
        job_dir: PathBuf,
    }

    impl TestRun {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-control-{}-{nanos}",
                std::process::id()
            ));
            let job_dir = root.join("job-test");
            let run_dir = job_dir.join("pdf-v3").join("runs").join("run-test");
            fs::create_dir_all(run_dir.parent().expect("run parent")).expect("run parent");
            let pages = PageSet::parse("1-3,9", 10).expect("PageSet");
            let spec = PdfV3RunSpec {
                run_id: "run-test".to_string(),
                source_fingerprint: format!("sha256:{}", "a".repeat(64)),
                source_page_count: 10,
                requested_pages: pages,
                source_language: "en".to_string(),
                target_language: "zh-CN".to_string(),
                engine_version: "pdf-v3-control-test".to_string(),
                page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
                translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
                renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
            };
            let scheduler = DurablePdfV3Scheduler::create(
                &run_dir,
                spec,
                PdfV3SchedulerCapacity {
                    max_extracting_pages: 1,
                    max_extracted_pages: 2,
                    max_translating_pages: 1,
                },
                "owner-a",
                1,
            )
            .expect("scheduler");
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
                    component_id: "pdf-v3-control-test".to_string(),
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
                regular_font: &regular,
                bold_font: None,
            })
            .expect("runtime manifest");
            commit_translation_runtime_manifest(&run_dir, &manifest).expect("runtime commit");
            Self { root, job_dir }
        }

        fn job_dir(&self) -> &Path {
            &self.job_dir
        }

        fn run_dir(&self) -> PathBuf {
            self.job_dir.join("pdf-v3").join("runs").join("run-test")
        }
    }

    impl Drop for TestRun {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
