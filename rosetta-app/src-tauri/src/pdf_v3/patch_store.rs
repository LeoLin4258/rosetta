use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    translation_patch::{
        decode_and_validate_translation_patch, decode_and_validate_translation_patch_identity,
        encode_translation_patch, ensure_translation_patch_renderer_resolved,
        validate_translation_patch, TranslationPatchError,
    },
    types::{PageGraph, TranslationPatch},
};

const PATCH_STORE_SCHEMA_VERSION: u32 = 2;
const MANIFEST_FILENAME: &str = "manifest.json";
const PAGES_PER_SHARD: u32 = 64;
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_PATCH_BYTES: u64 = 16 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static STORE_COORDINATORS: Lazy<Mutex<BTreeMap<PathBuf, Weak<StoreCoordinator>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Default)]
struct StoreCoordinatorState {
    repaired: bool,
}

#[derive(Debug, Default)]
struct StoreCoordinator {
    state: Mutex<StoreCoordinatorState>,
}

#[derive(Debug, Clone)]
pub(crate) struct TranslationPatchStore {
    language_dir: PathBuf,
    source_fingerprint: String,
    target_language: String,
    coordinator: Arc<StoreCoordinator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchStoreManifest {
    pub schema_version: u32,
    pub manifest_id: String,
    pub source_fingerprint: String,
    pub target_language: String,
    pub pages_per_shard: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranslationPatchManifestShard {
    schema_version: u32,
    shard_id: String,
    source_fingerprint: String,
    target_language: String,
    shard_index: u32,
    generation: u64,
    pages: Vec<TranslationPatchManifestPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchManifestPage {
    pub page_number: u32,
    pub source_page_hash: String,
    pub translation_revision: u64,
    pub patch_id: String,
    pub patch_file: String,
    pub patch_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPatchStoreSnapshot {
    pub manifest_id: String,
    pub source_fingerprint: String,
    pub target_language: String,
    pub shard_count: u32,
    pub pages: Vec<TranslationPatchManifestPage>,
    pub patch_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranslationPatchCommitKind {
    Written,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPatchCommitOutcome {
    pub kind: TranslationPatchCommitKind,
    pub page_number: u32,
    pub translation_revision: u64,
    pub shard_generation: u64,
    pub patch_bytes: u64,
    pub uncompressed_patch_bytes: u64,
    pub cleanup: TranslationPatchStoreRepairReport,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredTranslationPatch {
    pub patch: TranslationPatch,
    pub shard_generation: u64,
    pub patch_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TranslationPatchStoreRepairReport {
    pub promoted_manifest: bool,
    pub promoted_shards: u64,
    pub dropped_invalid_shards: u64,
    pub dropped_invalid_pages: u64,
    pub removed_index_candidates: u64,
    pub removed_patch_temps: u64,
    pub removed_orphan_patches: u64,
    pub cleanup_failures: u64,
}

impl TranslationPatchStoreRepairReport {
    fn merge(&mut self, other: Self) {
        self.promoted_manifest |= other.promoted_manifest;
        self.promoted_shards += other.promoted_shards;
        self.dropped_invalid_shards += other.dropped_invalid_shards;
        self.dropped_invalid_pages += other.dropped_invalid_pages;
        self.removed_index_candidates += other.removed_index_candidates;
        self.removed_patch_temps += other.removed_patch_temps;
        self.removed_orphan_patches += other.removed_orphan_patches;
        self.cleanup_failures += other.cleanup_failures;
    }
}

#[derive(Debug)]
pub(crate) enum TranslationPatchStoreError {
    InvalidIdentity(&'static str),
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
    IndexInvalid(String),
    ManifestRecoveryFailed,
    ShardGenerationOverflow,
    TargetLanguageMismatch,
    SourcePageConflict {
        page_number: u32,
    },
    StaleRevision {
        page_number: u32,
        current: u64,
        incoming: u64,
    },
    RevisionConflict {
        page_number: u32,
        revision: u64,
    },
    PatchFileMismatch {
        page_number: u32,
    },
    PatchCompression(String),
    PatchTooLarge {
        bytes: u64,
        maximum: u64,
    },
    Patch(TranslationPatchError),
}

impl fmt::Display for TranslationPatchStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(field) => {
                write!(formatter, "TranslationPatch store identity {field} is invalid")
            }
            Self::LockPoisoned => formatter.write_str("TranslationPatch store lock is poisoned"),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} TranslationPatch store path {}: {message}",
                path.display()
            ),
            Self::IndexTooLarge { bytes, maximum } => write!(
                formatter,
                "TranslationPatch index has {bytes} bytes, above maximum {maximum}"
            ),
            Self::IndexInvalid(message) => {
                write!(formatter, "TranslationPatch index is invalid: {message}")
            }
            Self::ManifestRecoveryFailed => formatter.write_str(
                "TranslationPatch manifest recovery found state but no valid candidate",
            ),
            Self::ShardGenerationOverflow => {
                formatter.write_str("TranslationPatch shard generation overflow")
            }
            Self::TargetLanguageMismatch => formatter.write_str(
                "TranslationPatch target language does not match the patch store",
            ),
            Self::SourcePageConflict { page_number } => write!(
                formatter,
                "TranslationPatch page {page_number} changed source identity inside one store"
            ),
            Self::StaleRevision {
                page_number,
                current,
                incoming,
            } => write!(
                formatter,
                "TranslationPatch page {page_number} revision {incoming} is older than current revision {current}"
            ),
            Self::RevisionConflict {
                page_number,
                revision,
            } => write!(
                formatter,
                "TranslationPatch page {page_number} revision {revision} has conflicting content"
            ),
            Self::PatchFileMismatch { page_number } => write!(
                formatter,
                "TranslationPatch page {page_number} file does not match its index entry"
            ),
            Self::PatchCompression(message) => formatter.write_str(message),
            Self::PatchTooLarge { bytes, maximum } => write!(
                formatter,
                "TranslationPatch size {bytes} exceeds maximum {maximum}"
            ),
            Self::Patch(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranslationPatchStoreError {}

impl From<TranslationPatchError> for TranslationPatchStoreError {
    fn from(error: TranslationPatchError) -> Self {
        Self::Patch(error)
    }
}

impl TranslationPatchStore {
    pub(crate) fn new(
        translations_root: &Path,
        source_fingerprint: impl Into<String>,
        target_language: impl Into<String>,
    ) -> Result<Self, TranslationPatchStoreError> {
        if !translations_root.is_absolute() {
            return Err(TranslationPatchStoreError::InvalidIdentity(
                "translationsRoot",
            ));
        }
        let source_fingerprint = source_fingerprint.into();
        let target_language = target_language.into();
        validate_identity(&source_fingerprint, "sourceFingerprint")?;
        validate_identity(&target_language, "targetLanguage")?;
        let language_dir = translations_root.join(language_directory_name(&target_language));
        let coordinator = store_coordinator(&language_dir)?;
        Ok(Self {
            language_dir,
            source_fingerprint,
            target_language,
            coordinator,
        })
    }

    pub(crate) fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub(crate) fn target_language(&self) -> &str {
        &self.target_language
    }

    #[cfg(test)]
    fn language_dir(&self) -> &Path {
        &self.language_dir
    }

    pub(crate) fn commit(
        &self,
        page: &PageGraph,
        patch: &TranslationPatch,
    ) -> Result<TranslationPatchCommitOutcome, TranslationPatchStoreError> {
        if patch.target_language != self.target_language {
            return Err(TranslationPatchStoreError::TargetLanguageMismatch);
        }
        validate_translation_patch(page, patch)?;
        ensure_translation_patch_renderer_resolved(patch)?;
        let encoded_patch = encode_translation_patch(patch)?;
        let uncompressed_patch_byte_count = u64::try_from(encoded_patch.len()).unwrap_or(u64::MAX);
        let patch_bytes = compress_patch(&encoded_patch)?;
        let patch_byte_count = u64::try_from(patch_bytes.len()).map_err(|_| {
            TranslationPatchStoreError::PatchFileMismatch {
                page_number: patch.page_number,
            }
        })?;

        self.with_lock(|state| {
            let mut cleanup = self.prepare_locked(state)?;
            let shard_index = shard_index(patch.page_number);
            let mut shard = self.read_canonical_shard(shard_index)?;
            let mut previous_patch_file = shard.as_ref().and_then(|shard| {
                shard
                    .pages
                    .iter()
                    .find(|entry| entry.page_number == patch.page_number)
                    .map(|entry| entry.patch_file.clone())
            });
            let mut repair_idempotent_patch = false;

            if let Some(current) = shard.as_ref().and_then(|shard| {
                shard
                    .pages
                    .iter()
                    .find(|entry| entry.page_number == patch.page_number)
            }) {
                if current.source_page_hash != patch.source_page_hash {
                    return Err(TranslationPatchStoreError::SourcePageConflict {
                        page_number: patch.page_number,
                    });
                }
                if patch.translation_revision < current.translation_revision {
                    return Err(TranslationPatchStoreError::StaleRevision {
                        page_number: patch.page_number,
                        current: current.translation_revision,
                        incoming: patch.translation_revision,
                    });
                }
                if patch.translation_revision == current.translation_revision {
                    if patch.patch_id != current.patch_id {
                        return Err(TranslationPatchStoreError::RevisionConflict {
                            page_number: patch.page_number,
                            revision: patch.translation_revision,
                        });
                    }
                    match self.load_entry(page, current) {
                        Ok(_) => {
                            return Ok(TranslationPatchCommitOutcome {
                                kind: TranslationPatchCommitKind::Unchanged,
                                page_number: patch.page_number,
                                translation_revision: patch.translation_revision,
                                shard_generation: shard
                                    .as_ref()
                                    .map(|shard| shard.generation)
                                    .unwrap_or(0),
                                patch_bytes: current.patch_bytes,
                                uncompressed_patch_bytes: uncompressed_patch_byte_count,
                                cleanup,
                            });
                        }
                        Err(error @ TranslationPatchStoreError::Io { .. }) => return Err(error),
                        Err(_) => {}
                    }
                    repair_idempotent_patch = true;
                }
            }

            if repair_idempotent_patch {
                state.repaired = false;
                let repair = self.repair_locked()?;
                state.repaired = true;
                cleanup.merge(repair);
                shard = self.read_canonical_shard(shard_index)?;
                previous_patch_file = None;
            }

            let patch_file = patch_filename(patch);
            self.write_immutable_patch(&patch_file, &patch_bytes, patch.page_number)?;
            let next_generation = shard
                .as_ref()
                .map(|shard| shard.generation)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(TranslationPatchStoreError::ShardGenerationOverflow)?;
            let mut next = shard.unwrap_or_else(|| self.empty_shard(shard_index, next_generation));
            next.generation = next_generation;
            next.pages
                .retain(|entry| entry.page_number != patch.page_number);
            next.pages.push(TranslationPatchManifestPage {
                page_number: patch.page_number,
                source_page_hash: patch.source_page_hash.clone(),
                translation_revision: patch.translation_revision,
                patch_id: patch.patch_id.clone(),
                patch_file,
                patch_bytes: patch_byte_count,
            });
            next.pages.sort_by_key(|entry| entry.page_number);
            next.shard_id = shard_id(&next)?;
            if let Err(error) = self.write_shard_atomic(&next) {
                state.repaired = false;
                return Err(error);
            }
            if let Some(previous) = previous_patch_file.filter(|previous| {
                next.pages
                    .iter()
                    .find(|entry| entry.page_number == patch.page_number)
                    .is_some_and(|entry| entry.patch_file.as_str() != previous.as_str())
            }) {
                match fs::remove_file(self.language_dir.join(previous)) {
                    Ok(_) => cleanup.removed_orphan_patches += 1,
                    Err(_) => cleanup.cleanup_failures += 1,
                }
            }

            Ok(TranslationPatchCommitOutcome {
                kind: TranslationPatchCommitKind::Written,
                page_number: patch.page_number,
                translation_revision: patch.translation_revision,
                shard_generation: next.generation,
                patch_bytes: patch_byte_count,
                uncompressed_patch_bytes: uncompressed_patch_byte_count,
                cleanup,
            })
        })
    }

    pub(crate) fn load(
        &self,
        page: &PageGraph,
    ) -> Result<Option<StoredTranslationPatch>, TranslationPatchStoreError> {
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            match self.load_from_shard(
                page,
                self.read_canonical_shard(shard_index(page.page_number))?,
            ) {
                Ok(stored) => Ok(stored),
                Err(error @ TranslationPatchStoreError::Io { .. }) => Err(error),
                Err(_) => {
                    state.repaired = false;
                    self.repair_locked()?;
                    state.repaired = true;
                    self.load_from_shard(
                        page,
                        self.read_canonical_shard(shard_index(page.page_number))?,
                    )
                }
            }
        })
    }

    pub(crate) fn snapshot(
        &self,
    ) -> Result<TranslationPatchStoreSnapshot, TranslationPatchStoreError> {
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            let manifest = self.read_manifest()?;
            let mut pages = Vec::new();
            let mut shard_count = 0u32;
            for path in self.canonical_shard_paths()? {
                let index = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(parse_shard_candidate_name)
                    .ok_or_else(|| {
                        TranslationPatchStoreError::IndexInvalid(format!(
                            "invalid shard filename {}",
                            path.display()
                        ))
                    })?;
                let shard = self.read_shard_candidate(&path, index)?;
                shard_count = shard_count.saturating_add(1);
                pages.extend(shard.pages);
            }
            pages.sort_by_key(|entry| entry.page_number);
            let patch_bytes = pages
                .iter()
                .fold(0u64, |total, page| total.saturating_add(page.patch_bytes));
            Ok(TranslationPatchStoreSnapshot {
                manifest_id: manifest.manifest_id,
                source_fingerprint: manifest.source_fingerprint,
                target_language: manifest.target_language,
                shard_count,
                pages,
                patch_bytes,
            })
        })
    }

    pub(crate) fn repair(
        &self,
    ) -> Result<TranslationPatchStoreRepairReport, TranslationPatchStoreError> {
        self.with_lock(|state| {
            let result = self.repair_locked();
            if result.is_ok() {
                state.repaired = true;
            }
            result
        })
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&mut StoreCoordinatorState) -> Result<T, TranslationPatchStoreError>,
    ) -> Result<T, TranslationPatchStoreError> {
        let mut state = self
            .coordinator
            .state
            .lock()
            .map_err(|_| TranslationPatchStoreError::LockPoisoned)?;
        operation(&mut state)
    }

    fn prepare_locked(
        &self,
        state: &mut StoreCoordinatorState,
    ) -> Result<TranslationPatchStoreRepairReport, TranslationPatchStoreError> {
        if state.repaired {
            return Ok(TranslationPatchStoreRepairReport::default());
        }
        let report = self.repair_locked()?;
        state.repaired = true;
        Ok(report)
    }

    fn repair_locked(
        &self,
    ) -> Result<TranslationPatchStoreRepairReport, TranslationPatchStoreError> {
        fs::create_dir_all(&self.language_dir)
            .map_err(|error| io_error("create", &self.language_dir, error))?;
        let mut report = self.recover_manifest()?;
        let groups = self.shard_candidate_groups()?;
        let mut referenced = BTreeSet::new();
        for (index, candidates) in groups {
            if let Some(shard) = self.recover_shard(index, &candidates, &mut report)? {
                referenced.extend(shard.pages.into_iter().map(|page| page.patch_file));
            }
        }
        report.merge(self.cleanup_patch_files(&referenced));
        Ok(report)
    }

    fn recover_manifest(
        &self,
    ) -> Result<TranslationPatchStoreRepairReport, TranslationPatchStoreError> {
        let target = self.language_dir.join(MANIFEST_FILENAME);
        let candidates = self.manifest_candidate_paths()?;
        let mut valid = Vec::new();
        for path in &candidates {
            match self.read_manifest_candidate(path) {
                Ok(manifest) => valid.push((path.clone(), manifest)),
                Err(error @ TranslationPatchStoreError::Io { .. }) => return Err(error),
                Err(_) => {}
            }
        }
        let expected = self.expected_manifest()?;
        let mut report = TranslationPatchStoreRepairReport::default();
        if let Some((candidate, _)) = valid.into_iter().max_by_key(|(path, _)| path == &target) {
            if candidate != target {
                self.write_manifest_atomic(&expected)?;
                report.promoted_manifest = true;
            }
        } else if candidates.is_empty() {
            self.write_manifest_atomic(&expected)?;
        } else {
            return Err(TranslationPatchStoreError::ManifestRecoveryFailed);
        }
        for path in self.manifest_candidate_paths()? {
            if path != target {
                remove_for_repair(&path, &mut report);
            }
        }
        Ok(report)
    }

    fn recover_shard(
        &self,
        index: u32,
        candidates: &[PathBuf],
        report: &mut TranslationPatchStoreRepairReport,
    ) -> Result<Option<TranslationPatchManifestShard>, TranslationPatchStoreError> {
        let target = self.language_dir.join(shard_filename(index));
        let mut valid = Vec::new();
        for path in candidates {
            match self.read_shard_candidate(path, index) {
                Ok(shard) => valid.push((path.clone(), shard)),
                Err(error @ TranslationPatchStoreError::Io { .. }) => return Err(error),
                Err(_) => {}
            }
        }
        let Some((candidate, mut shard)) = valid.into_iter().max_by(|left, right| {
            left.1
                .generation
                .cmp(&right.1.generation)
                .then_with(|| (left.0 == target).cmp(&(right.0 == target)))
                .then_with(|| left.1.shard_id.cmp(&right.1.shard_id))
        }) else {
            report.dropped_invalid_shards += 1;
            for path in candidates {
                remove_for_repair(path, report);
            }
            return Ok(None);
        };

        let original_count = shard.pages.len();
        let mut valid_pages = Vec::with_capacity(original_count);
        for entry in shard.pages {
            if self.patch_entry_file_is_valid(&entry)? {
                valid_pages.push(entry);
            }
        }
        shard.pages = valid_pages;
        let dropped = original_count.saturating_sub(shard.pages.len());
        report.dropped_invalid_pages += u64::try_from(dropped).unwrap_or(u64::MAX);
        if shard.pages.is_empty() {
            for path in candidates {
                remove_for_repair(path, report);
            }
            return Ok(None);
        }
        if dropped > 0 {
            shard.generation = shard
                .generation
                .checked_add(1)
                .ok_or(TranslationPatchStoreError::ShardGenerationOverflow)?;
            shard.shard_id = shard_id(&shard)?;
        }
        if candidate != target || dropped > 0 {
            self.write_shard_atomic(&shard)?;
            if candidate != target {
                report.promoted_shards += 1;
            }
        }
        for path in candidates {
            if path != &target {
                remove_for_repair(path, report);
            }
        }
        Ok(Some(shard))
    }

    fn expected_manifest(
        &self,
    ) -> Result<TranslationPatchStoreManifest, TranslationPatchStoreError> {
        let mut manifest = TranslationPatchStoreManifest {
            schema_version: PATCH_STORE_SCHEMA_VERSION,
            manifest_id: String::new(),
            source_fingerprint: self.source_fingerprint.clone(),
            target_language: self.target_language.clone(),
            pages_per_shard: PAGES_PER_SHARD,
        };
        manifest.manifest_id = manifest_id(&manifest)?;
        Ok(manifest)
    }

    fn empty_shard(&self, index: u32, generation: u64) -> TranslationPatchManifestShard {
        TranslationPatchManifestShard {
            schema_version: PATCH_STORE_SCHEMA_VERSION,
            shard_id: String::new(),
            source_fingerprint: self.source_fingerprint.clone(),
            target_language: self.target_language.clone(),
            shard_index: index,
            generation,
            pages: Vec::new(),
        }
    }

    fn read_manifest(&self) -> Result<TranslationPatchStoreManifest, TranslationPatchStoreError> {
        self.read_manifest_candidate(&self.language_dir.join(MANIFEST_FILENAME))
    }

    fn read_manifest_candidate(
        &self,
        path: &Path,
    ) -> Result<TranslationPatchStoreManifest, TranslationPatchStoreError> {
        let bytes = read_limited(path, MAX_INDEX_BYTES)?;
        let manifest = serde_json::from_slice::<TranslationPatchStoreManifest>(&bytes)
            .map_err(|error| index_error(path, error))?;
        if manifest.schema_version != PATCH_STORE_SCHEMA_VERSION
            || manifest.source_fingerprint != self.source_fingerprint
            || manifest.target_language != self.target_language
            || manifest.pages_per_shard != PAGES_PER_SHARD
            || manifest_id(&manifest)? != manifest.manifest_id
        {
            return Err(TranslationPatchStoreError::IndexInvalid(format!(
                "manifest identity mismatch at {}",
                path.display()
            )));
        }
        Ok(manifest)
    }

    fn read_canonical_shard(
        &self,
        index: u32,
    ) -> Result<Option<TranslationPatchManifestShard>, TranslationPatchStoreError> {
        let path = self.language_dir.join(shard_filename(index));
        if !try_exists(&path)? {
            return Ok(None);
        }
        self.read_shard_candidate(&path, index).map(Some)
    }

    fn read_shard_candidate(
        &self,
        path: &Path,
        expected_index: u32,
    ) -> Result<TranslationPatchManifestShard, TranslationPatchStoreError> {
        let bytes = read_limited(path, MAX_INDEX_BYTES)?;
        let shard = serde_json::from_slice::<TranslationPatchManifestShard>(&bytes)
            .map_err(|error| index_error(path, error))?;
        self.validate_shard(&shard)?;
        if shard.shard_index != expected_index {
            return Err(TranslationPatchStoreError::IndexInvalid(format!(
                "shard filename/index mismatch at {}",
                path.display()
            )));
        }
        Ok(shard)
    }

    fn validate_shard(
        &self,
        shard: &TranslationPatchManifestShard,
    ) -> Result<(), TranslationPatchStoreError> {
        if shard.schema_version != PATCH_STORE_SCHEMA_VERSION
            || shard.source_fingerprint != self.source_fingerprint
            || shard.target_language != self.target_language
            || shard.generation == 0
        {
            return Err(TranslationPatchStoreError::IndexInvalid(
                "shard identity mismatch".to_string(),
            ));
        }
        let first_page = shard
            .shard_index
            .checked_mul(PAGES_PER_SHARD)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                TranslationPatchStoreError::IndexInvalid("shard page overflow".to_string())
            })?;
        let last_page = first_page
            .checked_add(PAGES_PER_SHARD - 1)
            .unwrap_or(u32::MAX);
        let mut previous = None;
        for page in &shard.pages {
            if !(first_page..=last_page).contains(&page.page_number)
                || page.translation_revision == 0
                || page.source_page_hash.is_empty()
                || page.patch_bytes == 0
                || page.patch_bytes > MAX_PATCH_BYTES
                || previous.is_some_and(|previous| page.page_number <= previous)
                || !is_safe_file_name(&page.patch_file)
                || page.patch_file != patch_filename_from_entry(page)
            {
                return Err(TranslationPatchStoreError::IndexInvalid(format!(
                    "invalid shard page {}",
                    page.page_number
                )));
            }
            previous = Some(page.page_number);
        }
        if shard_id(shard)? != shard.shard_id {
            return Err(TranslationPatchStoreError::IndexInvalid(
                "shardId mismatch".to_string(),
            ));
        }
        Ok(())
    }

    fn load_from_shard(
        &self,
        page: &PageGraph,
        shard: Option<TranslationPatchManifestShard>,
    ) -> Result<Option<StoredTranslationPatch>, TranslationPatchStoreError> {
        let Some(shard) = shard else {
            return Ok(None);
        };
        let Some(entry) = shard
            .pages
            .iter()
            .find(|entry| entry.page_number == page.page_number)
        else {
            return Ok(None);
        };
        let patch = self.load_entry(page, entry)?;
        Ok(Some(StoredTranslationPatch {
            patch,
            shard_generation: shard.generation,
            patch_bytes: entry.patch_bytes,
        }))
    }

    fn load_entry(
        &self,
        page: &PageGraph,
        entry: &TranslationPatchManifestPage,
    ) -> Result<TranslationPatch, TranslationPatchStoreError> {
        let path = self.language_dir.join(&entry.patch_file);
        if !try_exists(&path)? {
            return Err(TranslationPatchStoreError::PatchFileMismatch {
                page_number: entry.page_number,
            });
        }
        let compressed = read_limited(&path, MAX_PATCH_BYTES)?;
        if u64::try_from(compressed.len()).ok() != Some(entry.patch_bytes) {
            return Err(TranslationPatchStoreError::PatchFileMismatch {
                page_number: entry.page_number,
            });
        }
        let bytes = decompress_patch(&compressed)?;
        let patch = decode_and_validate_translation_patch(page, &bytes)?;
        ensure_translation_patch_renderer_resolved(&patch)?;
        if !entry_matches_patch(entry, &patch, &self.target_language) {
            return Err(TranslationPatchStoreError::PatchFileMismatch {
                page_number: entry.page_number,
            });
        }
        Ok(patch)
    }

    fn patch_entry_file_is_valid(
        &self,
        entry: &TranslationPatchManifestPage,
    ) -> Result<bool, TranslationPatchStoreError> {
        let path = self.language_dir.join(&entry.patch_file);
        if !try_exists(&path)? {
            return Ok(false);
        }
        let compressed = match read_limited(&path, MAX_PATCH_BYTES) {
            Ok(bytes) => bytes,
            Err(error @ TranslationPatchStoreError::Io { .. }) => return Err(error),
            Err(_) => return Ok(false),
        };
        if u64::try_from(compressed.len()).ok() != Some(entry.patch_bytes) {
            return Ok(false);
        }
        Ok(decompress_patch(&compressed)
            .and_then(|patch| {
                let patch = decode_and_validate_translation_patch_identity(&patch)?;
                ensure_translation_patch_renderer_resolved(&patch)?;
                Ok(patch)
            })
            .map(|patch| entry_matches_patch(entry, &patch, &self.target_language))
            .unwrap_or(false))
    }

    fn write_immutable_patch(
        &self,
        filename: &str,
        bytes: &[u8],
        page_number: u32,
    ) -> Result<(), TranslationPatchStoreError> {
        let target = self.language_dir.join(filename);
        if try_exists(&target)? {
            let existing = read_limited(&target, MAX_PATCH_BYTES)?;
            if existing == bytes {
                return Ok(());
            }
            return Err(TranslationPatchStoreError::PatchFileMismatch { page_number });
        }
        let temp = unique_sidecar_path(&target, "tmp");
        write_new_synced_file(&temp, bytes)?;
        match fs::rename(&temp, &target) {
            Ok(_) => {
                sync_parent_directory(&self.language_dir)?;
                Ok(())
            }
            Err(error) => {
                if try_exists(&target)? {
                    let _ = fs::remove_file(&temp);
                    let existing = read_limited(&target, MAX_PATCH_BYTES)?;
                    return if existing == bytes {
                        Ok(())
                    } else {
                        Err(TranslationPatchStoreError::PatchFileMismatch { page_number })
                    };
                }
                let _ = fs::remove_file(&temp);
                Err(io_error("commit", &target, error))
            }
        }
    }

    fn write_manifest_atomic(
        &self,
        manifest: &TranslationPatchStoreManifest,
    ) -> Result<(), TranslationPatchStoreError> {
        let bytes = encode_index(manifest)?;
        write_index_atomic(&self.language_dir.join(MANIFEST_FILENAME), &bytes)
    }

    fn write_shard_atomic(
        &self,
        shard: &TranslationPatchManifestShard,
    ) -> Result<(), TranslationPatchStoreError> {
        self.validate_shard(shard)?;
        let bytes = encode_index(shard)?;
        write_index_atomic(
            &self.language_dir.join(shard_filename(shard.shard_index)),
            &bytes,
        )
    }

    fn manifest_candidate_paths(&self) -> Result<Vec<PathBuf>, TranslationPatchStoreError> {
        let mut result = Vec::new();
        for path in list_files(&self.language_dir)? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == MANIFEST_FILENAME || is_index_sidecar(name, MANIFEST_FILENAME) {
                result.push(path);
            }
        }
        result.sort();
        Ok(result)
    }

