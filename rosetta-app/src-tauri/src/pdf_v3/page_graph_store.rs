use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Instant,
};

use flate2::{read::GzDecoder, Compression, GzBuilder};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    extract::page_source_hash,
    types::{PageGraph, PageGroupKind, PageReconciliationStatus, PAGE_GRAPH_SCHEMA_VERSION},
    visual_grouping::{
        MAX_FLOW_CONTAINERS_PER_PAGE, MAX_VISUAL_LINES_PER_PAGE, MAX_VISUAL_PARAGRAPHS_PER_PAGE,
    },
};

const STORE_SCHEMA_VERSION: u32 = 1;
const MANIFEST_FILENAME: &str = "manifest.json";
const PAGES_PER_SHARD: u32 = 64;
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_PAGE_GRAPH_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;

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
pub(crate) struct PageGraphStore {
    root: PathBuf,
    source_fingerprint: String,
    source_page_count: u32,
    engine_version: String,
    coordinator: Arc<StoreCoordinator>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageGraphStoreManifest {
    schema_version: u32,
    manifest_id: String,
    source_fingerprint: String,
    source_page_count: u32,
    engine_version: String,
    page_graph_schema_version: u32,
    pages_per_shard: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageGraphStoreShard {
    schema_version: u32,
    shard_id: String,
    source_fingerprint: String,
    engine_version: String,
    shard_index: u32,
    generation: u64,
    pages: Vec<PageGraphArtifactEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageGraphArtifactEntry {
    pub page_number: u32,
    pub source_page_hash: String,
    pub artifact_id: String,
    pub artifact_file: String,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageGraphCommitKind {
    Written,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageGraphCommitOutcome {
    pub kind: PageGraphCommitKind,
    pub authority: PageGraphArtifactEntry,
    pub shard_generation: u64,
    pub serialize_compress_us: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredPageGraph {
    pub page: PageGraph,
    pub authority: PageGraphArtifactEntry,
    pub shard_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageGraphStoreSnapshot {
    pub source_fingerprint: String,
    pub source_page_count: u32,
    pub engine_version: String,
    pub shard_count: u32,
    pub pages: Vec<PageGraphArtifactEntry>,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PageGraphStoreRepairReport {
    pub rebuilt_shards: u32,
    pub removed_stale_shards: u32,
    pub removed_invalid_artifacts: u32,
    pub removed_sidecars: u32,
}

#[derive(Debug)]
pub(crate) enum PageGraphStoreError {
    InvalidIdentity(&'static str),
    InvalidPageGraph(&'static str),
    SourcePageConflict {
        page_number: u32,
    },
    ArtifactConflict {
        page_number: u32,
    },
    ArtifactMismatch {
        page_number: u32,
    },
    ArtifactTooLarge {
        bytes: u64,
        maximum: u64,
    },
    IndexTooLarge {
        bytes: u64,
        maximum: u64,
    },
    IndexInvalid(String),
    Serialization(String),
    Compression(String),
    GenerationOverflow,
    LockPoisoned,
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for PageGraphStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(field) => write!(formatter, "invalid PageGraph store {field}"),
            Self::InvalidPageGraph(field) => write!(formatter, "invalid PageGraph {field}"),
            Self::SourcePageConflict { page_number } => write!(
                formatter,
                "PageGraph page {page_number} conflicts with stored source identity"
            ),
            Self::ArtifactConflict { page_number } => write!(
                formatter,
                "PageGraph page {page_number} has a different artifact for the same engine"
            ),
            Self::ArtifactMismatch { page_number } => {
                write!(
                    formatter,
                    "PageGraph artifact for page {page_number} is invalid"
                )
            }
            Self::ArtifactTooLarge { bytes, maximum } => write!(
                formatter,
                "PageGraph artifact is {bytes} bytes, above the {maximum}-byte limit"
            ),
            Self::IndexTooLarge { bytes, maximum } => write!(
                formatter,
                "PageGraph index is {bytes} bytes, above the {maximum}-byte limit"
            ),
            Self::IndexInvalid(message) => write!(formatter, "invalid PageGraph index: {message}"),
            Self::Serialization(message) => formatter.write_str(message),
            Self::Compression(message) => formatter.write_str(message),
            Self::GenerationOverflow => formatter.write_str("PageGraph shard generation overflow"),
            Self::LockPoisoned => formatter.write_str("PageGraph store lock is poisoned"),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PageGraphStoreError {}

impl PageGraphStore {
    pub(crate) fn new(
        root: &Path,
        source_fingerprint: impl Into<String>,
        source_page_count: u32,
        engine_version: impl Into<String>,
    ) -> Result<Self, PageGraphStoreError> {
        if !root.is_absolute() {
            return Err(PageGraphStoreError::InvalidIdentity("root"));
        }
        let source_fingerprint = source_fingerprint.into();
        let engine_version = engine_version.into();
        validate_identity(&source_fingerprint, "sourceFingerprint")?;
        validate_identity(&engine_version, "engineVersion")?;
        if source_page_count == 0 {
            return Err(PageGraphStoreError::InvalidIdentity("sourcePageCount"));
        }
        Ok(Self {
            root: root.to_path_buf(),
            source_fingerprint,
            source_page_count,
            engine_version,
            coordinator: store_coordinator(root)?,
        })
    }

    pub(crate) fn commit(
        &self,
        page: &PageGraph,
    ) -> Result<PageGraphCommitOutcome, PageGraphStoreError> {
        self.validate_page(page)?;
        let serialize_compress_started = Instant::now();
        let (compressed, uncompressed_bytes) = serialize_compressed_page_graph(page)?;
        let serialize_compress_us = elapsed_us(serialize_compress_started.elapsed());
        let artifact_id = format!("sha256:{}", sha256(&compressed));
        let authority = PageGraphArtifactEntry {
            page_number: page.page_number,
            source_page_hash: page.source_page_hash.clone(),
            artifact_file: artifact_filename(page.page_number, &artifact_id)?,
            artifact_id,
            uncompressed_bytes,
            compressed_bytes: compressed.len() as u64,
        };

        self.with_lock(|state| {
            self.prepare_locked(state)?;
            let index = shard_index(page.page_number);
            let mut shard = self
                .read_shard(index)?
                .unwrap_or_else(|| self.empty_shard(index, 0));
            if let Some(existing) = shard
                .pages
                .iter()
                .find(|entry| entry.page_number == page.page_number)
            {
                if existing.source_page_hash != page.source_page_hash {
                    return Err(PageGraphStoreError::SourcePageConflict {
                        page_number: page.page_number,
                    });
                }
                if existing.artifact_id != authority.artifact_id {
                    return Err(PageGraphStoreError::ArtifactConflict {
                        page_number: page.page_number,
                    });
                }
                match self.load_entry(existing) {
                    Ok(_) => {
                        return Ok(PageGraphCommitOutcome {
                            kind: PageGraphCommitKind::Unchanged,
                            authority: existing.clone(),
                            shard_generation: shard.generation,
                            serialize_compress_us,
                        });
                    }
                    Err(error @ PageGraphStoreError::Io { .. }) => return Err(error),
                    Err(_) => {}
                }
            }

            self.write_artifact(&authority, &compressed)?;
            shard.generation = shard
                .generation
                .checked_add(1)
                .ok_or(PageGraphStoreError::GenerationOverflow)?;
            shard
                .pages
                .retain(|entry| entry.page_number != page.page_number);
            shard.pages.push(authority.clone());
            shard.pages.sort_by_key(|entry| entry.page_number);
            shard.shard_id = shard_id(&shard)?;
            self.write_shard(&shard)?;
            Ok(PageGraphCommitOutcome {
                kind: PageGraphCommitKind::Written,
                authority,
                shard_generation: shard.generation,
                serialize_compress_us,
            })
        })
    }

    pub(crate) fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub(crate) fn source_page_count(&self) -> u32 {
        self.source_page_count
    }

    pub(crate) fn engine_version(&self) -> &str {
        &self.engine_version
    }

    pub(crate) fn load(
        &self,
        page_number: u32,
    ) -> Result<Option<StoredPageGraph>, PageGraphStoreError> {
        if page_number == 0 || page_number > self.source_page_count {
            return Err(PageGraphStoreError::InvalidPageGraph("pageNumber"));
        }
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            let Some(shard) = self.read_shard(shard_index(page_number))? else {
                return Ok(None);
            };
            let Some(entry) = shard
                .pages
                .iter()
                .find(|entry| entry.page_number == page_number)
            else {
                return Ok(None);
            };
            match self.load_entry(entry) {
                Ok(page) => Ok(Some(StoredPageGraph {
                    page,
                    authority: entry.clone(),
                    shard_generation: shard.generation,
                })),
                Err(_) => {
                    state.repaired = false;
                    self.repair_locked()?;
                    state.repaired = true;
                    Ok(None)
                }
            }
        })
    }

    pub(crate) fn validated_snapshot(&self) -> Result<PageGraphStoreSnapshot, PageGraphStoreError> {
        self.with_lock(|state| {
            self.prepare_locked(state)?;
            let mut pages = Vec::new();
            let mut shard_count = 0u32;
            for path in self.shard_paths()? {
                let index = parse_shard_filename(
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                )
                .ok_or_else(|| {
                    PageGraphStoreError::IndexInvalid(format!(
                        "invalid shard filename {}",
                        path.display()
                    ))
                })?;
                let shard = self.read_shard_path(&path, index)?;
                for entry in &shard.pages {
                    self.load_entry(entry)?;
                }
                shard_count = shard_count.saturating_add(1);
                pages.extend(shard.pages);
            }
            pages.sort_by_key(|entry| entry.page_number);
            let uncompressed_bytes = pages.iter().fold(0u64, |total, page| {
                total.saturating_add(page.uncompressed_bytes)
            });
            let compressed_bytes = pages.iter().fold(0u64, |total, page| {
                total.saturating_add(page.compressed_bytes)
            });
            Ok(PageGraphStoreSnapshot {
                source_fingerprint: self.source_fingerprint.clone(),
                source_page_count: self.source_page_count,
                engine_version: self.engine_version.clone(),
                shard_count,
                pages,
                uncompressed_bytes,
                compressed_bytes,
            })
        })
    }

    pub(crate) fn repair(&self) -> Result<PageGraphStoreRepairReport, PageGraphStoreError> {
        self.with_lock(|state| {
            let report = self.repair_locked()?;
            state.repaired = true;
            Ok(report)
        })
    }

    fn prepare_locked(&self, state: &mut StoreCoordinatorState) -> Result<(), PageGraphStoreError> {
        fs::create_dir_all(&self.root).map_err(|error| io_error("create", &self.root, error))?;
        let manifest_path = self.root.join(MANIFEST_FILENAME);
        if try_exists(&manifest_path)? {
            self.read_manifest()?;
        } else {
            let entries = fs::read_dir(&self.root)
                .map_err(|error| io_error("list", &self.root, error))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| io_error("list", &self.root, error))?;
            if entries
                .iter()
                .all(|entry| entry.file_name().to_str().is_some_and(is_store_sidecar))
            {
                for entry in entries {
                    let _ = fs::remove_file(entry.path());
                }
                self.write_manifest()?;
            } else {
                return Err(PageGraphStoreError::IndexInvalid(
                    "manifest is missing from a non-empty store".to_string(),
                ));
            }
        }
        if !state.repaired {
            self.repair_locked()?;
            state.repaired = true;
        }
        Ok(())
    }

    fn repair_locked(&self) -> Result<PageGraphStoreRepairReport, PageGraphStoreError> {
        self.read_manifest()?;
        let mut report = PageGraphStoreRepairReport::default();
        let mut by_shard = BTreeMap::<u32, Vec<PageGraphArtifactEntry>>::new();
        for entry in
            fs::read_dir(&self.root).map_err(|error| io_error("list", &self.root, error))?
        {
            let entry = entry.map_err(|error| io_error("list", &self.root, error))?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if is_store_sidecar(name) {
                if fs::remove_file(&path).is_ok() {
                    report.removed_sidecars = report.removed_sidecars.saturating_add(1);
                }
                continue;
            }
            if !is_artifact_filename(name) {
                continue;
            }
            match self.read_artifact_path(&path) {
                Ok((page, authority)) => {
                    let pages = by_shard.entry(shard_index(page.page_number)).or_default();
                    if let Some(existing) = pages
                        .iter()
                        .find(|entry| entry.page_number == page.page_number)
                    {
                        if existing.artifact_id != authority.artifact_id {
                            return Err(PageGraphStoreError::ArtifactConflict {
                                page_number: page.page_number,
                            });
                        }
                    } else {
                        pages.push(authority);
                    }
                }
                Err(error @ PageGraphStoreError::Io { .. }) => return Err(error),
                Err(_) => {
                    if fs::remove_file(&path).is_ok() {
                        report.removed_invalid_artifacts =
                            report.removed_invalid_artifacts.saturating_add(1);
                    }
                }
            }
        }
        for pages in by_shard.values_mut() {
            pages.sort_by_key(|entry| entry.page_number);
        }

        let expected = by_shard.keys().copied().collect::<BTreeSet<_>>();
        for path in self.shard_paths()? {
            let Some(index) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_shard_filename)
            else {
                continue;
            };
            if !expected.contains(&index) && fs::remove_file(&path).is_ok() {
                report.removed_stale_shards = report.removed_stale_shards.saturating_add(1);
            }
        }
        for (index, pages) in by_shard {
            let current = match self.read_shard(index) {
                Ok(current) => current,
                Err(error @ PageGraphStoreError::Io { .. }) => return Err(error),
                Err(_) => None,
            };
            if current.as_ref().is_some_and(|shard| shard.pages == pages) {
                continue;
            }
            let generation = current
                .as_ref()
                .map(|shard| shard.generation)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(PageGraphStoreError::GenerationOverflow)?;
            let mut shard = self.empty_shard(index, generation);
            shard.pages = pages;
            shard.shard_id = shard_id(&shard)?;
            self.write_shard(&shard)?;
            report.rebuilt_shards = report.rebuilt_shards.saturating_add(1);
        }
        Ok(report)
    }

    fn validate_page(&self, page: &PageGraph) -> Result<(), PageGraphStoreError> {
        if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
            return Err(PageGraphStoreError::InvalidPageGraph("schemaVersion"));
        }
        if page.page_number == 0 || page.page_number > self.source_page_count {
            return Err(PageGraphStoreError::InvalidPageGraph("pageNumber"));
        }
        if page.source_page_hash != page_source_hash(&self.source_fingerprint, page.page_number) {
            return Err(PageGraphStoreError::InvalidPageGraph("sourcePageHash"));
        }
        if page.reconciliation.status == PageReconciliationStatus::Unreconciled {
            return Err(PageGraphStoreError::InvalidPageGraph(
                "reconciliationStatus",
            ));
        }
        let mut atom_ids = BTreeSet::new();
        let mut atom_orders = BTreeSet::new();
        if page.atoms.iter().any(|atom| {
            atom.atom_id.is_empty()
                || !atom_ids.insert(atom.atom_id.as_str())
                || !atom_orders.insert(atom.order)
        }) {
            return Err(PageGraphStoreError::InvalidPageGraph("atoms"));
        }
        let style_ids = page
            .styles
            .iter()
            .map(|style| style.style_id.as_str())
            .collect::<BTreeSet<_>>();
        if style_ids.len() != page.styles.len()
            || page.atoms.iter().any(|atom| {
                atom.style_id
                    .as_deref()
                    .is_some_and(|style_id| !style_ids.contains(style_id))
            })
        {
            return Err(PageGraphStoreError::InvalidPageGraph("styles"));
        }
        let mut group_ids = BTreeSet::new();
        let mut line_atoms = BTreeSet::new();
        let mut paragraph_atoms = BTreeSet::new();
        let mut container_atoms = BTreeSet::new();
        let mut line_count = 0usize;
        let mut paragraph_count = 0usize;
        let mut container_count = 0usize;
        for group in &page.groups {
            if group.group_id.is_empty()
                || !group_ids.insert(group.group_id.as_str())
                || !group.confidence.is_finite()
                || !(0.0..=1.0).contains(&group.confidence)
                || group.bounds.iter().any(|value| !value.is_finite())
                || group.bounds[2] <= group.bounds[0]
                || group.bounds[3] <= group.bounds[1]
            {
                return Err(PageGraphStoreError::InvalidPageGraph("groups"));
            }
            let structured_atoms = match group.kind {
                PageGroupKind::Line => {
                    line_count = line_count.saturating_add(1);
                    Some(&mut line_atoms)
                }
                PageGroupKind::Paragraph => {
                    paragraph_count = paragraph_count.saturating_add(1);
                    Some(&mut paragraph_atoms)
                }
                PageGroupKind::FlowContainer => {
                    container_count = container_count.saturating_add(1);
                    Some(&mut container_atoms)
                }
                PageGroupKind::Column
                | PageGroupKind::Table
                | PageGroupKind::TableCell
                | PageGroupKind::Caption
                | PageGroupKind::VisualRegion
                | PageGroupKind::Unknown => None,
            };
            let mut local_atoms = BTreeSet::new();
            if group.atom_ids.iter().any(|atom_id| {
                !atom_ids.contains(atom_id.as_str()) || !local_atoms.insert(atom_id.as_str())
            }) {
                return Err(PageGraphStoreError::InvalidPageGraph("groupAtoms"));
            }
            if let Some(owned_atoms) = structured_atoms {
                if group.atom_ids.is_empty()
                    || group
                        .atom_ids
                        .iter()
                        .any(|atom_id| !owned_atoms.insert(atom_id.as_str()))
                {
                    return Err(PageGraphStoreError::InvalidPageGraph("groupOwnership"));
                }
            }
        }
        if line_count > MAX_VISUAL_LINES_PER_PAGE
            || paragraph_count > MAX_VISUAL_PARAGRAPHS_PER_PAGE
            || container_count > MAX_FLOW_CONTAINERS_PER_PAGE
        {
            return Err(PageGraphStoreError::InvalidPageGraph("groupLimits"));
        }
        let has_visual_hierarchy = line_count > 0 || paragraph_count > 0 || container_count > 0;
        if has_visual_hierarchy
            && (line_count == 0
                || paragraph_count == 0
                || container_count == 0
                || line_atoms != paragraph_atoms
                || line_atoms != container_atoms)
        {
            return Err(PageGraphStoreError::InvalidPageGraph("groupHierarchy"));
        }
        Ok(())
    }

    fn read_artifact_path(
        &self,
        path: &Path,
    ) -> Result<(PageGraph, PageGraphArtifactEntry), PageGraphStoreError> {
        let compressed = read_limited(path, MAX_COMPRESSED_BYTES)?;
        let artifact_id = format!("sha256:{}", sha256(&compressed));
        let encoded = decompress_page_graph(&compressed)?;
        let page = decode_page_graph(&encoded)?;
        self.validate_page(&page)?;
        let authority = PageGraphArtifactEntry {
            page_number: page.page_number,
            source_page_hash: page.source_page_hash.clone(),
            artifact_file: artifact_filename(page.page_number, &artifact_id)?,
            artifact_id,
            uncompressed_bytes: encoded.len() as u64,
            compressed_bytes: compressed.len() as u64,
        };
        if path.file_name().and_then(|name| name.to_str()) != Some(authority.artifact_file.as_str())
        {
            return Err(PageGraphStoreError::ArtifactMismatch {
                page_number: page.page_number,
            });
        }
        Ok((page, authority))
    }

    fn load_entry(&self, entry: &PageGraphArtifactEntry) -> Result<PageGraph, PageGraphStoreError> {
        self.validate_entry(entry)?;
        let (page, actual) = self.read_artifact_path(&self.root.join(&entry.artifact_file))?;
        if &actual != entry {
            return Err(PageGraphStoreError::ArtifactMismatch {
                page_number: entry.page_number,
            });
        }
        Ok(page)
    }

    fn validate_entry(&self, entry: &PageGraphArtifactEntry) -> Result<(), PageGraphStoreError> {
        if entry.page_number == 0
            || entry.page_number > self.source_page_count
            || entry.source_page_hash
                != page_source_hash(&self.source_fingerprint, entry.page_number)
            || entry.uncompressed_bytes == 0
            || entry.uncompressed_bytes > MAX_PAGE_GRAPH_BYTES
            || entry.compressed_bytes == 0
            || entry.compressed_bytes > MAX_COMPRESSED_BYTES
            || !is_safe_file_name(&entry.artifact_file)
            || artifact_filename(entry.page_number, &entry.artifact_id)? != entry.artifact_file
        {
            return Err(PageGraphStoreError::ArtifactMismatch {
                page_number: entry.page_number,
            });
        }
        Ok(())
    }

    fn write_artifact(
        &self,
        authority: &PageGraphArtifactEntry,
        bytes: &[u8],
    ) -> Result<(), PageGraphStoreError> {
        let path = self.root.join(&authority.artifact_file);
        if try_exists(&path)? {
            match read_limited(&path, MAX_COMPRESSED_BYTES) {
                Ok(existing) if existing == bytes => return Ok(()),
                Ok(_) => {}
                Err(error @ PageGraphStoreError::Io { .. }) => return Err(error),
                Err(_) => {}
            }
        }
        write_atomic(&path, bytes)
    }

    fn read_manifest(&self) -> Result<PageGraphStoreManifest, PageGraphStoreError> {
        let path = self.root.join(MANIFEST_FILENAME);
        let bytes = read_limited(&path, MAX_INDEX_BYTES)?;
        let manifest = serde_json::from_slice::<PageGraphStoreManifest>(&bytes)
            .map_err(|error| index_error(&path, error))?;
        if manifest.schema_version != STORE_SCHEMA_VERSION
            || manifest.source_fingerprint != self.source_fingerprint
            || manifest.source_page_count != self.source_page_count
            || manifest.engine_version != self.engine_version
            || manifest.page_graph_schema_version != PAGE_GRAPH_SCHEMA_VERSION
            || manifest.pages_per_shard != PAGES_PER_SHARD
            || manifest_id(&manifest)? != manifest.manifest_id
        {
            return Err(PageGraphStoreError::IndexInvalid(
                "manifest identity mismatch".to_string(),
            ));
        }
        Ok(manifest)
    }

    fn write_manifest(&self) -> Result<(), PageGraphStoreError> {
        let mut manifest = PageGraphStoreManifest {
            schema_version: STORE_SCHEMA_VERSION,
            manifest_id: String::new(),
            source_fingerprint: self.source_fingerprint.clone(),
            source_page_count: self.source_page_count,
            engine_version: self.engine_version.clone(),
            page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            pages_per_shard: PAGES_PER_SHARD,
        };
        manifest.manifest_id = manifest_id(&manifest)?;
        write_atomic(
            &self.root.join(MANIFEST_FILENAME),
            &encode_index(&manifest)?,
        )
    }

    fn empty_shard(&self, index: u32, generation: u64) -> PageGraphStoreShard {
        PageGraphStoreShard {
            schema_version: STORE_SCHEMA_VERSION,
            shard_id: String::new(),
            source_fingerprint: self.source_fingerprint.clone(),
            engine_version: self.engine_version.clone(),
            shard_index: index,
            generation,
            pages: Vec::new(),
        }
    }

    fn read_shard(&self, index: u32) -> Result<Option<PageGraphStoreShard>, PageGraphStoreError> {
        let path = self.root.join(shard_filename(index));
        if !try_exists(&path)? {
            return Ok(None);
        }
        self.read_shard_path(&path, index).map(Some)
    }

    fn read_shard_path(
        &self,
        path: &Path,
        expected_index: u32,
    ) -> Result<PageGraphStoreShard, PageGraphStoreError> {
        let bytes = read_limited(path, MAX_INDEX_BYTES)?;
        let shard = serde_json::from_slice::<PageGraphStoreShard>(&bytes)
            .map_err(|error| index_error(path, error))?;
        if shard.schema_version != STORE_SCHEMA_VERSION
            || shard.source_fingerprint != self.source_fingerprint
            || shard.engine_version != self.engine_version
            || shard.shard_index != expected_index
            || shard.generation == 0
            || shard_id(&shard)? != shard.shard_id
        {
            return Err(PageGraphStoreError::IndexInvalid(format!(
                "shard identity mismatch at {}",
                path.display()
            )));
        }
        let first_page = expected_index
            .checked_mul(PAGES_PER_SHARD)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| PageGraphStoreError::IndexInvalid("shard overflow".to_string()))?;
        let last_page = first_page.saturating_add(PAGES_PER_SHARD - 1);
        let mut previous = None;
        for entry in &shard.pages {
            self.validate_entry(entry)?;
            if !(first_page..=last_page).contains(&entry.page_number)
                || previous.is_some_and(|page| entry.page_number <= page)
            {
                return Err(PageGraphStoreError::IndexInvalid(
                    "shard page order mismatch".to_string(),
                ));
            }
            previous = Some(entry.page_number);
        }
        Ok(shard)
    }

    fn write_shard(&self, shard: &PageGraphStoreShard) -> Result<(), PageGraphStoreError> {
        write_atomic(
            &self.root.join(shard_filename(shard.shard_index)),
            &encode_index(shard)?,
        )
    }

    fn shard_paths(&self) -> Result<Vec<PathBuf>, PageGraphStoreError> {
        let mut paths = fs::read_dir(&self.root)
            .map_err(|error| io_error("list", &self.root, error))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(parse_shard_filename)
                    .is_some()
            })
            .collect::<Vec<_>>();
        paths.sort();
        Ok(paths)
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&mut StoreCoordinatorState) -> Result<T, PageGraphStoreError>,
    ) -> Result<T, PageGraphStoreError> {
        let mut state = self
            .coordinator
            .state
            .lock()
            .map_err(|_| PageGraphStoreError::LockPoisoned)?;
        operation(&mut state)
    }
}

fn decode_page_graph(bytes: &[u8]) -> Result<PageGraph, PageGraphStoreError> {
    enforce_size(bytes.len(), MAX_PAGE_GRAPH_BYTES)?;
    serde_json::from_slice(bytes).map_err(|error| {
        PageGraphStoreError::Serialization(format!("failed to decode PageGraph: {error}"))
    })
}

fn serialize_compressed_page_graph(
    page: &PageGraph,
) -> Result<(Vec<u8>, u64), PageGraphStoreError> {
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::fast());
    let buffered = BufWriter::with_capacity(64 * 1024, encoder);
    let mut writer = LimitedWriter::new(buffered, MAX_PAGE_GRAPH_BYTES);
    serde_json::to_writer(&mut writer, page).map_err(|error| {
        PageGraphStoreError::Serialization(format!("failed to encode PageGraph: {error}"))
    })?;
    let (mut buffered, uncompressed_bytes) = writer.into_parts();
    buffered.flush().map_err(|error| {
        PageGraphStoreError::Compression(format!("failed to flush PageGraph compression: {error}"))
    })?;
    let encoder = buffered.into_inner().map_err(|error| {
        PageGraphStoreError::Compression(format!(
            "failed to finish PageGraph buffer: {}",
            error.into_error()
        ))
    })?;
    let compressed = encoder.finish().map_err(|error| {
        PageGraphStoreError::Compression(format!("failed to finish PageGraph compression: {error}"))
    })?;
    enforce_size(compressed.len(), MAX_COMPRESSED_BYTES)?;
    Ok((compressed, uncompressed_bytes))
}

fn decompress_page_graph(bytes: &[u8]) -> Result<Vec<u8>, PageGraphStoreError> {
    enforce_size(bytes.len(), MAX_COMPRESSED_BYTES)?;
    let mut decoded = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_PAGE_GRAPH_BYTES + 1)
        .read_to_end(&mut decoded)
        .map_err(|error| {
            PageGraphStoreError::Compression(format!("failed to decompress PageGraph: {error}"))
        })?;
    enforce_size(decoded.len(), MAX_PAGE_GRAPH_BYTES)?;
    Ok(decoded)
}

