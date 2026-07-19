use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde::Serialize;
use tokio::{sync::oneshot, time::MissedTickBehavior};

use crate::{
    pdf_v3::scheduler::{DurablePdfV3Scheduler, PdfV3SchedulerError},
    rosetta_jobs::path::timestamp_ms_string,
};

pub(crate) const PDF_V3_OWNER_HEARTBEAT_INTERVAL_MS: u64 = 10_000;
static PDF_V3_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3LeaseHeartbeatStatus {
    pub active: bool,
    pub interval_ms: u64,
    pub last_success_at_ms: Option<u64>,
    pub consecutive_failures: u32,
}

impl PdfV3LeaseHeartbeatStatus {
    pub(crate) fn inactive() -> Self {
        Self {
            active: false,
            interval_ms: PDF_V3_OWNER_HEARTBEAT_INTERVAL_MS,
            last_success_at_ms: None,
            consecutive_failures: 0,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PdfV3RunLifecycleError {
    InvalidRunDirectory,
    Clock,
    LockPoisoned,
    Scheduler(PdfV3SchedulerError),
}

impl fmt::Display for PdfV3RunLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRunDirectory => {
                formatter.write_str("PDF v3 lifecycle run directory is invalid")
            }
            Self::Clock => formatter.write_str("PDF v3 lifecycle could not read current time"),
            Self::LockPoisoned => formatter.write_str("PDF v3 lifecycle lock is poisoned"),
            Self::Scheduler(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PdfV3RunLifecycleError {}

impl From<PdfV3SchedulerError> for PdfV3RunLifecycleError {
    fn from(value: PdfV3SchedulerError) -> Self {
        Self::Scheduler(value)
    }
}

pub struct PdfV3RunLifecycleState {
    inner: Arc<PdfV3RunLifecycleInner>,
}

struct PdfV3RunLifecycleInner {
    session_id: String,
    heartbeat_interval_ms: u64,
    next_token: AtomicU64,
    active: Mutex<BTreeMap<PathBuf, ActiveHeartbeat>>,
}

struct ActiveHeartbeat {
    token: u64,
    stop: Option<oneshot::Sender<()>>,
    health: Arc<Mutex<HeartbeatHealth>>,
}

#[derive(Default)]
struct HeartbeatHealth {
    last_success_at_ms: Option<u64>,
    consecutive_failures: u32,
}

enum HeartbeatAttempt {
    Renewed(u64),
    Terminal,
    OwnerLost,
    Failed,
}

impl PdfV3RunLifecycleState {
    fn new(heartbeat_interval_ms: u64) -> Self {
        let session_counter = PDF_V3_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::new(PdfV3RunLifecycleInner {
                session_id: format!(
                    "pdf-v3-session-{}-{}-{}",
                    std::process::id(),
                    timestamp_ms_string(),
                    session_counter
                ),
                heartbeat_interval_ms,
                next_token: AtomicU64::new(1),
                active: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    pub(crate) fn ensure_heartbeat(
        &self,
        run_directory: &Path,
    ) -> Result<PdfV3LeaseHeartbeatStatus, PdfV3RunLifecycleError> {
        validate_run_directory(run_directory)?;
        if let Some(status) = self.active_status(run_directory)? {
            return Ok(status);
        }

        let scheduler = DurablePdfV3Scheduler::open(run_directory)?;
        let now_ms = current_time_ms()?;
        if !scheduler.renew_owner(self.session_id(), now_ms)? {
            return Ok(PdfV3LeaseHeartbeatStatus::inactive());
        }

        let mut active = self
            .inner
            .active
            .lock()
            .map_err(|_| PdfV3RunLifecycleError::LockPoisoned)?;
        if let Some(entry) = active.get(run_directory) {
            return heartbeat_status(entry, self.inner.heartbeat_interval_ms);
        }
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        let health = Arc::new(Mutex::new(HeartbeatHealth {
            last_success_at_ms: Some(now_ms),
            consecutive_failures: 0,
        }));
        let (stop, stop_rx) = oneshot::channel();
        active.insert(
            run_directory.to_path_buf(),
            ActiveHeartbeat {
                token,
                stop: Some(stop),
                health: health.clone(),
            },
        );
        drop(active);
        spawn_heartbeat(
            self.inner.clone(),
            run_directory.to_path_buf(),
            token,
            health,
            stop_rx,
        );
        self.heartbeat_status(run_directory)
    }

    pub(crate) fn heartbeat_status(
        &self,
        run_directory: &Path,
    ) -> Result<PdfV3LeaseHeartbeatStatus, PdfV3RunLifecycleError> {
        validate_run_directory(run_directory)?;
        Ok(self
            .active_status(run_directory)?
            .unwrap_or_else(PdfV3LeaseHeartbeatStatus::inactive))
    }

    pub(crate) fn stop_heartbeat(
        &self,
        run_directory: &Path,
    ) -> Result<PdfV3LeaseHeartbeatStatus, PdfV3RunLifecycleError> {
        validate_run_directory(run_directory)?;
        let removed = self
            .inner
            .active
            .lock()
            .map_err(|_| PdfV3RunLifecycleError::LockPoisoned)?
            .remove(run_directory);
        if let Some(mut entry) = removed {
            if let Some(stop) = entry.stop.take() {
                let _ = stop.send(());
            }
        }
        Ok(PdfV3LeaseHeartbeatStatus::inactive())
    }

    pub fn shutdown(&self) {
        let Ok(mut active) = self.inner.active.lock() else {
            return;
        };
        for (_, mut entry) in std::mem::take(&mut *active) {
            if let Some(stop) = entry.stop.take() {
                let _ = stop.send(());
            }
        }
    }

    fn active_status(
        &self,
        run_directory: &Path,
    ) -> Result<Option<PdfV3LeaseHeartbeatStatus>, PdfV3RunLifecycleError> {
        let active = self
            .inner
            .active
            .lock()
            .map_err(|_| PdfV3RunLifecycleError::LockPoisoned)?;
        active
            .get(run_directory)
            .map(|entry| heartbeat_status(entry, self.inner.heartbeat_interval_ms))
            .transpose()
    }
}

impl Default for PdfV3RunLifecycleState {
    fn default() -> Self {
        Self::new(PDF_V3_OWNER_HEARTBEAT_INTERVAL_MS)
    }
}

impl Drop for PdfV3RunLifecycleState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn heartbeat_status(
    entry: &ActiveHeartbeat,
    interval_ms: u64,
) -> Result<PdfV3LeaseHeartbeatStatus, PdfV3RunLifecycleError> {
    let health = entry
        .health
        .lock()
        .map_err(|_| PdfV3RunLifecycleError::LockPoisoned)?;
    Ok(PdfV3LeaseHeartbeatStatus {
        active: true,
        interval_ms,
        last_success_at_ms: health.last_success_at_ms,
        consecutive_failures: health.consecutive_failures,
    })
}

fn spawn_heartbeat(
    inner: Arc<PdfV3RunLifecycleInner>,
    run_directory: PathBuf,
    token: u64,
    health: Arc<Mutex<HeartbeatHealth>>,
    mut stop: oneshot::Receiver<()>,
) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_millis(inner.heartbeat_interval_ms));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            let attempt = tokio::select! {
                _ = &mut stop => break,
                _ = interval.tick() => {
                    let path = run_directory.clone();
                    let session_id = inner.session_id.clone();
                    tokio::task::spawn_blocking(move || heartbeat_once(&path, &session_id))
                        .await
                        .unwrap_or(HeartbeatAttempt::Failed)
                }
            };
            match attempt {
                HeartbeatAttempt::Renewed(now_ms) => {
                    if let Ok(mut health) = health.lock() {
                        health.last_success_at_ms = Some(now_ms);
                        health.consecutive_failures = 0;
                    }
                }
                HeartbeatAttempt::Failed => {
                    if let Ok(mut health) = health.lock() {
                        health.consecutive_failures = health.consecutive_failures.saturating_add(1);
                    }
                }
                HeartbeatAttempt::Terminal | HeartbeatAttempt::OwnerLost => break,
            }
        }
        if let Ok(mut active) = inner.active.lock() {
            if active
                .get(&run_directory)
                .is_some_and(|entry| entry.token == token)
            {
                active.remove(&run_directory);
            }
        }
    });
}

fn heartbeat_once(run_directory: &Path, session_id: &str) -> HeartbeatAttempt {
    let scheduler = match DurablePdfV3Scheduler::open(run_directory) {
        Ok(scheduler) => scheduler,
        Err(_) => return HeartbeatAttempt::Failed,
    };
    let now_ms = match current_time_ms() {
        Ok(now_ms) => now_ms,
        Err(_) => return HeartbeatAttempt::Failed,
    };
    match scheduler.renew_owner(session_id, now_ms) {
        Ok(true) => HeartbeatAttempt::Renewed(now_ms),
        Ok(false) => HeartbeatAttempt::Terminal,
        Err(PdfV3SchedulerError::OwnerMismatch) => HeartbeatAttempt::OwnerLost,
        Err(_) => HeartbeatAttempt::Failed,
    }
}

fn current_time_ms() -> Result<u64, PdfV3RunLifecycleError> {
    timestamp_ms_string()
        .parse::<u64>()
        .map_err(|_| PdfV3RunLifecycleError::Clock)
}

fn validate_run_directory(run_directory: &Path) -> Result<(), PdfV3RunLifecycleError> {
    if !run_directory.is_absolute() || !run_directory.is_dir() {
        return Err(PdfV3RunLifecycleError::InvalidRunDirectory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, SystemTime},
    };

    use crate::pdf_v3::{
        page_set::PageSet,
        scheduler::{DurablePdfV3Scheduler, PdfV3RunSpec, PdfV3SchedulerCapacity},
        types::{PAGE_GRAPH_SCHEMA_VERSION, TRANSLATION_PATCH_SCHEMA_VERSION},
    };

    use super::{PdfV3RunLifecycleError, PdfV3RunLifecycleState};

    #[tokio::test]
    async fn native_heartbeat_renews_owner_and_stops_on_terminal_state() {
        let run = TestRun::new();
        let lifecycle = PdfV3RunLifecycleState::new(20);
        let scheduler = run.create(lifecycle.session_id());

        let started = lifecycle
            .ensure_heartbeat(&run.path)
            .expect("start heartbeat");
        assert!(started.active);
        assert_eq!(started.interval_ms, 20);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let renewed = scheduler.status_snapshot().expect("renewed scheduler");
        assert!(renewed.owner_lease_updated_at_ms > 1);
        let health = lifecycle
            .heartbeat_status(&run.path)
            .expect("heartbeat health");
        assert!(health.active);
        assert_eq!(health.consecutive_failures, 0);

        scheduler
            .request_cancel(
                lifecycle.session_id(),
                renewed.owner_lease_updated_at_ms + 1,
                "test",
            )
            .expect("request cancel");
        scheduler
            .finish_cancellation(
                lifecycle.session_id(),
                renewed.owner_lease_updated_at_ms + 2,
            )
            .expect("finish cancel");
        for _ in 0..20 {
            if !lifecycle
                .heartbeat_status(&run.path)
                .expect("terminal status")
                .active
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !lifecycle
                .heartbeat_status(&run.path)
                .expect("stopped heartbeat")
                .active
        );
        lifecycle.shutdown();
    }

    #[tokio::test]
    async fn heartbeat_never_adopts_a_run_owned_by_another_session() {
        let run = TestRun::new();
        let _scheduler = run.create("owner-other");
        let lifecycle = PdfV3RunLifecycleState::new(20);

        assert!(matches!(
            lifecycle.ensure_heartbeat(&run.path),
            Err(PdfV3RunLifecycleError::Scheduler(
                crate::pdf_v3::scheduler::PdfV3SchedulerError::OwnerMismatch
            ))
        ));
        assert!(
            !lifecycle
                .heartbeat_status(&run.path)
                .expect("inactive status")
                .active
        );
    }

    #[tokio::test]
    async fn shutdown_stops_active_heartbeats_without_another_renewal() {
        let run = TestRun::new();
        let lifecycle = PdfV3RunLifecycleState::new(20);
        let scheduler = run.create(lifecycle.session_id());
        lifecycle
            .ensure_heartbeat(&run.path)
            .expect("start heartbeat");
        let before_shutdown = scheduler
            .status_snapshot()
            .expect("status before shutdown")
            .owner_lease_updated_at_ms;

        lifecycle.shutdown();
        assert!(
            !lifecycle
                .heartbeat_status(&run.path)
                .expect("status after shutdown")
                .active
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            scheduler
                .status_snapshot()
                .expect("status after wait")
                .owner_lease_updated_at_ms,
            before_shutdown
        );
    }

    #[test]
    fn heartbeat_status_serializes_only_bounded_health_fields() {
        let encoded = serde_json::to_value(super::PdfV3LeaseHeartbeatStatus {
            active: true,
            interval_ms: 10_000,
            last_success_at_ms: Some(42),
            consecutive_failures: 3,
        })
        .expect("encode heartbeat status");

        assert_eq!(
            encoded,
            serde_json::json!({
                "active": true,
                "intervalMs": 10_000,
                "lastSuccessAtMs": 42,
                "consecutiveFailures": 3
            })
        );
    }

    struct TestRun {
        path: PathBuf,
    }

    impl TestRun {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            Self {
                path: std::env::temp_dir().join(format!(
                    "rosetta-pdf-v3-lifecycle-{}-{nanos}",
                    std::process::id()
                )),
            }
        }

        fn create(&self, owner: &str) -> DurablePdfV3Scheduler {
            DurablePdfV3Scheduler::create(
                &self.path,
                PdfV3RunSpec {
                    run_id: "run-lifecycle".to_string(),
                    source_fingerprint: "sha256:source".to_string(),
                    source_page_count: 1,
                    requested_pages: PageSet::all(1).expect("PageSet"),
                    source_language: "en".to_string(),
                    target_language: "zh-CN".to_string(),
                    engine_version: "pdf-v3-lifecycle-test".to_string(),
                    page_graph_schema_version: PAGE_GRAPH_SCHEMA_VERSION,
                    translation_patch_schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
                    renderer_version: "renderer-test".to_string(),
                },
                PdfV3SchedulerCapacity {
                    max_extracting_pages: 1,
                    max_extracted_pages: 1,
                    max_translating_pages: 1,
                },
                owner,
                1,
            )
            .expect("scheduler")
        }
    }

    impl Drop for TestRun {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