    fn shard_candidate_groups(
        &self,
    ) -> Result<BTreeMap<u32, Vec<PathBuf>>, TranslationPatchStoreError> {
        let mut groups = BTreeMap::<u32, Vec<PathBuf>>::new();
        for path in list_files(&self.language_dir)? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some(index) = parse_shard_candidate_name(name) {
                groups.entry(index).or_default().push(path);
            }
        }
        for paths in groups.values_mut() {
            paths.sort();
        }
        Ok(groups)
    }

    fn canonical_shard_paths(&self) -> Result<Vec<PathBuf>, TranslationPatchStoreError> {
        let mut paths = self
            .shard_candidate_groups()?
            .into_iter()
            .filter_map(|(index, candidates)| {
                let target = self.language_dir.join(shard_filename(index));
                candidates
                    .into_iter()
                    .find(|candidate| candidate == &target)
            })
            .collect::<Vec<_>>();
        paths.sort();
        Ok(paths)
    }

    fn cleanup_patch_files(
        &self,
        referenced: &BTreeSet<String>,
    ) -> TranslationPatchStoreRepairReport {
        let mut report = TranslationPatchStoreRepairReport::default();
        let Ok(paths) = list_files(&self.language_dir) else {
            report.cleanup_failures += 1;
            return report;
        };
        for path in paths {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let kind = if name.starts_with(".page-") && name.ends_with(".tmp") {
                Some(true)
            } else if is_patch_filename(name) && !referenced.contains(name) {
                Some(false)
            } else {
                None
            };
            let Some(is_temp) = kind else {
                continue;
            };
            match fs::remove_file(path) {
                Ok(_) if is_temp => report.removed_patch_temps += 1,
                Ok(_) => report.removed_orphan_patches += 1,
                Err(_) => report.cleanup_failures += 1,
            }
        }
        report
    }
}