fn enforce_size(bytes: usize, maximum: u64) -> Result<(), PageGraphStoreError> {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    if bytes > maximum {
        return Err(PageGraphStoreError::ArtifactTooLarge { bytes, maximum });
    }
    Ok(())
}

fn elapsed_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

struct LimitedWriter<W> {
    inner: W,
    written: u64,
    maximum: u64,
}

impl<W> LimitedWriter<W> {
    fn new(inner: W, maximum: u64) -> Self {
        Self {
            inner,
            written: 0,
            maximum,
        }
    }

    fn into_parts(self) -> (W, u64) {
        (self.inner, self.written)
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let incoming = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(incoming) > self.maximum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "PageGraph exceeds the uncompressed byte limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.written = self
            .written
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn validate_identity(value: &str, field: &'static str) -> Result<(), PageGraphStoreError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(PageGraphStoreError::InvalidIdentity(field));
    }
    Ok(())
}

fn artifact_filename(page_number: u32, artifact_id: &str) -> Result<String, PageGraphStoreError> {
    let digest = artifact_id
        .strip_prefix("sha256:")
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(PageGraphStoreError::InvalidIdentity("artifactId"))?;
    Ok(format!("page-{page_number:010}-{digest}.pagegraph.json.gz"))
}

fn is_artifact_filename(name: &str) -> bool {
    name.starts_with("page-") && name.ends_with(".pagegraph.json.gz")
}

fn is_store_sidecar(name: &str) -> bool {
    name.starts_with('.') && (name.ends_with(".tmp") || name.ends_with(".bak"))
}

fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
}

