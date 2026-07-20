use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, atomic::Ordering, Mutex},
};

use crate::pdf_v3::{
    page_set::{PageSet, PageSetError},
    patch_renderer::TranslationPatchRenderPolicy,
    region_renderer::REGION_TRANSLATION_RENDERER_VERSION,
    region_translation_patch::REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
    scheduler::{DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3SchedulerCapacity},
    types::{PAGE_GRAPH_SCHEMA_VERSION, PDF_V3_ENGINE_VERSION},
};

use super::{
    v3_component::ResolvedPdfV3TranslationComponent,
    v3_control::{build_status, PdfV3RunControlStatus, DEFAULT_PDF_V3_STATUS_WINDOW},
    v3_runtime::{
        build_translation_runtime_manifest, commit_translation_runtime_manifest,
        load_translation_runtime_manifest, PdfV3TranslationRuntimeSpec,
    },
};

const DEFAULT_MAX_EXTRACTING_PAGES: u32 = 2;
const DEFAULT_MAX_EXTRACTED_PAGES: u32 = 4;
const DEFAULT_MAX_TRANSLATING_PAGES: u32 = 1;
const CREATION_STAGING_PREFIX: &str = ".pdf-v3-run-creating-";

static RUN_CREATION_LOCK: Mutex<()> = Mutex::new(());
static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) struct PdfV3RunCreationRequest<'a> {
    pub job_directory: &'a Path,
    pub source_fingerprint: &'a str,
    pub source_page_count: u32,
    pub requested_page_set: Option<&'a str>,
    pub preferred_page_number: Option<u32>,
    pub source_language: &'a str,
    pub target_language: &'a str,
    pub owner_session_id: &'a str,
    pub now_ms: u64,
    pub component: &'a ResolvedPdfV3TranslationComponent,
}

#[derive(Debug)]
pub(crate) enum PdfV3RunCreationError {
    InvalidJob,
    InvalidSource,
    InvalidLanguage,
    UnsupportedDirection,
    InvalidPageSet,
    RevisionOverflow,
    ExistingRunInvalid,
    Storage,
    LockPoisoned,
}

impl fmt::Display for PdfV3RunCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJob => formatter.write_str("PDF v3 job is unavailable"),
            Self::InvalidSource => formatter.write_str("PDF v3 source authority is invalid"),
            Self::InvalidLanguage => formatter.write_str("PDF v3 language identity is invalid"),
            Self::UnsupportedDirection => {
                formatter.write_str("PDF v3 translation direction is unsupported")
            }
            Self::InvalidPageSet => formatter.write_str("PDF v3 requested page set is invalid"),
            Self::RevisionOverflow => {
                formatter.write_str("PDF v3 translation revision is exhausted")
            }
            Self::ExistingRunInvalid => {
                formatter.write_str("an existing PDF v3 run has invalid immutable identity")
            }
            Self::Storage => formatter.write_str("PDF v3 run could not be committed"),
            Self::LockPoisoned => formatter.write_str("PDF v3 run creation lock is poisoned"),
        }
    }
}

impl std::error::Error for PdfV3RunCreationError {}