fn store_coordinator(path: &Path) -> Result<Arc<StoreCoordinator>, TranslationPatchStoreError> {
    let mut coordinators = STORE_COORDINATORS
        .lock()
        .map_err(|_| TranslationPatchStoreError::LockPoisoned)?;
    coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
    if let Some(coordinator) = coordinators.get(path).and_then(Weak::upgrade) {
        return Ok(coordinator);
    }
    let coordinator = Arc::new(StoreCoordinator::default());
    coordinators.insert(path.to_path_buf(), Arc::downgrade(&coordinator));
    Ok(coordinator)
}

fn validate_identity(value: &str, field: &'static str) -> Result<(), TranslationPatchStoreError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(TranslationPatchStoreError::InvalidIdentity(field));
    }
    Ok(())
}

fn language_directory_name(target_language: &str) -> String {
    format!("language-{}", sha256(target_language.as_bytes()))
}

fn shard_index(page_number: u32) -> u32 {
    (page_number - 1) / PAGES_PER_SHARD
}

fn shard_filename(index: u32) -> String {
    format!("shard-{index:08}.json")
}

fn parse_shard_candidate_name(name: &str) -> Option<u32> {
    let normalized = name.strip_prefix('.').unwrap_or(name);
    let json_end = normalized.find(".json")? + ".json".len();
    let base = &normalized[..json_end];
    let suffix = &normalized[json_end..];
    if !suffix.is_empty()
        && !(suffix.starts_with('.') && (suffix.ends_with(".tmp") || suffix.ends_with(".bak")))
    {
        return None;
    }
    base.strip_prefix("shard-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn is_index_sidecar(name: &str, target_name: &str) -> bool {
    name.starts_with(&format!(".{target_name}."))
        && (name.ends_with(".tmp") || name.ends_with(".bak"))
}

fn patch_filename(patch: &TranslationPatch) -> String {
    format!(
        "page-{:010}-revision-{:020}-{}.patch.json.gz",
        patch.page_number, patch.translation_revision, patch.patch_id
    )
}

fn patch_filename_from_entry(entry: &TranslationPatchManifestPage) -> String {
    format!(
        "page-{:010}-revision-{:020}-{}.patch.json.gz",
        entry.page_number, entry.translation_revision, entry.patch_id
    )
}

fn is_patch_filename(name: &str) -> bool {
    name.starts_with("page-") && name.ends_with(".patch.json.gz")
}

fn compress_patch(bytes: &[u8]) -> Result<Vec<u8>, TranslationPatchStoreError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(bytes)
        .map_err(|error| TranslationPatchStoreError::PatchCompression(error.to_string()))?;
    let compressed = encoder
        .finish()
        .map_err(|error| TranslationPatchStoreError::PatchCompression(error.to_string()))?;
    let byte_count = u64::try_from(compressed.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_PATCH_BYTES {
        return Err(TranslationPatchStoreError::PatchTooLarge {
            bytes: byte_count,
            maximum: MAX_PATCH_BYTES,
        });
    }
    Ok(compressed)
}

fn decompress_patch(bytes: &[u8]) -> Result<Vec<u8>, TranslationPatchStoreError> {
    let decoder = GzDecoder::new(bytes);
    let mut decoded = Vec::new();
    decoder
        .take(MAX_PATCH_BYTES.saturating_add(1))
        .read_to_end(&mut decoded)
        .map_err(|error| TranslationPatchStoreError::PatchCompression(error.to_string()))?;
    let byte_count = u64::try_from(decoded.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_PATCH_BYTES {
        return Err(TranslationPatchStoreError::PatchTooLarge {
            bytes: byte_count,
            maximum: MAX_PATCH_BYTES,
        });
    }
    Ok(decoded)
}

fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
}

fn entry_matches_patch(
    entry: &TranslationPatchManifestPage,
    patch: &TranslationPatch,
    target_language: &str,
) -> bool {
    entry.page_number == patch.page_number
        && entry.source_page_hash == patch.source_page_hash
        && entry.translation_revision == patch.translation_revision
        && entry.patch_id == patch.patch_id
        && entry.patch_file == patch_filename(patch)
        && patch.target_language == target_language
}

fn manifest_id(
    manifest: &TranslationPatchStoreManifest,
) -> Result<String, TranslationPatchStoreError> {
    let mut canonical = manifest.clone();
    canonical.manifest_id.clear();
    Ok(format!("manifest-{}", sha256(&encode_index(&canonical)?)))
}

fn shard_id(shard: &TranslationPatchManifestShard) -> Result<String, TranslationPatchStoreError> {
    let mut canonical = shard.clone();
    canonical.shard_id.clear();
    Ok(format!("shard-{}", sha256(&encode_index(&canonical)?)))
}

fn encode_index<T: Serialize>(value: &T) -> Result<Vec<u8>, TranslationPatchStoreError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        TranslationPatchStoreError::IndexInvalid(format!("encode failed: {error}"))
    })?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_count > MAX_INDEX_BYTES {
        return Err(TranslationPatchStoreError::IndexTooLarge {
            bytes: byte_count,
            maximum: MAX_INDEX_BYTES,
        });
    }
    Ok(bytes)
}

