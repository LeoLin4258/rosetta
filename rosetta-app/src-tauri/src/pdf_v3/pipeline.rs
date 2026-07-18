use std::{collections::BTreeMap, fmt, time::Instant};

use super::{
    document::DocumentHandle,
    mapping::{PageOperandMappingError, PageOperandMappingIndex},
    page_graph_store::{PageGraphStore, PageGraphStoreError},
    reconcile::build_reconciled_page_graph_from_handle_with_index,
    scheduler::{
        DurablePdfV3Scheduler, PdfV3ExtractionAuthority, PdfV3PageClaim, PdfV3SchedulerError,
    },
    types::PAGE_GRAPH_SCHEMA_VERSION,
};

const RECONCILIATION_FAILURE_REASON: &str = "pdf-v3-native-reconciliation-failed";
const PAGE_GRAPH_STORE_FAILURE_REASON: &str = "pdf-v3-page-graph-store-failed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfV3ExtractionFailureKind {
    Reconciliation,
    PageGraphStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfV3ExtractionPageOutcome {
    Committed {
        page_number: u32,
        authority: PdfV3ExtractionAuthority,
    },
    Failed {
        page_number: u32,
        kind: PdfV3ExtractionFailureKind,
        retryable: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PdfV3ExtractionBatchOutcome {
    pub claimed_pages: u32,
    pub committed_pages: u32,
    pub failed_pages: u32,
    pub claim_us: u64,
    pub reconciliation_us: u64,
    pub page_graph_store_us: u64,
    pub page_graph_serialize_compress_us: u64,
    pub scheduler_commit_us: u64,
    pub pages: Vec<PdfV3ExtractionPageOutcome>,
}

#[derive(Debug)]
pub(crate) enum PdfV3ExtractionWorkerError {
    Scheduler(PdfV3SchedulerError),
    PageIndex(PageOperandMappingError),
    Store(PageGraphStoreError),
    BindingMismatch(&'static str),
}

impl fmt::Display for PdfV3ExtractionWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheduler(error) => error.fmt(formatter),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::BindingMismatch(field) => {
                write!(formatter, "PDF v3 extraction worker {field} does not match")
            }
        }
    }
}

impl std::error::Error for PdfV3ExtractionWorkerError {}

impl From<PdfV3SchedulerError> for PdfV3ExtractionWorkerError {
    fn from(value: PdfV3SchedulerError) -> Self {
        Self::Scheduler(value)
    }
}

impl From<PageOperandMappingError> for PdfV3ExtractionWorkerError {
    fn from(value: PageOperandMappingError) -> Self {
        Self::PageIndex(value)
    }
}

impl From<PageGraphStoreError> for PdfV3ExtractionWorkerError {
    fn from(value: PageGraphStoreError) -> Self {
        Self::Store(value)
    }
}

pub(crate) struct PdfV3ExtractionWorker<'a, 'pdfium> {
    handle: &'a DocumentHandle<'pdfium>,
    scheduler: &'a DurablePdfV3Scheduler,
    store: &'a PageGraphStore,
    index: PageOperandMappingIndex,
    owner_session_id: String,
}

impl<'a, 'pdfium> PdfV3ExtractionWorker<'a, 'pdfium> {
    pub(crate) fn new(
        handle: &'a DocumentHandle<'pdfium>,
        scheduler: &'a DurablePdfV3Scheduler,
        store: &'a PageGraphStore,
        owner_session_id: impl Into<String>,
    ) -> Result<Self, PdfV3ExtractionWorkerError> {
        let binding = scheduler.extraction_binding()?;
        if binding.source_fingerprint != handle.source_fingerprint() {
            return Err(PdfV3ExtractionWorkerError::BindingMismatch(
                "sourceFingerprint",
            ));
        }
        if binding.source_page_count != handle.page_count() {
            return Err(PdfV3ExtractionWorkerError::BindingMismatch(
                "sourcePageCount",
            ));
        }
        if binding.page_graph_schema_version != PAGE_GRAPH_SCHEMA_VERSION {
            return Err(PdfV3ExtractionWorkerError::BindingMismatch(
                "pageGraphSchemaVersion",
            ));
        }
        if store.source_fingerprint() != handle.source_fingerprint()
            || store.source_page_count() != handle.page_count()
            || store.engine_version() != binding.engine_version
        {
            return Err(PdfV3ExtractionWorkerError::BindingMismatch(
                "PageGraphStore",
            ));
        }
        let index = PageOperandMappingIndex::resolve(handle, &binding.requested_pages)?;
        Ok(Self {
            handle,
            scheduler,
            store,
            index,
            owner_session_id: owner_session_id.into(),
        })
    }