pub(crate) fn create_pdf_v3_run(
    request: PdfV3RunCreationRequest<'_>,
) -> Result<PdfV3RunControlStatus, PdfV3RunCreationError> {
    validate_request(&request)?;
    let requested_pages = match request.requested_page_set {
        None => PageSet::all(request.source_page_count),
        Some(value) if !value.trim().is_empty() => PageSet::parse(value, request.source_page_count),
        Some(_) => Err(PageSetError::InvalidPage("empty page set".to_string())),
    }
    .map_err(|_| PdfV3RunCreationError::InvalidPageSet)?;
    if requested_pages.is_empty() {
        return Err(PdfV3RunCreationError::InvalidPageSet);
    }
    if request
        .preferred_page_number
        .is_some_and(|page_number| !requested_pages.contains(page_number))
    {
        return Err(PdfV3RunCreationError::InvalidPageSet);
    }

    let _guard = RUN_CREATION_LOCK
        .lock()
        .map_err(|_| PdfV3RunCreationError::LockPoisoned)?;
    let runs_directory = request.job_directory.join("pdf-v3").join("runs");
    fs::create_dir_all(&runs_directory).map_err(|_| PdfV3RunCreationError::Storage)?;
    let translation_revision = next_translation_revision(&runs_directory)?;
    let run_id = allocate_run_id(&runs_directory, request.now_ms, translation_revision)?;
    let final_directory = runs_directory.join(&run_id);
    let staging_directory = unique_staging_directory(&runs_directory);

    let result = create_staged_run(
        &staging_directory,
        &run_id,
        translation_revision,
        requested_pages,
        &request,
    );
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_directory);
            return Err(error);
        }
    };
    if let Err(error) = fs::rename(&staging_directory, &final_directory) {
        let _ = fs::remove_dir_all(&staging_directory);
        if final_directory.exists() {
            return Err(PdfV3RunCreationError::Storage);
        }
        let _ = error;
        return Err(PdfV3RunCreationError::Storage);
    }
    sync_directory(&runs_directory)?;
    Ok(status)
}

fn create_staged_run(
    staging_directory: &Path,
    run_id: &str,
    translation_revision: u64,
    requested_pages: PageSet,
    request: &PdfV3RunCreationRequest<'_>,
) -> Result<PdfV3RunControlStatus, PdfV3RunCreationError> {
    let scheduler = DurablePdfV3Scheduler::create(
        staging_directory,
        PdfV3RunSpec {
            run_id: run_id.to_string(),
            source_fingerprint: request.source_fingerprint.to_string(),
            source_page_count: request.source_page_count,
            requested_pages,
            source_language: request.source_language.to_string(),
            target_language: request.target_language.to_string(),
            engine_version: PDF_V3_ENGINE_VERSION.to_string(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            translation_patch_schema_version: REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
        },
        PdfV3SchedulerCapacity {
            max_extracting_pages: DEFAULT_MAX_EXTRACTING_PAGES,
            max_extracted_pages: DEFAULT_MAX_EXTRACTED_PAGES,
            max_translating_pages: DEFAULT_MAX_TRANSLATING_PAGES,
        },
        request.owner_session_id,
        request.now_ms,
    )
    .map_err(|_| PdfV3RunCreationError::Storage)?;
    if let Some(page_number) = request.preferred_page_number {
        scheduler
            .set_initial_page_priority(request.owner_session_id, page_number, request.now_ms)
            .map_err(|_| PdfV3RunCreationError::Storage)?;
    }
    let binding = scheduler
        .translation_binding()
        .map_err(|_| PdfV3RunCreationError::Storage)?;
    let manifest = build_translation_runtime_manifest(PdfV3TranslationRuntimeSpec {
        binding: &binding,
        translation_revision,
        component: request.component.component.clone(),
        render_policy: TranslationPatchRenderPolicy::default(),
        regular_font: &request.component.regular_font,
        bold_font: request.component.bold_font.as_ref(),
    })
    .map_err(|_| PdfV3RunCreationError::Storage)?;
    commit_translation_runtime_manifest(staging_directory, &manifest)
        .map_err(|_| PdfV3RunCreationError::Storage)?;
    build_status(
        &scheduler,
        staging_directory,
        request.owner_session_id,
        None,
        DEFAULT_PDF_V3_STATUS_WINDOW,
    )
    .map_err(|_| PdfV3RunCreationError::Storage)
}

fn validate_request(request: &PdfV3RunCreationRequest<'_>) -> Result<(), PdfV3RunCreationError> {
    if !request.job_directory.is_absolute() || !request.job_directory.is_dir() {
        return Err(PdfV3RunCreationError::InvalidJob);
    }
    if request.source_page_count == 0
        || !is_sha256_fingerprint(request.source_fingerprint)
        || request.now_ms == 0
    {
        return Err(PdfV3RunCreationError::InvalidSource);
    }
    let source = validate_language(request.source_language)?;
    let target = validate_language(request.target_language)?;
    if source == target {
        return Err(PdfV3RunCreationError::UnsupportedDirection);
    }
    let direction = format!("{source}-{target}");
    if !request
        .component
        .supported_directions
        .contains(&direction.as_str())
    {
        return Err(PdfV3RunCreationError::UnsupportedDirection);
    }
    Ok(())
}

fn validate_language(value: &str) -> Result<String, PdfV3RunCreationError> {
    if value.is_empty()
        || value.len() > 64
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3RunCreationError::InvalidLanguage);
    }
    let primary = value
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if primary.is_empty() || !primary.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return Err(PdfV3RunCreationError::InvalidLanguage);
    }
    Ok(primary)
}