fn write_index_atomic(target: &Path, bytes: &[u8]) -> Result<(), TranslationPatchStoreError> {
    let temp = unique_sidecar_path(target, "tmp");
    let backup = unique_sidecar_path(target, "bak");
    write_new_synced_file(&temp, bytes)?;
    let had_target = try_exists(target)?;
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

fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, TranslationPatchStoreError> {
    let metadata = fs::metadata(path).map_err(|error| io_error("inspect", path, error))?;
    if metadata.len() > maximum {
        return Err(TranslationPatchStoreError::IndexTooLarge {
            bytes: metadata.len(),
            maximum,
        });
    }
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| TranslationPatchStoreError::IndexTooLarge {
            bytes: metadata.len(),
            maximum,
        })?;
    let file = File::open(path).map_err(|error| io_error("open", path, error))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read", path, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(TranslationPatchStoreError::IndexTooLarge {
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(bytes)
}

fn try_exists(path: &Path) -> Result<bool, TranslationPatchStoreError> {
    path.try_exists()
        .map_err(|error| io_error("inspect", path, error))
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), TranslationPatchStoreError> {
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
        .unwrap_or("patch-store");
    target.with_file_name(format!(
        ".{name}.{}.{}.{extension}",
        std::process::id(),
        counter
    ))
}