fn shard_index(page_number: u32) -> u32 {
    (page_number - 1) / PAGES_PER_SHARD
}

fn shard_filename(index: u32) -> String {
    format!("shard-{index:08}.json")
}

fn parse_shard_filename(name: &str) -> Option<u32> {
    name.strip_prefix("shard-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

fn manifest_id(manifest: &PageGraphStoreManifest) -> Result<String, PageGraphStoreError> {
    let mut canonical = manifest.clone();
    canonical.manifest_id.clear();
    Ok(format!("manifest-{}", sha256(&encode_index(&canonical)?)))
}

fn shard_id(shard: &PageGraphStoreShard) -> Result<String, PageGraphStoreError> {
    let mut canonical = shard.clone();
    canonical.shard_id.clear();
    Ok(format!("shard-{}", sha256(&encode_index(&canonical)?)))
}

fn encode_index(value: &impl Serialize) -> Result<Vec<u8>, PageGraphStoreError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| PageGraphStoreError::IndexInvalid(format!("encode failed: {error}")))?;
    let count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if count > MAX_INDEX_BYTES {
        return Err(PageGraphStoreError::IndexTooLarge {
            bytes: count,
            maximum: MAX_INDEX_BYTES,
        });
    }
    Ok(bytes)
}

fn read_limited(path: &Path, maximum: u64) -> Result<Vec<u8>, PageGraphStoreError> {
    let file = File::open(path).map_err(|error| io_error("open", path, error))?;
    let size = file
        .metadata()
        .map_err(|error| io_error("inspect", path, error))?
        .len();
    if size > maximum {
        return Err(PageGraphStoreError::ArtifactTooLarge {
            bytes: size,
            maximum,
        });
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read", path, error))?;
    enforce_size(bytes.len(), maximum)?;
    Ok(bytes)
}

fn try_exists(path: &Path) -> Result<bool, PageGraphStoreError> {
    path.try_exists()
        .map_err(|error| io_error("inspect", path, error))
}

fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), PageGraphStoreError> {
    let temp = unique_sidecar_path(target, "tmp");
    let backup = unique_sidecar_path(target, "bak");
    write_new_synced_file(&temp, bytes)?;
    let had_target = try_exists(target)?;
    if had_target {
        fs::rename(target, &backup).map_err(|error| io_error("backup", target, error))?;
    }
    if let Err(error) = fs::rename(&temp, target) {
        if had_target && !target.exists() {
            let _ = fs::rename(&backup, target);
        }
        let _ = fs::remove_file(&temp);
        return Err(io_error("commit", target, error));
    }
    if had_target {
        let _ = fs::remove_file(&backup);
    }
    sync_parent_directory(
        target
            .parent()
            .ok_or(PageGraphStoreError::InvalidIdentity("root"))?,
    )
}

