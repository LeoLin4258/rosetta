use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RENDER_CACHE_SCHEMA_VERSION: u32 = 1;
const CACHE_VERSION_DIRECTORY: &str = "v1";
const MANIFEST_FILENAME: &str = "manifest.json";
const INDEX_SHARD_COUNT: u32 = 64;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const ABSOLUTE_MAX_ENTRIES: usize = 16_384;
const ABSOLUTE_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

pub(crate) const DEFAULT_RENDER_CACHE_MAX_BYTES: u64 = 384 * 1024 * 1024;
pub(crate) const DEFAULT_RENDER_CACHE_MAX_ENTRIES: usize = 4_096;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static CACHE_COORDINATORS: Lazy<Mutex<BTreeMap<PathBuf, Weak<RenderCacheCoordinator>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderCacheConfig {
    pub max_bytes: u64,
    pub max_entries: usize,
}

impl Default for RenderCacheConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_RENDER_CACHE_MAX_BYTES,
            max_entries: DEFAULT_RENDER_CACHE_MAX_ENTRIES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderCacheKey {
    pub source_fingerprint: String,
    pub page_number: u32,
    pub patch_id: String,
    pub translation_revision: u64,
    pub renderer_version: String,
    pub options: RenderCacheOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderCacheOptions {
    pub output_kind: RenderCacheOutputKind,
    pub pixel_width: Option<u32>,
    pub scale_milli: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RenderCacheOutputKind {
    PreviewPng,
    TranslatedPagePdf,
}

impl RenderCacheOutputKind {
    fn extension(self) -> &'static str {
        match self {
            Self::PreviewPng => "png",
            Self::TranslatedPagePdf => "pdf",
        }
    }

    fn has_valid_signature(self, bytes: &[u8]) -> bool {
        match self {
            Self::PreviewPng => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::TranslatedPagePdf => bytes.starts_with(b"%PDF-"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderCacheInsertKind {
    Written,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderCacheInsertOutcome {
    pub kind: RenderCacheInsertKind,
    pub key_id: String,
    pub artifact_bytes: u64,
    pub evicted_entries: u64,
    pub evicted_bytes: u64,
    pub cleanup: RenderCacheRepairReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderCacheSnapshot {
    pub cache_id: String,
    pub entry_count: usize,
    pub artifact_bytes: u64,
    pub active_leases: usize,
    pub max_entries: usize,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RenderCacheRepairReport {
    pub reset_manifest: bool,
    pub dropped_invalid_shards: u64,
    pub dropped_invalid_entries: u64,
    pub removed_index_temps: u64,
    pub removed_artifact_temps: u64,
    pub removed_orphan_artifacts: u64,
    pub evicted_entries: u64,
    pub evicted_bytes: u64,
    pub cleanup_failures: u64,
}

#[derive(Debug)]
pub(crate) enum RenderCacheError {
    InvalidConfig(&'static str),
    InvalidKey(&'static str),
    InvalidArtifact(&'static str),
    LockPoisoned,
    ConfigConflict,
    ActiveLeases,
    EntryInUse,
    ArtifactTooLarge {
        bytes: u64,
        maximum: u64,
    },
    QuotaUnavailable {
        required: u64,
        available: u64,
    },
    IndexTooLarge {
        bytes: u64,
        maximum: u64,
    },
    IndexInvalid(String),
    CorruptArtifact {
        key_id: String,
    },
    AccessClockOverflow,
    ShardGenerationOverflow,
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for RenderCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(field) => write!(formatter, "render cache config {field} is invalid"),
            Self::InvalidKey(field) => write!(formatter, "render cache key {field} is invalid"),
            Self::InvalidArtifact(reason) => write!(formatter, "render cache artifact is invalid: {reason}"),
            Self::LockPoisoned => formatter.write_str("render cache lock is poisoned"),
            Self::ConfigConflict => formatter.write_str("render cache root is already open with a different quota"),
            Self::ActiveLeases => formatter.write_str("render cache cannot be repaired while artifacts are leased"),
            Self::EntryInUse => formatter.write_str("render cache entry is currently leased"),
            Self::ArtifactTooLarge { bytes, maximum } => write!(formatter, "render cache artifact has {bytes} bytes, above quota {maximum}"),
            Self::QuotaUnavailable { required, available } => write!(formatter, "render cache needs {required} bytes but only {available} evictable bytes are available"),
            Self::IndexTooLarge { bytes, maximum } => write!(formatter, "render cache index has {bytes} bytes, above maximum {maximum}"),
            Self::IndexInvalid(message) => write!(formatter, "render cache index is invalid: {message}"),
            Self::CorruptArtifact { key_id } => write!(formatter, "render cache artifact {key_id} failed integrity validation"),
            Self::AccessClockOverflow => formatter.write_str("render cache access clock overflow"),
            Self::ShardGenerationOverflow => formatter.write_str("render cache shard generation overflow"),
            Self::Io { operation, path, message } => write!(formatter, "failed to {operation} render cache path {}: {message}", path.display()),
        }
    }
}

impl std::error::Error for RenderCacheError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderCacheManifest {
    schema_version: u32,
    cache_id: String,
    index_shard_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderCacheIndexShard {
    schema_version: u32,
    shard_id: String,
    shard_index: u32,
    generation: u64,
    entries: Vec<RenderCacheIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderCacheIndexEntry {
    key_id: String,
    key: RenderCacheKey,
    artifact_file: String,
    artifact_bytes: u64,
    artifact_sha256: String,
    last_access: u64,
}

#[derive(Debug, Default)]
struct RenderCacheState {
    initialized: bool,
    access_clock: u64,
    entries: BTreeMap<String, RenderCacheIndexEntry>,
    shard_generations: BTreeMap<u32, u64>,
    active: BTreeMap<String, usize>,
}

#[derive(Debug)]
struct RenderCacheCoordinator {
    config: RenderCacheConfig,
    state: Mutex<RenderCacheState>,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderCache {
    cache_dir: PathBuf,
    config: RenderCacheConfig,
    coordinator: Arc<RenderCacheCoordinator>,
}

#[derive(Debug)]
pub(crate) struct RenderCacheLease {
    key_id: String,
    entry: RenderCacheIndexEntry,
    file: Option<File>,
    cache_dir: PathBuf,
    coordinator: Arc<RenderCacheCoordinator>,
    finished: bool,
}

impl RenderCache {
    pub(crate) fn new(
        render_cache_root: &Path,
        config: RenderCacheConfig,
    ) -> Result<Self, RenderCacheError> {
        if !render_cache_root.is_absolute() {
            return Err(RenderCacheError::InvalidKey("renderCacheRoot"));
        }
        validate_config(config)?;
        let cache_dir = render_cache_root.join(CACHE_VERSION_DIRECTORY);
        let coordinator = cache_coordinator(&cache_dir, config)?;
        Ok(Self {
            cache_dir,
            config,
            coordinator,
        })
    }

    #[cfg(test)]
    fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub(crate) fn insert(
        &self,
        key: &RenderCacheKey,
        bytes: &[u8],
    ) -> Result<RenderCacheInsertOutcome, RenderCacheError> {
        validate_key(key)?;
        validate_artifact(key.options.output_kind, bytes)?;
        let artifact_bytes =
            u64::try_from(bytes.len()).map_err(|_| RenderCacheError::ArtifactTooLarge {
                bytes: u64::MAX,
                maximum: self.config.max_bytes,
            })?;
        if artifact_bytes > self.config.max_bytes {
            return Err(RenderCacheError::ArtifactTooLarge {
                bytes: artifact_bytes,
                maximum: self.config.max_bytes,
            });
        }
        let key_id = key_id(key)?;
        let artifact_sha256 = sha256(bytes);
        let artifact_file = artifact_filename(&key_id, &artifact_sha256, key.options.output_kind);

        self.with_lock(|state| {
            let cleanup = self.prepare_locked(state)?;
            if state.active.get(&key_id).copied().unwrap_or(0) > 0 {
                return Err(RenderCacheError::EntryInUse);
            }
            if let Some(current) = state.entries.get(&key_id).cloned() {
                if current.artifact_sha256 == artifact_sha256
                    && current.artifact_bytes == artifact_bytes
                    && self.entry_file_matches_metadata(&current)
                {
                    self.touch_locked(state, &key_id);
                    let _ = self.persist_shard_locked(state, shard_index(&key_id)?);
                    return Ok(RenderCacheInsertOutcome {
                        kind: RenderCacheInsertKind::Unchanged,
                        key_id,
                        artifact_bytes,
                        evicted_entries: 0,
                        evicted_bytes: 0,
                        cleanup,
                    });
                }
            }

            let old_entry = state.entries.get(&key_id).cloned();
            let old_bytes = old_entry
                .as_ref()
                .map(|entry| entry.artifact_bytes)
                .unwrap_or(0);
            let old_count = usize::from(old_entry.is_some());
            let current_bytes = total_bytes(&state.entries);
            let current_count = state.entries.len();
            let target_bytes = current_bytes
                .saturating_sub(old_bytes)
                .saturating_add(artifact_bytes);
            let target_count = current_count.saturating_sub(old_count).saturating_add(1);
            let victims = self.select_victims(state, &key_id, target_bytes, target_count)?;

            let mut changed_shards = BTreeSet::new();
            let mut evicted_entries = 0_u64;
            let mut evicted_bytes = 0_u64;
            for victim_id in victims {
                if let Some(victim) = state.entries.remove(&victim_id) {
                    changed_shards.insert(shard_index(&victim_id)?);
                    evicted_entries += 1;
                    evicted_bytes = evicted_bytes.saturating_add(victim.artifact_bytes);
                    remove_cache_file(&self.cache_dir.join(&victim.artifact_file))?;
                }
            }
            if let Some(previous) = state.entries.remove(&key_id) {
                changed_shards.insert(shard_index(&key_id)?);
                if previous.artifact_file != artifact_file {
                    remove_cache_file(&self.cache_dir.join(previous.artifact_file))?;
                }
            }

            let artifact_path = self.cache_dir.join(&artifact_file);
            if artifact_path.exists() {
                remove_cache_file(&artifact_path)?;
            }
            if let Err(error) = write_artifact_atomic(&artifact_path, bytes) {
                state.initialized = false;
                return Err(error);
            }
            let last_access = next_access(state)?;
            state.entries.insert(
                key_id.clone(),
                RenderCacheIndexEntry {
                    key_id: key_id.clone(),
                    key: key.clone(),
                    artifact_file,
                    artifact_bytes,
                    artifact_sha256,
                    last_access,
                },
            );
            changed_shards.insert(shard_index(&key_id)?);
            for shard in changed_shards {
                if let Err(error) = self.persist_shard_locked(state, shard) {
                    state.initialized = false;
                    return Err(error);
                }
            }
            Ok(RenderCacheInsertOutcome {
                kind: RenderCacheInsertKind::Written,
                key_id,
                artifact_bytes,
                evicted_entries,
                evicted_bytes,
                cleanup,
            })
        })
    }

    pub(crate) fn open(
        &self,
        key: &RenderCacheKey,
    ) -> Result<Option<RenderCacheLease>, RenderCacheError> {
        validate_key(key)?;
        let key_id = key_id(key)?;
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            let Some(entry) = state.entries.get(&key_id).cloned() else {
                return Ok(None);
            };
            let path = self.cache_dir.join(&entry.artifact_file);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.drop_entry_locked(state, &key_id)?;
                    return Ok(None);
                }
                Err(error) => return Err(io_error("open", &path, error)),
            };
            let actual_bytes = file
                .metadata()
                .map_err(|error| io_error("inspect", &path, error))?
                .len();
            if actual_bytes != entry.artifact_bytes {
                drop(file);
                self.drop_entry_locked(state, &key_id)?;
                let _ = fs::remove_file(path);
                return Ok(None);
            }
            *state.active.entry(key_id.clone()).or_insert(0) += 1;
            self.touch_locked(state, &key_id);
            let _ = self.persist_shard_locked(state, shard_index(&key_id)?);
            Ok(Some(RenderCacheLease {
                key_id,
                entry,
                file: Some(file),
                cache_dir: self.cache_dir.clone(),
                coordinator: Arc::clone(&self.coordinator),
                finished: false,
            }))
        })
    }

    pub(crate) fn snapshot(&self) -> Result<RenderCacheSnapshot, RenderCacheError> {
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            Ok(RenderCacheSnapshot {
                cache_id: expected_manifest()?.cache_id,
                entry_count: state.entries.len(),
                artifact_bytes: total_bytes(&state.entries),
                active_leases: state.active.values().sum(),
                max_entries: self.config.max_entries,
                max_bytes: self.config.max_bytes,
            })
        })
    }

    pub(crate) fn repair(&self) -> Result<RenderCacheRepairReport, RenderCacheError> {
        self.with_lock(|state| {
            if !state.active.is_empty() {
                return Err(RenderCacheError::ActiveLeases);
            }
            state.initialized = false;
            self.prepare_locked(state)
        })
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&mut RenderCacheState) -> Result<T, RenderCacheError>,
    ) -> Result<T, RenderCacheError> {
        let mut state = self
            .coordinator
            .state
            .lock()
            .map_err(|_| RenderCacheError::LockPoisoned)?;
        operation(&mut state)
    }

    fn prepare_locked(
        &self,
        state: &mut RenderCacheState,
    ) -> Result<RenderCacheRepairReport, RenderCacheError> {
        if state.initialized {
            return Ok(RenderCacheRepairReport::default());
        }
        if !state.active.is_empty() {
            return Err(RenderCacheError::ActiveLeases);
        }
        fs::create_dir_all(&self.cache_dir)
            .map_err(|error| io_error("create", &self.cache_dir, error))?;
        let mut report = RenderCacheRepairReport::default();
        self.ensure_manifest(&mut report)?;

        let mut entries = BTreeMap::new();
        let mut generations = BTreeMap::new();
        let mut changed_shards = BTreeSet::new();
        for shard in 0..INDEX_SHARD_COUNT {
            let path = self.cache_dir.join(shard_filename(shard));
            if !path.exists() {
                continue;
            }
            let loaded = self.read_shard(&path, shard);
            let Ok(index) = loaded else {
                report.dropped_invalid_shards += 1;
                changed_shards.insert(shard);
                let _ = fs::remove_file(&path);
                continue;
            };
            generations.insert(shard, index.generation);
            for entry in index.entries {
                if validate_stored_entry(&entry, shard).is_err()
                    || !self.entry_file_matches_metadata(&entry)
                    || entries.contains_key(&entry.key_id)
                {
                    report.dropped_invalid_entries += 1;
                    changed_shards.insert(shard);
                    continue;
                }
                entries.insert(entry.key_id.clone(), entry);
            }
        }
        state.entries = entries;
        state.shard_generations = generations;
        state.access_clock = state
            .entries
            .values()
            .map(|entry| entry.last_access)
            .max()
            .unwrap_or(0);

        let victims =
            self.select_victims(state, "", total_bytes(&state.entries), state.entries.len())?;
        for victim_id in victims {
            if let Some(victim) = state.entries.remove(&victim_id) {
                changed_shards.insert(shard_index(&victim_id)?);
                report.evicted_entries += 1;
                report.evicted_bytes = report.evicted_bytes.saturating_add(victim.artifact_bytes);
                if fs::remove_file(self.cache_dir.join(victim.artifact_file)).is_err() {
                    report.cleanup_failures += 1;
                }
            }
        }
        for shard in changed_shards {
            self.persist_shard_locked(state, shard)?;
        }
        self.cleanup_files(state, &mut report)?;
        state.initialized = true;
        Ok(report)
    }

    fn ensure_manifest(
        &self,
        report: &mut RenderCacheRepairReport,
    ) -> Result<(), RenderCacheError> {
        let path = self.cache_dir.join(MANIFEST_FILENAME);
        let expected = expected_manifest()?;
        let valid = if path.exists() {
            read_limited(&path, MAX_MANIFEST_BYTES)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<RenderCacheManifest>(&bytes).ok())
                .is_some_and(|manifest| manifest == expected)
        } else {
            false
        };
        if valid {
            return Ok(());
        }
        report.reset_manifest = path.exists();
        self.clear_recognized_cache_files(report)?;
        let bytes = encode_index(&expected, MAX_MANIFEST_BYTES)?;
        write_index_atomic(&path, &bytes)
    }

    fn clear_recognized_cache_files(
        &self,
        report: &mut RenderCacheRepairReport,
    ) -> Result<(), RenderCacheError> {
        for path in list_files(&self.cache_dir)? {
            let name = file_name(&path);
            if name == MANIFEST_FILENAME
                || is_manifest_sidecar(name)
                || parse_shard_filename(name).is_some()
                || is_index_sidecar(name)
                || is_artifact_filename(name)
                || is_artifact_temp(name)
            {
                if fs::remove_file(&path).is_err() {
                    report.cleanup_failures += 1;
                }
            }
        }
        Ok(())
    }

    fn read_shard(
        &self,
        path: &Path,
        expected_index: u32,
    ) -> Result<RenderCacheIndexShard, RenderCacheError> {
        let bytes = read_limited(path, MAX_INDEX_BYTES)?;
        let shard = serde_json::from_slice::<RenderCacheIndexShard>(&bytes)
            .map_err(|error| index_error(path, error))?;
        if shard.schema_version != RENDER_CACHE_SCHEMA_VERSION
            || shard.shard_index != expected_index
            || shard.entries.len() > ABSOLUTE_MAX_ENTRIES
            || shard_id(&shard)? != shard.shard_id
        {
            return Err(RenderCacheError::IndexInvalid(format!(
                "invalid shard {}",
                path.display()
            )));
        }
        Ok(shard)
    }

    fn persist_shard_locked(
        &self,
        state: &mut RenderCacheState,
        shard: u32,
    ) -> Result<(), RenderCacheError> {
        let path = self.cache_dir.join(shard_filename(shard));
        let entries = state
            .entries
            .values()
            .filter(|entry| shard_index(&entry.key_id).ok() == Some(shard))
            .cloned()
            .collect::<Vec<_>>();
        if entries.is_empty() {
            if path.exists() {
                remove_cache_file(&path)?;
            }
            state.shard_generations.remove(&shard);
            return Ok(());
        }
        let generation = state
            .shard_generations
            .get(&shard)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(RenderCacheError::ShardGenerationOverflow)?;
        let mut index = RenderCacheIndexShard {
            schema_version: RENDER_CACHE_SCHEMA_VERSION,
            shard_id: String::new(),
            shard_index: shard,
            generation,
            entries,
        };
        index.shard_id = shard_id(&index)?;
        write_index_atomic(&path, &encode_index(&index, MAX_INDEX_BYTES)?)?;
        state.shard_generations.insert(shard, generation);
        Ok(())
    }

    fn select_victims(
        &self,
        state: &RenderCacheState,
        protected_key: &str,
        mut projected_bytes: u64,
        mut projected_count: usize,
    ) -> Result<Vec<String>, RenderCacheError> {
        if projected_bytes <= self.config.max_bytes && projected_count <= self.config.max_entries {
            return Ok(Vec::new());
        }
        let mut candidates = state
            .entries
            .values()
            .filter(|entry| entry.key_id != protected_key)
            .filter(|entry| state.active.get(&entry.key_id).copied().unwrap_or(0) == 0)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            (left.last_access, &left.key_id).cmp(&(right.last_access, &right.key_id))
        });
        let mut victims = Vec::new();
        for entry in candidates {
            if projected_bytes <= self.config.max_bytes
                && projected_count <= self.config.max_entries
            {
                break;
            }
            projected_bytes = projected_bytes.saturating_sub(entry.artifact_bytes);
            projected_count = projected_count.saturating_sub(1);
            victims.push(entry.key_id.clone());
        }
        if projected_bytes > self.config.max_bytes || projected_count > self.config.max_entries {
            return Err(RenderCacheError::QuotaUnavailable {
                required: projected_bytes.saturating_sub(self.config.max_bytes),
                available: self
                    .config
                    .max_bytes
                    .saturating_sub(total_bytes(&state.entries)),
            });
        }
        Ok(victims)
    }

    fn touch_locked(&self, state: &mut RenderCacheState, key_id: &str) {
        let Ok(access) = next_access(state) else {
            return;
        };
        if let Some(entry) = state.entries.get_mut(key_id) {
            entry.last_access = access;
        }
    }

    fn drop_entry_locked(
        &self,
        state: &mut RenderCacheState,
        key_id: &str,
    ) -> Result<(), RenderCacheError> {
        if let Some(entry) = state.entries.remove(key_id) {
            let shard = shard_index(key_id)?;
            self.persist_shard_locked(state, shard)?;
            let _ = fs::remove_file(self.cache_dir.join(entry.artifact_file));
        }
        Ok(())
    }

    fn entry_file_matches_metadata(&self, entry: &RenderCacheIndexEntry) -> bool {
        fs::metadata(self.cache_dir.join(&entry.artifact_file))
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == entry.artifact_bytes)
    }

    fn cleanup_files(
        &self,
        state: &RenderCacheState,
        report: &mut RenderCacheRepairReport,
    ) -> Result<(), RenderCacheError> {
        let referenced = state
            .entries
            .values()
            .map(|entry| entry.artifact_file.as_str())
            .collect::<BTreeSet<_>>();
        for path in list_files(&self.cache_dir)? {
            let name = file_name(&path);
            let result = if is_index_sidecar(name) || is_manifest_sidecar(name) {
                report.removed_index_temps += 1;
                fs::remove_file(&path)
            } else if is_artifact_temp(name) {
                report.removed_artifact_temps += 1;
                fs::remove_file(&path)
            } else if is_artifact_filename(name) && !referenced.contains(name) {
                report.removed_orphan_artifacts += 1;
                fs::remove_file(&path)
            } else {
                continue;
            };
            if result.is_err() {
                report.cleanup_failures += 1;
            }
        }
        Ok(())
    }
}

impl RenderCacheLease {
    pub(crate) fn artifact_bytes(&self) -> u64 {
        self.entry.artifact_bytes
    }

    pub(crate) fn read_bytes(mut self) -> Result<Vec<u8>, RenderCacheError> {
        let mut file = self.file.take().expect("render cache lease file");
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            io_error(
                "seek",
                &self.cache_dir.join(&self.entry.artifact_file),
                error,
            )
        })?;
        let capacity = usize::try_from(self.entry.artifact_bytes).map_err(|_| {
            RenderCacheError::ArtifactTooLarge {
                bytes: self.entry.artifact_bytes,
                maximum: self.coordinator.config.max_bytes,
            }
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        (&mut file)
            .take(self.entry.artifact_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                io_error(
                    "read",
                    &self.cache_dir.join(&self.entry.artifact_file),
                    error,
                )
            })?;
        drop(file);
        let valid = u64::try_from(bytes.len()).ok() == Some(self.entry.artifact_bytes)
            && sha256(&bytes) == self.entry.artifact_sha256
            && self
                .entry
                .key
                .options
                .output_kind
                .has_valid_signature(&bytes);
        self.finish(!valid)?;
        if !valid {
            return Err(RenderCacheError::CorruptArtifact {
                key_id: self.key_id.clone(),
            });
        }
        Ok(bytes)
    }

    fn finish(&mut self, invalidate: bool) -> Result<(), RenderCacheError> {
        if self.finished {
            return Ok(());
        }
        self.file.take();
        let mut state = self
            .coordinator
            .state
            .lock()
            .map_err(|_| RenderCacheError::LockPoisoned)?;
        decrement_active(&mut state, &self.key_id);
        if invalidate {
            if let Some(entry) = state.entries.remove(&self.key_id) {
                let shard = shard_index(&self.key_id)?;
                let cache = RenderCache {
                    cache_dir: self.cache_dir.clone(),
                    config: self.coordinator.config,
                    coordinator: Arc::clone(&self.coordinator),
                };
                cache.persist_shard_locked(&mut state, shard)?;
                let _ = fs::remove_file(self.cache_dir.join(entry.artifact_file));
            }
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for RenderCacheLease {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.file.take();
        if let Ok(mut state) = self.coordinator.state.lock() {
            decrement_active(&mut state, &self.key_id);
        }
        self.finished = true;
    }
}

fn decrement_active(state: &mut RenderCacheState, key_id: &str) {
    if let Some(count) = state.active.get_mut(key_id) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            state.active.remove(key_id);
        }
    }
}