fn list_files(directory: &Path) -> Result<Vec<PathBuf>, TranslationPatchStoreError> {
    let entries = fs::read_dir(directory).map_err(|error| io_error("list", directory, error))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read", directory, error))?;
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn remove_for_repair(path: &Path, report: &mut TranslationPatchStoreRepairReport) {
    match fs::remove_file(path) {
        Ok(_) => report.removed_index_candidates += 1,
        Err(_) => report.cleanup_failures += 1,
    }
}

fn index_error(path: &Path, error: serde_json::Error) -> TranslationPatchStoreError {
    TranslationPatchStoreError::IndexInvalid(format!(
        "failed to decode {}: {error}",
        path.display()
    ))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), TranslationPatchStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync", path, error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), TranslationPatchStoreError> {
    Ok(())
}

fn io_error(
    operation: &'static str,
    path: &Path,
    error: std::io::Error,
) -> TranslationPatchStoreError {
    TranslationPatchStoreError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Barrier},
        thread,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    use super::{
        decompress_patch, encode_index, is_patch_filename, shard_filename, shard_id,
        TranslationPatchCommitKind, TranslationPatchStore, TranslationPatchStoreError,
    };
    use crate::pdf_v3::{
        translation_patch::{
            build_translation_patch, resolve_translation_patch_renderer_decisions,
            TranslationPatchDraft, TranslationPatchEntryDraft,
        },
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageReconciliationSummary,
            PageStyle, TranslationPatch, TranslationPatchRendererDecision,
            PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    #[test]
    fn commits_and_loads_compact_page_authority() {
        let temp = TestDirectory::new("roundtrip");
        let store = patch_store(temp.path());
        let page = page_graph(1, "secret source page one");
        let patch = patch(&page, 1, "译文一");

        let outcome = store.commit(&page, &patch).expect("commit patch");
        assert_eq!(outcome.kind, TranslationPatchCommitKind::Written);
        assert_eq!(outcome.shard_generation, 1);
        assert!(outcome.patch_bytes < outcome.uncompressed_patch_bytes);
        let loaded = store
            .load(&page)
            .expect("load patch")
            .expect("stored patch");
        assert_eq!(loaded.patch, patch);
        assert_eq!(loaded.shard_generation, 1);
        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.pages.len(), 1);
        assert_eq!(snapshot.shard_count, 1);
        assert_eq!(snapshot.patch_bytes, loaded.patch_bytes);
        assert!(snapshot.manifest_id.starts_with("manifest-"));
        let stored_patch = fs::read(store.language_dir().join(&snapshot.pages[0].patch_file))
            .expect("compressed patch file");
        assert_eq!(stored_patch.get(..2), Some([0x1f, 0x8b].as_slice()));

        let language_name = store
            .language_dir()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("language directory");
        assert!(language_name.starts_with("language-"));
        assert!(!language_name.contains("zh-CN"));
        for entry in fs::read_dir(store.language_dir()).expect("store files") {
            let path = entry.expect("store entry").path();
            if path.is_file() {
                let bytes = fs::read(&path).expect("store file");
                let decoded = if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_patch_filename)
                {
                    decompress_patch(&bytes).expect("compressed patch")
                } else {
                    bytes
                };
                let contents = String::from_utf8(decoded).expect("UTF-8 store payload");
                assert!(!contents.contains("secret source page one"));
            }
        }
    }

    #[test]
    fn rejects_pending_renderer_drafts() {
        let temp = TestDirectory::new("pending-draft");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        let patch = pending_patch(&page, 1, "译文");

        assert!(matches!(
            store.commit(&page, &patch).expect_err("pending patch"),
            TranslationPatchStoreError::Patch(
                crate::pdf_v3::translation_patch::TranslationPatchError::RendererDecisionPending(_)
            )
        ));
        assert!(store.snapshot().expect("snapshot").pages.is_empty());
    }

    #[test]
    fn revision_rules_are_idempotent_and_remove_old_patch_files() {
        let temp = TestDirectory::new("revisions");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        let first = patch(&page, 1, "第一版");
        store.commit(&page, &first).expect("first commit");
        let repeated = store.commit(&page, &first).expect("idempotent commit");
        assert_eq!(repeated.kind, TranslationPatchCommitKind::Unchanged);
        assert_eq!(repeated.shard_generation, 1);

        let conflict = patch(&page, 1, "冲突版本");
        assert!(matches!(
            store
                .commit(&page, &conflict)
                .expect_err("same revision conflict"),
            TranslationPatchStoreError::RevisionConflict { .. }
        ));
        let second = patch(&page, 2, "第二版");
        let outcome = store.commit(&page, &second).expect("second commit");
        assert_eq!(outcome.shard_generation, 2);
        assert!(matches!(
            store.commit(&page, &first).expect_err("stale revision"),
            TranslationPatchStoreError::StaleRevision { .. }
        ));
        let files = patch_files(store.language_dir());
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .file_name()
            .and_then(|name| name.to_str())
            .expect("patch filename")
            .contains("revision-00000000000000000002"));
    }

    #[test]
    fn idempotent_commit_repairs_a_corrupted_current_patch() {
        let temp = TestDirectory::new("repair-current");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        let patch = patch(&page, 1, "译文");
        store.commit(&page, &patch).expect("initial commit");
        let snapshot = store.snapshot().expect("snapshot");
        fs::write(
            store.language_dir().join(&snapshot.pages[0].patch_file),
            b"corrupt",
        )
        .expect("corrupt current patch");

        let outcome = store.commit(&page, &patch).expect("repair commit");
        assert_eq!(outcome.kind, TranslationPatchCommitKind::Written);
        assert_eq!(outcome.cleanup.dropped_invalid_pages, 1);
        assert_eq!(
            store.load(&page).expect("load").expect("patch").patch,
            patch
        );
    }

    #[test]
    fn idempotent_commit_propagates_patch_io_failure() {
        let temp = TestDirectory::new("recommit-io");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        let patch = patch(&page, 1, "译文");
        store.commit(&page, &patch).expect("initial commit");
        let snapshot = store.snapshot().expect("snapshot");
        let patch_path = store.language_dir().join(&snapshot.pages[0].patch_file);
        fs::remove_file(&patch_path).expect("remove patch");
        fs::create_dir(&patch_path).expect("replace patch with directory");

        assert!(matches!(
            store.commit(&page, &patch),
            Err(TranslationPatchStoreError::Io { .. })
        ));
    }

    #[test]
    fn recovery_prefers_newer_shard_and_cleans_orphans() {
        let temp = TestDirectory::new("recovery");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        let first = patch(&page, 1, "第一版");
        store.commit(&page, &first).expect("first commit");
        let first_shard = store
            .read_canonical_shard(0)
            .expect("shard")
            .expect("first shard");
        let first_bytes = encode_index(&first_shard).expect("first shard bytes");
        let first_entry = first_shard.pages[0].clone();
        let first_patch = fs::read(store.language_dir().join(&first_entry.patch_file))
            .expect("first patch bytes");

        let second = patch(&page, 2, "第二版");
        store.commit(&page, &second).expect("second commit");
        let second_shard = store
            .read_canonical_shard(0)
            .expect("shard")
            .expect("second shard");
        let second_bytes = encode_index(&second_shard).expect("second shard bytes");
        fs::write(
            store.language_dir().join(&first_entry.patch_file),
            first_patch,
        )
        .expect("restore old patch");
        let canonical = store.language_dir().join(shard_filename(0));
        fs::write(&canonical, first_bytes.clone()).expect("restore old canonical shard");
        fs::write(
            store.language_dir().join(".shard-00000000.json.test.bak"),
            first_bytes,
        )
        .expect("backup shard");
        fs::write(
            store.language_dir().join(".shard-00000000.json.test.tmp"),
            second_bytes,
        )
        .expect("new shard temp");
        fs::write(store.language_dir().join(".page-test.tmp"), b"partial").expect("patch temp");

        let report = store.repair().expect("repair");
        assert_eq!(report.promoted_shards, 1);
        assert!(report.removed_index_candidates >= 1);
        assert_eq!(report.removed_patch_temps, 1);
        assert_eq!(report.removed_orphan_patches, 1);
        assert_eq!(
            store.load(&page).expect("load").expect("patch").patch,
            second
        );
    }

    #[test]
    fn repair_drops_only_missing_pages_and_keeps_the_rest_loadable() {
        let temp = TestDirectory::new("missing-page");
        let store = patch_store(temp.path());
        let first_page = page_graph(1, "source one");
        let second_page = page_graph(2, "source two");
        store
            .commit(&first_page, &patch(&first_page, 1, "译文一"))
            .expect("first commit");
        store
            .commit(&second_page, &patch(&second_page, 1, "译文二"))
            .expect("second commit");
        let snapshot = store.snapshot().expect("snapshot");
        let missing = snapshot
            .pages
            .iter()
            .find(|entry| entry.page_number == 2)
            .expect("second page");
        fs::remove_file(store.language_dir().join(&missing.patch_file)).expect("remove patch");

        let report = store.repair().expect("repair");
        assert_eq!(report.dropped_invalid_pages, 1);
        let repaired = store.snapshot().expect("snapshot");
        assert_eq!(repaired.pages.len(), 1);
        assert_eq!(repaired.pages[0].page_number, 1);
        assert!(store.load(&first_page).expect("load first").is_some());
        assert!(store.load(&second_page).expect("load second").is_none());
    }

    #[test]
    fn recovery_rejects_a_shard_whose_filename_and_identity_disagree() {
        let temp = TestDirectory::new("shard-identity");
        let store = patch_store(temp.path());
        let page = page_graph(1, "source");
        store
            .commit(&page, &patch(&page, 1, "译文"))
            .expect("commit");
        let mut shard = store
            .read_canonical_shard(0)
            .expect("shard")
            .expect("stored shard");
        shard.shard_index = 1;
        shard.shard_id = shard_id(&shard).expect("shard id");
        fs::write(
            store.language_dir().join(shard_filename(0)),
            encode_index(&shard).expect("shard bytes"),
        )
        .expect("mismatched shard");

        let report = store.repair().expect("repair");
        assert_eq!(report.dropped_invalid_shards, 1);
        assert_eq!(report.removed_orphan_patches, 1);
        assert!(store.snapshot().expect("snapshot").pages.is_empty());
    }

    #[test]
    fn concurrent_page_commits_do_not_lose_shard_updates() {
        let temp = TestDirectory::new("concurrent");
        let store = patch_store(temp.path());
        let first_page = page_graph(1, "source one");
        let second_page = page_graph(2, "source two");
        let first_patch = patch(&first_page, 1, "译文一");
        let second_patch = patch(&second_page, 1, "译文二");
        let barrier = Arc::new(Barrier::new(3));
        let first_handle =
            spawn_commit(store.clone(), first_page, first_patch, Arc::clone(&barrier));
        let second_handle = spawn_commit(
            store.clone(),
            second_page,
            second_patch,
            Arc::clone(&barrier),
        );
        barrier.wait();
        first_handle.join().expect("first thread");
        second_handle.join().expect("second thread");
        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.shard_count, 1);
        assert_eq!(
            snapshot
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn language_identity_cannot_escape_the_translation_root() {
        let temp = TestDirectory::new("path-safety");
        let store = TranslationPatchStore::new(temp.path(), "sha256:source", "../../zh-CN/秘密")
            .expect("hashed language path");
        let page = page_graph(1, "source");
        let mut patch = patch(&page, 1, "译文");
        patch.target_language = "../../zh-CN/秘密".to_string();
        rebuild_patch_id(&page, &mut patch);
        store.commit(&page, &patch).expect("safe commit");
        assert_eq!(store.language_dir().parent().expect("root"), temp.path());

        assert!(matches!(
            TranslationPatchStore::new(
                Path::new("relative-translations"),
                "sha256:source",
                "zh-CN"
            )
            .expect_err("relative path"),
            TranslationPatchStoreError::InvalidIdentity("translationsRoot")
        ));
    }

    #[test]
    #[ignore = "manual Windows 1000-page sharded patch-store probe"]
    fn manual_windows_thousand_page_patch_store_probe() {
        let temp = TestDirectory::new("thousand-pages");
        let store = patch_store(temp.path());
        let started = Instant::now();
        for page_number in 1..=1_000 {
            let page = page_graph(page_number, "source");
            store
                .commit(&page, &patch(&page, 1, &format!("译文{page_number}")))
                .expect("page commit");
        }
        let elapsed = started.elapsed();
        let snapshot = store.snapshot().expect("snapshot");
        let index_bytes = index_files(store.language_dir())
            .iter()
            .map(|path| fs::metadata(path).expect("index metadata").len())
            .sum::<u64>();
        assert_eq!(snapshot.pages.len(), 1_000);
        assert_eq!(snapshot.shard_count, 16);
        assert_eq!(patch_files(store.language_dir()).len(), 1_000);
        assert!(index_bytes < 1024 * 1024);
        assert!(snapshot.patch_bytes < 2 * 1024 * 1024);
        eprintln!(
            "1000-page sharded patch store: elapsedMs={}, indexBytes={index_bytes}, patchBytes={}",
            elapsed.as_millis(),
            snapshot.patch_bytes
        );
    }

    fn spawn_commit(
        store: TranslationPatchStore,
        page: PageGraph,
        patch: TranslationPatch,
        barrier: Arc<Barrier>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            barrier.wait();
            store.commit(&page, &patch).expect("concurrent commit");
        })
    }

    fn patch_store(root: &Path) -> TranslationPatchStore {
        TranslationPatchStore::new(root, "sha256:source", "zh-CN").expect("patch store")
    }

    fn page_graph(page_number: u32, source_text: &str) -> PageGraph {
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number,
            source_page_hash: format!("sha256:page-{page_number}"),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms: vec![PageAtom {
                atom_id: format!("atom-{page_number}"),
                source_text: source_text.to_string(),
                source_object_id: Some(format!("object-{page_number}")),
                kind: PageAtomKind::Body,
                style_id: Some("style-1".to_string()),
                bounds: [10.0, 20.0, 100.0, 32.0],
                loose_bounds: None,
                origin: Some([10.0, 20.0]),
                text_matrix: Some([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]),
                angle_degrees: Some(0.0),
                order: 0,
                generated: false,
                hyphen: false,
                requires_translation: true,
                source_kind: PageAtomSourceKind::PdfiumVerified,
                source_provenance: None,
            }],
            styles: vec![PageStyle {
                style_id: "style-1".to_string(),
                font_resource: Some("F1".to_string()),
                font_size: 12.0,
                scaled_font_size: 12.0,
                font_weight: Some(400),
                italic: false,
                serif: false,
                fill_color: Some([0.0, 0.0, 0.0, 1.0]),
                stroke_color: None,
                opacity: Some(1.0),
                render_mode: Some("filled-unstroked".to_string()),
            }],
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(1),
            warnings: Vec::new(),
        }
    }

    fn patch(page: &PageGraph, revision: u64, translated_text: &str) -> TranslationPatch {
        let pending = pending_patch(page, revision, translated_text);
        let decisions = pending
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.entry_id.clone(),
                    TranslationPatchRendererDecision::Preserved {
                        reason_code: "store-test-resolved".to_string(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        resolve_translation_patch_renderer_decisions(page, &pending, &decisions)
            .expect("resolved translation patch")
    }

    fn pending_patch(page: &PageGraph, revision: u64, translated_text: &str) -> TranslationPatch {
        build_translation_patch(
            page,
            TranslationPatchDraft {
                target_language: "zh-CN".to_string(),
                translation_revision: revision,
                provider_id: "rwkv-local".to_string(),
                model_id: "rwkv-test".to_string(),
                renderer_version: "pdf-v3-test".to_string(),
                entries: vec![TranslationPatchEntryDraft {
                    atom_ids: vec![page.atoms[0].atom_id.clone()],
                    translated_text: translated_text.to_string(),
                    protected_spans: Vec::new(),
                }],
            },
        )
        .expect("translation patch")
    }

    fn rebuild_patch_id(page: &PageGraph, patch: &mut TranslationPatch) {
        let pending = build_translation_patch(
            page,
            TranslationPatchDraft {
                target_language: patch.target_language.clone(),
                translation_revision: patch.translation_revision,
                provider_id: patch.provider.provider_id.clone(),
                model_id: patch.provider.model_id.clone(),
                renderer_version: patch.renderer_version.clone(),
                entries: vec![TranslationPatchEntryDraft {
                    atom_ids: patch.entries[0]
                        .atoms
                        .iter()
                        .map(|atom| atom.atom_id.clone())
                        .collect(),
                    translated_text: patch.entries[0].translated_text.clone(),
                    protected_spans: Vec::new(),
                }],
            },
        )
        .expect("rebuilt patch");
        let decisions = pending
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.entry_id.clone(),
                    TranslationPatchRendererDecision::Preserved {
                        reason_code: "store-test-resolved".to_string(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        *patch = resolve_translation_patch_renderer_decisions(page, &pending, &decisions)
            .expect("resolved rebuilt patch");
    }

    fn patch_files(directory: &Path) -> Vec<PathBuf> {
        matching_files(directory, |name| {
            name.starts_with("page-") && name.ends_with(".patch.json.gz")
        })
    }

    fn index_files(directory: &Path) -> Vec<PathBuf> {
        matching_files(directory, |name| {
            name == "manifest.json" || (name.starts_with("shard-") && name.ends_with(".json"))
        })
    }

    fn matching_files(directory: &Path, predicate: impl Fn(&str) -> bool) -> Vec<PathBuf> {
        let mut files = fs::read_dir(directory)
            .expect("store directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(&predicate)
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-patch-store-{label}-{}-{nanos}",
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
