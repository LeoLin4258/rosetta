use std::{fmt, fs, path::Path};

use serde::Serialize;

use crate::{
    pdf_v3::scheduler::{DurablePdfV3Scheduler, PdfV3RunState, PdfV3SchedulerSummary},
    rosetta_jobs::path::is_safe_job_id,
};

use super::{
    v3_control::PDF_V3_OWNER_LEASE_TIMEOUT_MS,
    v3_runtime::{load_translation_runtime_manifest, validate_runtime_manifest_binding},
};

pub(crate) const DEFAULT_PDF_V3_RUN_LIST_LIMIT: usize = 16;
pub(crate) const MAX_PDF_V3_RUN_LIST_LIMIT: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RunListItem {
    pub run_id: String,
    pub translation_revision: u64,
    pub state: PdfV3RunState,
    pub source_page_count: u32,
    pub requested_page_set: String,
    pub source_language: String,
    pub target_language: String,
    pub owned_by_current_session: bool,
    pub owner_recovery_eligible_at_ms: u64,
    pub summary: PdfV3SchedulerSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RunList {
    pub schema: &'static str,
    pub runs: Vec<PdfV3RunListItem>,
    pub next_before_revision: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfV3RunListError {
    InvalidJobDirectory,
    InvalidLimit(usize),
    InvalidBeforeRevision,
    InvalidTargetLanguage,
    Storage,
    InvalidCommittedRun,
}

impl fmt::Display for PdfV3RunListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJobDirectory => formatter.write_str("PDF v3 job directory is invalid"),
            Self::InvalidLimit(limit) => write!(
                formatter,
                "PDF v3 run list limit {limit} is outside 1..={MAX_PDF_V3_RUN_LIST_LIMIT}"
            ),
            Self::InvalidBeforeRevision => {
                formatter.write_str("PDF v3 run list revision cursor is invalid")
            }
            Self::InvalidTargetLanguage => {
                formatter.write_str("PDF v3 run list target language is invalid")
            }
            Self::Storage => formatter.write_str("PDF v3 run list storage is unavailable"),
            Self::InvalidCommittedRun => {
                formatter.write_str("PDF v3 committed run index is invalid")
            }
        }
    }
}

impl std::error::Error for PdfV3RunListError {}

pub(crate) fn list_pdf_v3_runs(
    job_directory: &Path,
    current_session_id: &str,
    target_language: Option<&str>,
    before_revision: Option<u64>,
    limit: usize,
) -> Result<PdfV3RunList, PdfV3RunListError> {
    if !job_directory.is_absolute() || !job_directory.is_dir() {
        return Err(PdfV3RunListError::InvalidJobDirectory);
    }
    if limit == 0 || limit > MAX_PDF_V3_RUN_LIST_LIMIT {
        return Err(PdfV3RunListError::InvalidLimit(limit));
    }
    if before_revision == Some(0) {
        return Err(PdfV3RunListError::InvalidBeforeRevision);
    }
    let target_language = target_language.map(normalize_language).transpose()?;
    let runs_directory = job_directory.join("pdf-v3").join("runs");
    if !runs_directory.exists() {
        return Ok(empty_run_list());
    }
    if !runs_directory.is_dir() {
        return Err(PdfV3RunListError::Storage);
    }

    let mut retained = Vec::with_capacity(limit);
    let mut eligible_count = 0usize;
    for entry in fs::read_dir(&runs_directory).map_err(|_| PdfV3RunListError::Storage)? {
        let entry = entry.map_err(|_| PdfV3RunListError::Storage)?;
        let file_type = entry.file_type().map_err(|_| PdfV3RunListError::Storage)?;
        if !file_type.is_dir() {
            continue;
        }
        let Some(run_id) = entry.file_name().to_str().map(str::to_string) else {
            return Err(PdfV3RunListError::InvalidCommittedRun);
        };
        if run_id.starts_with('.') {
            continue;
        }
        if !is_safe_job_id(&run_id) {
            return Err(PdfV3RunListError::InvalidCommittedRun);
        }
        let item = load_run_item(&entry.path(), current_session_id)?;
        if let Some(target) = target_language.as_ref() {
            let item_target = normalize_language(&item.target_language)
                .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
            if item_target != *target {
                continue;
            }
        }
        if before_revision.is_some_and(|before| item.translation_revision >= before) {
            continue;
        }
        eligible_count = eligible_count.saturating_add(1);
        retain_highest_revision(&mut retained, item, limit);
    }

    retained.sort_by(|left, right| {
        right
            .translation_revision
            .cmp(&left.translation_revision)
            .then_with(|| right.run_id.cmp(&left.run_id))
    });
    let has_more = eligible_count > retained.len();
    let next_before_revision = has_more
        .then(|| retained.last().map(|run| run.translation_revision))
        .flatten();
    Ok(PdfV3RunList {
        schema: "rosetta-pdf-v3-run-list/1",
        runs: retained,
        next_before_revision,
        has_more,
    })
}