fn validate_config(config: RenderCacheConfig) -> Result<(), RenderCacheError> {
    if config.max_bytes == 0 || config.max_bytes > ABSOLUTE_MAX_BYTES {
        return Err(RenderCacheError::InvalidConfig("maxBytes"));
    }
    if config.max_entries == 0 || config.max_entries > ABSOLUTE_MAX_ENTRIES {
        return Err(RenderCacheError::InvalidConfig("maxEntries"));
    }
    Ok(())
}

fn validate_key(key: &RenderCacheKey) -> Result<(), RenderCacheError> {
    validate_identifier(&key.source_fingerprint, "sourceFingerprint")?;
    validate_identifier(&key.patch_id, "patchId")?;
    validate_identifier(&key.renderer_version, "rendererVersion")?;
    if key.page_number == 0 {
        return Err(RenderCacheError::InvalidKey("pageNumber"));
    }
    if key.translation_revision == 0 {
        return Err(RenderCacheError::InvalidKey("translationRevision"));
    }
    if key
        .options
        .pixel_width
        .is_some_and(|width| width == 0 || width > 32_768)
    {
        return Err(RenderCacheError::InvalidKey("pixelWidth"));
    }
    if key
        .options
        .scale_milli
        .is_some_and(|scale| scale == 0 || scale > 16_000)
    {
        return Err(RenderCacheError::InvalidKey("scaleMilli"));
    }
    match key.options.output_kind {
        RenderCacheOutputKind::PreviewPng
            if key.options.pixel_width.is_none() && key.options.scale_milli.is_none() =>
        {
            Err(RenderCacheError::InvalidKey("previewSize"))
        }
        RenderCacheOutputKind::TranslatedPagePdf
            if key.options.pixel_width.is_some() || key.options.scale_milli.is_some() =>
        {
            Err(RenderCacheError::InvalidKey("pagePdfSize"))
        }
        _ => Ok(()),
    }
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), RenderCacheError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(RenderCacheError::InvalidKey(field));
    }
    Ok(())
}

