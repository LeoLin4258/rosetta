use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::oneshot;

use crate::{
    pdf_v3::{
        document::{DocumentHandle, VerifiedDocumentIdentity},
        page_graph_store::PageGraphStore,
        patch_store::TranslationPatchStore,
        pipeline::{PdfV3ExtractionWorker, PdfV3TranslationWorker},
        scheduler::{DurablePdfV3Scheduler, PdfV3RunState, PdfV3SchedulerError},
        source_object::PdfSourceObjectStore,
    },
    rosetta_jobs::formats::pdf::runtime::{get_pdfium, lock_pdfium},
};

use super::{
    v3_component::ResolvedPdfV3TranslationComponent,
    v3_lifecycle::PdfV3RunLifecycleState,
    v3_processor::{PdfV3LocalPageProcessor, PdfV3LocalPageProcessorConfig},
    v3_runtime::{
        load_translation_runtime_manifest, BoundPdfV3TranslationRuntime,
        PdfV3TranslationRuntimeManifest,
    },
};

const EXTRACTION_BATCH_LIMIT: u32 = 2;
const TRANSLATION_BATCH_LIMIT: u32 = 1;
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const QUIESCENT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PdfV3RunWorkerStage {
    Starting,
    Extracting,
    Translating,
    Waiting,
    Stopping,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3RunWorkerStatus {
    pub active: bool,
    pub stage: Option<PdfV3RunWorkerStage>,
    pub last_progress_at_ms: Option<u64>,
    pub consecutive_failures: u32,
}

impl PdfV3RunWorkerStatus {
    pub(crate) fn inactive() -> Self {
        Self::default()
    }
}

#[derive(Default)]
pub struct PdfV3RunWorkerState {
    inner: Arc<PdfV3RunWorkerInner>,
}

#[derive(Default)]
struct PdfV3RunWorkerInner {
    next_token: AtomicU64,
    active: Mutex<BTreeMap<PathBuf, ActiveWorker>>,
}

struct ActiveWorker {
    token: u64,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    health: Arc<Mutex<WorkerHealth>>,
    completed: Option<oneshot::Receiver<()>>,
}

struct WorkerHealth {
    stage: PdfV3RunWorkerStage,
    last_progress_at_ms: Option<u64>,
    consecutive_failures: u32,
}

struct PreparedWorker {
    scheduler: DurablePdfV3Scheduler,
    manifest: PdfV3TranslationRuntimeManifest,
    runtime: BoundPdfV3TranslationRuntime,
    pdf_v3_directory: PathBuf,
}

impl PdfV3RunWorkerState {
    pub(crate) fn ensure_worker(
        &self,
        app: AppHandle,
        lifecycle: PdfV3RunLifecycleState,
        run_directory: &Path,
        source_identity: VerifiedDocumentIdentity,
        component: ResolvedPdfV3TranslationComponent,
    ) -> Result<PdfV3RunWorkerStatus, String> {
        validate_run_directory(run_directory)?;
        if !source_identity.source_path().is_absolute() || !source_identity.source_path().is_file()
        {
            return Err("PDF v3 worker source is unavailable".to_string());
        }

        let mut active = self
            .inner
            .active
            .lock()
            .map_err(|_| "PDF v3 worker registry is unavailable".to_string())?;
        if let Some(entry) = active.get(run_directory) {
            return worker_status(entry);
        }

        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let health = Arc::new(Mutex::new(WorkerHealth {
            stage: PdfV3RunWorkerStage::Starting,
            last_progress_at_ms: None,
            consecutive_failures: 0,
        }));
        let (completed_tx, completed_rx) = oneshot::channel();
        active.insert(
            run_directory.to_path_buf(),
            ActiveWorker {
                token,
                stop: Arc::clone(&stop),
                cancel: Arc::clone(&cancel),
                health: Arc::clone(&health),
                completed: Some(completed_rx),
            },
        );
        drop(active);

        let inner = Arc::clone(&self.inner);
        let run_directory = run_directory.to_path_buf();
        let task_directory = run_directory.clone();
        let owner_session_id = lifecycle.session_id().to_string();
        tauri::async_runtime::spawn(async move {
            run_supervisor(
                app,
                &task_directory,
                source_identity,
                component,
                owner_session_id,
                Arc::clone(&stop),
                Arc::clone(&cancel),
                Arc::clone(&health),
            )
            .await;
            let _ = lifecycle.stop_heartbeat(&task_directory);
            unload_worker(&inner, &task_directory, token);
            let _ = completed_tx.send(());
        });

        self.worker_status(&run_directory)
    }

    pub(crate) fn worker_status(
        &self,
        run_directory: &Path,
    ) -> Result<PdfV3RunWorkerStatus, String> {
        validate_run_directory(run_directory)?;
        let active = self
            .inner
            .active
            .lock()
            .map_err(|_| "PDF v3 worker registry is unavailable".to_string())?;
        active
            .get(run_directory)
            .map(worker_status)
            .transpose()
            .map(|status| status.unwrap_or_else(PdfV3RunWorkerStatus::inactive))
    }

    pub(crate) fn request_cancel(&self, run_directory: &Path) -> Result<(), String> {
        validate_run_directory(run_directory)?;
        if let Some(entry) = self
            .inner
            .active
            .lock()
            .map_err(|_| "PDF v3 worker registry is unavailable".to_string())?
            .get(run_directory)
        {
            entry.cancel.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(crate) fn stop_worker(&self, run_directory: &Path) -> Result<(), String> {
        validate_run_directory(run_directory)?;
        let removed = self
            .inner
            .active
            .lock()
            .map_err(|_| "PDF v3 worker registry is unavailable".to_string())?
            .remove(run_directory);
        if let Some(entry) = removed {
            entry.stop.store(true, Ordering::SeqCst);
            entry.cancel.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        let entries = {
            let Ok(mut active) = self.inner.active.lock() else {
                return;
            };
            std::mem::take(&mut *active)
                .into_values()
                .collect::<Vec<_>>()
        };
        stop_and_wait(entries).await;
    }

    pub(crate) async fn shutdown_job(&self, job_directory: &Path) -> Result<(), String> {
        if !job_directory.is_absolute() || !job_directory.is_dir() {
            return Err("PDF v3 worker job directory is invalid".to_string());
        }
        let runs_directory = job_directory.join("pdf-v3").join("runs");
        let entries = {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "PDF v3 worker registry is unavailable".to_string())?;
            let paths = active
                .keys()
                .filter(|path| path.starts_with(&runs_directory))
                .cloned()
                .collect::<Vec<_>>();
            paths
                .into_iter()
                .filter_map(|path| active.remove(&path))
                .collect::<Vec<_>>()
        };
        stop_and_wait(entries).await;
        Ok(())
    }

    pub fn request_shutdown(&self) {
        let Ok(active) = self.inner.active.lock() else {
            return;
        };
        for entry in active.values() {
            entry.stop.store(true, Ordering::SeqCst);
            entry.cancel.store(true, Ordering::SeqCst);
        }
    }
}

async fn stop_and_wait(entries: Vec<ActiveWorker>) {
    for entry in &entries {
        entry.stop.store(true, Ordering::SeqCst);
        entry.cancel.store(true, Ordering::SeqCst);
    }
    for mut entry in entries {
        if let Some(completed) = entry.completed.take() {
            let _ = completed.await;
        }
    }
}

impl Drop for PdfV3RunWorkerState {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

async fn run_supervisor(
    app: AppHandle,
    run_directory: &Path,
    source_identity: VerifiedDocumentIdentity,
    component: ResolvedPdfV3TranslationComponent,
    owner_session_id: String,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    health: Arc<Mutex<WorkerHealth>>,
) {
    let prepare_directory = run_directory.to_path_buf();
    let prepare_identity = source_identity.clone();
    let prepare_owner = owner_session_id.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        prepare_worker(
            &prepare_directory,
            &prepare_identity,
            component,
            &prepare_owner,
        )
    })
    .await;
    let prepared = match prepared {
        Ok(Ok(prepared)) => prepared,
        _ => {
            record_failure(&health);
            return;
        }
    };
    if stop.load(Ordering::SeqCst) {
        return;
    }

    let extraction_scheduler = prepared.scheduler.clone();
    let extraction_manifest = prepared.manifest.clone();
    let extraction_root = prepared.pdf_v3_directory.join("extraction");
    let extraction_identity = source_identity.clone();
    let extraction_stop = Arc::clone(&stop);
    let extraction_health = Arc::clone(&health);
    let extraction_owner = owner_session_id.clone();
    let extraction = tokio::task::spawn_blocking(move || {
        run_extraction_loop(
            &app,
            extraction_scheduler,
            extraction_manifest,
            extraction_root,
            extraction_identity,
            extraction_owner,
            extraction_stop,
            extraction_health,
        )
    });

    let translation_scheduler = prepared.scheduler;
    let translation_root = prepared.pdf_v3_directory.join("extraction");
    let patches_root = prepared.pdf_v3_directory.join("translations");
    let translation_source = source_identity.source_path().to_path_buf();
    let translation_stop = Arc::clone(&stop);
    let translation_cancel = Arc::clone(&cancel);
    let translation_health = Arc::clone(&health);
    let runtime_handle = tokio::runtime::Handle::current();
    let settle_owner = owner_session_id.clone();
    let translation = tokio::task::spawn_blocking(move || {
        run_translation_loop(
            runtime_handle,
            translation_scheduler,
            prepared.runtime,
            translation_root,
            patches_root,
            translation_source,
            owner_session_id,
            translation_stop,
            translation_cancel,
            translation_health,
        )
    });

    let _ = tokio::join!(extraction, translation);
    stop.store(true, Ordering::SeqCst);
    set_stage(&health, PdfV3RunWorkerStage::Stopping);

    let settle_directory = run_directory.to_path_buf();
    let _ =
        tokio::task::spawn_blocking(move || settle_cancellation(&settle_directory, &settle_owner))
            .await;
}

fn prepare_worker(
    run_directory: &Path,
    source_identity: &VerifiedDocumentIdentity,
    component: ResolvedPdfV3TranslationComponent,
    owner_session_id: &str,
) -> Result<PreparedWorker, String> {
    let prepared = prepare_worker_binding(run_directory, source_identity, component)?;
    let status = prepared
        .scheduler
        .status_snapshot()
        .map_err(|error| error.to_string())?;
    if status.owner_session_id != owner_session_id {
        return Err("PDF v3 worker owner identity changed".to_string());
    }
    Ok(prepared)
}

pub(crate) fn validate_worker_binding(
    run_directory: &Path,
    source_identity: &VerifiedDocumentIdentity,
    component: &ResolvedPdfV3TranslationComponent,
) -> Result<(), String> {
    prepare_worker_binding(run_directory, source_identity, component.clone()).map(drop)
}

fn prepare_worker_binding(
    run_directory: &Path,
    source_identity: &VerifiedDocumentIdentity,
    component: ResolvedPdfV3TranslationComponent,
) -> Result<PreparedWorker, String> {
    let scheduler =
        DurablePdfV3Scheduler::open(run_directory).map_err(|error| error.to_string())?;
    let binding = scheduler
        .translation_binding()
        .map_err(|error| error.to_string())?;
    if source_identity.source_fingerprint() != binding.source_fingerprint {
        return Err("PDF v3 worker source identity changed".to_string());
    }
    let manifest =
        load_translation_runtime_manifest(run_directory).map_err(|error| error.to_string())?;
    if manifest.component != component.component {
        return Err("PDF v3 worker component identity changed".to_string());
    }
    let runtime = BoundPdfV3TranslationRuntime::new(
        &binding,
        manifest.clone(),
        component.provider,
        component.regular_font,
        component.bold_font,
    )
    .map_err(|error| error.to_string())?;
    let pdf_v3_directory = run_directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "PDF v3 worker run layout is invalid".to_string())?
        .to_path_buf();
    Ok(PreparedWorker {
        scheduler,
        manifest,
        runtime,
        pdf_v3_directory,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_extraction_loop(
    app: &AppHandle,
    scheduler: DurablePdfV3Scheduler,
    manifest: PdfV3TranslationRuntimeManifest,
    extraction_root: PathBuf,
    source_identity: VerifiedDocumentIdentity,
    owner_session_id: String,
    stop: Arc<AtomicBool>,
    health: Arc<Mutex<WorkerHealth>>,
) {
    let opening_guard = lock_pdfium();
    let pdfium = match get_pdfium(app) {
        Ok(pdfium) => pdfium,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let handle = match DocumentHandle::open_verified(pdfium, source_identity) {
        Ok(handle) => handle,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let store = match PageGraphStore::new(
        &extraction_root,
        manifest.source_fingerprint,
        manifest.source_page_count,
        manifest.engine_version,
    ) {
        Ok(store) => store,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let expected_owner = owner_session_id.clone();
    let worker = match PdfV3ExtractionWorker::new(&handle, &scheduler, &store, owner_session_id) {
        Ok(worker) => worker,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    drop(opening_guard);
    while !stop.load(Ordering::SeqCst) {
        let snapshot = match scheduler.status_snapshot() {
            Ok(snapshot) => snapshot,
            Err(PdfV3SchedulerError::OwnerMismatch) => break,
            Err(_) => {
                record_failure(&health);
                thread::sleep(failure_backoff(&health));
                continue;
            }
        };
        if snapshot.owner_session_id != expected_owner {
            break;
        }
        match snapshot.run_state {
            PdfV3RunState::Running => {
                set_stage(&health, PdfV3RunWorkerStage::Extracting);
                let result = {
                    let _operation_guard = lock_pdfium();
                    worker.run_batch(EXTRACTION_BATCH_LIMIT, current_time_ms)
                };
                match result {
                    Ok(outcome) => {
                        record_success(&health, outcome.claimed_pages != 0);
                        if outcome.claimed_pages == 0 {
                            set_stage(&health, PdfV3RunWorkerStage::Waiting);
                            thread::sleep(ACTIVE_POLL_INTERVAL);
                        }
                    }
                    Err(crate::pdf_v3::pipeline::PdfV3ExtractionWorkerError::Scheduler(
                        PdfV3SchedulerError::RunNotClaimable(_),
                    )) => thread::sleep(ACTIVE_POLL_INTERVAL),
                    Err(crate::pdf_v3::pipeline::PdfV3ExtractionWorkerError::Scheduler(
                        PdfV3SchedulerError::OwnerMismatch,
                    )) => break,
                    Err(_) => {
                        record_failure(&health);
                        thread::sleep(failure_backoff(&health));
                    }
                }
            }
            PdfV3RunState::Paused => {
                set_stage(&health, PdfV3RunWorkerStage::Waiting);
                thread::sleep(QUIESCENT_POLL_INTERVAL);
            }
            PdfV3RunState::Cancelling | PdfV3RunState::Cancelled | PdfV3RunState::Completed => {
                break
            }
        }
    }
    let _closing_guard = lock_pdfium();
    drop(worker);
    drop(handle);
}

#[allow(clippy::too_many_arguments)]
fn run_translation_loop(
    runtime_handle: tokio::runtime::Handle,
    scheduler: DurablePdfV3Scheduler,
    runtime: BoundPdfV3TranslationRuntime,
    extraction_root: PathBuf,
    patches_root: PathBuf,
    source_path: PathBuf,
    owner_session_id: String,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    health: Arc<Mutex<WorkerHealth>>,
) {
    let binding = match scheduler.translation_binding() {
        Ok(binding) => binding,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let page_graph_store = match PageGraphStore::new(
        &extraction_root,
        binding.source_fingerprint.clone(),
        binding.source_page_count,
        binding.engine_version.clone(),
    ) {
        Ok(store) => store,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let patch_store = match TranslationPatchStore::new(
        &patches_root,
        binding.source_fingerprint.clone(),
        binding.target_language.clone(),
    ) {
        Ok(store) => store,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let source_objects = match PdfSourceObjectStore::open(&source_path) {
        Ok(source) => source,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let config = PdfV3LocalPageProcessorConfig::from_runtime(&runtime, Arc::clone(&cancel));
    let mut processor = match PdfV3LocalPageProcessor::new(&source_objects, &binding, config) {
        Ok(processor) => processor,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };
    let worker = match PdfV3TranslationWorker::new(
        &scheduler,
        &page_graph_store,
        &patch_store,
        owner_session_id,
    ) {
        Ok(worker) => worker,
        Err(_) => {
            record_failure(&health);
            stop.store(true, Ordering::SeqCst);
            return;
        }
    };

    while !stop.load(Ordering::SeqCst) {
        let snapshot = match scheduler.status_snapshot() {
            Ok(snapshot) => snapshot,
            Err(PdfV3SchedulerError::OwnerMismatch) => break,
            Err(_) => {
                record_failure(&health);
                thread::sleep(failure_backoff(&health));
                continue;
            }
        };
        match snapshot.run_state {
            PdfV3RunState::Running => {
                set_stage(&health, PdfV3RunWorkerStage::Translating);
                let result = runtime_handle.block_on(worker.run_batch(
                    TRANSLATION_BATCH_LIMIT,
                    current_time_ms,
                    &mut processor,
                ));
                match result {
                    Ok(outcome) => {
                        record_success(&health, outcome.claimed_pages != 0);
                        if outcome.claimed_pages == 0 {
                            set_stage(&health, PdfV3RunWorkerStage::Waiting);
                            thread::sleep(ACTIVE_POLL_INTERVAL);
                        }
                    }
                    Err(crate::pdf_v3::pipeline::PdfV3TranslationWorkerError::Scheduler(
                        PdfV3SchedulerError::RunNotClaimable(_),
                    )) => thread::sleep(ACTIVE_POLL_INTERVAL),
                    Err(crate::pdf_v3::pipeline::PdfV3TranslationWorkerError::Scheduler(
                        PdfV3SchedulerError::OwnerMismatch,
                    )) => break,
                    Err(_) => {
                        record_failure(&health);
                        thread::sleep(failure_backoff(&health));
                    }
                }
            }
            PdfV3RunState::Paused => {
                set_stage(&health, PdfV3RunWorkerStage::Waiting);
                thread::sleep(QUIESCENT_POLL_INTERVAL);
            }
            PdfV3RunState::Cancelling => {
                cancel.store(true, Ordering::SeqCst);
                break;
            }
            PdfV3RunState::Cancelled | PdfV3RunState::Completed => break,
        }
    }
}

fn settle_cancellation(run_directory: &Path, owner_session_id: &str) {
    let Ok(scheduler) = DurablePdfV3Scheduler::open(run_directory) else {
        return;
    };
    let Ok(snapshot) = scheduler.status_snapshot() else {
        return;
    };
    if snapshot.owner_session_id == owner_session_id
        && snapshot.run_state == PdfV3RunState::Cancelling
        && snapshot.summary.extracting_pages == 0
        && snapshot.summary.translating_pages == 0
    {
        let _ = scheduler.finish_cancellation(owner_session_id, current_time_ms());
    }
}

fn record_success(health: &Arc<Mutex<WorkerHealth>>, progressed: bool) {
    if let Ok(mut health) = health.lock() {
        health.consecutive_failures = 0;
        if progressed {
            health.last_progress_at_ms = Some(current_time_ms());
        }
    }
}

fn record_failure(health: &Arc<Mutex<WorkerHealth>>) {
    if let Ok(mut health) = health.lock() {
        health.consecutive_failures = health.consecutive_failures.saturating_add(1);
    }
}

fn failure_backoff(health: &Arc<Mutex<WorkerHealth>>) -> Duration {
    let failures = health
        .lock()
        .map(|health| health.consecutive_failures)
        .unwrap_or(1)
        .min(5);
    Duration::from_millis(25_u64.saturating_mul(1_u64 << failures)).min(MAX_FAILURE_BACKOFF)
}

fn set_stage(health: &Arc<Mutex<WorkerHealth>>, stage: PdfV3RunWorkerStage) {
    if let Ok(mut health) = health.lock() {
        health.stage = stage;
    }
}

fn worker_status(entry: &ActiveWorker) -> Result<PdfV3RunWorkerStatus, String> {
    let health = entry
        .health
        .lock()
        .map_err(|_| "PDF v3 worker health is unavailable".to_string())?;
    Ok(PdfV3RunWorkerStatus {
        active: true,
        stage: Some(health.stage),
        last_progress_at_ms: health.last_progress_at_ms,
        consecutive_failures: health.consecutive_failures,
    })
}

fn unload_worker(inner: &PdfV3RunWorkerInner, run_directory: &Path, token: u64) {
    if let Ok(mut active) = inner.active.lock() {
        if active
            .get(run_directory)
            .is_some_and(|entry| entry.token == token)
        {
            active.remove(run_directory);
        }
    }
}

fn validate_run_directory(run_directory: &Path) -> Result<(), String> {
    if !run_directory.is_absolute() || !run_directory.is_dir() {
        return Err("PDF v3 worker run directory is invalid".to_string());
    }
    Ok(())
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, SystemTime},
    };

    use tokio::sync::oneshot;

    use super::{
        unload_worker, ActiveWorker, PdfV3RunWorkerStage, PdfV3RunWorkerState,
        PdfV3RunWorkerStatus, WorkerHealth,
    };

    #[test]
    fn worker_status_serializes_only_bounded_health_fields() {
        let encoded = serde_json::to_value(PdfV3RunWorkerStatus {
            active: true,
            stage: Some(PdfV3RunWorkerStage::Translating),
            last_progress_at_ms: Some(42),
            consecutive_failures: 3,
        })
        .expect("encode worker status");

        assert_eq!(
            encoded,
            serde_json::json!({
                "active": true,
                "stage": "translating",
                "lastProgressAtMs": 42,
                "consecutiveFailures": 3
            })
        );
    }

    #[tokio::test]
    async fn registry_keeps_one_task_per_run_and_cancel_is_level_triggered() {
        let run = TestRun::new("one-task");
        let state = PdfV3RunWorkerState::default();
        let finish = Arc::new(AtomicBool::new(false));

        let (inserted, cancel) = register_test_worker(&state, &run.path, Arc::clone(&finish));
        assert!(inserted);
        let (inserted_again, same_cancel) =
            register_test_worker(&state, &run.path, Arc::clone(&finish));
        assert!(!inserted_again);
        assert!(Arc::ptr_eq(&cancel, &same_cancel));
        assert_eq!(state.inner.active.lock().expect("registry").len(), 1);

        state.request_cancel(&run.path).expect("cancel worker");
        assert!(cancel.load(Ordering::SeqCst));
        state.shutdown().await;
        assert!(
            !state
                .worker_status(&run.path)
                .expect("inactive status")
                .active
        );
    }

    #[tokio::test]
    async fn terminal_task_auto_unloads_and_shutdown_waits_for_tasks() {
        let terminal_run = TestRun::new("terminal");
        let shutdown_run = TestRun::new("shutdown");
        let state = PdfV3RunWorkerState::default();
        let terminal = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(false));
        assert!(register_test_worker(&state, &terminal_run.path, Arc::clone(&terminal)).0);
        assert!(register_test_worker(&state, &shutdown_run.path, Arc::clone(&pending)).0);

        terminal.store(true, Ordering::SeqCst);
        for _ in 0..50 {
            if !state
                .worker_status(&terminal_run.path)
                .expect("terminal status")
                .active
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            !state
                .worker_status(&terminal_run.path)
                .expect("unloaded status")
                .active
        );
        assert!(
            state
                .worker_status(&shutdown_run.path)
                .expect("active status")
                .active
        );

        state.shutdown().await;
        assert!(
            !state
                .worker_status(&shutdown_run.path)
                .expect("shutdown status")
                .active
        );
    }

    #[tokio::test]
    async fn job_shutdown_waits_only_for_workers_below_that_job() {
        let first_job = TestRun::new("job-first");
        let second_job = TestRun::new("job-second");
        let first_run = first_job.path.join("pdf-v3/runs/run-1");
        let second_run = second_job.path.join("pdf-v3/runs/run-2");
        fs::create_dir_all(&first_run).expect("create first run");
        fs::create_dir_all(&second_run).expect("create second run");
        let state = PdfV3RunWorkerState::default();
        assert!(register_test_worker(&state, &first_run, Arc::new(AtomicBool::new(false))).0);
        assert!(register_test_worker(&state, &second_run, Arc::new(AtomicBool::new(false))).0);

        state
            .shutdown_job(&first_job.path)
            .await
            .expect("shutdown first job");
        assert!(
            !state
                .worker_status(&first_run)
                .expect("first status")
                .active
        );
        assert!(
            state
                .worker_status(&second_run)
                .expect("second status")
                .active
        );

        state.shutdown().await;
    }

    fn register_test_worker(
        state: &PdfV3RunWorkerState,
        run_directory: &Path,
        finish: Arc<AtomicBool>,
    ) -> (bool, Arc<AtomicBool>) {
        let mut active = state.inner.active.lock().expect("registry");
        if let Some(entry) = active.get(run_directory) {
            return (false, Arc::clone(&entry.cancel));
        }
        let token = state.inner.next_token.fetch_add(1, Ordering::Relaxed) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let health = Arc::new(std::sync::Mutex::new(WorkerHealth {
            stage: PdfV3RunWorkerStage::Waiting,
            last_progress_at_ms: None,
            consecutive_failures: 0,
        }));
        let (completed_tx, completed_rx) = oneshot::channel();
        active.insert(
            run_directory.to_path_buf(),
            ActiveWorker {
                token,
                stop: Arc::clone(&stop),
                cancel: Arc::clone(&cancel),
                health,
                completed: Some(completed_rx),
            },
        );
        drop(active);

        let inner = Arc::clone(&state.inner);
        let path = run_directory.to_path_buf();
        tauri::async_runtime::spawn(async move {
            while !stop.load(Ordering::SeqCst) && !finish.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            unload_worker(&inner, &path, token);
            let _ = completed_tx.send(());
        });
        (true, cancel)
    }

    struct TestRun {
        path: PathBuf,
    }

    impl TestRun {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-worker-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test run");
            Self { path }
        }
    }

    impl Drop for TestRun {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