    pub(crate) fn run_batch(
        &self,
        requested_limit: u32,
        mut now_ms: impl FnMut() -> u64,
    ) -> Result<PdfV3ExtractionBatchOutcome, PdfV3ExtractionWorkerError> {
        let mut outcome = PdfV3ExtractionBatchOutcome::default();
        for _ in 0..requested_limit {
            let claim_started = Instant::now();
            let Some(claim) = self
                .scheduler
                .claim_extraction(&self.owner_session_id, 1, now_ms())?
                .into_iter()
                .next()
            else {
                outcome.claim_us = outcome
                    .claim_us
                    .saturating_add(elapsed_us(claim_started.elapsed()));
                break;
            };
            outcome.claim_us = outcome
                .claim_us
                .saturating_add(elapsed_us(claim_started.elapsed()));
            outcome.claimed_pages = outcome.claimed_pages.saturating_add(1);
            let reconciliation_started = Instant::now();
            let reconciled = build_reconciled_page_graph_from_handle_with_index(
                self.handle,
                &self.index,
                claim.page_number,
            );
            outcome.reconciliation_us = outcome
                .reconciliation_us
                .saturating_add(elapsed_us(reconciliation_started.elapsed()));
            match reconciled {
                Ok(page) => {
                    let store_started = Instant::now();
                    let stored = self.store.commit(&page);
                    outcome.page_graph_store_us = outcome
                        .page_graph_store_us
                        .saturating_add(elapsed_us(store_started.elapsed()));
                    match stored {
                        Ok(committed) => {
                            outcome.page_graph_serialize_compress_us = outcome
                                .page_graph_serialize_compress_us
                                .saturating_add(committed.serialize_compress_us);
                            let authority = PdfV3ExtractionAuthority {
                                artifact_id: committed.authority.artifact_id,
                                source_page_hash: committed.authority.source_page_hash,
                            };
                            let scheduler_started = Instant::now();
                            self.scheduler.commit_extraction(
                                &self.owner_session_id,
                                &claim,
                                authority.clone(),
                                now_ms(),
                            )?;
                            outcome.scheduler_commit_us = outcome
                                .scheduler_commit_us
                                .saturating_add(elapsed_us(scheduler_started.elapsed()));
                            outcome.committed_pages = outcome.committed_pages.saturating_add(1);
                            outcome.pages.push(PdfV3ExtractionPageOutcome::Committed {
                                page_number: claim.page_number,
                                authority,
                            });
                        }
                        Err(_) => {
                            let scheduler_started = Instant::now();
                            self.fail_claim(
                                &claim,
                                PAGE_GRAPH_STORE_FAILURE_REASON,
                                true,
                                now_ms(),
                            )?;
                            outcome.scheduler_commit_us = outcome
                                .scheduler_commit_us
                                .saturating_add(elapsed_us(scheduler_started.elapsed()));
                            outcome.failed_pages = outcome.failed_pages.saturating_add(1);
                            outcome.pages.push(PdfV3ExtractionPageOutcome::Failed {
                                page_number: claim.page_number,
                                kind: PdfV3ExtractionFailureKind::PageGraphStore,
                                retryable: true,
                            });
                        }
                    }
                }
                Err(_) => {
                    let scheduler_started = Instant::now();
                    self.fail_claim(&claim, RECONCILIATION_FAILURE_REASON, false, now_ms())?;
                    outcome.scheduler_commit_us = outcome
                        .scheduler_commit_us
                        .saturating_add(elapsed_us(scheduler_started.elapsed()));
                    outcome.failed_pages = outcome.failed_pages.saturating_add(1);
                    outcome.pages.push(PdfV3ExtractionPageOutcome::Failed {
                        page_number: claim.page_number,
                        kind: PdfV3ExtractionFailureKind::Reconciliation,
                        retryable: false,
                    });
                }
            }
        }
        Ok(outcome)
    }

    fn fail_claim(
        &self,
        claim: &PdfV3PageClaim,
        reason_code: &str,
        retryable: bool,
        now_ms: u64,
    ) -> Result<(), PdfV3ExtractionWorkerError> {
        self.scheduler.fail_claim(
            &self.owner_session_id,
            claim,
            reason_code,
            retryable,
            now_ms,
        )?;
        Ok(())
    }
}