fn validate_artifact(kind: RenderCacheOutputKind, bytes: &[u8]) -> Result<(), RenderCacheError> {
    if bytes.is_empty() {
        return Err(RenderCacheError::InvalidArtifact("empty"));
    }
    if !kind.has_valid_signature(bytes) {
        return Err(RenderCacheError::InvalidArtifact("signature"));
    }
    Ok(())
}

fn cache_coordinator(
    path: &Path,
    config: RenderCacheConfig,
) -> Result<Arc<RenderCacheCoordinator>, RenderCacheError> {
    let mut coordinators = CACHE_COORDINATORS
        .lock()
        .map_err(|_| RenderCacheError::LockPoisoned)?;
    coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
    if let Some(coordinator) = coordinators.get(path).and_then(Weak::upgrade) {
        if coordinator.config != config {
            return Err(RenderCacheError::ConfigConflict);
        }
        return Ok(coordinator);
    }
    let coordinator = Arc::new(RenderCacheCoordinator {
        config,
        state: Mutex::new(RenderCacheState::default()),
    });
    coordinators.insert(path.to_path_buf(), Arc::downgrade(&coordinator));
    Ok(coordinator)
}

fn expected_manifest() -> Result<RenderCacheManifest, RenderCacheError> {
    let mut manifest = RenderCacheManifest {
        schema_version: RENDER_CACHE_SCHEMA_VERSION,
        cache_id: String::new(),
        index_shard_count: INDEX_SHARD_COUNT,
    };
    manifest.cache_id = manifest_id(&manifest)?;
    Ok(manifest)
}