fn write_new_synced_file(path: &Path, bytes: &[u8]) -> Result<(), PageGraphStoreError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| io_error("create", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync", path, error))
}

fn unique_sidecar_path(target: &Path, kind: &str) -> PathBuf {
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pagegraph");
    target.with_file_name(format!(".{name}.{}-{sequence}.{kind}", std::process::id()))
}

fn store_coordinator(root: &Path) -> Result<Arc<StoreCoordinator>, PageGraphStoreError> {
    let mut coordinators = STORE_COORDINATORS
        .lock()
        .map_err(|_| PageGraphStoreError::LockPoisoned)?;
    if let Some(coordinator) = coordinators.get(root).and_then(Weak::upgrade) {
        return Ok(coordinator);
    }
    let coordinator = Arc::new(StoreCoordinator::default());
    coordinators.insert(root.to_path_buf(), Arc::downgrade(&coordinator));
    Ok(coordinator)
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

fn index_error(path: &Path, error: serde_json::Error) -> PageGraphStoreError {
    PageGraphStoreError::IndexInvalid(format!("{}: {error}", path.display()))
}

fn io_error(operation: &'static str, path: &Path, error: std::io::Error) -> PageGraphStoreError {
    PageGraphStoreError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), PageGraphStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync directory", path, error))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), PageGraphStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{PageGraphCommitKind, PageGraphStore, PageGraphStoreError};
    use crate::pdf_v3::{
        extract::page_source_hash,
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind,
            PageReconciliationStatus, PageReconciliationSummary, PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    const SOURCE_FINGERPRINT: &str = "sha256:page-graph-store-test-source";
    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TempStore {
        path: PathBuf,
    }

    impl TempStore {
        fn new(name: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-page-graph-store-{name}-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            Self { path }
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn store(temp: &TempStore) -> PageGraphStore {
        PageGraphStore::new(&temp.path, SOURCE_FINGERPRINT, 200, "pdf-v3-test")
            .expect("PageGraph store")
    }

    fn page(page_number: u32, atom_count: u32) -> PageGraph {
        let atoms = (0..atom_count)
            .map(|order| PageAtom {
                atom_id: format!("page-{page_number}-atom-{order}"),
                source_text: "Repeated source text for deterministic compression. ".to_string(),
                source_object_id: Some(format!("page-{page_number}-object-1")),
                kind: PageAtomKind::Body,
                style_id: None,
                bounds: [10.0, 20.0, 30.0, 40.0],
                loose_bounds: None,
                origin: None,
                text_matrix: None,
                angle_degrees: Some(0.0),
                order,
                generated: false,
                hyphen: false,
                requires_translation: true,
                source_kind: PageAtomSourceKind::PdfiumVerified,
                source_provenance: None,
            })
            .collect::<Vec<_>>();
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number,
            source_page_hash: page_source_hash(SOURCE_FINGERPRINT, page_number),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms,
            styles: Vec::new(),
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary {
                status: PageReconciliationStatus::Complete,
                mapped_object_count: 1,
                preserved_object_count: 0,
                verified_atom_count: atom_count as usize,
                corrected_atom_count: 0,
                synthetic_whitespace_atom_count: 0,
                unrepresented_source_whitespace_count: 0,
                preserved_atom_count: 0,
                fallback_reasons: Vec::new(),
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn commits_loads_and_compresses_page_graph_authority() {
        let temp = TempStore::new("roundtrip");
        let store = store(&temp);
        let page = page(1, 200);

        let first = store.commit(&page).expect("first commit");
        assert_eq!(first.kind, PageGraphCommitKind::Written);
        assert!(first.authority.compressed_bytes < first.authority.uncompressed_bytes);
        let second = store.commit(&page).expect("idempotent commit");
        assert_eq!(second.kind, PageGraphCommitKind::Unchanged);

        let stored = store.load(1).expect("load").expect("stored page");
        assert_eq!(stored.page, page);
        assert_eq!(stored.authority, first.authority);
    }

    #[test]
    fn validated_snapshot_spans_shards_and_rejects_wrong_source_identity() {
        let temp = TempStore::new("snapshot");
        let store = store(&temp);
        for page_number in [1, 65, 129] {
            store
                .commit(&page(page_number, 2))
                .expect("commit sparse page");
        }
        let snapshot = store.validated_snapshot().expect("validated snapshot");
        assert_eq!(snapshot.shard_count, 3);
        assert_eq!(snapshot.pages.len(), 3);

        let mut wrong = page(2, 1);
        wrong.source_page_hash = "sha256:other".to_string();
        assert!(matches!(
            store.commit(&wrong),
            Err(PageGraphStoreError::InvalidPageGraph("sourcePageHash"))
        ));
    }

    #[test]
    fn corrupted_artifact_is_removed_and_can_be_recommitted() {
        let temp = TempStore::new("repair");
        let store = store(&temp);
        let page = page(1, 10);
        let committed = store.commit(&page).expect("commit");
        fs::write(
            temp.path.join(&committed.authority.artifact_file),
            b"corrupt",
        )
        .expect("corrupt artifact");

        assert!(store.load(1).expect("repairing load").is_none());
        let repaired = store.commit(&page).expect("recommit");
        assert_eq!(repaired.kind, PageGraphCommitKind::Written);
        assert_eq!(
            store.load(1).expect("load").expect("stored page").page,
            page
        );
    }

    #[test]
    fn rejects_invalid_visual_group_references_and_overlapping_ownership() {
        let temp = TempStore::new("invalid-groups");
        let store = store(&temp);
        let mut invalid_reference = page(1, 2);
        invalid_reference.groups = vec![PageGroup {
            group_id: "line-1".to_string(),
            kind: PageGroupKind::Line,
            atom_ids: vec!["missing-atom".to_string()],
            bounds: [10.0, 20.0, 30.0, 40.0],
            confidence: 0.9,
        }];
        assert!(matches!(
            store.commit(&invalid_reference),
            Err(PageGraphStoreError::InvalidPageGraph("groupAtoms"))
        ));

        let mut overlapping = page(1, 2);
        let first_atom = overlapping.atoms[0].atom_id.clone();
        overlapping.groups = vec![
            PageGroup {
                group_id: "line-1".to_string(),
                kind: PageGroupKind::Line,
                atom_ids: vec![first_atom.clone()],
                bounds: [10.0, 20.0, 30.0, 40.0],
                confidence: 0.9,
            },
            PageGroup {
                group_id: "line-2".to_string(),
                kind: PageGroupKind::Line,
                atom_ids: vec![first_atom],
                bounds: [10.0, 20.0, 30.0, 40.0],
                confidence: 0.9,
            },
        ];
        assert!(matches!(
            store.commit(&overlapping),
            Err(PageGraphStoreError::InvalidPageGraph("groupOwnership"))
        ));
    }

    #[test]
    fn recommit_propagates_artifact_io_failure() {
        let temp = TempStore::new("recommit-io");
        let store = store(&temp);
        let page = page(1, 10);
        let committed = store.commit(&page).expect("commit");
        let artifact_path = temp.path.join(&committed.authority.artifact_file);
        fs::remove_file(&artifact_path).expect("remove artifact");
        fs::create_dir(&artifact_path).expect("replace artifact with directory");

        assert!(matches!(
            store.commit(&page),
            Err(PageGraphStoreError::Io { .. })
        ));
    }
}