fn empty_run_list() -> PdfV3RunList {
    PdfV3RunList {
        schema: "rosetta-pdf-v3-run-list/1",
        runs: Vec::new(),
        next_before_revision: None,
        has_more: false,
    }
}

fn load_run_item(
    run_directory: &Path,
    current_session_id: &str,
) -> Result<PdfV3RunListItem, PdfV3RunListError> {
    let scheduler = DurablePdfV3Scheduler::open(run_directory)
        .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
    let snapshot = scheduler
        .status_snapshot()
        .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
    let binding = scheduler
        .translation_binding()
        .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
    let manifest = load_translation_runtime_manifest(run_directory)
        .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
    validate_runtime_manifest_binding(&manifest, &binding)
        .map_err(|_| PdfV3RunListError::InvalidCommittedRun)?;
    let directory_run_id = run_directory
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(PdfV3RunListError::InvalidCommittedRun)?;
    if snapshot.run_id != directory_run_id || manifest.translation_revision == 0 {
        return Err(PdfV3RunListError::InvalidCommittedRun);
    }

    Ok(PdfV3RunListItem {
        run_id: snapshot.run_id,
        translation_revision: manifest.translation_revision,
        state: snapshot.run_state,
        source_page_count: snapshot.source_page_count,
        requested_page_set: snapshot.requested_pages.canonical_string(),
        source_language: snapshot.source_language,
        target_language: snapshot.target_language,
        owned_by_current_session: snapshot.owner_session_id == current_session_id,
        owner_recovery_eligible_at_ms: snapshot
            .owner_lease_updated_at_ms
            .saturating_add(PDF_V3_OWNER_LEASE_TIMEOUT_MS)
            .saturating_add(1),
        summary: snapshot.summary,
    })
}

fn retain_highest_revision(
    retained: &mut Vec<PdfV3RunListItem>,
    item: PdfV3RunListItem,
    limit: usize,
) {
    if retained.len() < limit {
        retained.push(item);
        return;
    }
    let Some((lowest_index, lowest)) = retained
        .iter()
        .enumerate()
        .min_by_key(|(_, run)| run.translation_revision)
    else {
        return;
    };
    if item.translation_revision > lowest.translation_revision {
        retained[lowest_index] = item;
    }
}