fn manifest_id(manifest: &RenderCacheManifest) -> Result<String, RenderCacheError> {
    let mut canonical = manifest.clone();
    canonical.cache_id.clear();
    Ok(format!(
        "render-cache-{}",
        sha256(&encode_index(&canonical, MAX_MANIFEST_BYTES)?)
    ))
}

fn shard_id(shard: &RenderCacheIndexShard) -> Result<String, RenderCacheError> {
    let mut canonical = shard.clone();
    canonical.shard_id.clear();
    Ok(format!(
        "render-cache-shard-{}",
        sha256(&encode_index(&canonical, MAX_INDEX_BYTES)?)
    ))
}

fn key_id(key: &RenderCacheKey) -> Result<String, RenderCacheError> {
    let bytes = serde_json::to_vec(key).map_err(|error| {
        RenderCacheError::IndexInvalid(format!("failed to encode cache key: {error}"))
    })?;
    Ok(sha256(&bytes))
}

fn validate_stored_entry(
    entry: &RenderCacheIndexEntry,
    expected_shard: u32,
) -> Result<(), RenderCacheError> {
    validate_key(&entry.key)?;
    if entry.key_id != key_id(&entry.key)?
        || shard_index(&entry.key_id)? != expected_shard
        || entry.artifact_bytes == 0
        || !is_sha256(&entry.artifact_sha256)
        || entry.artifact_file
            != artifact_filename(
                &entry.key_id,
                &entry.artifact_sha256,
                entry.key.options.output_kind,
            )
        || entry.last_access == 0
        || !is_safe_file_name(&entry.artifact_file)
    {
        return Err(RenderCacheError::IndexInvalid(
            "invalid cache entry".to_string(),
        ));
    }
    Ok(())
}