fn next_translation_revision(runs_directory: &Path) -> Result<u64, PdfV3RunCreationError> {
    let mut maximum = 0u64;
    for entry in fs::read_dir(runs_directory).map_err(|_| PdfV3RunCreationError::Storage)? {
        let entry = entry.map_err(|_| PdfV3RunCreationError::Storage)?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        if !crate::rosetta_jobs::path::is_safe_job_id(name) {
            return Err(PdfV3RunCreationError::ExistingRunInvalid);
        }
        let manifest = load_translation_runtime_manifest(&path)
            .map_err(|_| PdfV3RunCreationError::ExistingRunInvalid)?;
        maximum = maximum.max(manifest.translation_revision);
    }
    maximum
        .checked_add(1)
        .ok_or(PdfV3RunCreationError::RevisionOverflow)
}

fn allocate_run_id(
    runs_directory: &Path,
    now_ms: u64,
    translation_revision: u64,
) -> Result<String, PdfV3RunCreationError> {
    for _ in 0..64 {
        let counter = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let run_id = format!("run-pdf-v3-{now_ms}-{translation_revision}-{counter}");
        if !runs_directory.join(&run_id).exists() {
            return Ok(run_id);
        }
    }
    Err(PdfV3RunCreationError::Storage)
}

fn unique_staging_directory(runs_directory: &Path) -> PathBuf {
    let counter = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    runs_directory.join(format!(
        "{CREATION_STAGING_PREFIX}{}-{counter}",
        std::process::id()
    ))
}

