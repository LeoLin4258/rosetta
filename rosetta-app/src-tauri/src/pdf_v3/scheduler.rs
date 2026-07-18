use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::page_set::PageSet;

const SCHEDULER_SCHEMA_VERSION: u32 = 1;
const MANIFEST_FILENAME: &str = "manifest.json";
const PAGES_PER_SHARD: u32 = 64;
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_STATUS_WINDOW: usize = 256;
const MAX_CAPACITY: u32 = 4096;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static LEASE_COUNTER: AtomicU64 = AtomicU64::new(1);
static SCHEDULER_COORDINATORS: Lazy<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfV3RunSpec {
    pub run_id: String,
    pub source_fingerprint: String,
    pub source_page_count: u32,
    pub requested_pages: PageSet,
    pub source_language: String,
    pub target_language: String,
    pub engine_version: String,
    pub page_graph_schema_version: u32,
    pub translation_patch_schema_version: u32,
    pub renderer_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3SchedulerCapacity {
    pub max_extracting_pages: u32,
    pub max_extracted_pages: u32,
    pub max_translating_pages: u32,
}

impl PdfV3SchedulerCapacity {
    pub(crate) fn validate(self) -> Result<Self, PdfV3SchedulerError> {
        for (field, value) in [
            ("maxExtractingPages", self.max_extracting_pages),
            ("maxExtractedPages", self.max_extracted_pages),
            ("maxTranslatingPages", self.max_translating_pages),
        ] {
            if value == 0 || value > MAX_CAPACITY {
                return Err(PdfV3SchedulerError::InvalidCapacity { field, value });
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PdfV3RunState {
    Running,
    Paused,
    Cancelling,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3Cancellation {
    pub requested_at_ms: u64,
    pub reason_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PdfV3SchedulerStage {
    Extraction,
    Translation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3ExtractionAuthority {
    pub artifact_id: String,
    pub source_page_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3PatchAuthority {
    pub patch_id: String,
    pub translation_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3PageLease {
    pub lease_id: String,
    pub owner_session_id: String,
    pub stage: PdfV3SchedulerStage,
    pub leased_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum PdfV3PageState {
    Pending,
    Extracted {
        extraction: PdfV3ExtractionAuthority,
    },
    Completed {
        extraction: PdfV3ExtractionAuthority,
        patch: PdfV3PatchAuthority,
    },
    Preserved {
        extraction: PdfV3ExtractionAuthority,
        reason_code: String,
    },
    Failed {
        stage: PdfV3SchedulerStage,
        extraction: Option<PdfV3ExtractionAuthority>,
        reason_code: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3PageRecord {
    pub page_number: u32,
    pub state: PdfV3PageState,
    pub lease: Option<PdfV3PageLease>,
    pub extraction_attempts: u32,
    pub translation_attempts: u32,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfV3PageClaim {
    pub page_number: u32,
    pub lease: PdfV3PageLease,
    pub extraction: Option<PdfV3ExtractionAuthority>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfV3TranslationCommit {
    Patch(PdfV3PatchAuthority),
    Preserved { reason_code: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3SchedulerSummary {
    pub requested_pages: u32,
    pub pending_pages: u32,
    pub extracting_pages: u32,
    pub extracted_pages: u32,
    pub translating_pages: u32,
    pub completed_pages: u32,
    pub preserved_pages: u32,
    pub failed_pages: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PdfV3RecoveryInventory {
    pub extractions: BTreeMap<u32, PdfV3ExtractionAuthority>,
    pub patches: BTreeMap<u32, PdfV3PatchAuthority>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PdfV3RecoveryReport {
    pub released_extraction_leases: u32,
    pub released_translation_leases: u32,
    pub promoted_extractions: u32,
    pub promoted_patches: u32,
    pub invalidated_extractions: u32,
    pub invalidated_patches: u32,
    pub retained_completed_pages: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PdfV3SchedulerManifest {
    schema_version: u32,
    run_id: String,
    source_fingerprint: String,
    source_page_count: u32,
    requested_page_set: String,
    source_language: String,
    target_language: String,
    engine_version: String,
    page_graph_schema_version: u32,
    translation_patch_schema_version: u32,
    renderer_version: String,
    pages_per_shard: u32,
    capacity: PdfV3SchedulerCapacity,
    run_state: PdfV3RunState,
    owner_session_id: String,
    owner_lease_updated_at_ms: u64,
    cancellation: Option<PdfV3Cancellation>,
    extraction_cursor: u32,
    translation_cursor: u32,
    generation: u64,
    summary: PdfV3SchedulerSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PdfV3SchedulerShard {
    schema_version: u32,
    run_id: String,
    shard_index: u32,
    generation: u64,
    records: Vec<PdfV3PageRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct DurablePdfV3Scheduler {
    run_dir: PathBuf,
    coordinator: Arc<Mutex<()>>,
}

#[derive(Debug)]
pub(crate) enum PdfV3SchedulerError {
    InvalidPath,
    InvalidIdentity(&'static str),
    InvalidPageSet(String),
    InvalidCapacity {
        field: &'static str,
        value: u32,
    },
    InvalidLimit {
        value: usize,
        maximum: usize,
    },
    AlreadyExists,
    LockPoisoned,
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    IndexTooLarge {
        bytes: u64,
        maximum: u64,
    },
    Corrupt(String),
    OwnerLeaseActive,
    OwnerMismatch,
    RunNotClaimable(PdfV3RunState),
    RunNotPausable(PdfV3RunState),
    RunNotResumable(PdfV3RunState),
    RunNotCancellable(PdfV3RunState),
    RunNotRetryable(PdfV3RunState),
    CancellationHasActiveLeases,
    PageNotRequested(u32),
    PageNotClaimed {
        page_number: u32,
    },
    LeaseMismatch {
        page_number: u32,
    },
    StageMismatch {
        page_number: u32,
    },
    InvalidTransition {
        page_number: u32,
    },
    InvalidAuthority(&'static str),
    AttemptOverflow {
        page_number: u32,
    },
    GenerationOverflow,
}

impl fmt::Display for PdfV3SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("PDF v3 scheduler run path must be absolute"),
            Self::InvalidIdentity(field) => {
                write!(formatter, "PDF v3 scheduler {field} is invalid")
            }
            Self::InvalidPageSet(message) => {
                write!(formatter, "PDF v3 scheduler page set is invalid: {message}")
            }
            Self::InvalidCapacity { field, value } => write!(
                formatter,
                "PDF v3 scheduler capacity {field}={value} is outside 1..={MAX_CAPACITY}"
            ),
            Self::InvalidLimit { value, maximum } => write!(
                formatter,
                "PDF v3 scheduler window limit {value} is outside 1..={maximum}"
            ),
            Self::AlreadyExists => formatter.write_str("PDF v3 scheduler run already exists"),
            Self::LockPoisoned => formatter.write_str("PDF v3 scheduler lock is poisoned"),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} PDF v3 scheduler path {}: {message}",
                path.display()
            ),
            Self::IndexTooLarge { bytes, maximum } => write!(
                formatter,
                "PDF v3 scheduler index has {bytes} bytes, above maximum {maximum}"
            ),
            Self::Corrupt(message) => {
                write!(formatter, "PDF v3 scheduler state is invalid: {message}")
            }
            Self::OwnerLeaseActive => {
                formatter.write_str("PDF v3 scheduler run is owned by a live session")
            }
            Self::OwnerMismatch => {
                formatter.write_str("PDF v3 scheduler owner session does not match")
            }
            Self::RunNotClaimable(state) => write!(
                formatter,
                "PDF v3 scheduler run state {state:?} does not accept new claims"
            ),
            Self::RunNotPausable(state) => write!(
                formatter,
                "PDF v3 scheduler run state {state:?} cannot be paused"
            ),
            Self::RunNotResumable(state) => write!(
                formatter,
                "PDF v3 scheduler run state {state:?} cannot be resumed"
            ),
            Self::RunNotCancellable(state) => write!(
                formatter,
                "PDF v3 scheduler run state {state:?} cannot be cancelled"
            ),
            Self::RunNotRetryable(state) => write!(
                formatter,
                "PDF v3 scheduler run state {state:?} cannot retry failed pages"
            ),
            Self::CancellationHasActiveLeases => {
                formatter.write_str("PDF v3 scheduler cancellation still has active page leases")
            }
            Self::PageNotRequested(page) => {
                write!(formatter, "PDF page {page} is not requested by this run")
            }
            Self::PageNotClaimed { page_number } => {
                write!(formatter, "PDF page {page_number} has no active lease")
            }
            Self::LeaseMismatch { page_number } => write!(
                formatter,
                "PDF page {page_number} lease identity does not match"
            ),
            Self::StageMismatch { page_number } => write!(
                formatter,
                "PDF page {page_number} lease stage does not match"
            ),
            Self::InvalidTransition { page_number } => write!(
                formatter,
                "PDF page {page_number} cannot perform this state transition"
            ),
            Self::InvalidAuthority(field) => {
                write!(formatter, "PDF v3 scheduler authority {field} is invalid")
            }
            Self::AttemptOverflow { page_number } => write!(
                formatter,
                "PDF page {page_number} attempt counter overflowed"
            ),
            Self::GenerationOverflow => {
                formatter.write_str("PDF v3 scheduler generation overflowed")
            }
        }
    }
}

impl std::error::Error for PdfV3SchedulerError {}

impl DurablePdfV3Scheduler {
    pub(crate) fn create(
        run_dir: &Path,
        spec: PdfV3RunSpec,
        capacity: PdfV3SchedulerCapacity,
        owner_session_id: impl Into<String>,
        now_ms: u64,
    ) -> Result<Self, PdfV3SchedulerError> {
        validate_run_dir(run_dir)?;
        validate_spec(&spec)?;
        let capacity = capacity.validate()?;
        let owner_session_id = owner_session_id.into();
        validate_identity(&owner_session_id, "ownerSessionId")?;
        let coordinator = scheduler_coordinator(run_dir)?;
        let scheduler = Self {
            run_dir: run_dir.to_path_buf(),
            coordinator,
        };
        let _guard = scheduler
            .coordinator
            .lock()
            .map_err(|_| PdfV3SchedulerError::LockPoisoned)?;
        if scheduler.run_dir.exists() {
            return Err(PdfV3SchedulerError::AlreadyExists);
        }
        let parent = scheduler
            .run_dir
            .parent()
            .ok_or(PdfV3SchedulerError::InvalidPath)?;
        fs::create_dir_all(parent).map_err(|error| io_error("create", parent, error))?;
        let staging_dir = unique_sidecar_path(&scheduler.run_dir, "creating");
        fs::create_dir(&staging_dir).map_err(|error| io_error("create", &staging_dir, error))?;
        let staging = Self {
            run_dir: staging_dir.clone(),
            coordinator: scheduler.coordinator.clone(),
        };
        if let Err(error) = staging.initialize_new_run(spec, capacity, owner_session_id, now_ms) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = fs::rename(&staging_dir, &scheduler.run_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            if scheduler.run_dir.exists() {
                return Err(PdfV3SchedulerError::AlreadyExists);
            }
            return Err(io_error("commit", &scheduler.run_dir, error));
        }
        sync_parent_directory(parent)?;
        drop(_guard);
        Ok(scheduler)
    }

    fn initialize_new_run(
        &self,
        spec: PdfV3RunSpec,
        capacity: PdfV3SchedulerCapacity,
        owner_session_id: String,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let mut grouped = BTreeMap::<u32, Vec<PdfV3PageRecord>>::new();
        for page_number in spec.requested_pages.pages().iter().copied() {
            grouped
                .entry(shard_index(page_number))
                .or_default()
                .push(PdfV3PageRecord {
                    page_number,
                    state: PdfV3PageState::Pending,
                    lease: None,
                    extraction_attempts: 0,
                    translation_attempts: 0,
                    updated_at_ms: now_ms,
                });
        }
        for (index, records) in grouped {
            self.write_shard(&PdfV3SchedulerShard {
                schema_version: SCHEDULER_SCHEMA_VERSION,
                run_id: spec.run_id.clone(),
                shard_index: index,
                generation: 1,
                records,
            })?;
        }
        let requested_pages = u32::try_from(spec.requested_pages.pages().len())
            .map_err(|_| PdfV3SchedulerError::InvalidPageSet("too many pages".to_string()))?;
        let run_state = if requested_pages == 0 {
            PdfV3RunState::Completed
        } else {
            PdfV3RunState::Running
        };
        self.write_manifest(&PdfV3SchedulerManifest {
            schema_version: SCHEDULER_SCHEMA_VERSION,
            run_id: spec.run_id,
            source_fingerprint: spec.source_fingerprint,
            source_page_count: spec.source_page_count,
            requested_page_set: spec.requested_pages.canonical_string(),
            source_language: spec.source_language,
            target_language: spec.target_language,
            engine_version: spec.engine_version,
            page_graph_schema_version: spec.page_graph_schema_version,
            translation_patch_schema_version: spec.translation_patch_schema_version,
            renderer_version: spec.renderer_version,
            pages_per_shard: PAGES_PER_SHARD,
            capacity,
            run_state,
            owner_session_id,
            owner_lease_updated_at_ms: now_ms,
            cancellation: None,
            extraction_cursor: 0,
            translation_cursor: 0,
            generation: 1,
            summary: PdfV3SchedulerSummary {
                requested_pages,
                pending_pages: requested_pages,
                ..PdfV3SchedulerSummary::default()
            },
        })
    }

    pub(crate) fn open(run_dir: &Path) -> Result<Self, PdfV3SchedulerError> {
        validate_run_dir(run_dir)?;
        let scheduler = Self {
            run_dir: run_dir.to_path_buf(),
            coordinator: scheduler_coordinator(run_dir)?,
        };
        let _guard = scheduler
            .coordinator
            .lock()
            .map_err(|_| PdfV3SchedulerError::LockPoisoned)?;
        let mut manifest = scheduler.read_manifest()?;
        scheduler.validate_all_shards(&manifest)?;
        let summary = scheduler.scan_summary(&manifest)?;
        let mut changed = summary != manifest.summary;
        manifest.summary = summary;
        if manifest.run_state == PdfV3RunState::Completed && !summary_is_complete(&manifest.summary)
        {
            manifest.run_state = PdfV3RunState::Running;
            changed = true;
        } else if matches!(
            manifest.run_state,
            PdfV3RunState::Running | PdfV3RunState::Paused
        ) && summary_is_complete(&manifest.summary)
        {
            manifest.run_state = PdfV3RunState::Completed;
            changed = true;
        }
        if changed {
            bump_manifest_generation(&mut manifest)?;
            scheduler.write_manifest(&manifest)?;
        }
        drop(_guard);
        Ok(scheduler)
    }

    pub(crate) fn manifest_snapshot(
        &self,
    ) -> Result<(PdfV3RunState, PdfV3SchedulerSummary), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let manifest = self.read_manifest()?;
        Ok((manifest.run_state, manifest.summary))
    }

    pub(crate) fn page_window(
        &self,
        start_after: Option<u32>,
        limit: usize,
    ) -> Result<Vec<PdfV3PageRecord>, PdfV3SchedulerError> {
        if limit == 0 || limit > MAX_STATUS_WINDOW {
            return Err(PdfV3SchedulerError::InvalidLimit {
                value: limit,
                maximum: MAX_STATUS_WINDOW,
            });
        }
        let _guard = self.lock()?;
        let manifest = self.read_manifest()?;
        let requested = requested_pages(&manifest)?;
        let mut records = Vec::with_capacity(limit);
        let mut loaded_shard = None;
        for page_number in requested
            .pages()
            .iter()
            .copied()
            .filter(|page| start_after.is_none_or(|start| *page > start))
        {
            let index = shard_index(page_number);
            if loaded_shard
                .as_ref()
                .is_none_or(|shard: &PdfV3SchedulerShard| shard.shard_index != index)
            {
                loaded_shard = Some(self.read_shard(&manifest, index)?);
            }
            let shard = loaded_shard.as_ref().expect("loaded above");
            records.push(find_record(shard, page_number)?.clone());
            if records.len() == limit {
                break;
            }
        }
        Ok(records)
    }

    pub(crate) fn renew_owner(
        &self,
        owner_session_id: &str,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    pub(crate) fn pause(
        &self,
        owner_session_id: &str,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if manifest.run_state != PdfV3RunState::Running {
            return Err(PdfV3SchedulerError::RunNotPausable(manifest.run_state));
        }
        manifest.run_state = PdfV3RunState::Paused;
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    pub(crate) fn resume(
        &self,
        owner_session_id: &str,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if manifest.run_state != PdfV3RunState::Paused {
            return Err(PdfV3SchedulerError::RunNotResumable(manifest.run_state));
        }
        manifest.run_state = PdfV3RunState::Running;
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    pub(crate) fn request_cancel(
        &self,
        owner_session_id: &str,
        now_ms: u64,
        reason_code: impl Into<String>,
    ) -> Result<(), PdfV3SchedulerError> {
        let reason_code = reason_code.into();
        validate_authority(&reason_code, "cancellationReasonCode")?;
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if !matches!(
            manifest.run_state,
            PdfV3RunState::Running | PdfV3RunState::Paused
        ) {
            return Err(PdfV3SchedulerError::RunNotCancellable(manifest.run_state));
        }
        manifest.run_state = PdfV3RunState::Cancelling;
        manifest.cancellation = Some(PdfV3Cancellation {
            requested_at_ms: now_ms,
            reason_code,
        });
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    pub(crate) fn finish_cancellation(
        &self,
        owner_session_id: &str,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if manifest.run_state != PdfV3RunState::Cancelling {
            return Err(PdfV3SchedulerError::RunNotCancellable(manifest.run_state));
        }
        let summary = self.scan_summary(&manifest)?;
        if summary.extracting_pages != 0 || summary.translating_pages != 0 {
            return Err(PdfV3SchedulerError::CancellationHasActiveLeases);
        }
        manifest.run_state = PdfV3RunState::Cancelled;
        manifest.summary = summary;
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)
    }

    pub(crate) fn claim_extraction(
        &self,
        owner_session_id: &str,
        requested_limit: u32,
        now_ms: u64,
    ) -> Result<Vec<PdfV3PageClaim>, PdfV3SchedulerError> {
        self.claim_stage(
            owner_session_id,
            PdfV3SchedulerStage::Extraction,
            requested_limit,
            now_ms,
        )
    }

    pub(crate) fn claim_translation(
        &self,
        owner_session_id: &str,
        requested_limit: u32,
        now_ms: u64,
    ) -> Result<Vec<PdfV3PageClaim>, PdfV3SchedulerError> {
        self.claim_stage(
            owner_session_id,
            PdfV3SchedulerStage::Translation,
            requested_limit,
            now_ms,
        )
    }

    fn claim_stage(
        &self,
        owner_session_id: &str,
        stage: PdfV3SchedulerStage,
        requested_limit: u32,
        now_ms: u64,
    ) -> Result<Vec<PdfV3PageClaim>, PdfV3SchedulerError> {
        if requested_limit == 0 {
            return Ok(Vec::new());
        }
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if manifest.run_state != PdfV3RunState::Running {
            return Err(PdfV3SchedulerError::RunNotClaimable(manifest.run_state));
        }
        let requested = requested_pages(&manifest)?;
        let summary = self.scan_summary(&manifest)?;
        let available = match stage {
            PdfV3SchedulerStage::Extraction => {
                let extraction_slots = manifest
                    .capacity
                    .max_extracting_pages
                    .saturating_sub(summary.extracting_pages);
                let backlog_slots = manifest
                    .capacity
                    .max_extracted_pages
                    .saturating_sub(summary.extracted_pages + summary.extracting_pages);
                extraction_slots.min(backlog_slots)
            }
            PdfV3SchedulerStage::Translation => manifest
                .capacity
                .max_translating_pages
                .saturating_sub(summary.translating_pages),
        }
        .min(requested_limit);
        if available == 0 {
            return Ok(Vec::new());
        }

        let cursor = match stage {
            PdfV3SchedulerStage::Extraction => manifest.extraction_cursor,
            PdfV3SchedulerStage::Translation => manifest.translation_cursor,
        };
        let ordered_pages = pages_after_cursor(&requested, cursor);
        let mut claims = Vec::with_capacity(available as usize);
        let mut shard_cache = None::<PdfV3SchedulerShard>;
        let mut dirty = false;
        for page_number in ordered_pages {
            let index = shard_index(page_number);
            if shard_cache
                .as_ref()
                .is_none_or(|shard| shard.shard_index != index)
            {
                if let Some(mut shard) = shard_cache.take() {
                    if dirty {
                        bump_shard_generation(&mut shard)?;
                        self.write_shard(&shard)?;
                    }
                }
                shard_cache = Some(self.read_shard(&manifest, index)?);
                dirty = false;
            }
            let record = find_record_mut(shard_cache.as_mut().expect("loaded above"), page_number)?;
            let extraction = match (&record.state, stage) {
                (PdfV3PageState::Pending, PdfV3SchedulerStage::Extraction)
                    if record.lease.is_none() =>
                {
                    None
                }
                (PdfV3PageState::Extracted { extraction }, PdfV3SchedulerStage::Translation)
                    if record.lease.is_none() =>
                {
                    Some(extraction.clone())
                }
                _ => continue,
            };
            let lease = PdfV3PageLease {
                lease_id: lease_id(
                    &manifest.run_id,
                    owner_session_id,
                    page_number,
                    stage,
                    now_ms,
                ),
                owner_session_id: owner_session_id.to_string(),
                stage,
                leased_at_ms: now_ms,
            };
            match stage {
                PdfV3SchedulerStage::Extraction => {
                    record.extraction_attempts = record
                        .extraction_attempts
                        .checked_add(1)
                        .ok_or(PdfV3SchedulerError::AttemptOverflow { page_number })?;
                }
                PdfV3SchedulerStage::Translation => {
                    record.translation_attempts = record
                        .translation_attempts
                        .checked_add(1)
                        .ok_or(PdfV3SchedulerError::AttemptOverflow { page_number })?;
                }
            }
            record.lease = Some(lease.clone());
            record.updated_at_ms = now_ms;
            dirty = true;
            claims.push(PdfV3PageClaim {
                page_number,
                lease,
                extraction,
            });
            if claims.len() == available as usize {
                break;
            }
        }
        if let Some(mut shard) = shard_cache {
            if dirty {
                bump_shard_generation(&mut shard)?;
                self.write_shard(&shard)?;
            }
        }
        if let Some(last) = claims.last() {
            match stage {
                PdfV3SchedulerStage::Extraction => manifest.extraction_cursor = last.page_number,
                PdfV3SchedulerStage::Translation => manifest.translation_cursor = last.page_number,
            }
        }
        manifest.summary = self.scan_summary(&manifest)?;
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)?;
        Ok(claims)
    }

    pub(crate) fn commit_extraction(
        &self,
        owner_session_id: &str,
        claim: &PdfV3PageClaim,
        extraction: PdfV3ExtractionAuthority,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        validate_extraction(&extraction)?;
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        let mut shard = self.read_requested_shard(&manifest, claim.page_number)?;
        let record = find_record_mut(&mut shard, claim.page_number)?;
        ensure_claim(
            record,
            claim,
            owner_session_id,
            PdfV3SchedulerStage::Extraction,
        )?;
        if !matches!(record.state, PdfV3PageState::Pending) {
            return Err(PdfV3SchedulerError::InvalidTransition {
                page_number: claim.page_number,
            });
        }
        record.state = PdfV3PageState::Extracted { extraction };
        record.lease = None;
        record.updated_at_ms = now_ms;
        self.commit_shard_and_refresh(&mut manifest, &mut shard, now_ms)
    }

    pub(crate) fn commit_translation(
        &self,
        owner_session_id: &str,
        claim: &PdfV3PageClaim,
        result: PdfV3TranslationCommit,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        match &result {
            PdfV3TranslationCommit::Patch(patch) => validate_patch(patch)?,
            PdfV3TranslationCommit::Preserved { reason_code } => {
                validate_authority(reason_code, "preservationReasonCode")?
            }
        }
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        let mut shard = self.read_requested_shard(&manifest, claim.page_number)?;
        let record = find_record_mut(&mut shard, claim.page_number)?;
        ensure_claim(
            record,
            claim,
            owner_session_id,
            PdfV3SchedulerStage::Translation,
        )?;
        let PdfV3PageState::Extracted { extraction } = &record.state else {
            return Err(PdfV3SchedulerError::InvalidTransition {
                page_number: claim.page_number,
            });
        };
        let extraction = extraction.clone();
        record.state = match result {
            PdfV3TranslationCommit::Patch(patch) => PdfV3PageState::Completed { extraction, patch },
            PdfV3TranslationCommit::Preserved { reason_code } => PdfV3PageState::Preserved {
                extraction,
                reason_code,
            },
        };
        record.lease = None;
        record.updated_at_ms = now_ms;
        self.commit_shard_and_refresh(&mut manifest, &mut shard, now_ms)
    }

    pub(crate) fn fail_claim(
        &self,
        owner_session_id: &str,
        claim: &PdfV3PageClaim,
        reason_code: impl Into<String>,
        retryable: bool,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let reason_code = reason_code.into();
        validate_authority(&reason_code, "failureReasonCode")?;
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        let mut shard = self.read_requested_shard(&manifest, claim.page_number)?;
        let record = find_record_mut(&mut shard, claim.page_number)?;
        ensure_claim(record, claim, owner_session_id, claim.lease.stage)?;
        let extraction = match (&record.state, claim.lease.stage) {
            (PdfV3PageState::Pending, PdfV3SchedulerStage::Extraction) => None,
            (PdfV3PageState::Extracted { extraction }, PdfV3SchedulerStage::Translation) => {
                Some(extraction.clone())
            }
            _ => {
                return Err(PdfV3SchedulerError::InvalidTransition {
                    page_number: claim.page_number,
                })
            }
        };
        record.state = PdfV3PageState::Failed {
            stage: claim.lease.stage,
            extraction,
            reason_code,
            retryable,
        };
        record.lease = None;
        record.updated_at_ms = now_ms;
        self.commit_shard_and_refresh(&mut manifest, &mut shard, now_ms)
    }

    pub(crate) fn retry_failed(
        &self,
        owner_session_id: &str,
        page_number: u32,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        ensure_owner(&manifest, owner_session_id)?;
        if !matches!(
            manifest.run_state,
            PdfV3RunState::Running | PdfV3RunState::Paused
        ) {
            return Err(PdfV3SchedulerError::RunNotRetryable(manifest.run_state));
        }
        let mut shard = self.read_requested_shard(&manifest, page_number)?;
        let record = find_record_mut(&mut shard, page_number)?;
        let PdfV3PageState::Failed {
            extraction,
            retryable: true,
            ..
        } = &record.state
        else {
            return Err(PdfV3SchedulerError::InvalidTransition { page_number });
        };
        record.state = extraction
            .clone()
            .map(|extraction| PdfV3PageState::Extracted { extraction })
            .unwrap_or(PdfV3PageState::Pending);
        record.updated_at_ms = now_ms;
        self.commit_shard_and_refresh(&mut manifest, &mut shard, now_ms)
    }

    pub(crate) fn recover_stale_owner(
        &self,
        new_owner_session_id: impl Into<String>,
        now_ms: u64,
        stale_before_ms: u64,
        inventory: &PdfV3RecoveryInventory,
    ) -> Result<PdfV3RecoveryReport, PdfV3SchedulerError> {
        let new_owner_session_id = new_owner_session_id.into();
        validate_identity(&new_owner_session_id, "ownerSessionId")?;
        for extraction in inventory.extractions.values() {
            validate_extraction(extraction)?;
        }
        for patch in inventory.patches.values() {
            validate_patch(patch)?;
        }
        let _guard = self.lock()?;
        let mut manifest = self.read_manifest()?;
        if manifest.owner_session_id != new_owner_session_id
            && manifest.owner_lease_updated_at_ms >= stale_before_ms
        {
            return Err(PdfV3SchedulerError::OwnerLeaseActive);
        }
        let mut report = PdfV3RecoveryReport::default();
        for index in requested_shard_indices(&manifest)? {
            let mut shard = self.read_shard(&manifest, index)?;
            let mut dirty = false;
            for record in &mut shard.records {
                let mut record_dirty = false;
                if let Some(lease) = record.lease.take() {
                    match lease.stage {
                        PdfV3SchedulerStage::Extraction => report.released_extraction_leases += 1,
                        PdfV3SchedulerStage::Translation => report.released_translation_leases += 1,
                    }
                    dirty = true;
                    record_dirty = true;
                }
                let valid_extraction = inventory.extractions.get(&record.page_number).cloned();
                let valid_patch = inventory.patches.get(&record.page_number).cloned();
                let next_state = match &record.state {
                    PdfV3PageState::Pending => match (valid_extraction, valid_patch) {
                        (Some(extraction), Some(patch)) => {
                            report.promoted_extractions += 1;
                            report.promoted_patches += 1;
                            Some(PdfV3PageState::Completed { extraction, patch })
                        }
                        (Some(extraction), None) => {
                            report.promoted_extractions += 1;
                            Some(PdfV3PageState::Extracted { extraction })
                        }
                        (None, _) => None,
                    },
                    PdfV3PageState::Extracted { extraction } => {
                        match (valid_extraction, valid_patch) {
                            (Some(valid), Some(patch)) if &valid == extraction => {
                                report.promoted_patches += 1;
                                Some(PdfV3PageState::Completed {
                                    extraction: valid,
                                    patch,
                                })
                            }
                            (Some(valid), None) if &valid == extraction => None,
                            (Some(valid), Some(patch)) => {
                                report.invalidated_extractions += 1;
                                report.promoted_patches += 1;
                                Some(PdfV3PageState::Completed {
                                    extraction: valid,
                                    patch,
                                })
                            }
                            (Some(valid), None) => {
                                report.invalidated_extractions += 1;
                                Some(PdfV3PageState::Extracted { extraction: valid })
                            }
                            (None, _) => {
                                report.invalidated_extractions += 1;
                                Some(PdfV3PageState::Pending)
                            }
                        }
                    }
                    PdfV3PageState::Completed { extraction, patch } => {
                        match (valid_extraction, valid_patch) {
                            (Some(valid_extraction), Some(valid_patch))
                                if &valid_extraction == extraction && &valid_patch == patch =>
                            {
                                report.retained_completed_pages += 1;
                                None
                            }
                            (Some(valid_extraction), Some(valid_patch)) => {
                                report.invalidated_patches += 1;
                                Some(PdfV3PageState::Completed {
                                    extraction: valid_extraction,
                                    patch: valid_patch,
                                })
                            }
                            (Some(valid_extraction), None) => {
                                report.invalidated_patches += 1;
                                Some(PdfV3PageState::Extracted {
                                    extraction: valid_extraction,
                                })
                            }
                            (None, _) => {
                                report.invalidated_patches += 1;
                                report.invalidated_extractions += 1;
                                Some(PdfV3PageState::Pending)
                            }
                        }
                    }
                    PdfV3PageState::Preserved { extraction, .. } => {
                        if valid_extraction.as_ref() == Some(extraction) {
                            None
                        } else {
                            report.invalidated_extractions += 1;
                            Some(PdfV3PageState::Pending)
                        }
                    }
                    PdfV3PageState::Failed {
                        extraction: stored_extraction,
                        ..
                    } => match (stored_extraction, valid_extraction, valid_patch) {
                        (_, Some(valid), Some(patch)) => {
                            report.promoted_patches += 1;
                            Some(PdfV3PageState::Completed {
                                extraction: valid,
                                patch,
                            })
                        }
                        (Some(stored), Some(valid), None) if stored == &valid => None,
                        (Some(_), Some(valid), None) => {
                            report.invalidated_extractions += 1;
                            Some(PdfV3PageState::Extracted { extraction: valid })
                        }
                        (Some(_), None, _) => {
                            report.invalidated_extractions += 1;
                            Some(PdfV3PageState::Pending)
                        }
                        (None, _, _) => None,
                    },
                };
                if let Some(state) = next_state {
                    record.state = state;
                    dirty = true;
                    record_dirty = true;
                }
                if record_dirty {
                    record.updated_at_ms = now_ms;
                }
            }
            if dirty {
                bump_shard_generation(&mut shard)?;
                self.write_shard(&shard)?;
            }
        }
        manifest.owner_session_id = new_owner_session_id;
        manifest.owner_lease_updated_at_ms = now_ms;
        manifest.summary = self.scan_summary(&manifest)?;
        if manifest.run_state == PdfV3RunState::Completed && !summary_is_complete(&manifest.summary)
        {
            manifest.run_state = PdfV3RunState::Running;
        }
        if matches!(
            manifest.run_state,
            PdfV3RunState::Running | PdfV3RunState::Paused
        ) && summary_is_complete(&manifest.summary)
        {
            manifest.run_state = PdfV3RunState::Completed;
        }
        bump_manifest_generation(&mut manifest)?;
        self.write_manifest(&manifest)?;
        Ok(report)
    }

    fn commit_shard_and_refresh(
        &self,
        manifest: &mut PdfV3SchedulerManifest,
        shard: &mut PdfV3SchedulerShard,
        now_ms: u64,
    ) -> Result<(), PdfV3SchedulerError> {
        bump_shard_generation(shard)?;
        self.write_shard(shard)?;
        manifest.summary = self.scan_summary(manifest)?;
        if manifest.run_state == PdfV3RunState::Running && summary_is_complete(&manifest.summary) {
            manifest.run_state = PdfV3RunState::Completed;
        }
        manifest.owner_lease_updated_at_ms = now_ms;
        bump_manifest_generation(manifest)?;
        self.write_manifest(manifest)
    }

    fn scan_summary(
        &self,
        manifest: &PdfV3SchedulerManifest,
    ) -> Result<PdfV3SchedulerSummary, PdfV3SchedulerError> {
        let mut summary = PdfV3SchedulerSummary::default();
        for index in requested_shard_indices(manifest)? {
            let shard = self.read_shard(manifest, index)?;
            for record in shard.records {
                summary.requested_pages =
                    summary.requested_pages.checked_add(1).ok_or_else(|| {
                        PdfV3SchedulerError::Corrupt("page count overflow".to_string())
                    })?;
                match record.state {
                    PdfV3PageState::Pending => summary.pending_pages += 1,
                    PdfV3PageState::Extracted { .. } => {
                        if record
                            .lease
                            .as_ref()
                            .is_some_and(|lease| lease.stage == PdfV3SchedulerStage::Translation)
                        {
                            summary.translating_pages += 1;
                        } else {
                            summary.extracted_pages += 1;
                        }
                    }
                    PdfV3PageState::Completed { .. } => summary.completed_pages += 1,
                    PdfV3PageState::Preserved { .. } => summary.preserved_pages += 1,
                    PdfV3PageState::Failed { .. } => summary.failed_pages += 1,
                }
                if record
                    .lease
                    .as_ref()
                    .is_some_and(|lease| lease.stage == PdfV3SchedulerStage::Extraction)
                {
                    summary.extracting_pages += 1;
                }
            }
        }
        Ok(summary)
    }

    fn validate_all_shards(
        &self,
        manifest: &PdfV3SchedulerManifest,
    ) -> Result<(), PdfV3SchedulerError> {
        let expected = requested_pages(manifest)?;
        let mut expected_pages = expected.pages().iter().copied();
        for index in requested_shard_indices(manifest)? {
            let shard = self.read_shard(manifest, index)?;
            for record in shard.records {
                if expected_pages.next() != Some(record.page_number) {
                    return Err(PdfV3SchedulerError::Corrupt(
                        "page shards do not exactly cover requested page set".to_string(),
                    ));
                }
            }
        }
        if expected_pages.next().is_some() {
            return Err(PdfV3SchedulerError::Corrupt(
                "page shards do not exactly cover requested page set".to_string(),
            ));
        }
        Ok(())
    }

    fn read_requested_shard(
        &self,
        manifest: &PdfV3SchedulerManifest,
        page_number: u32,
    ) -> Result<PdfV3SchedulerShard, PdfV3SchedulerError> {
        if !requested_pages(manifest)?.contains(page_number) {
            return Err(PdfV3SchedulerError::PageNotRequested(page_number));
        }
        self.read_shard(manifest, shard_index(page_number))
    }

    fn read_manifest(&self) -> Result<PdfV3SchedulerManifest, PdfV3SchedulerError> {
        let path = self.run_dir.join(MANIFEST_FILENAME);
        let candidates = index_candidate_paths(&path)?;
        let mut valid = Vec::new();
        for candidate in &candidates {
            let Ok(bytes) = read_limited(candidate, MAX_INDEX_BYTES) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_slice::<PdfV3SchedulerManifest>(&bytes) else {
                continue;
            };
            if validate_manifest(&manifest).is_ok() {
                valid.push((
                    manifest.generation,
                    candidate == &path,
                    candidate.clone(),
                    manifest,
                ));
            }
        }
        valid.sort_by_key(|(generation, canonical, _, _)| (*generation, *canonical));
        let Some((_, canonical, _, manifest)) = valid.pop() else {
            return Err(PdfV3SchedulerError::Corrupt(
                "no valid manifest candidate".to_string(),
            ));
        };
        if !canonical {
            self.write_manifest(&manifest)?;
        }
        cleanup_index_sidecars(&path, &candidates);
        Ok(manifest)
    }

    fn read_shard(
        &self,
        manifest: &PdfV3SchedulerManifest,
        index: u32,
    ) -> Result<PdfV3SchedulerShard, PdfV3SchedulerError> {
        self.read_shard_path(manifest, &self.run_dir.join(shard_filename(index)))
    }

    fn read_shard_path(
        &self,
        manifest: &PdfV3SchedulerManifest,
        path: &Path,
    ) -> Result<PdfV3SchedulerShard, PdfV3SchedulerError> {
        let expected_index = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_canonical_shard_name)
            .ok_or_else(|| PdfV3SchedulerError::Corrupt("invalid shard path".to_string()))?;
        let candidates = index_candidate_paths(path)?;
        let mut valid = Vec::new();
        for candidate in &candidates {
            let Ok(bytes) = read_limited(candidate, MAX_INDEX_BYTES) else {
                continue;
            };
            let Ok(shard) = serde_json::from_slice::<PdfV3SchedulerShard>(&bytes) else {
                continue;
            };
            if shard.shard_index == expected_index && validate_shard(manifest, &shard).is_ok() {
                valid.push((
                    shard.generation,
                    candidate == path,
                    candidate.clone(),
                    shard,
                ));
            }
        }
        valid.sort_by_key(|(generation, canonical, _, _)| (*generation, *canonical));
        let Some((_, canonical, _, shard)) = valid.pop() else {
            return Err(PdfV3SchedulerError::Corrupt(format!(
                "no valid candidate for shard {expected_index}"
            )));
        };
        if !canonical {
            self.write_shard(&shard)?;
        }
        cleanup_index_sidecars(path, &candidates);
        Ok(shard)
    }

    fn write_manifest(&self, manifest: &PdfV3SchedulerManifest) -> Result<(), PdfV3SchedulerError> {
        validate_manifest(manifest)?;
        write_json_atomic(&self.run_dir.join(MANIFEST_FILENAME), manifest)
    }

    fn write_shard(&self, shard: &PdfV3SchedulerShard) -> Result<(), PdfV3SchedulerError> {
        validate_shard_without_manifest(shard)?;
        write_json_atomic(&self.run_dir.join(shard_filename(shard.shard_index)), shard)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, PdfV3SchedulerError> {
        self.coordinator
            .lock()
            .map_err(|_| PdfV3SchedulerError::LockPoisoned)
    }
}

fn validate_run_dir(run_dir: &Path) -> Result<(), PdfV3SchedulerError> {
    if !run_dir.is_absolute() {
        return Err(PdfV3SchedulerError::InvalidPath);
    }
    Ok(())
}

fn validate_spec(spec: &PdfV3RunSpec) -> Result<(), PdfV3SchedulerError> {
    for (value, field) in [
        (&spec.run_id, "runId"),
        (&spec.source_fingerprint, "sourceFingerprint"),
        (&spec.source_language, "sourceLanguage"),
        (&spec.target_language, "targetLanguage"),
        (&spec.engine_version, "engineVersion"),
        (&spec.renderer_version, "rendererVersion"),
    ] {
        validate_identity(value, field)?;
    }
    if spec.source_page_count == 0 && !spec.requested_pages.is_empty() {
        return Err(PdfV3SchedulerError::InvalidPageSet(
            "non-empty selection requires a positive source page count".to_string(),
        ));
    }
    if let Some(page) = spec
        .requested_pages
        .pages()
        .iter()
        .copied()
        .find(|page| *page > spec.source_page_count)
    {
        return Err(PdfV3SchedulerError::InvalidPageSet(format!(
            "page {page} is outside 1..={}",
            spec.source_page_count
        )));
    }
    if spec.page_graph_schema_version == 0 || spec.translation_patch_schema_version == 0 {
        return Err(PdfV3SchedulerError::InvalidIdentity("schemaVersion"));
    }
    Ok(())
}

fn validate_identity(value: &str, field: &'static str) -> Result<(), PdfV3SchedulerError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3SchedulerError::InvalidIdentity(field));
    }
    Ok(())
}

fn validate_authority(value: &str, field: &'static str) -> Result<(), PdfV3SchedulerError> {
    if value.is_empty()
        || value.len() > 512
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PdfV3SchedulerError::InvalidAuthority(field));
    }
    Ok(())
}

fn validate_extraction(extraction: &PdfV3ExtractionAuthority) -> Result<(), PdfV3SchedulerError> {
    validate_authority(&extraction.artifact_id, "artifactId")?;
    validate_authority(&extraction.source_page_hash, "sourcePageHash")
}

fn validate_patch(patch: &PdfV3PatchAuthority) -> Result<(), PdfV3SchedulerError> {
    validate_authority(&patch.patch_id, "patchId")?;
    if patch.translation_revision == 0 {
        return Err(PdfV3SchedulerError::InvalidAuthority("translationRevision"));
    }
    Ok(())
}

fn validate_manifest(manifest: &PdfV3SchedulerManifest) -> Result<(), PdfV3SchedulerError> {
    if manifest.schema_version != SCHEDULER_SCHEMA_VERSION {
        return Err(PdfV3SchedulerError::Corrupt(format!(
            "unsupported manifest schema {}",
            manifest.schema_version
        )));
    }
    if manifest.pages_per_shard != PAGES_PER_SHARD {
        return Err(PdfV3SchedulerError::Corrupt(
            "pagesPerShard does not match the scheduler contract".to_string(),
        ));
    }
    for (value, field) in [
        (&manifest.run_id, "runId"),
        (&manifest.source_fingerprint, "sourceFingerprint"),
        (&manifest.source_language, "sourceLanguage"),
        (&manifest.target_language, "targetLanguage"),
        (&manifest.engine_version, "engineVersion"),
        (&manifest.renderer_version, "rendererVersion"),
        (&manifest.owner_session_id, "ownerSessionId"),
    ] {
        validate_identity(value, field)?;
    }
    manifest.capacity.validate()?;
    requested_pages(manifest)?;
    if manifest.page_graph_schema_version == 0
        || manifest.translation_patch_schema_version == 0
        || manifest.generation == 0
    {
        return Err(PdfV3SchedulerError::Corrupt(
            "schema and generation values must be positive".to_string(),
        ));
    }
    Ok(())
}

fn validate_shard_without_manifest(shard: &PdfV3SchedulerShard) -> Result<(), PdfV3SchedulerError> {
    if shard.schema_version != SCHEDULER_SCHEMA_VERSION || shard.generation == 0 {
        return Err(PdfV3SchedulerError::Corrupt(
            "shard schema and generation must be valid".to_string(),
        ));
    }
    validate_identity(&shard.run_id, "runId")?;
    if shard.records.is_empty() || shard.records.len() > PAGES_PER_SHARD as usize {
        return Err(PdfV3SchedulerError::Corrupt(format!(
            "shard {} has invalid record count {}",
            shard.shard_index,
            shard.records.len()
        )));
    }
    let mut previous = 0;
    for record in &shard.records {
        if record.page_number <= previous || shard_index(record.page_number) != shard.shard_index {
            return Err(PdfV3SchedulerError::Corrupt(format!(
                "shard {} page order or ownership is invalid",
                shard.shard_index
            )));
        }
        validate_record(record)?;
        previous = record.page_number;
    }
    Ok(())
}

fn validate_shard(
    manifest: &PdfV3SchedulerManifest,
    shard: &PdfV3SchedulerShard,
) -> Result<(), PdfV3SchedulerError> {
    validate_shard_without_manifest(shard)?;
    if shard.run_id != manifest.run_id {
        return Err(PdfV3SchedulerError::Corrupt(format!(
            "shard {} belongs to another run",
            shard.shard_index
        )));
    }
    let requested = requested_pages(manifest)?;
    if let Some(page) = shard
        .records
        .iter()
        .map(|record| record.page_number)
        .find(|page| !requested.contains(*page))
    {
        return Err(PdfV3SchedulerError::Corrupt(format!(
            "shard contains unrequested page {page}"
        )));
    }
    Ok(())
}

fn validate_record(record: &PdfV3PageRecord) -> Result<(), PdfV3SchedulerError> {
    if record.page_number == 0 {
        return Err(PdfV3SchedulerError::Corrupt(
            "page record number must be positive".to_string(),
        ));
    }
    match &record.state {
        PdfV3PageState::Pending => {
            if record
                .lease
                .as_ref()
                .is_some_and(|lease| lease.stage != PdfV3SchedulerStage::Extraction)
            {
                return Err(PdfV3SchedulerError::Corrupt(
                    "pending page has a non-extraction lease".to_string(),
                ));
            }
        }
        PdfV3PageState::Extracted { extraction } => {
            validate_extraction(extraction)?;
            if record
                .lease
                .as_ref()
                .is_some_and(|lease| lease.stage != PdfV3SchedulerStage::Translation)
            {
                return Err(PdfV3SchedulerError::Corrupt(
                    "extracted page has a non-translation lease".to_string(),
                ));
            }
        }
        PdfV3PageState::Completed { extraction, patch } => {
            validate_extraction(extraction)?;
            validate_patch(patch)?;
            if record.lease.is_some() {
                return Err(PdfV3SchedulerError::Corrupt(
                    "completed page has an active lease".to_string(),
                ));
            }
        }
        PdfV3PageState::Preserved {
            extraction,
            reason_code,
        } => {
            validate_extraction(extraction)?;
            validate_authority(reason_code, "preservationReasonCode")?;
            if record.lease.is_some() {
                return Err(PdfV3SchedulerError::Corrupt(
                    "preserved page has an active lease".to_string(),
                ));
            }
        }
        PdfV3PageState::Failed {
            stage,
            extraction,
            reason_code,
            ..
        } => {
            if (*stage == PdfV3SchedulerStage::Extraction && extraction.is_some())
                || (*stage == PdfV3SchedulerStage::Translation && extraction.is_none())
            {
                return Err(PdfV3SchedulerError::Corrupt(
                    "failed page does not retain the expected extraction authority".to_string(),
                ));
            }
            if let Some(extraction) = extraction {
                validate_extraction(extraction)?;
            }
            validate_authority(reason_code, "failureReasonCode")?;
            if record.lease.is_some() {
                return Err(PdfV3SchedulerError::Corrupt(
                    "failed page has an active lease".to_string(),
                ));
            }
        }
    }
    if let Some(lease) = &record.lease {
        validate_authority(&lease.lease_id, "leaseId")?;
        validate_identity(&lease.owner_session_id, "ownerSessionId")?;
    }
    Ok(())
}

fn requested_pages(manifest: &PdfV3SchedulerManifest) -> Result<PageSet, PdfV3SchedulerError> {
    PageSet::parse(&manifest.requested_page_set, manifest.source_page_count)
        .map_err(|error| PdfV3SchedulerError::InvalidPageSet(error.to_string()))
}

fn requested_shard_indices(
    manifest: &PdfV3SchedulerManifest,
) -> Result<Vec<u32>, PdfV3SchedulerError> {
    let mut indices = requested_pages(manifest)?
        .pages()
        .iter()
        .copied()
        .map(shard_index)
        .collect::<Vec<_>>();
    indices.dedup();
    Ok(indices)
}

fn ensure_owner(
    manifest: &PdfV3SchedulerManifest,
    owner_session_id: &str,
) -> Result<(), PdfV3SchedulerError> {
    if manifest.owner_session_id != owner_session_id {
        return Err(PdfV3SchedulerError::OwnerMismatch);
    }
    Ok(())
}

fn ensure_claim(
    record: &PdfV3PageRecord,
    claim: &PdfV3PageClaim,
    owner_session_id: &str,
    stage: PdfV3SchedulerStage,
) -> Result<(), PdfV3SchedulerError> {
    let lease = record
        .lease
        .as_ref()
        .ok_or(PdfV3SchedulerError::PageNotClaimed {
            page_number: record.page_number,
        })?;
    if record.page_number != claim.page_number
        || lease.lease_id != claim.lease.lease_id
        || lease.owner_session_id != owner_session_id
    {
        return Err(PdfV3SchedulerError::LeaseMismatch {
            page_number: record.page_number,
        });
    }
    if lease.stage != stage || claim.lease.stage != stage {
        return Err(PdfV3SchedulerError::StageMismatch {
            page_number: record.page_number,
        });
    }
    Ok(())
}

fn find_record(
    shard: &PdfV3SchedulerShard,
    page_number: u32,
) -> Result<&PdfV3PageRecord, PdfV3SchedulerError> {
    shard
        .records
        .binary_search_by_key(&page_number, |record| record.page_number)
        .ok()
        .map(|index| &shard.records[index])
        .ok_or(PdfV3SchedulerError::PageNotRequested(page_number))
}

fn find_record_mut(
    shard: &mut PdfV3SchedulerShard,
    page_number: u32,
) -> Result<&mut PdfV3PageRecord, PdfV3SchedulerError> {
    shard
        .records
        .binary_search_by_key(&page_number, |record| record.page_number)
        .ok()
        .map(|index| &mut shard.records[index])
        .ok_or(PdfV3SchedulerError::PageNotRequested(page_number))
}

fn pages_after_cursor(page_set: &PageSet, cursor: u32) -> impl Iterator<Item = u32> + '_ {
    page_set
        .pages()
        .iter()
        .copied()
        .filter(move |page| *page > cursor)
        .chain(
            page_set
                .pages()
                .iter()
                .copied()
                .filter(move |page| *page <= cursor),
        )
}

fn summary_is_complete(summary: &PdfV3SchedulerSummary) -> bool {
    summary.requested_pages == summary.completed_pages + summary.preserved_pages
        && summary.extracting_pages == 0
        && summary.translating_pages == 0
}

fn bump_manifest_generation(
    manifest: &mut PdfV3SchedulerManifest,
) -> Result<(), PdfV3SchedulerError> {
    manifest.generation = manifest
        .generation
        .checked_add(1)
        .ok_or(PdfV3SchedulerError::GenerationOverflow)?;
    Ok(())
}

fn bump_shard_generation(shard: &mut PdfV3SchedulerShard) -> Result<(), PdfV3SchedulerError> {
    shard.generation = shard
        .generation
        .checked_add(1)
        .ok_or(PdfV3SchedulerError::GenerationOverflow)?;
    Ok(())
}

fn shard_index(page_number: u32) -> u32 {
    (page_number - 1) / PAGES_PER_SHARD
}

fn shard_filename(index: u32) -> String {
    format!("shard-{index:08}.json")
}

fn parse_canonical_shard_name(name: &str) -> Option<u32> {
    let index = name.strip_prefix("shard-")?.strip_suffix(".json")?;
    if index.len() != 8 {
        return None;
    }
    index.parse().ok()
}

fn lease_id(
    run_id: &str,
    owner_session_id: &str,
    page_number: u32,
    stage: PdfV3SchedulerStage,
    now_ms: u64,
) -> String {
    let counter = LEASE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(run_id.as_bytes());
    hasher.update([0]);
    hasher.update(owner_session_id.as_bytes());
    hasher.update(page_number.to_le_bytes());
    hasher.update([match stage {
        PdfV3SchedulerStage::Extraction => 1,
        PdfV3SchedulerStage::Translation => 2,
    }]);
    hasher.update(now_ms.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    format!("lease-{:x}", hasher.finalize())
}

fn scheduler_coordinator(run_dir: &Path) -> Result<Arc<Mutex<()>>, PdfV3SchedulerError> {
    let mut coordinators = SCHEDULER_COORDINATORS
        .lock()
        .map_err(|_| PdfV3SchedulerError::LockPoisoned)?;
    coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
    if let Some(coordinator) = coordinators.get(run_dir).and_then(Weak::upgrade) {
        return Ok(coordinator);
    }
    let coordinator = Arc::new(Mutex::new(()));
    coordinators.insert(run_dir.to_path_buf(), Arc::downgrade(&coordinator));
    Ok(coordinator)
}

fn index_candidate_paths(target: &Path) -> Result<Vec<PathBuf>, PdfV3SchedulerError> {
    let parent = target
        .parent()
        .ok_or_else(|| PdfV3SchedulerError::Corrupt("index has no parent".to_string()))?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PdfV3SchedulerError::Corrupt("index name is invalid".to_string()))?;
    let entries = fs::read_dir(parent).map_err(|error| io_error("list", parent, error))?;
    let sidecar_prefix = format!(".{target_name}.");
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read", parent, error))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_file()
            && (path == target
                || (name.starts_with(&sidecar_prefix)
                    && (name.ends_with(".tmp") || name.ends_with(".bak"))))
        {
            candidates.push(path);
        }
    }
    candidates.sort();
    Ok(candidates)
}

fn cleanup_index_sidecars(target: &Path, candidates: &[PathBuf]) {
    for candidate in candidates {
        if candidate != target {
            let _ = fs::remove_file(candidate);
        }
    }
}

fn write_json_atomic<T: Serialize>(target: &Path, value: &T) -> Result<(), PdfV3SchedulerError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| PdfV3SchedulerError::Corrupt(format!("encode failed: {error}")))?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_INDEX_BYTES {
        return Err(PdfV3SchedulerError::IndexTooLarge {
            bytes: byte_count,
            maximum: MAX_INDEX_BYTES,
        });
    }
    let temp = unique_sidecar_path(target, "tmp");
    let backup = unique_sidecar_path(target, "bak");
    write_new_synced_file(&temp, &bytes)?;
    let had_target = target.exists();
    if had_target {
        if let Err(error) = fs::rename(target, &backup) {
            let _ = fs::remove_file(&temp);
            return Err(io_error("backup", target, error));
        }
    }
    if let Err(error) = fs::rename(&temp, target) {
        if had_target && !target.exists() {
            let _ = fs::rename(&backup, target);
        }
        let _ = fs::remove_file(&temp);
        return Err(io_error("commit", target, error));
    }
    if let Some(parent) = target.parent() {
        sync_parent_directory(parent)?;
    }
    if had_target {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, PdfV3SchedulerError> {
    let metadata = fs::metadata(path).map_err(|error| io_error("inspect", path, error))?;
    if metadata.len() > maximum {
        return Err(PdfV3SchedulerError::IndexTooLarge {
            bytes: metadata.len(),
            maximum,
        });
    }
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| PdfV3SchedulerError::IndexTooLarge {
            bytes: metadata.len(),
            maximum,
        })?;
    let file = File::open(path).map_err(|error| io_error("open", path, error))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read", path, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(PdfV3SchedulerError::IndexTooLarge {
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(bytes)
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), PdfV3SchedulerError> {
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

fn unique_sidecar_path(target: &Path, extension: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scheduler");
    target.with_file_name(format!(
        ".{name}.{}.{}.{extension}",
        std::process::id(),
        counter
    ))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), PdfV3SchedulerError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync", path, error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), PdfV3SchedulerError> {
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, error: std::io::Error) -> PdfV3SchedulerError {
    PdfV3SchedulerError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        DurablePdfV3Scheduler, PdfV3ExtractionAuthority, PdfV3PageState, PdfV3PatchAuthority,
        PdfV3RecoveryInventory, PdfV3RunSpec, PdfV3RunState, PdfV3SchedulerCapacity,
        PdfV3SchedulerError, PdfV3SchedulerManifest, PdfV3SchedulerShard, PdfV3SchedulerSummary,
        PdfV3TranslationCommit, MAX_STATUS_WINDOW, PAGES_PER_SHARD,
    };
    use crate::pdf_v3::{
        page_set::PageSet,
        types::{PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
    };

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct TempRun {
        path: PathBuf,
    }

    impl TempRun {
        fn new(name: &str) -> Self {
            let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-scheduler-{name}-{}-{counter}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            Self { path }
        }
    }

    impl Drop for TempRun {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn spec(page_count: u32) -> PdfV3RunSpec {
        PdfV3RunSpec {
            run_id: format!("run-{page_count}"),
            source_fingerprint: "sha256:source".to_string(),
            source_page_count: page_count,
            requested_pages: PageSet::all(page_count).expect("page set"),
            source_language: "en".to_string(),
            target_language: "zh-CN".to_string(),
            engine_version: "pdf-v3-test".to_string(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
            renderer_version: "renderer-test".to_string(),
        }
    }

    fn capacity() -> PdfV3SchedulerCapacity {
        PdfV3SchedulerCapacity {
            max_extracting_pages: 3,
            max_extracted_pages: 5,
            max_translating_pages: 2,
        }
    }

    fn extraction(page_number: u32) -> PdfV3ExtractionAuthority {
        PdfV3ExtractionAuthority {
            artifact_id: format!("page-graph-{page_number}"),
            source_page_hash: format!("sha256:page-{page_number}"),
        }
    }

    fn patch(page_number: u32) -> PdfV3PatchAuthority {
        PdfV3PatchAuthority {
            patch_id: format!("patch-{page_number}"),
            translation_revision: 1,
        }
    }

    fn create(run: &TempRun, page_count: u32) -> DurablePdfV3Scheduler {
        DurablePdfV3Scheduler::create(&run.path, spec(page_count), capacity(), "owner-a", 1)
            .expect("create scheduler")
    }

    #[test]
    fn thousand_page_scheduler_enforces_capacity_without_chunk_semantics() {
        let run = TempRun::new("capacity");
        let scheduler = create(&run, 1_000);

        let shard_paths = fs::read_dir(&run.path)
            .expect("list run")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("shard-") && name.ends_with(".json"))
            })
            .collect::<Vec<_>>();
        assert_eq!(shard_paths.len(), 16);
        for path in shard_paths {
            let shard: PdfV3SchedulerShard =
                serde_json::from_slice(&fs::read(path).expect("read shard")).expect("decode shard");
            assert!(shard.records.len() <= PAGES_PER_SHARD as usize);
        }

        let first = scheduler
            .claim_extraction("owner-a", 100, 10)
            .expect("claim extraction");
        assert_eq!(
            first
                .iter()
                .map(|claim| claim.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(scheduler
            .claim_extraction("owner-a", 100, 11)
            .expect("capacity blocks extraction")
            .is_empty());
        for claim in &first {
            scheduler
                .commit_extraction("owner-a", claim, extraction(claim.page_number), 20)
                .expect("commit extraction");
        }

        let second = scheduler
            .claim_extraction("owner-a", 100, 21)
            .expect("claim backlog remainder");
        assert_eq!(second.len(), 2);
        assert!(scheduler
            .claim_extraction("owner-a", 100, 22)
            .expect("backpressure blocks extraction")
            .is_empty());
        let translations = scheduler
            .claim_translation("owner-a", 100, 23)
            .expect("claim translation");
        assert_eq!(translations.len(), 2);
        assert!(scheduler
            .claim_translation("owner-a", 100, 24)
            .expect("translation capacity blocks claims")
            .is_empty());

        let (_, summary) = scheduler.manifest_snapshot().expect("snapshot");
        assert_eq!(summary.requested_pages, 1_000);
        assert_eq!(summary.extracting_pages, 2);
        assert_eq!(summary.extracted_pages, 1);
        assert_eq!(summary.translating_pages, 2);
        assert!(matches!(
            scheduler.page_window(None, MAX_STATUS_WINDOW + 1),
            Err(PdfV3SchedulerError::InvalidLimit { .. })
        ));
    }

    #[test]
    fn restart_uses_valid_artifact_authority_and_only_releases_missing_work() {
        let run = TempRun::new("recovery");
        let scheduler = create(&run, 1_000);
        let claims = scheduler
            .claim_extraction("owner-a", 3, 10)
            .expect("claim extraction");
        assert!(matches!(
            scheduler.recover_stale_owner("owner-b", 20, 10, &PdfV3RecoveryInventory::default()),
            Err(PdfV3SchedulerError::OwnerLeaseActive)
        ));

        let mut inventory = PdfV3RecoveryInventory::default();
        inventory.extractions.insert(1, extraction(1));
        inventory.extractions.insert(2, extraction(2));
        inventory.patches.insert(1, patch(1));
        let report = scheduler
            .recover_stale_owner("owner-b", 100, 50, &inventory)
            .expect("recover stale owner");
        assert_eq!(report.released_extraction_leases, claims.len() as u32);
        assert_eq!(report.promoted_extractions, 2);
        assert_eq!(report.promoted_patches, 1);

        drop(scheduler);
        let reopened = DurablePdfV3Scheduler::open(&run.path).expect("reopen scheduler");
        let records = reopened.page_window(None, 3).expect("status window");
        assert!(matches!(records[0].state, PdfV3PageState::Completed { .. }));
        assert!(matches!(records[1].state, PdfV3PageState::Extracted { .. }));
        assert!(matches!(records[2].state, PdfV3PageState::Pending));
        assert!(records.iter().all(|record| record.lease.is_none()));

        let translation = reopened
            .claim_translation("owner-b", 10, 101)
            .expect("resume translation");
        assert_eq!(translation.len(), 1);
        assert_eq!(translation[0].page_number, 2);
        let extraction = reopened
            .claim_extraction("owner-b", 10, 102)
            .expect("resume missing extraction");
        assert!(extraction.iter().all(|claim| claim.page_number > 2));
    }

    #[test]
    fn completed_and_preserved_pages_finish_the_run() {
        let run = TempRun::new("complete");
        let scheduler = create(&run, 2);
        let extraction_claims = scheduler
            .claim_extraction("owner-a", 2, 10)
            .expect("claim extraction");
        for claim in &extraction_claims {
            scheduler
                .commit_extraction("owner-a", claim, extraction(claim.page_number), 20)
                .expect("commit extraction");
        }
        let translation_claims = scheduler
            .claim_translation("owner-a", 2, 30)
            .expect("claim translation");
        scheduler
            .commit_translation(
                "owner-a",
                &translation_claims[0],
                PdfV3TranslationCommit::Patch(patch(1)),
                40,
            )
            .expect("commit patch");
        scheduler
            .commit_translation(
                "owner-a",
                &translation_claims[1],
                PdfV3TranslationCommit::Preserved {
                    reason_code: "unsupported-complex-page".to_string(),
                },
                41,
            )
            .expect("commit preservation");

        let (state, summary) = scheduler.manifest_snapshot().expect("snapshot");
        assert_eq!(state, PdfV3RunState::Completed);
        assert_eq!(summary.completed_pages, 1);
        assert_eq!(summary.preserved_pages, 1);
    }

    #[test]
    fn pause_retry_and_cancellation_keep_transitions_explicit() {
        let run = TempRun::new("control");
        let scheduler = create(&run, 1);
        scheduler.pause("owner-a", 2).expect("pause");
        assert!(matches!(
            scheduler.claim_extraction("owner-a", 1, 3),
            Err(PdfV3SchedulerError::RunNotClaimable(PdfV3RunState::Paused))
        ));
        scheduler.resume("owner-a", 4).expect("resume");
        let first = scheduler
            .claim_extraction("owner-a", 1, 5)
            .expect("claim")
            .remove(0);
        scheduler
            .fail_claim("owner-a", &first, "extract-transient", true, 6)
            .expect("fail retryable claim");
        scheduler
            .retry_failed("owner-a", 1, 7)
            .expect("retry failure");
        let second = scheduler
            .claim_extraction("owner-a", 1, 8)
            .expect("reclaim")
            .remove(0);
        scheduler
            .fail_claim("owner-a", &second, "extract-permanent", false, 9)
            .expect("fail permanent claim");
        assert!(matches!(
            scheduler.retry_failed("owner-a", 1, 10),
            Err(PdfV3SchedulerError::InvalidTransition { page_number: 1 })
        ));
        scheduler
            .request_cancel("owner-a", 11, "user-requested")
            .expect("request cancellation");
        scheduler
            .finish_cancellation("owner-a", 12)
            .expect("finish cancellation");
        assert_eq!(
            scheduler.manifest_snapshot().expect("snapshot").0,
            PdfV3RunState::Cancelled
        );
        assert!(matches!(
            scheduler.retry_failed("owner-a", 1, 13),
            Err(PdfV3SchedulerError::RunNotRetryable(
                PdfV3RunState::Cancelled
            ))
        ));
    }

    #[test]
    fn open_rebuilds_a_stale_manifest_summary_from_page_shards() {
        let run = TempRun::new("summary-repair");
        let scheduler = create(&run, 2);
        let claim = scheduler
            .claim_extraction("owner-a", 1, 10)
            .expect("claim")
            .remove(0);
        scheduler
            .commit_extraction("owner-a", &claim, extraction(1), 20)
            .expect("commit extraction");
        drop(scheduler);

        let manifest_path = run.path.join("manifest.json");
        let mut manifest: PdfV3SchedulerManifest =
            serde_json::from_slice(&fs::read(&manifest_path).expect("read manifest"))
                .expect("decode manifest");
        manifest.summary = PdfV3SchedulerSummary {
            requested_pages: 2,
            pending_pages: 2,
            ..PdfV3SchedulerSummary::default()
        };
        manifest.generation += 1;
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("encode manifest"),
        )
        .expect("write stale manifest");

        let reopened = DurablePdfV3Scheduler::open(&run.path).expect("reopen scheduler");
        let (_, summary) = reopened.manifest_snapshot().expect("snapshot");
        assert_eq!(summary.pending_pages, 1);
        assert_eq!(summary.extracted_pages, 1);
    }

    #[test]
    fn open_promotes_synced_manifest_and_shard_backups() {
        let run = TempRun::new("sidecar");
        let scheduler = create(&run, 2);
        drop(scheduler);
        move_to_backup(&run.path.join("manifest.json"));
        move_to_backup(&run.path.join("shard-00000000.json"));

        let reopened = DurablePdfV3Scheduler::open(&run.path).expect("recover sidecars");
        assert_eq!(reopened.page_window(None, 10).expect("window").len(), 2);
        assert!(run.path.join("manifest.json").is_file());
        assert!(run.path.join("shard-00000000.json").is_file());
    }

    fn move_to_backup(path: &Path) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name");
        let backup = path.with_file_name(format!(".{name}.crash.bak"));
        fs::rename(path, backup).expect("move to backup");
    }
}