fn normalize_language(value: &str) -> Result<String, PdfV3RunListError> {
    if value.is_empty()
        || value.len() > 64
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3RunListError::InvalidTargetLanguage);
    }
    let primary = value
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if primary.is_empty() || !primary.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return Err(PdfV3RunListError::InvalidTargetLanguage);
    }
    Ok(primary)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use crate::{
        pdf_v3::font::{TranslationFontAsset, TranslationFontWeight},
        rosetta_jobs::formats::pdf::{
            unit_translation::{LlamaCppPdfApiConfig, PdfUnitProviderConfig},
            v3_component::ResolvedPdfV3TranslationComponent,
            v3_run_creation::{create_pdf_v3_run, PdfV3RunCreationRequest},
            v3_runtime::PdfV3TranslationComponentBinding,
        },
    };

    use super::{
        list_pdf_v3_runs, PdfV3RunListError, DEFAULT_PDF_V3_RUN_LIST_LIMIT,
        MAX_PDF_V3_RUN_LIST_LIMIT,
    };

    #[test]
    fn lists_newest_revisions_first_with_strict_cursor_pagination() {
        let fixture = TestFixture::new("pagination");
        for now_ms in 100..=103 {
            fixture.create("en", "zh-CN", "owner-current", now_ms);
        }

        let first =
            list_pdf_v3_runs(fixture.job(), "owner-current", None, None, 2).expect("first page");
        assert_eq!(revisions(&first), vec![4, 3]);
        assert_eq!(first.next_before_revision, Some(3));
        assert!(first.has_more);
        assert!(first.runs.iter().all(|run| run.owned_by_current_session));
        assert_eq!(first.runs[0].requested_page_set, "1-3,9");

        let second = list_pdf_v3_runs(
            fixture.job(),
            "owner-current",
            None,
            first.next_before_revision,
            2,
        )
        .expect("second page");
        assert_eq!(revisions(&second), vec![2, 1]);
        assert_eq!(second.next_before_revision, None);
        assert!(!second.has_more);
    }

    #[test]
    fn target_filter_normalizes_primary_language_without_rewriting_items() {
        let fixture = TestFixture::new("language");
        fixture.create("en", "zh-CN", "owner-a", 100);
        fixture.create("zh-CN", "en-US", "owner-b", 101);

        let chinese = list_pdf_v3_runs(
            fixture.job(),
            "owner-a",
            Some("ZH_cn"),
            None,
            DEFAULT_PDF_V3_RUN_LIST_LIMIT,
        )
        .expect("Chinese target runs");
        assert_eq!(revisions(&chinese), vec![1]);
        assert_eq!(chinese.runs[0].target_language, "zh-CN");
        assert!(chinese.runs[0].owned_by_current_session);

        let english = list_pdf_v3_runs(
            fixture.job(),
            "owner-a",
            Some("en"),
            None,
            DEFAULT_PDF_V3_RUN_LIST_LIMIT,
        )
        .expect("English target runs");
        assert_eq!(revisions(&english), vec![2]);
        assert_eq!(english.runs[0].target_language, "en-US");
        assert!(!english.runs[0].owned_by_current_session);
    }

    #[test]
    fn skips_hidden_staging_but_rejects_malformed_committed_runs() {
        let fixture = TestFixture::new("commit-boundary");
        fixture.create("en", "zh-CN", "owner-a", 100);
        let runs = fixture.job().join("pdf-v3").join("runs");
        fs::create_dir(runs.join(".pdf-v3-run-creating-abandoned"))
            .expect("hidden staging directory");

        let listed = list_pdf_v3_runs(fixture.job(), "owner-a", None, None, 16)
            .expect("hidden staging is ignored");
        assert_eq!(revisions(&listed), vec![1]);

        fs::create_dir(runs.join("run-malformed")).expect("malformed committed run");
        assert!(matches!(
            list_pdf_v3_runs(fixture.job(), "owner-a", None, None, 16),
            Err(PdfV3RunListError::InvalidCommittedRun)
        ));
    }

    #[test]
    fn rejects_unbounded_or_invalid_list_inputs() {
        let fixture = TestFixture::new("bounds");
        assert_eq!(DEFAULT_PDF_V3_RUN_LIST_LIMIT, 16);
        assert_eq!(MAX_PDF_V3_RUN_LIST_LIMIT, 64);
        assert!(matches!(
            list_pdf_v3_runs(fixture.job(), "owner-a", None, None, 0),
            Err(PdfV3RunListError::InvalidLimit(0))
        ));
        assert!(matches!(
            list_pdf_v3_runs(
                fixture.job(),
                "owner-a",
                None,
                None,
                MAX_PDF_V3_RUN_LIST_LIMIT + 1,
            ),
            Err(PdfV3RunListError::InvalidLimit(value))
                if value == MAX_PDF_V3_RUN_LIST_LIMIT + 1
        ));
        assert!(matches!(
            list_pdf_v3_runs(fixture.job(), "owner-a", None, Some(0), 16),
            Err(PdfV3RunListError::InvalidBeforeRevision)
        ));
        assert!(matches!(
            list_pdf_v3_runs(fixture.job(), "owner-a", Some(" zh-CN"), None, 16),
            Err(PdfV3RunListError::InvalidTargetLanguage)
        ));
    }

    #[test]
    fn serialized_list_exposes_only_the_public_bounded_projection() {
        let fixture = TestFixture::new("privacy");
        fixture.create("en", "zh-CN", "owner-a", 100);
        let listed = list_pdf_v3_runs(fixture.job(), "owner-a", None, None, 16).expect("run list");
        let value = serde_json::to_value(listed).expect("serialized list");
        let object = value.as_object().expect("list object");
        assert_eq!(
            sorted_keys(object),
            vec!["hasMore", "nextBeforeRevision", "runs", "schema"]
        );
        let item = object["runs"][0].as_object().expect("run item");
        assert_eq!(
            sorted_keys(item),
            vec![
                "ownedByCurrentSession",
                "ownerRecoveryEligibleAtMs",
                "requestedPageSet",
                "runId",
                "sourceLanguage",
                "sourcePageCount",
                "state",
                "summary",
                "targetLanguage",
                "translationRevision",
            ]
        );
    }

    fn revisions(list: &super::PdfV3RunList) -> Vec<u64> {
        list.runs
            .iter()
            .map(|run| run.translation_revision)
            .collect()
    }

    fn sorted_keys(object: &serde_json::Map<String, serde_json::Value>) -> Vec<&str> {
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }

    struct TestFixture {
        root: PathBuf,
        job: PathBuf,
        component: ResolvedPdfV3TranslationComponent,
    }

    impl TestFixture {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-run-list-{label}-{}-{nanos}",
                std::process::id()
            ));
            let job = root.join("job-test");
            fs::create_dir_all(&job).expect("job directory");
            Self {
                root,
                job,
                component: test_component(),
            }
        }

        fn job(&self) -> &Path {
            &self.job
        }

        fn create(&self, source: &str, target: &str, owner: &str, now_ms: u64) {
            create_pdf_v3_run(PdfV3RunCreationRequest {
                job_directory: &self.job,
                source_fingerprint: &format!("sha256:{}", "a".repeat(64)),
                source_page_count: 10,
                requested_page_set: Some("1-3,9"),
                preferred_page_number: None,
                source_language: source,
                target_language: target,
                owner_session_id: owner,
                now_ms,
                component: &self.component,
            })
            .expect("create run");
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
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
                component_id: "pdf-v3-run-list-test".to_string(),
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
        }
    }
}