fn is_sha256_fingerprint(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PdfV3RunCreationError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PdfV3RunCreationError::Storage)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), PdfV3RunCreationError> {
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use crate::{
        pdf_v3::{
            font::{TranslationFontAsset, TranslationFontWeight},
            scheduler::DurablePdfV3Scheduler,
        },
        rosetta_jobs::formats::pdf::{
            unit_translation::{LlamaCppPdfApiConfig, PdfUnitProviderConfig},
            v3_component::{
                PdfV3ComponentResolutionDiagnostics, ResolvedPdfV3TranslationComponent,
            },
            v3_runtime::PdfV3TranslationComponentBinding,
        },
    };

    use super::{create_pdf_v3_run, PdfV3RunCreationRequest, CREATION_STAGING_PREFIX};

    #[test]
    fn creation_atomically_commits_scheduler_runtime_and_monotonic_revision() {
        let root = temp_root("commit");
        let job = root.join("job-test");
        fs::create_dir_all(&job).expect("job directory");
        let component = test_component();

        let first = create(&job, &component, Some("1-3,9"), 10, 100).expect("first run");
        assert_eq!(first.requested_page_set, "1-3,9");
        assert_eq!(first.runtime.translation_revision, 1);
        assert!(first.owned_by_current_session);
        let first_run = job.join("pdf-v3").join("runs").join(&first.run_id);
        assert!(first_run.join("manifest.json").is_file());
        assert!(first_run.join("runtime-manifest.json").is_file());

        let second = create(&job, &component, None, 10, 101).expect("second run");
        assert_eq!(second.requested_page_set, "1-10");
        assert_eq!(second.runtime.translation_revision, 2);
        assert_ne!(first.run_id, second.run_id);
        assert_no_staging_directories(&job);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_runtime_identity_leaves_no_visible_or_staged_run() {
        let root = temp_root("rollback");
        let job = root.join("job-test");
        fs::create_dir_all(&job).expect("job directory");
        let mut component = test_component();
        component.component.model_sha256 = "invalid".to_string();

        assert!(create(&job, &component, Some("1"), 1, 100).is_err());
        let runs = job.join("pdf-v3").join("runs");
        assert_eq!(fs::read_dir(&runs).expect("runs").count(), 0);
        assert_no_staging_directories(&job);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn creation_applies_only_a_requested_initial_page_priority() {
        let root = temp_root("priority");
        let job = root.join("job-test");
        fs::create_dir_all(&job).expect("job directory");
        let component = test_component();

        let status = create_with_priority(&job, &component, Some("1-3,7-9"), Some(7), 10, 100)
            .expect("prioritized run");
        let scheduler =
            DurablePdfV3Scheduler::open(&job.join("pdf-v3").join("runs").join(status.run_id))
                .expect("open prioritized scheduler");
        let claims = scheduler
            .claim_extraction("owner-test", 2, 101)
            .expect("claim prioritized pages");
        assert_eq!(
            claims
                .iter()
                .map(|claim| claim.page_number)
                .collect::<Vec<_>>(),
            vec![7, 8]
        );

        assert!(matches!(
            create_with_priority(&job, &component, Some("1-3,7-9"), Some(6), 10, 102),
            Err(super::PdfV3RunCreationError::InvalidPageSet)
        ));
        assert_no_staging_directories(&job);
        let _ = fs::remove_dir_all(root);
    }

    fn create(
        job: &Path,
        component: &ResolvedPdfV3TranslationComponent,
        pages: Option<&str>,
        page_count: u32,
        now_ms: u64,
    ) -> Result<
        crate::rosetta_jobs::formats::pdf::v3_control::PdfV3RunControlStatus,
        super::PdfV3RunCreationError,
    > {
        create_with_priority(job, component, pages, None, page_count, now_ms)
    }

    fn create_with_priority(
        job: &Path,
        component: &ResolvedPdfV3TranslationComponent,
        pages: Option<&str>,
        preferred_page_number: Option<u32>,
        page_count: u32,
        now_ms: u64,
    ) -> Result<
        crate::rosetta_jobs::formats::pdf::v3_control::PdfV3RunControlStatus,
        super::PdfV3RunCreationError,
    > {
        create_pdf_v3_run(PdfV3RunCreationRequest {
            job_directory: job,
            source_fingerprint: &format!("sha256:{}", "a".repeat(64)),
            source_page_count: page_count,
            requested_page_set: pages,
            preferred_page_number,
            source_language: "en",
            target_language: "zh-CN",
            owner_session_id: "owner-test",
            now_ms,
            component,
        })
    }

    fn test_component() -> ResolvedPdfV3TranslationComponent {
        let regular = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            Path::new(r"C:\Windows\Fonts\arial.ttf"),
            0,
        )
        .expect("Windows Arial");
        ResolvedPdfV3TranslationComponent {
            component: PdfV3TranslationComponentBinding {
                component_id: "pdf-v3-creation-test".to_string(),
                component_version: "1".to_string(),
                component_manifest_id: "component-manifest-test".to_string(),
                component_build_sha256: "b".repeat(64),
                platform_os: std::env::consts::OS.to_string(),
                platform_arch: std::env::consts::ARCH.to_string(),
                provider_id: "llama-cpp-chat-completions".to_string(),
                model_id: "model-test".to_string(),
                model_sha256: "c".repeat(64),
            },
            provider: PdfUnitProviderConfig::LlamaCpp(LlamaCppPdfApiConfig {
                base_url: "http://127.0.0.1:1".to_string(),
                timeout_ms: 1,
            }),
            regular_font: regular,
            bold_font: None,
            runtime_release_sha256: None,
            supported_directions: &["en-zh", "zh-en"],
            resolution_diagnostics: PdfV3ComponentResolutionDiagnostics::default(),
        }
    }

    fn assert_no_staging_directories(job: &Path) {
        let runs = job.join("pdf-v3").join("runs");
        assert!(fs::read_dir(runs).expect("runs").all(|entry| {
            !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(CREATION_STAGING_PREFIX)
        }));
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rosetta-pdf-v3-run-creation-{label}-{}-{nanos}",
            std::process::id()
        ))
    }
}