fn shard_index(key_id: &str) -> Result<u32, RenderCacheError> {
    if !is_sha256(key_id) {
        return Err(RenderCacheError::IndexInvalid("invalid key ID".to_string()));
    }
    let prefix = u8::from_str_radix(&key_id[..2], 16)
        .map_err(|_| RenderCacheError::IndexInvalid("invalid key ID".to_string()))?;
    Ok(u32::from(prefix) % INDEX_SHARD_COUNT)
}

fn shard_filename(index: u32) -> String {
    format!("shard-{index:02}.json")
}

fn parse_shard_filename(name: &str) -> Option<u32> {
    let index = name
        .strip_prefix("shard-")?
        .strip_suffix(".json")?
        .parse::<u32>()
        .ok()?;
    (index < INDEX_SHARD_COUNT).then_some(index)
}

fn artifact_filename(key_id: &str, content_hash: &str, kind: RenderCacheOutputKind) -> String {
    format!("artifact-{key_id}-{content_hash}.{}", kind.extension())
}

fn is_artifact_filename(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("artifact-") else {
        return false;
    };
    let Some((identity, extension)) = rest.rsplit_once('.') else {
        return false;
    };
    let Some((key, content)) = identity.split_once('-') else {
        return false;
    };
    is_sha256(key) && is_sha256(content) && matches!(extension, "png" | "pdf")
}

fn is_artifact_temp(name: &str) -> bool {
    name.starts_with(".artifact-") && name.ends_with(".tmp")
}

fn is_index_sidecar(name: &str) -> bool {
    name.starts_with(".shard-") && (name.ends_with(".tmp") || name.ends_with(".bak"))
}

fn is_manifest_sidecar(name: &str) -> bool {
    name.starts_with(".manifest.json.") && (name.ends_with(".tmp") || name.ends_with(".bak"))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
}

fn total_bytes(entries: &BTreeMap<String, RenderCacheIndexEntry>) -> u64 {
    entries.values().fold(0_u64, |total, entry| {
        total.saturating_add(entry.artifact_bytes)
    })
}

