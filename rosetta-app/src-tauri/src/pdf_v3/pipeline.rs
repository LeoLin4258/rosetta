use std::{collections::BTreeMap, fmt, future::Future, pin::Pin, time::Instant};

use super::{
    document::DocumentHandle,
    mapping::{PageOperandMappingError, PageOperandMappingIndex},
    page_graph_store::{PageGraphStore, PageGraphStoreError},
    patch_store::{TranslationPatchStore, TranslationPatchStoreError},
    reconcile::build_reconciled_page_graph_from_handle_with_index,
    scheduler::{
        DurablePdfV3Scheduler, PdfV3ExtractionAuthority, PdfV3PageClaim, PdfV3PatchAuthority,
        PdfV3RecoveryInventory, PdfV3SchedulerError, PdfV3TranslationBinding,
        PdfV3TranslationCommit,
    },
    translation_patch::{ensure_translation_patch_renderer_resolved, validate_translation_patch},
    types::{TranslationPatch, PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
};

const RECONCILIATION_FAILURE_REASON: &str = "pdf-v3-native-reconciliation-failed";
const PAGE_GRAPH_STORE_FAILURE_REASON: &str = "pdf-v3-page-graph-store-failed";
const PAGE_GRAPH_AUTHORITY_FAILURE_REASON: &str = "pdf-v3-page-graph-authority-unavailable";
const INVALID_TRANSLATION_PATCH_REASON: &str = "pdf-v3-translation-patch-invalid";
const TRANSLATION_PATCH_STORE_FAILURE_REASON: &str = "pdf-v3-translation-patch-store-failed";

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PdfV3TranslationPageResult {
    Patch(TranslationPatch),
    Preserved { reason_code: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfV3TranslationProcessFailure {
    pub reason_code: &'static str,
    pub retryable: bool,
}

pub(crate) trait PdfV3TranslationPageProcessor {
    fn process_page<'a>(
        &'a mut self,
        page: &'a super::types::PageGraph,
        binding: &'a PdfV3TranslationBinding,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PdfV3TranslationPageResult, PdfV3TranslationProcessFailure>>
                + Send
                + 'a,
        >,
    >;

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfV3TranslationFailureKind {
    PageGraphAuthority,
    Processor,
    InvalidPatch,
    PatchStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfV3TranslationPageOutcome {
    Committed {
        page_number: u32,
        authority: PdfV3PatchAuthority,
    },
    Preserved {
        page_number: u32,
        reason_code: &'static str,
    },
    Failed {
        page_number: u32,
        kind: PdfV3TranslationFailureKind,
        retryable: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PdfV3TranslationBatchOutcome {
    pub claimed_pages: u32,
    pub committed_pages: u32,
    pub preserved_pages: u32,
    pub failed_pages: u32,
    pub claim_us: u64,
    pub page_graph_load_us: u64,
    pub processor_us: u64,
    pub patch_store_us: u64,
    pub scheduler_commit_us: u64,
    pub patch_bytes: u64,
    pub uncompressed_patch_bytes: u64,
    pub first_committed_page_number: Option<u32>,
    pub first_committed_page_us: Option<u64>,
    pub pages: Vec<PdfV3TranslationPageOutcome>,
}

#[derive(Debug)]
pub(crate) enum PdfV3TranslationWorkerError {
    Scheduler(PdfV3SchedulerError),
    PageGraphStore(PageGraphStoreError),
    PatchStore(TranslationPatchStoreError),
    BindingMismatch(&'static str),
}

impl fmt::Display for PdfV3TranslationWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheduler(error) => error.fmt(formatter),
            Self::PageGraphStore(error) => error.fmt(formatter),
            Self::PatchStore(error) => error.fmt(formatter),
            Self::BindingMismatch(field) => {
                write!(
                    formatter,
                    "PDF v3 translation worker {field} does not match"
                )
            }
        }
    }
}

impl std::error::Error for PdfV3TranslationWorkerError {}

impl From<PdfV3SchedulerError> for PdfV3TranslationWorkerError {
    fn from(value: PdfV3SchedulerError) -> Self {
        Self::Scheduler(value)
    }
}

impl From<PageGraphStoreError> for PdfV3TranslationWorkerError {
    fn from(value: PageGraphStoreError) -> Self {
        Self::PageGraphStore(value)
    }
}

impl From<TranslationPatchStoreError> for PdfV3TranslationWorkerError {
    fn from(value: TranslationPatchStoreError) -> Self {
        Self::PatchStore(value)
    }
}

pub(crate) struct PdfV3TranslationWorker<'a> {
    scheduler: &'a DurablePdfV3Scheduler,
    page_graph_store: &'a PageGraphStore,
    patch_store: &'a TranslationPatchStore,
    binding: PdfV3TranslationBinding,
    owner_session_id: String,
}

impl<'a> PdfV3TranslationWorker<'a> {
    pub(crate) fn new(
        scheduler: &'a DurablePdfV3Scheduler,
        page_graph_store: &'a PageGraphStore,
        patch_store: &'a TranslationPatchStore,
        owner_session_id: impl Into<String>,
    ) -> Result<Self, PdfV3TranslationWorkerError> {
        let binding = scheduler.translation_binding()?;
        validate_translation_binding(&binding, page_graph_store, patch_store)?;
        Ok(Self {
            scheduler,
            page_graph_store,
            patch_store,
            binding,
            owner_session_id: owner_session_id.into(),
        })
    }

    pub(crate) async fn run_batch(
        &self,
        requested_limit: u32,
        mut now_ms: impl FnMut() -> u64,
        processor: &mut impl PdfV3TranslationPageProcessor,
    ) -> Result<PdfV3TranslationBatchOutcome, PdfV3TranslationWorkerError> {
        let batch_started = Instant::now();
        let mut outcome = PdfV3TranslationBatchOutcome::default();
        for _ in 0..requested_limit {
            let claim_started = Instant::now();
            let Some(claim) = self
                .scheduler
                .claim_translation(&self.owner_session_id, 1, now_ms())?
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

            let page_graph_started = Instant::now();
            let stored_page = self.page_graph_store.load(claim.page_number);
            outcome.page_graph_load_us = outcome
                .page_graph_load_us
                .saturating_add(elapsed_us(page_graph_started.elapsed()));
            let Some(stored_page) = stored_page.ok().flatten().filter(|stored| {
                claim.extraction.as_ref().is_some_and(|authority| {
                    authority.artifact_id == stored.authority.artifact_id
                        && authority.source_page_hash == stored.authority.source_page_hash
                })
            }) else {
                self.fail_claim(
                    &claim,
                    PAGE_GRAPH_AUTHORITY_FAILURE_REASON,
                    true,
                    PdfV3TranslationFailureKind::PageGraphAuthority,
                    now_ms(),
                    &mut outcome,
                )?;
                continue;
            };

            let processor_started = Instant::now();
            let processed = processor
                .process_page(&stored_page.page, &self.binding)
                .await;
            outcome.processor_us = outcome
                .processor_us
                .saturating_add(elapsed_us(processor_started.elapsed()));
            match processed {
                Ok(PdfV3TranslationPageResult::Patch(patch)) => {
                    if processor.is_cancelled() {
                        self.fail_claim(
                            &claim,
                            "pdf-v3-translation-cancelled",
                            true,
                            PdfV3TranslationFailureKind::Processor,
                            now_ms(),
                            &mut outcome,
                        )?;
                        continue;
                    }
                    if !self.patch_matches_binding(&stored_page.page, &patch) {
                        self.fail_claim(
                            &claim,
                            INVALID_TRANSLATION_PATCH_REASON,
                            false,
                            PdfV3TranslationFailureKind::InvalidPatch,
                            now_ms(),
                            &mut outcome,
                        )?;
                        continue;
                    }
                    let store_started = Instant::now();
                    let committed = self.patch_store.commit(&stored_page.page, &patch);
                    outcome.patch_store_us = outcome
                        .patch_store_us
                        .saturating_add(elapsed_us(store_started.elapsed()));
                    match committed {
                        Ok(committed) => {
                            let authority = PdfV3PatchAuthority {
                                patch_id: patch.patch_id,
                                translation_revision: committed.translation_revision,
                            };
                            let scheduler_started = Instant::now();
                            self.scheduler.commit_translation(
                                &self.owner_session_id,
                                &claim,
                                PdfV3TranslationCommit::Patch(authority.clone()),
                                now_ms(),
                            )?;
                            outcome.scheduler_commit_us = outcome
                                .scheduler_commit_us
                                .saturating_add(elapsed_us(scheduler_started.elapsed()));
                            outcome.committed_pages = outcome.committed_pages.saturating_add(1);
                            if outcome.first_committed_page_us.is_none() {
                                outcome.first_committed_page_number = Some(claim.page_number);
                                outcome.first_committed_page_us =
                                    Some(elapsed_us(batch_started.elapsed()));
                            }
                            outcome.patch_bytes =
                                outcome.patch_bytes.saturating_add(committed.patch_bytes);
                            outcome.uncompressed_patch_bytes = outcome
                                .uncompressed_patch_bytes
                                .saturating_add(committed.uncompressed_patch_bytes);
                            outcome.pages.push(PdfV3TranslationPageOutcome::Committed {
                                page_number: claim.page_number,
                                authority,
                            });
                        }
                        Err(error) => {
                            let retryable = matches!(
                                error,
                                TranslationPatchStoreError::Io { .. }
                                    | TranslationPatchStoreError::LockPoisoned
                            );
                            self.fail_claim(
                                &claim,
                                TRANSLATION_PATCH_STORE_FAILURE_REASON,
                                retryable,
                                PdfV3TranslationFailureKind::PatchStore,
                                now_ms(),
                                &mut outcome,
                            )?;
                        }
                    }
                }
                Ok(PdfV3TranslationPageResult::Preserved { reason_code }) => {
                    let scheduler_started = Instant::now();
                    self.scheduler.commit_translation(
                        &self.owner_session_id,
                        &claim,
                        PdfV3TranslationCommit::Preserved {
                            reason_code: reason_code.to_string(),
                        },
                        now_ms(),
                    )?;
                    outcome.scheduler_commit_us = outcome
                        .scheduler_commit_us
                        .saturating_add(elapsed_us(scheduler_started.elapsed()));
                    outcome.preserved_pages = outcome.preserved_pages.saturating_add(1);
                    outcome.pages.push(PdfV3TranslationPageOutcome::Preserved {
                        page_number: claim.page_number,
                        reason_code,
                    });
                }
                Err(failure) => {
                    self.fail_claim(
                        &claim,
                        failure.reason_code,
                        failure.retryable,
                        PdfV3TranslationFailureKind::Processor,
                        now_ms(),
                        &mut outcome,
                    )?;
                }
            }
        }
        Ok(outcome)
    }

    fn patch_matches_binding(
        &self,
        page: &super::types::PageGraph,
        patch: &TranslationPatch,
    ) -> bool {
        patch.schema_version == self.binding.translation_patch_schema_version
            && patch.target_language == self.binding.target_language
            && patch.renderer_version == self.binding.renderer_version
            && validate_translation_patch(page, patch).is_ok()
            && ensure_translation_patch_renderer_resolved(patch).is_ok()
    }

    fn fail_claim(
        &self,
        claim: &PdfV3PageClaim,
        reason_code: &'static str,
        retryable: bool,
        kind: PdfV3TranslationFailureKind,
        now_ms: u64,
        outcome: &mut PdfV3TranslationBatchOutcome,
    ) -> Result<(), PdfV3TranslationWorkerError> {
        let scheduler_started = Instant::now();
        self.scheduler.fail_claim(
            &self.owner_session_id,
            claim,
            reason_code,
            retryable,
            now_ms,
        )?;
        outcome.scheduler_commit_us = outcome
            .scheduler_commit_us
            .saturating_add(elapsed_us(scheduler_started.elapsed()));
        outcome.failed_pages = outcome.failed_pages.saturating_add(1);
        outcome.pages.push(PdfV3TranslationPageOutcome::Failed {
            page_number: claim.page_number,
            kind,
            retryable,
        });
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

pub(crate) fn validated_recovery_inventory(
    scheduler: &DurablePdfV3Scheduler,
    page_graph_store: &PageGraphStore,
    patch_store: &TranslationPatchStore,
    expected_translation_revision: u64,
) -> Result<PdfV3RecoveryInventory, PdfV3TranslationWorkerError> {
    if expected_translation_revision == 0 {
        return Err(PdfV3TranslationWorkerError::BindingMismatch(
            "translationRevision",
        ));
    }
    let binding = scheduler.translation_binding()?;
    validate_translation_binding(&binding, page_graph_store, patch_store)?;
    let extraction_snapshot = page_graph_store.validated_snapshot()?;
    let extractions = extraction_snapshot
        .pages
        .into_iter()
        .filter(|entry| binding.requested_pages.contains(entry.page_number))
        .map(|entry| {
            (
                entry.page_number,
                PdfV3ExtractionAuthority {
                    artifact_id: entry.artifact_id,
                    source_page_hash: entry.source_page_hash,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut patches = BTreeMap::new();
    for entry in patch_store.snapshot()?.pages.into_iter().filter(|entry| {
        binding.requested_pages.contains(entry.page_number)
            && entry.translation_revision == expected_translation_revision
    }) {
        let Some(extraction) = extractions.get(&entry.page_number) else {
            continue;
        };
        let Some(stored_page) = page_graph_store.load(entry.page_number)? else {
            continue;
        };
        if extraction.artifact_id != stored_page.authority.artifact_id
            || extraction.source_page_hash != stored_page.authority.source_page_hash
        {
            continue;
        }
        let Some(stored_patch) = patch_store.load(&stored_page.page)? else {
            continue;
        };
        if stored_patch.patch.patch_id != entry.patch_id
            || stored_patch.patch.translation_revision != entry.translation_revision
        {
            continue;
        }
        patches.insert(
            entry.page_number,
            PdfV3PatchAuthority {
                patch_id: entry.patch_id,
                translation_revision: entry.translation_revision,
            },
        );
    }
    Ok(PdfV3RecoveryInventory {
        extractions,
        patches,
    })
}

fn validate_translation_binding(
    binding: &PdfV3TranslationBinding,
    page_graph_store: &PageGraphStore,
    patch_store: &TranslationPatchStore,
) -> Result<(), PdfV3TranslationWorkerError> {
    if binding.page_graph_schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(PdfV3TranslationWorkerError::BindingMismatch(
            "pageGraphSchemaVersion",
        ));
    }
    if binding.translation_patch_schema_version != TRANSLATION_PATCH_SCHEMA_VERSION {
        return Err(PdfV3TranslationWorkerError::BindingMismatch(
            "translationPatchSchemaVersion",
        ));
    }
    if page_graph_store.source_fingerprint() != binding.source_fingerprint
        || page_graph_store.source_page_count() != binding.source_page_count
        || page_graph_store.engine_version() != binding.engine_version
    {
        return Err(PdfV3TranslationWorkerError::BindingMismatch(
            "PageGraphStore",
        ));
    }
    if patch_store.source_fingerprint() != binding.source_fingerprint
        || patch_store.target_language() != binding.target_language
    {
        return Err(PdfV3TranslationWorkerError::BindingMismatch(
            "TranslationPatchStore",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        future::Future,
        path::PathBuf,
        pin::Pin,
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };

    use super::{
        validated_extraction_inventory, validated_recovery_inventory, PdfV3ExtractionPageOutcome,
        PdfV3ExtractionWorker, PdfV3TranslationPageOutcome, PdfV3TranslationPageProcessor,
        PdfV3TranslationPageResult, PdfV3TranslationProcessFailure, PdfV3TranslationWorker,
    };
    use crate::{
        pdf_v3::{
            document::DocumentHandle,
            page_graph_store::PageGraphStore,
            page_set::PageSet,
            patch_store::TranslationPatchStore,
            scheduler::{
                DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3SchedulerCapacity,
                PdfV3TranslationBinding,
            },
            translation_patch::{build_translation_patch, TranslationPatchDraft},
            types::{PageGraph, PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    enum TestTranslationBehavior {
        EmptyPatch {
            renderer_version: Option<String>,
        },
        Preserved {
            reason_code: &'static str,
        },
        Failed {
            reason_code: &'static str,
            retryable: bool,
        },
    }

    struct TestTranslationProcessor {
        behavior: TestTranslationBehavior,
        cancel_after_process: bool,
        cancelled: bool,
    }

    impl PdfV3TranslationPageProcessor for TestTranslationProcessor {
        fn process_page<'a>(
            &'a mut self,
            page: &'a PageGraph,
            binding: &'a PdfV3TranslationBinding,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<PdfV3TranslationPageResult, PdfV3TranslationProcessFailure>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                match &self.behavior {
                    TestTranslationBehavior::EmptyPatch { renderer_version } => {
                        let patch = build_translation_patch(
                            page,
                            TranslationPatchDraft {
                                target_language: binding.target_language.clone(),
                                translation_revision: 1,
                                provider_id: "provider-test".to_string(),
                                model_id: "model-test".to_string(),
                                renderer_version: renderer_version
                                    .clone()
                                    .unwrap_or_else(|| binding.renderer_version.clone()),
                                entries: Vec::new(),
                            },
                        )
                        .expect("resolved empty patch");
                        if self.cancel_after_process {
                            self.cancelled = true;
                        }
                        Ok(PdfV3TranslationPageResult::Patch(patch))
                    }
                    TestTranslationBehavior::Preserved { reason_code } => {
                        Ok(PdfV3TranslationPageResult::Preserved { reason_code })
                    }
                    TestTranslationBehavior::Failed {
                        reason_code,
                        retryable,
                    } => Err(PdfV3TranslationProcessFailure {
                        reason_code,
                        retryable: *retryable,
                    }),
                }
            })
        }

        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
    }

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

    fn test_scheduler(
        temp: &TempPipeline,
        handle: &DocumentHandle<'_>,
        run_id: &str,
    ) -> DurablePdfV3Scheduler {
        DurablePdfV3Scheduler::create(
            &temp.root.join("scheduler"),
            PdfV3RunSpec {
                run_id: run_id.to_string(),
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
        .expect("scheduler")
    }

    fn test_page_graph_store(temp: &TempPipeline, handle: &DocumentHandle<'_>) -> PageGraphStore {
        PageGraphStore::new(
            &temp.root.join("page-graphs"),
            handle.source_fingerprint(),
            handle.page_count(),
            "pdf-v3-test",
        )
        .expect("PageGraph store")
    }

    fn extract_test_page(
        handle: &DocumentHandle<'_>,
        scheduler: &DurablePdfV3Scheduler,
        store: &PageGraphStore,
        timestamp: &mut u64,
    ) {
        let worker =
            PdfV3ExtractionWorker::new(handle, scheduler, store, "owner-a").expect("worker");
        let batch = worker
            .run_batch(1, || {
                *timestamp += 1;
                *timestamp
            })
            .expect("extraction batch");
        assert_eq!(batch.committed_pages, 1);
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

    #[tokio::test]
    async fn translation_worker_commits_patch_authority_and_builds_recovery_inventory() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-translation-worker");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);

        let worker =
            PdfV3TranslationWorker::new(&scheduler, &page_graph_store, &patch_store, "owner-a")
                .expect("translation worker");
        let mut processor = TestTranslationProcessor {
            behavior: TestTranslationBehavior::EmptyPatch {
                renderer_version: None,
            },
            cancel_after_process: false,
            cancelled: false,
        };
        let batch = worker
            .run_batch(
                1,
                || {
                    timestamp += 1;
                    timestamp
                },
                &mut processor,
            )
            .await
            .expect("translation batch");

        assert_eq!(batch.claimed_pages, 1);
        assert_eq!(batch.committed_pages, 1);
        assert_eq!(batch.failed_pages, 0);
        assert_eq!(batch.first_committed_page_number, Some(1));
        assert!(batch.first_committed_page_us.is_some());
        assert!(matches!(
            batch.pages.as_slice(),
            [PdfV3TranslationPageOutcome::Committed { page_number: 1, .. }]
        ));
        let (_, summary) = scheduler.manifest_snapshot().expect("scheduler summary");
        assert_eq!(summary.completed_pages, 1);
        let inventory =
            validated_recovery_inventory(&scheduler, &page_graph_store, &patch_store, 1)
                .expect("recovery inventory");
        assert_eq!(inventory.extractions.len(), 1);
        assert_eq!(inventory.patches.len(), 1);
    }

    #[test]
    fn recovery_promotes_patch_committed_before_scheduler_state() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-translation-recovery");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);
        let stored_page = page_graph_store
            .load(1)
            .expect("load PageGraph")
            .expect("stored PageGraph");
        let patch = build_translation_patch(
            &stored_page.page,
            TranslationPatchDraft {
                target_language: "zh-CN".to_string(),
                translation_revision: 1,
                provider_id: "provider-test".to_string(),
                model_id: "model-test".to_string(),
                renderer_version: "renderer-test".to_string(),
                entries: Vec::new(),
            },
        )
        .expect("resolved empty patch");
        patch_store
            .commit(&stored_page.page, &patch)
            .expect("durable patch commit");

        let wrong_revision =
            validated_recovery_inventory(&scheduler, &page_graph_store, &patch_store, 2)
                .expect("wrong-revision inventory");
        assert!(wrong_revision.patches.is_empty());
        let inventory =
            validated_recovery_inventory(&scheduler, &page_graph_store, &patch_store, 1)
                .expect("recovery inventory");
        let report = scheduler
            .recover_stale_owner("owner-b", 200, 100, &inventory)
            .expect("recover committed patch");
        assert_eq!(report.promoted_patches, 1);
        let (_, summary) = scheduler.manifest_snapshot().expect("scheduler summary");
        assert_eq!(summary.completed_pages, 1);
    }

    #[tokio::test]
    async fn translation_worker_rejects_patch_outside_scheduler_binding() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-invalid-translation-patch");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);
        let worker =
            PdfV3TranslationWorker::new(&scheduler, &page_graph_store, &patch_store, "owner-a")
                .expect("translation worker");
        let mut processor = TestTranslationProcessor {
            behavior: TestTranslationBehavior::EmptyPatch {
                renderer_version: Some("wrong-renderer".to_string()),
            },
            cancel_after_process: false,
            cancelled: false,
        };
        let batch = worker
            .run_batch(
                1,
                || {
                    timestamp += 1;
                    timestamp
                },
                &mut processor,
            )
            .await
            .expect("translation batch");

        assert_eq!(batch.failed_pages, 1);
        assert!(patch_store
            .snapshot()
            .expect("patch snapshot")
            .pages
            .is_empty());
        assert!(matches!(
            batch.pages.as_slice(),
            [PdfV3TranslationPageOutcome::Failed {
                page_number: 1,
                retryable: false,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn translation_worker_checks_cancellation_before_durable_patch_commit() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-cancel-before-patch-commit");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);
        let worker =
            PdfV3TranslationWorker::new(&scheduler, &page_graph_store, &patch_store, "owner-a")
                .expect("translation worker");
        let mut processor = TestTranslationProcessor {
            behavior: TestTranslationBehavior::EmptyPatch {
                renderer_version: None,
            },
            cancel_after_process: true,
            cancelled: false,
        };

        let batch = worker
            .run_batch(
                1,
                || {
                    timestamp += 1;
                    timestamp
                },
                &mut processor,
            )
            .await
            .expect("translation batch");

        assert_eq!(batch.failed_pages, 1);
        assert!(patch_store
            .snapshot()
            .expect("patch snapshot")
            .pages
            .is_empty());
        assert!(matches!(
            batch.pages.as_slice(),
            [PdfV3TranslationPageOutcome::Failed {
                page_number: 1,
                retryable: true,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn translation_worker_never_commits_a_failed_renderer_result() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-renderer-failure");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);
        let worker =
            PdfV3TranslationWorker::new(&scheduler, &page_graph_store, &patch_store, "owner-a")
                .expect("translation worker");
        let mut processor = TestTranslationProcessor {
            behavior: TestTranslationBehavior::Failed {
                reason_code: "pdf-v3-translation-renderer-failed",
                retryable: false,
            },
            cancel_after_process: false,
            cancelled: false,
        };

        let batch = worker
            .run_batch(
                1,
                || {
                    timestamp += 1;
                    timestamp
                },
                &mut processor,
            )
            .await
            .expect("translation batch");

        assert_eq!(batch.failed_pages, 1);
        assert!(patch_store
            .snapshot()
            .expect("patch snapshot")
            .pages
            .is_empty());
    }

    #[tokio::test]
    async fn translation_worker_commits_explicit_preservation_without_patch() {
        let _guard = pdfium_test_lock();
        let temp = TempPipeline::new();
        let source = fixture_path("simple-one-page.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let scheduler = test_scheduler(&temp, &handle, "run-preserved-page");
        let page_graph_store = test_page_graph_store(&temp, &handle);
        let patch_store = TranslationPatchStore::new(
            &temp.root.join("translations"),
            handle.source_fingerprint(),
            "zh-CN",
        )
        .expect("patch store");
        let mut timestamp = 10u64;
        extract_test_page(&handle, &scheduler, &page_graph_store, &mut timestamp);
        let worker =
            PdfV3TranslationWorker::new(&scheduler, &page_graph_store, &patch_store, "owner-a")
                .expect("translation worker");
        let mut processor = TestTranslationProcessor {
            behavior: TestTranslationBehavior::Preserved {
                reason_code: "pdf-v3-unsupported-page",
            },
            cancel_after_process: false,
            cancelled: false,
        };
        let batch = worker
            .run_batch(
                1,
                || {
                    timestamp += 1;
                    timestamp
                },
                &mut processor,
            )
            .await
            .expect("translation batch");

        assert_eq!(batch.preserved_pages, 1);
        assert_eq!(batch.first_committed_page_number, None);
        assert_eq!(batch.first_committed_page_us, None);
        assert!(patch_store
            .snapshot()
            .expect("patch snapshot")
            .pages
            .is_empty());
        let (_, summary) = scheduler.manifest_snapshot().expect("scheduler summary");
        assert_eq!(summary.preserved_pages, 1);
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