fn elapsed_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn validated_extraction_inventory(
    store: &PageGraphStore,
) -> Result<BTreeMap<u32, PdfV3ExtractionAuthority>, PdfV3ExtractionWorkerError> {
    Ok(store
        .validated_snapshot()?
        .pages
        .into_iter()
        .map(|entry| {
            (
                entry.page_number,
                PdfV3ExtractionAuthority {
                    artifact_id: entry.artifact_id,
                    source_page_hash: entry.source_page_hash,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };

    use super::{
        validated_extraction_inventory, PdfV3ExtractionPageOutcome, PdfV3ExtractionWorker,
    };
    use crate::{
        pdf_v3::{
            document::DocumentHandle,
            page_graph_store::PageGraphStore,
            page_set::PageSet,
            scheduler::{DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3SchedulerCapacity},
            types::{PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TempPipeline {
        root: PathBuf,
    }

    impl TempPipeline {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-extraction-pipeline-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("pipeline root");
            Self { root }
        }
    }

    impl Drop for TempPipeline {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn directory_file_bytes(path: &std::path::Path) -> u64 {
        fs::read_dir(path)
            .expect("list directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }

    #[test]
    fn worker_commits_page_graph_before_scheduler_extraction_authority() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = DurablePdfV3Scheduler::create(
            &temp.root.join("scheduler"),
            PdfV3RunSpec {
                run_id: "run-simple-page".to_string(),
                source_fingerprint: handle.source_fingerprint().to_string(),
                source_page_count: handle.page_count(),
                requested_pages: PageSet::all(handle.page_count()).expect("all pages"),
                source_language: "en".to_string(),
                target_language: "zh-CN".to_string(),
                engine_version: "pdf-v3-test".to_string(),
                page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
                translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
                renderer_version: "renderer-test".to_string(),
            },
            PdfV3SchedulerCapacity {
                max_extracting_pages: 1,
                max_extracted_pages: 1,
                max_translating_pages: 1,
            },
            "owner-a",
            1,
        )
        .expect("scheduler");
        let store = PageGraphStore::new(
            &temp.root.join("page-graphs"),
            handle.source_fingerprint(),
            handle.page_count(),
            "pdf-v3-test",
        )
        .expect("store");
        let worker =
            PdfV3ExtractionWorker::new(&handle, &scheduler, &store, "owner-a").expect("worker");
        let mut timestamp = 10u64;
        let batch = worker
            .run_batch(5, || {
                timestamp += 1;
                timestamp
            })
            .expect("extraction batch");

        assert_eq!(batch.claimed_pages, 1);
        assert_eq!(batch.committed_pages, 1);
        assert_eq!(batch.failed_pages, 0);
        assert!(matches!(
            batch.pages.as_slice(),
            [PdfV3ExtractionPageOutcome::Committed { page_number: 1, .. }]
        ));
        let (_, summary) = scheduler.manifest_snapshot().expect("scheduler summary");
        assert_eq!(summary.extracted_pages, 1);
        let inventory = validated_extraction_inventory(&store).expect("validated inventory");
        assert_eq!(inventory.len(), 1);
        assert_eq!(
            inventory.get(&1).expect("page authority").source_page_hash,
            store
                .load(1)
                .expect("load")
                .expect("stored page")
                .page
                .source_page_hash
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows real ten-page durable extraction pipeline probe"]
    fn manual_windows_real_ten_page_extraction_pipeline_probe() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let requested_pages = PageSet::from_pages(1..=10).expect("first ten pages");
        let scheduler_dir = temp.root.join("scheduler");
        let store_dir = temp.root.join("page-graphs");
        let scheduler = DurablePdfV3Scheduler::create(
            &scheduler_dir,
            PdfV3RunSpec {
                run_id: "run-real-ten-pages".to_string(),
                source_fingerprint: handle.source_fingerprint().to_string(),
                source_page_count: handle.page_count(),
                requested_pages,
                source_language: "en".to_string(),
                target_language: "zh-CN".to_string(),
                engine_version: "pdf-v3-test".to_string(),
                page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
                translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
                renderer_version: "renderer-test".to_string(),
            },
            PdfV3SchedulerCapacity {
                max_extracting_pages: 1,
                max_extracted_pages: 10,
                max_translating_pages: 1,
            },
            "owner-a",
            1,
        )
        .expect("scheduler");
        let store = PageGraphStore::new(
            &store_dir,
            handle.source_fingerprint(),
            handle.page_count(),
            "pdf-v3-test",
        )
        .expect("store");
        let worker =
            PdfV3ExtractionWorker::new(&handle, &scheduler, &store, "owner-a").expect("worker");
        let mut timestamp = 10u64;
        let started = Instant::now();
        let batch = worker
            .run_batch(10, || {
                timestamp += 1;
                timestamp
            })
            .expect("extraction batch");
        let elapsed_ms = started.elapsed().as_millis();
        let snapshot = store.validated_snapshot().expect("validated snapshot");
        let store_bytes = directory_file_bytes(&store_dir);
        let scheduler_bytes = directory_file_bytes(&scheduler_dir);

        assert_eq!(batch.committed_pages, 10);
        assert_eq!(snapshot.pages.len(), 10);
        println!(
            "pdf-v3 durable-extraction pages=10 elapsedMs={elapsed_ms} claimUs={} reconciliationUs={} pageGraphStoreUs={} pageGraphSerializeCompressUs={} schedulerCommitUs={} uncompressedBytes={} compressedBytes={} compressionRatio={:.4} storeDiskBytes={store_bytes} schedulerDiskBytes={scheduler_bytes}",
            batch.claim_us,
            batch.reconciliation_us,
            batch.page_graph_store_us,
            batch.page_graph_serialize_compress_us,
            batch.scheduler_commit_us,
            snapshot.uncompressed_bytes,
            snapshot.compressed_bytes,
            snapshot.compressed_bytes as f64 / snapshot.uncompressed_bytes as f64,
        );
    }
}