fn next_access(state: &mut RenderCacheState) -> Result<u64, RenderCacheError> {
    state.access_clock = state
        .access_clock
        .checked_add(1)
        .ok_or(RenderCacheError::AccessClockOverflow)?;
    Ok(state.access_clock)
}

fn encode_index<T: Serialize>(value: &T, maximum: u64) -> Result<Vec<u8>, RenderCacheError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| RenderCacheError::IndexInvalid(format!("encode failed: {error}")))?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_count > maximum {
        return Err(RenderCacheError::IndexTooLarge {
            bytes: byte_count,
            maximum,
        });
    }
    Ok(bytes)
}

fn write_index_atomic(target: &Path, bytes: &[u8]) -> Result<(), RenderCacheError> {
    let temp = unique_sidecar_path(target, "tmp");
    let backup = unique_sidecar_path(target, "bak");
    write_new_synced_file(&temp, bytes)?;
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

fn write_artifact_atomic(target: &Path, bytes: &[u8]) -> Result<(), RenderCacheError> {
    let temp = unique_sidecar_path(target, "tmp");
    write_new_synced_file(&temp, bytes)?;
    if let Err(error) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(io_error("commit", target, error));
    }
    if let Some(parent) = target.parent() {
        sync_parent_directory(parent)?;
    }
    Ok(())
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), RenderCacheError> {
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

fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, RenderCacheError> {
    let metadata = fs::metadata(path).map_err(|error| io_error("inspect", path, error))?;
    if metadata.len() > maximum {
        return Err(RenderCacheError::IndexTooLarge {
            bytes: metadata.len(),
            maximum,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    File::open(path)
        .map_err(|error| io_error("open", path, error))?
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read", path, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(RenderCacheError::IndexTooLarge {
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(bytes)
}

fn unique_sidecar_path(target: &Path, extension: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("render-cache");
    target.with_file_name(format!(
        ".{name}.{}.{}.{extension}",
        std::process::id(),
        counter
    ))
}

fn list_files(directory: &Path) -> Result<Vec<PathBuf>, RenderCacheError> {
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

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
}

fn remove_cache_file(path: &Path) -> Result<(), RenderCacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove", path, error)),
    }
}

fn index_error(path: &Path, error: serde_json::Error) -> RenderCacheError {
    RenderCacheError::IndexInvalid(format!("failed to decode {}: {error}", path.display()))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), RenderCacheError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync", path, error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), RenderCacheError> {
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, error: std::io::Error) -> RenderCacheError {
    RenderCacheError::Io {
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
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Barrier},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        artifact_filename, key_id, shard_filename, shard_index, RenderCache, RenderCacheConfig,
        RenderCacheError, RenderCacheInsertKind, RenderCacheKey, RenderCacheOptions,
        RenderCacheOutputKind,
    };

    #[test]
    fn round_trips_content_addressed_preview_without_exposing_identity_in_paths() {
        let temp = TestDirectory::new("roundtrip");
        let cache = render_cache(temp.path(), 1024, 8);
        let key = preview_key(7, 1, 1440);
        let bytes = png_bytes(64, 7);

        let outcome = cache.insert(&key, &bytes).expect("insert preview");
        assert_eq!(outcome.kind, RenderCacheInsertKind::Written);
        assert_eq!(outcome.artifact_bytes, bytes.len() as u64);
        let lease = cache.open(&key).expect("open preview").expect("cache hit");
        assert_eq!(lease.artifact_bytes(), bytes.len() as u64);
        assert_eq!(lease.read_bytes().expect("read preview"), bytes);

        let repeated = cache
            .insert(&key, &png_bytes(64, 7))
            .expect("idempotent insert");
        assert_eq!(repeated.kind, RenderCacheInsertKind::Unchanged);
        let snapshot = cache.snapshot().expect("snapshot");
        assert_eq!(snapshot.entry_count, 1);
        assert_eq!(snapshot.artifact_bytes, bytes.len() as u64);
        assert!(snapshot.cache_id.starts_with("render-cache-"));
        for path in files(cache.cache_dir()) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("file name");
            assert!(!name.contains("source-fixture"));
            assert!(!name.contains("patch-fixture"));
            assert!(!name.contains("renderer-test"));
        }
    }

    #[test]
    fn every_render_identity_dimension_changes_the_cache_key() {
        let temp = TestDirectory::new("identity");
        let cache = render_cache(temp.path(), 4096, 32);
        let base = preview_key(1, 1, 1200);
        cache.insert(&base, &png_bytes(32, 1)).expect("insert base");

        let mut variants = Vec::new();
        let mut source = base.clone();
        source.source_fingerprint = "source-other".to_string();
        variants.push(source);
        let mut page = base.clone();
        page.page_number = 2;
        variants.push(page);
        let mut patch = base.clone();
        patch.patch_id = "patch-other".to_string();
        variants.push(patch);
        let mut revision = base.clone();
        revision.translation_revision = 2;
        variants.push(revision);
        let mut renderer = base.clone();
        renderer.renderer_version = "renderer-other".to_string();
        variants.push(renderer);
        let mut width = base.clone();
        width.options.pixel_width = Some(1600);
        variants.push(width);
        let mut scale = base.clone();
        scale.options.scale_milli = Some(1250);
        variants.push(scale);
        variants.push(page_pdf_key(1, 1));

        for variant in variants {
            assert!(cache.open(&variant).expect("lookup variant").is_none());
        }
    }

    #[test]
    fn enforces_byte_quota_with_true_lru_access_order() {
        let temp = TestDirectory::new("lru");
        let artifact = png_bytes(64, 1);
        let cache = render_cache(temp.path(), artifact.len() as u64 * 2, 8);
        let first = preview_key(1, 1, 1200);
        let second = preview_key(2, 1, 1200);
        let third = preview_key(3, 1, 1200);
        cache.insert(&first, &artifact).expect("first");
        cache.insert(&second, &png_bytes(64, 2)).expect("second");
        cache
            .open(&first)
            .expect("touch first")
            .expect("first hit")
            .read_bytes()
            .expect("read first");

        let outcome = cache.insert(&third, &png_bytes(64, 3)).expect("third");
        assert_eq!(outcome.evicted_entries, 1);
        assert!(cache.open(&first).expect("first lookup").is_some());
        assert!(cache.open(&second).expect("second lookup").is_none());
        assert!(cache.open(&third).expect("third lookup").is_some());
        let snapshot = cache.snapshot().expect("snapshot");
        assert_eq!(snapshot.entry_count, 2);
        assert!(snapshot.artifact_bytes <= snapshot.max_bytes);
    }

    #[test]
    fn active_lease_blocks_eviction_until_released() {
        let temp = TestDirectory::new("lease");
        let artifact = png_bytes(64, 1);
        let cache = render_cache(temp.path(), artifact.len() as u64, 1);
        let first = preview_key(1, 1, 1200);
        let second = preview_key(2, 1, 1200);
        cache.insert(&first, &artifact).expect("first");
        let lease = cache.open(&first).expect("open first").expect("first hit");

        assert!(matches!(
            cache
                .insert(&second, &png_bytes(64, 2))
                .expect_err("lease protects first"),
            RenderCacheError::QuotaUnavailable { .. }
        ));
        assert!(matches!(
            cache.repair().expect_err("repair respects active lease"),
            RenderCacheError::ActiveLeases
        ));
        assert_eq!(cache.snapshot().expect("leased snapshot").active_leases, 1);
        drop(lease);
        cache
            .insert(&second, &png_bytes(64, 2))
            .expect("insert after release");
        assert!(cache.open(&first).expect("first lookup").is_none());
        assert!(cache.open(&second).expect("second lookup").is_some());
    }

    #[test]
    fn rejects_oversized_or_mislabeled_artifacts_before_disk_mutation() {
        let temp = TestDirectory::new("invalid-artifact");
        let cache = render_cache(temp.path(), 32, 4);
        let key = preview_key(1, 1, 1200);
        assert!(matches!(
            cache
                .insert(&key, &png_bytes(64, 1))
                .expect_err("oversized"),
            RenderCacheError::ArtifactTooLarge { .. }
        ));
        assert!(matches!(
            cache.insert(&key, b"not-a-png").expect_err("signature"),
            RenderCacheError::InvalidArtifact("signature")
        ));
        assert_eq!(cache.snapshot().expect("snapshot").entry_count, 0);
    }

    #[test]
    fn checksum_failure_invalidates_only_the_corrupt_artifact() {
        let temp = TestDirectory::new("checksum");
        let cache = render_cache(temp.path(), 4096, 8);
        let first = preview_key(1, 1, 1200);
        let second = preview_key(2, 1, 1200);
        cache.insert(&first, &png_bytes(64, 1)).expect("first");
        cache.insert(&second, &png_bytes(64, 2)).expect("second");
        let first_id = key_id(&first).expect("first ID");
        let artifact = artifact_path(cache.cache_dir(), &first_id);
        let length = fs::metadata(&artifact).expect("artifact metadata").len() as usize;
        fs::write(&artifact, png_bytes(length - 8, 9)).expect("corrupt same-size artifact");

        assert!(matches!(
            cache
                .open(&first)
                .expect("open corrupt")
                .expect("indexed corrupt entry")
                .read_bytes()
                .expect_err("checksum mismatch"),
            RenderCacheError::CorruptArtifact { .. }
        ));
        assert!(cache.open(&first).expect("first lookup").is_none());
        assert_eq!(
            cache
                .open(&second)
                .expect("second lookup")
                .expect("second remains")
                .read_bytes()
                .expect("read second"),
            png_bytes(64, 2)
        );
    }

    #[test]
    fn repair_drops_missing_artifact_without_losing_healthy_entries() {
        let temp = TestDirectory::new("missing");
        let cache = render_cache(temp.path(), 4096, 8);
        let first = preview_key(1, 1, 1200);
        let second = preview_key(2, 1, 1200);
        cache.insert(&first, &png_bytes(64, 1)).expect("first");
        cache.insert(&second, &png_bytes(64, 2)).expect("second");
        let first_id = key_id(&first).expect("first ID");
        fs::remove_file(artifact_path(cache.cache_dir(), &first_id)).expect("remove artifact");

        let report = cache.repair().expect("repair");
        assert_eq!(report.dropped_invalid_entries, 1);
        assert!(cache.open(&first).expect("first lookup").is_none());
        assert!(cache.open(&second).expect("second lookup").is_some());
    }

    #[test]
    fn corrupt_index_shard_isolated_from_other_shards() {
        let temp = TestDirectory::new("shard-corruption");
        let cache = render_cache(temp.path(), 8192, 32);
        let (first, second) = keys_in_distinct_shards();
        cache.insert(&first, &png_bytes(64, 1)).expect("first");
        cache.insert(&second, &png_bytes(64, 2)).expect("second");
        let first_id = key_id(&first).expect("first ID");
        let first_shard = shard_index(&first_id).expect("first shard");
        fs::write(
            cache.cache_dir().join(shard_filename(first_shard)),
            b"corrupt",
        )
        .expect("corrupt shard");

        let report = cache.repair().expect("repair");
        assert_eq!(report.dropped_invalid_shards, 1);
        assert!(cache.open(&first).expect("first lookup").is_none());
        assert!(cache.open(&second).expect("second lookup").is_some());
    }

    #[test]
    fn repair_cleans_interrupted_writes_and_orphan_artifacts() {
        let temp = TestDirectory::new("cleanup");
        let cache = render_cache(temp.path(), 4096, 8);
        cache.snapshot().expect("initialize");
        fs::write(
            cache.cache_dir().join(".artifact-incomplete.tmp"),
            b"partial",
        )
        .expect("artifact temp");
        fs::write(
            cache.cache_dir().join(".shard-00.json.test.tmp"),
            b"partial",
        )
        .expect("index temp");
        let orphan_key = "1".repeat(64);
        let orphan_hash = "2".repeat(64);
        let orphan =
            artifact_filename(&orphan_key, &orphan_hash, RenderCacheOutputKind::PreviewPng);
        fs::write(cache.cache_dir().join(orphan), png_bytes(8, 1)).expect("orphan");

        let report = cache.repair().expect("repair");
        assert_eq!(report.removed_artifact_temps, 1);
        assert_eq!(report.removed_index_temps, 1);
        assert_eq!(report.removed_orphan_artifacts, 1);
    }

    #[test]
    fn concurrent_handles_serialize_insert_read_and_eviction() {
        let temp = TestDirectory::new("concurrent");
        let artifact_size = png_bytes(64, 1).len() as u64;
        let cache = render_cache(temp.path(), artifact_size * 16, 16);
        let barrier = Arc::new(Barrier::new(9));
        let mut handles = Vec::new();
        for worker in 0..8_u32 {
            let cache = cache.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                for offset in 0..8_u32 {
                    let page = worker * 8 + offset + 1;
                    let key = preview_key(page, 1, 1200);
                    let bytes = png_bytes(64, page as u8);
                    cache.insert(&key, &bytes).expect("concurrent insert");
                    let loaded = cache
                        .open(&key)
                        .expect("concurrent open")
                        .expect("concurrent hit")
                        .read_bytes()
                        .expect("concurrent read");
                    assert_eq!(loaded, bytes);
                }
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().expect("worker");
        }
        let snapshot = cache.snapshot().expect("snapshot");
        assert!(snapshot.entry_count <= 16);
        assert!(snapshot.artifact_bytes <= snapshot.max_bytes);
        assert_eq!(snapshot.active_leases, 0);
        cache.repair().expect("post-concurrency repair");
    }

    #[test]
    fn thousand_page_stress_keeps_metadata_and_disk_growth_bounded() {
        let temp = TestDirectory::new("thousand-pages");
        let cache = render_cache(temp.path(), 128 * 1024, 128);
        for page in 1..=1_000_u32 {
            cache
                .insert(&preview_key(page, 1, 1000), &png_bytes(64, page as u8))
                .expect("stress insert");
        }
        let snapshot = cache.snapshot().expect("snapshot");
        assert_eq!(snapshot.entry_count, 128);
        assert!(snapshot.artifact_bytes <= snapshot.max_bytes);
        let paths = files(cache.cache_dir());
        let artifact_count = paths
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("artifact-"))
            })
            .count();
        let index_bytes = paths
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("shard-") || name == "manifest.json")
            })
            .map(|path| fs::metadata(path).expect("index metadata").len())
            .sum::<u64>();
        assert_eq!(artifact_count, 128);
        assert!(index_bytes < 1024 * 1024);
        assert_eq!(cache.repair().expect("repair").evicted_entries, 0);
    }

    #[test]
    fn root_and_config_validation_prevent_ambiguous_ownership() {
        let temp = TestDirectory::new("configuration");
        assert!(matches!(
            RenderCache::new(
                Path::new("relative-cache"),
                RenderCacheConfig {
                    max_bytes: 1024,
                    max_entries: 8,
                }
            )
            .expect_err("relative root"),
            RenderCacheError::InvalidKey("renderCacheRoot")
        ));
        let cache = render_cache(temp.path(), 1024, 8);
        assert!(matches!(
            RenderCache::new(
                temp.path(),
                RenderCacheConfig {
                    max_bytes: 2048,
                    max_entries: 8,
                }
            )
            .expect_err("conflicting quota"),
            RenderCacheError::ConfigConflict
        ));
        drop(cache);
        RenderCache::new(
            temp.path(),
            RenderCacheConfig {
                max_bytes: 2048,
                max_entries: 8,
            },
        )
        .expect("new owner can change quota");
    }

    #[test]
    fn reopening_with_a_smaller_policy_evicts_to_both_limits() {
        let temp = TestDirectory::new("quota-shrink");
        let artifact = png_bytes(64, 1);
        let cache = render_cache(temp.path(), artifact.len() as u64 * 4, 4);
        for page in 1..=4 {
            cache
                .insert(&preview_key(page, 1, 1200), &png_bytes(64, page as u8))
                .expect("initial insert");
        }
        drop(cache);

        let smaller = render_cache(temp.path(), artifact.len() as u64 * 2, 2);
        let snapshot = smaller.snapshot().expect("repaired snapshot");
        assert_eq!(snapshot.entry_count, 2);
        assert!(snapshot.artifact_bytes <= snapshot.max_bytes);
        assert!(smaller
            .open(&preview_key(1, 1, 1200))
            .expect("oldest lookup")
            .is_none());
        assert!(smaller
            .open(&preview_key(4, 1, 1200))
            .expect("newest lookup")
            .is_some());
    }

    fn render_cache(root: &Path, max_bytes: u64, max_entries: usize) -> RenderCache {
        RenderCache::new(
            root,
            RenderCacheConfig {
                max_bytes,
                max_entries,
            },
        )
        .expect("render cache")
    }

    fn preview_key(page_number: u32, revision: u64, width: u32) -> RenderCacheKey {
        RenderCacheKey {
            source_fingerprint: "source-fixture".to_string(),
            page_number,
            patch_id: format!("patch-fixture-{page_number}-{revision}"),
            translation_revision: revision,
            renderer_version: "renderer-test".to_string(),
            options: RenderCacheOptions {
                output_kind: RenderCacheOutputKind::PreviewPng,
                pixel_width: Some(width),
                scale_milli: None,
            },
        }
    }

    fn page_pdf_key(page_number: u32, revision: u64) -> RenderCacheKey {
        RenderCacheKey {
            source_fingerprint: "source-fixture".to_string(),
            page_number,
            patch_id: format!("patch-fixture-{page_number}-{revision}"),
            translation_revision: revision,
            renderer_version: "renderer-test".to_string(),
            options: RenderCacheOptions {
                output_kind: RenderCacheOutputKind::TranslatedPagePdf,
                pixel_width: None,
                scale_milli: None,
            },
        }
    }

    fn png_bytes(payload_bytes: usize, seed: u8) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend((0..payload_bytes).map(|offset| seed.wrapping_add(offset as u8)));
        bytes
    }

    fn artifact_path(cache_dir: &Path, expected_key_id: &str) -> PathBuf {
        files(cache_dir)
            .into_iter()
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("artifact-{expected_key_id}-")))
            })
            .expect("artifact path")
    }

    fn keys_in_distinct_shards() -> (RenderCacheKey, RenderCacheKey) {
        let first = preview_key(1, 1, 1200);
        let first_shard = shard_index(&key_id(&first).expect("first ID")).expect("first shard");
        let second = (2..1000)
            .map(|page| preview_key(page, 1, 1200))
            .find(|key| {
                shard_index(&key_id(key).expect("candidate ID")).expect("candidate shard")
                    != first_shard
            })
            .expect("distinct shard key");
        (first, second)
    }

    fn files(directory: &Path) -> Vec<PathBuf> {
        let mut files = fs::read_dir(directory)
            .expect("read cache directory")
            .map(|entry| entry.expect("cache entry").path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-render-cache-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
