use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

use crate::{
    managed_pdf2zh::{
        self,
        worker::{PdfPageResult, PdfPrepareTimings, PdfTranslationUnit},
    },
    rosetta_jobs::formats::pdf::{
        errors::PdfError,
        source_state,
        unit_translation::{
            translate_pdf_units_with_events, PdfUnitProviderConfig, PdfUnitTranslation,
            PdfUnitTranslationBatchResult, PdfUnitTranslationMetrics,
        },
    },
};

const PDF2ZH_PROGRESS_EVENT: &str = "rosetta-pdf2zh-progress";
const PDF_ENGINE_SCRATCH_ROOT: &str = "pdf-engine-scratch";
const PDF_TRANSLATION_QUEUE_CAPACITY_UNITS: usize = 32;
static PDF_ENGINE_SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct PdfEngineScratchDir {
    path: PathBuf,
}

impl PdfEngineScratchDir {
    fn create(app: &AppHandle) -> Result<Self, PdfError> {
        let root = app
            .path()
            .app_local_data_dir()
            .map_err(|error| PdfError::Read(format!("无法定位 PDF engine 临时目录: {error}")))?
            .join(PDF_ENGINE_SCRATCH_ROOT);
        Self::create_in(&root)
            .map_err(|error| PdfError::Read(format!("无法创建 PDF engine 临时目录: {error}")))
    }

    fn create_in(root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let process_id = std::process::id();

        for _ in 0..32 {
            let sequence = PDF_ENGINE_SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("{process_id:x}-{timestamp:x}-{sequence:x}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique PDF engine scratch directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PdfEngineScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pdf2zhProgressPayload {
    pub job_id: String,
    pub phase: String,
    pub percent: Option<u8>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_chars: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PageProgressContext {
    pub completed_before: u32,
    pub chunk_len: u32,
    pub total: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhInvokeOptions {
    pub job_id: String,
    pub provider: PdfUnitProviderConfig,
    pub source_lang: String,
    pub target_lang: String,
    pub pages: Option<Vec<u32>>,
    pub page_progress: Option<PageProgressContext>,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhOutput {
    pub warmup_ms: u64,
    pub process_ms: u64,
    pub prepare_cache_hit: bool,
    pub prepare_cache_tier: String,
    pub prepare_timings_ms: PdfPrepareTimings,
    pub render_ms: u64,
    pub render_call_count: u32,
    pub translation_queue_capacity_units: u64,
    pub translation_queue_peak_units: u64,
    pub translation_queue_peak_payload_bytes: u64,
    pub pending_translation_map_peak_units: u64,
    pub pending_translation_map_peak_chars: u64,
    pub pending_translation_map_peak_payload_bytes: u64,
    pub render_payload_peak_units: u64,
    pub render_payload_peak_chars: u64,
    pub render_payload_peak_bytes: u64,
    pub combined_pending_peak_units: u64,
    pub combined_pending_peak_chars: u64,
    pub combined_pending_peak_payload_bytes: u64,
    pub rwkv_metrics: PdfUnitTranslationMetrics,
}

#[derive(Debug, Clone)]
pub(crate) struct Pdf2zhPreparseOutput {
    pub warmup_ms: u64,
    pub cache_hit: bool,
    pub cache_tier: String,
    pub timings_ms: PdfPrepareTimings,
    pub unit_count: usize,
}

#[derive(Debug, Default)]
struct PdfRenderMetrics {
    total_ms: u64,
    call_count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PendingTranslationMetrics {
    queue_capacity_units: u64,
    queue_current_units: u64,
    queue_current_chars: u64,
    queue_current_payload_bytes: u64,
    queue_peak_units: u64,
    queue_peak_payload_bytes: u64,
    map_current_units: u64,
    map_current_chars: u64,
    map_current_payload_bytes: u64,
    map_peak_units: u64,
    map_peak_chars: u64,
    map_peak_payload_bytes: u64,
    render_current_units: u64,
    render_current_chars: u64,
    render_current_payload_bytes: u64,
    render_peak_units: u64,
    render_peak_chars: u64,
    render_peak_payload_bytes: u64,
    combined_peak_units: u64,
    combined_peak_chars: u64,
    combined_peak_payload_bytes: u64,
}

impl PendingTranslationMetrics {
    fn new(queue_capacity: usize) -> Self {
        Self {
            queue_capacity_units: queue_capacity as u64,
            ..Self::default()
        }
    }

    fn record_queue_enqueued(&mut self, translation: &PdfUnitTranslation) {
        let (chars, payload_bytes) = translation_payload_size(translation);
        self.queue_current_units += 1;
        self.queue_current_chars += chars;
        self.queue_current_payload_bytes += payload_bytes;
        self.queue_peak_units = self.queue_peak_units.max(self.queue_current_units);
        self.queue_peak_payload_bytes = self
            .queue_peak_payload_bytes
            .max(self.queue_current_payload_bytes);
        self.record_combined_peak();
    }

    fn record_queue_dequeued(&mut self, translation: &PdfUnitTranslation) {
        let (chars, payload_bytes) = translation_payload_size(translation);
        self.queue_current_units = self.queue_current_units.saturating_sub(1);
        self.queue_current_chars = self.queue_current_chars.saturating_sub(chars);
        self.queue_current_payload_bytes = self
            .queue_current_payload_bytes
            .saturating_sub(payload_bytes);
    }

    fn record_map_inserted(
        &mut self,
        unit_id_bytes: u64,
        text_chars: u64,
        text_bytes: u64,
        previous: Option<&str>,
    ) {
        if let Some(previous) = previous {
            self.map_current_chars = self
                .map_current_chars
                .saturating_sub(previous.chars().count() as u64);
            self.map_current_payload_bytes = self
                .map_current_payload_bytes
                .saturating_sub(previous.len() as u64);
        } else {
            self.map_current_units += 1;
            self.map_current_payload_bytes += unit_id_bytes;
        }
        self.map_current_chars += text_chars;
        self.map_current_payload_bytes += text_bytes;
        self.map_peak_units = self.map_peak_units.max(self.map_current_units);
        self.map_peak_chars = self.map_peak_chars.max(self.map_current_chars);
        self.map_peak_payload_bytes = self
            .map_peak_payload_bytes
            .max(self.map_current_payload_bytes);
        self.record_combined_peak();
    }

    fn record_map_removed(&mut self, unit_id: &str, text: &str) {
        self.map_current_units = self.map_current_units.saturating_sub(1);
        self.map_current_chars = self
            .map_current_chars
            .saturating_sub(text.chars().count() as u64);
        self.map_current_payload_bytes = self
            .map_current_payload_bytes
            .saturating_sub((unit_id.len() + text.len()) as u64);
    }

    fn record_render_started(&mut self, units: u64, chars: u64, payload_bytes: u64) {
        self.render_current_units = units;
        self.render_current_chars = chars;
        self.render_current_payload_bytes = payload_bytes;
        self.render_peak_units = self.render_peak_units.max(units);
        self.render_peak_chars = self.render_peak_chars.max(chars);
        self.render_peak_payload_bytes = self.render_peak_payload_bytes.max(payload_bytes);
        self.record_combined_peak();
    }

    fn record_render_finished(&mut self) {
        self.render_current_units = 0;
        self.render_current_chars = 0;
        self.render_current_payload_bytes = 0;
    }

    fn record_combined_peak(&mut self) {
        self.combined_peak_units = self
            .combined_peak_units
            .max(self.queue_current_units + self.map_current_units + self.render_current_units);
        self.combined_peak_chars = self
            .combined_peak_chars
            .max(self.queue_current_chars + self.map_current_chars + self.render_current_chars);
        self.combined_peak_payload_bytes = self.combined_peak_payload_bytes.max(
            self.queue_current_payload_bytes
                + self.map_current_payload_bytes
                + self.render_current_payload_bytes,
        );
    }
}

type SharedPendingTranslationMetrics = Arc<Mutex<PendingTranslationMetrics>>;

fn lock_pending_metrics(
    metrics: &SharedPendingTranslationMetrics,
) -> MutexGuard<'_, PendingTranslationMetrics> {
    metrics.lock().unwrap_or_else(|error| error.into_inner())
}

fn translation_payload_size(translation: &PdfUnitTranslation) -> (u64, u64) {
    (
        translation.text.chars().count() as u64,
        (translation.unit_id.len() + translation.text.len()) as u64,
    )
}

#[derive(Clone)]
struct PdfUnitTranslationSender {
    sender: mpsc::Sender<PdfUnitTranslation>,
    metrics: SharedPendingTranslationMetrics,
}

impl PdfUnitTranslationSender {
    async fn send(&self, translation: PdfUnitTranslation) -> Result<(), String> {
        let permit = self.sender.reserve().await.map_err(|_| {
            "PDF renderer stopped receiving translated units before translation completed."
                .to_string()
        })?;
        lock_pending_metrics(&self.metrics).record_queue_enqueued(&translation);
        permit.send(translation);
        Ok(())
    }
}

struct PdfUnitTranslationReceiver {
    receiver: mpsc::Receiver<PdfUnitTranslation>,
    metrics: SharedPendingTranslationMetrics,
}

impl PdfUnitTranslationReceiver {
    async fn recv(&mut self) -> Option<PdfUnitTranslation> {
        let translation = self.receiver.recv().await?;
        lock_pending_metrics(&self.metrics).record_queue_dequeued(&translation);
        Some(translation)
    }
}

fn bounded_translation_queue(
    capacity: usize,
    metrics: SharedPendingTranslationMetrics,
) -> (PdfUnitTranslationSender, PdfUnitTranslationReceiver) {
    let (sender, receiver) = mpsc::channel(capacity);
    (
        PdfUnitTranslationSender {
            sender,
            metrics: Arc::clone(&metrics),
        },
        PdfUnitTranslationReceiver { receiver, metrics },
    )
}

fn retain_translation(
    translations_by_unit_id: &mut BTreeMap<String, String>,
    metrics: &SharedPendingTranslationMetrics,
    translation: PdfUnitTranslation,
) {
    let PdfUnitTranslation { unit_id, text, .. } = translation;
    let unit_id_bytes = unit_id.len() as u64;
    let text_chars = text.chars().count() as u64;
    let text_bytes = text.len() as u64;
    let previous = translations_by_unit_id.insert(unit_id, text);
    lock_pending_metrics(metrics).record_map_inserted(
        unit_id_bytes,
        text_chars,
        text_bytes,
        previous.as_deref(),
    );
}

fn release_page_translations(
    unit_ids: &[String],
    translations_by_unit_id: &mut BTreeMap<String, String>,
    metrics: &SharedPendingTranslationMetrics,
) {
    for unit_id in unit_ids {
        if let Some(text) = translations_by_unit_id.remove(unit_id) {
            lock_pending_metrics(metrics).record_map_removed(unit_id, &text);
        }
    }
}

fn pdf_prepare_cache_key(
    source_path: &Path,
    pages: Option<&[u32]>,
    source_lang: &str,
    target_lang: &str,
) -> Result<String, PdfError> {
    let metadata = std::fs::metadata(source_path)
        .map_err(|error| PdfError::Read(format!("无法读取 PDF 源文件元数据: {error}")))?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let source_fingerprint = source_path
        .parent()
        .and_then(|job_dir| {
            source_state::read_pdf_source_metadata(job_dir)
                .ok()
                .flatten()
        })
        .map(|metadata| metadata.source_fingerprint)
        .unwrap_or_default();
    Ok(serde_json::json!({
        "schemaVersion": 1,
        "sourcePath": source_path.to_string_lossy(),
        "sourceBytes": metadata.len(),
        "sourceModifiedNs": modified_ns,
        "sourceFingerprint": source_fingerprint,
        "pages": pages,
        "sourceLang": source_lang,
        "targetLang": target_lang,
        "thread": 1,
    })
    .to_string())
}

fn persistent_layout_cache_dir(source_path: &Path, cache_key: &str) -> Result<PathBuf, PdfError> {
    let job_dir = source_path
        .parent()
        .ok_or_else(|| PdfError::Read("无法定位 PDF 项目目录，不能创建预解析缓存。".to_string()))?;
    let digest = Sha256::digest(cache_key.as_bytes());
    Ok(job_dir
        .join("pdf-prepare-cache")
        .join("v1")
        .join(format!("{digest:x}")))
}

fn persistent_source_fingerprint(source_path: &Path) -> String {
    source_path
        .parent()
        .and_then(|job_dir| {
            source_state::read_pdf_source_metadata(job_dir)
                .ok()
                .flatten()
        })
        .map(|metadata| metadata.source_fingerprint)
        .unwrap_or_default()
}

pub(crate) async fn preparse_pdf2zh(
    app: &AppHandle,
    cache_owner_key: &str,
    source_path: &Path,
    pages: Vec<u32>,
    source_lang: &str,
    target_lang: &str,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<Pdf2zhPreparseOutput, PdfError> {
    let status = managed_pdf2zh::build_static_status(app).map_err(PdfError::RuntimeMissing)?;
    if !status.install_plan.ready {
        return Err(PdfError::RuntimeMissing(status.install_plan.message));
    }
    if status.doclayout_model_path.is_none() {
        return Err(PdfError::RuntimeMissing(
            "PDF 版面处理组件缺少内置 ONNX 版面模型，请更新 PDF 组件。".to_string(),
        ));
    }

    let scratch_dir = PdfEngineScratchDir::create(app)?;
    let cache_key = pdf_prepare_cache_key(source_path, Some(&pages), source_lang, target_lang)?;
    let persistent_cache_dir = persistent_layout_cache_dir(source_path, &cache_key)?;
    let source_fingerprint = persistent_source_fingerprint(source_path);
    let mut stderr_lines = Vec::<String>::new();
    let mut on_stderr = |line: &str| {
        if stderr_lines.len() >= 30 {
            stderr_lines.remove(0);
        }
        stderr_lines.push(line.trim().to_string());
    };

    let started = std::time::Instant::now();
    let prepared = crate::managed_pdf2zh::worker::prepare_pdf_window(
        app,
        serde_json::json!({
            "file": source_path.to_string_lossy(),
            "outputDir": scratch_dir.path().to_string_lossy(),
            "tmpDir": scratch_dir.path().to_string_lossy(),
            "pages": pages,
            "langIn": pdf2zh_lang(source_lang),
            "langOut": pdf2zh_lang(target_lang),
            "thread": 1,
            "cacheKey": cache_key,
            "cacheOwnerKey": cache_owner_key,
            "options": {
                "cleanupScratchDir": true,
                "persistentLayoutCacheDir": persistent_cache_dir.to_string_lossy(),
                "persistentLayoutCacheKey": &cache_key,
                "persistentSourceFingerprint": source_fingerprint,
            },
        }),
        &mut on_stderr,
        &mut cancel_rx,
    )
    .await
    .map_err(worker_outcome_to_pdf_error)?;

    Ok(Pdf2zhPreparseOutput {
        warmup_ms: started.elapsed().as_millis() as u64,
        cache_hit: prepared.cache_hit,
        cache_tier: prepared.cache_tier,
        timings_ms: prepared.timings_ms,
        unit_count: prepared.units.len(),
    })
}

pub(crate) async fn invoke_pdf2zh(
    app: &AppHandle,
    source_path: &Path,
    output_dir: &Path,
    options: Pdf2zhInvokeOptions,
    mut cancel_rx: oneshot::Receiver<()>,
    mut on_page_result: Option<&mut (dyn FnMut(PdfPageResult) + Send)>,
) -> Result<Pdf2zhOutput, PdfError> {
    let invoke_started = std::time::Instant::now();
    std::fs::create_dir_all(output_dir)
        .map_err(|error| PdfError::Read(format!("无法创建 PDF engine 输出目录: {error}")))?;

    emit_progress(
        app,
        &options.job_id,
        "warmup",
        Some(0),
        "正在准备 PDF engine…",
        options.page_progress,
        0,
        None,
    );

    let status = managed_pdf2zh::build_static_status(app).map_err(PdfError::RuntimeMissing)?;
    if !status.install_plan.ready {
        return Err(PdfError::RuntimeMissing(status.install_plan.message));
    }
    if status.doclayout_model_path.is_none() {
        return Err(PdfError::RuntimeMissing(
            "PDF 版面处理组件缺少内置 ONNX 版面模型，请更新 PDF 组件。".to_string(),
        ));
    }
    let scratch_dir = PdfEngineScratchDir::create(app)?;

    let pages_done = Arc::new(AtomicU32::new(0));
    let mut stderr_lines = Vec::<String>::new();
    let mut on_stderr = |line: &str| {
        if stderr_lines.len() >= 30 {
            stderr_lines.remove(0);
        }
        stderr_lines.push(line.trim().to_string());
    };

    let warmup_started = std::time::Instant::now();
    let prepare_cache_key = pdf_prepare_cache_key(
        source_path,
        options.pages.as_deref(),
        &options.source_lang,
        &options.target_lang,
    )?;
    let persistent_cache_dir = persistent_layout_cache_dir(source_path, &prepare_cache_key)?;
    let source_fingerprint = persistent_source_fingerprint(source_path);
    let mut prepared = crate::managed_pdf2zh::worker::prepare_pdf_window(
        app,
        serde_json::json!({
            "file": source_path.to_string_lossy(),
            "outputDir": output_dir.to_string_lossy(),
            "tmpDir": scratch_dir.path().to_string_lossy(),
            "pages": options.pages.clone(),
            "langIn": pdf2zh_lang(&options.source_lang),
            "langOut": pdf2zh_lang(&options.target_lang),
            "thread": 1,
            "cacheKey": prepare_cache_key,
            "cacheOwnerKey": &options.job_id,
            "options": {
                "cleanupScratchDir": true,
                "persistentLayoutCacheDir": persistent_cache_dir.to_string_lossy(),
                "persistentLayoutCacheKey": &prepare_cache_key,
                "persistentSourceFingerprint": source_fingerprint,
            },
        }),
        &mut on_stderr,
        &mut cancel_rx,
    )
    .await
    .map_err(worker_outcome_to_pdf_error)?;
    let warmup_ms = warmup_started.elapsed().as_millis() as u64;

    emit_progress(
        app,
        &options.job_id,
        "translate",
        Some(20),
        "正在翻译 PDF 文本单元…",
        options.page_progress,
        0,
        None,
    );

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let translate_started = std::time::Instant::now();
    let ordered_pages = prepared.prepared_run.pages.clone();
    let mut unit_ids_by_page = unit_ids_by_page(&ordered_pages, &prepared.units);
    let mut translations_by_unit_id = BTreeMap::<String, String>::new();
    let pending_translation_metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(
        PDF_TRANSLATION_QUEUE_CAPACITY_UNITS,
    )));
    let mut rendered_pages = BTreeSet::<u32>::new();
    let mut render_metrics = PdfRenderMetrics::default();
    let (unit_tx, mut unit_rx) = bounded_translation_queue(
        PDF_TRANSLATION_QUEUE_CAPACITY_UNITS,
        Arc::clone(&pending_translation_metrics),
    );
    let provider_for_task = options.provider.clone();
    let source_lang_for_task = options.source_lang.clone();
    let target_lang_for_task = options.target_lang.clone();
    let units_for_task = std::mem::take(&mut prepared.units);
    let cancel_for_task = Arc::clone(&cancel_flag);
    let mut translate_task = tokio::spawn(async move {
        let mut on_unit_translation = move |translation: PdfUnitTranslation| {
            let unit_tx = unit_tx.clone();
            async move { unit_tx.send(translation).await }
        };
        translate_pdf_units_with_events(
            &provider_for_task,
            &source_lang_for_task,
            &target_lang_for_task,
            &units_for_task,
            Some(cancel_for_task),
            &mut on_unit_translation,
        )
        .await
    });

    let mut local_on_page = |page_result: PdfPageResult| {
        pages_done.fetch_add(1, Ordering::Relaxed);
        if let Some(callback) = on_page_result.as_deref_mut() {
            callback(page_result);
        }
    };

    match render_ready_pages(
        app,
        &prepared.prepared_run.prepared_run_id,
        output_dir,
        &ordered_pages,
        &mut unit_ids_by_page,
        &mut translations_by_unit_id,
        &pending_translation_metrics,
        &mut rendered_pages,
        &mut render_metrics,
        &mut on_stderr,
        &mut local_on_page,
        &mut cancel_rx,
    )
    .await
    {
        Ok(()) => {}
        Err(error) => {
            cancel_flag.store(true, Ordering::SeqCst);
            translate_task.abort();
            crate::managed_pdf2zh::worker::dispose_pdf_window(
                app,
                &prepared.prepared_run.prepared_run_id,
            )
            .await;
            return Err(error);
        }
    }

    let mut translation_result: Option<Result<PdfUnitTranslationBatchResult, String>> = None;
    let mut translate_task_done = false;
    loop {
        if translate_task_done {
            match unit_rx.recv().await {
                Some(translation) => {
                    retain_translation(
                        &mut translations_by_unit_id,
                        &pending_translation_metrics,
                        translation,
                    );
                    match render_ready_pages(
                        app,
                        &prepared.prepared_run.prepared_run_id,
                        output_dir,
                        &ordered_pages,
                        &mut unit_ids_by_page,
                        &mut translations_by_unit_id,
                        &pending_translation_metrics,
                        &mut rendered_pages,
                        &mut render_metrics,
                        &mut on_stderr,
                        &mut local_on_page,
                        &mut cancel_rx,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(error) => {
                            cancel_flag.store(true, Ordering::SeqCst);
                            crate::managed_pdf2zh::worker::dispose_pdf_window(
                                app,
                                &prepared.prepared_run.prepared_run_id,
                            )
                            .await;
                            return Err(error);
                        }
                    }
                }
                None => break,
            }
            continue;
        }

        tokio::select! {
            maybe_translation = unit_rx.recv() => {
                if let Some(translation) = maybe_translation {
                    retain_translation(
                        &mut translations_by_unit_id,
                        &pending_translation_metrics,
                        translation,
                    );
                    match render_ready_pages(
                        app,
                        &prepared.prepared_run.prepared_run_id,
                        output_dir,
                        &ordered_pages,
                        &mut unit_ids_by_page,
                        &mut translations_by_unit_id,
                        &pending_translation_metrics,
                        &mut rendered_pages,
                        &mut render_metrics,
                        &mut on_stderr,
                        &mut local_on_page,
                        &mut cancel_rx,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(error) => {
                            cancel_flag.store(true, Ordering::SeqCst);
                            translate_task.abort();
                            crate::managed_pdf2zh::worker::dispose_pdf_window(
                                app,
                                &prepared.prepared_run.prepared_run_id,
                            )
                            .await;
                            return Err(error);
                        }
                    }
                }
            }
            joined = &mut translate_task => {
                translate_task_done = true;
                translation_result = Some(match joined {
                    Ok(result) => result,
                    Err(error) => Err(format!("PDF unit translation task failed: {error}")),
                });
            }
            _ = &mut cancel_rx => {
                cancel_flag.store(true, Ordering::SeqCst);
                translate_task.abort();
                crate::managed_pdf2zh::worker::dispose_pdf_window(
                    app,
                    &prepared.prepared_run.prepared_run_id,
                ).await;
                return Err(PdfError::Cancelled);
            }
        }
    }

    let translation_result = match translation_result
        .unwrap_or_else(|| Err("PDF unit translation task ended without a result.".to_string()))
    {
        Ok(result) => result,
        Err(error) => {
            crate::managed_pdf2zh::worker::dispose_pdf_window(
                app,
                &prepared.prepared_run.prepared_run_id,
            )
            .await;
            return Err(PdfError::Pdf2zhFailed(error));
        }
    };
    match render_ready_pages(
        app,
        &prepared.prepared_run.prepared_run_id,
        output_dir,
        &ordered_pages,
        &mut unit_ids_by_page,
        &mut translations_by_unit_id,
        &pending_translation_metrics,
        &mut rendered_pages,
        &mut render_metrics,
        &mut on_stderr,
        &mut local_on_page,
        &mut cancel_rx,
    )
    .await
    {
        Ok(()) => {}
        Err(error) => {
            crate::managed_pdf2zh::worker::dispose_pdf_window(
                app,
                &prepared.prepared_run.prepared_run_id,
            )
            .await;
            return Err(error);
        }
    }

    let missing_pages = ordered_pages
        .iter()
        .copied()
        .filter(|page| !rendered_pages.contains(page))
        .collect::<Vec<_>>();
    if !missing_pages.is_empty() {
        cancel_flag.store(true, Ordering::SeqCst);
        crate::managed_pdf2zh::worker::dispose_pdf_window(
            app,
            &prepared.prepared_run.prepared_run_id,
        )
        .await;
        return Err(PdfError::Pdf2zhFailed(format!(
            "PDF unit translation finished but {} page(s) were not ready to render: {:?}",
            missing_pages.len(),
            missing_pages
        )));
    }

    emit_progress(
        app,
        &options.job_id,
        "render",
        Some(100),
        "译文 PDF 页面已生成。",
        options.page_progress,
        pages_done.load(Ordering::Relaxed),
        None,
    );

    let process_ms = translate_started.elapsed().as_millis() as u64;
    let pending_translation_metrics = *lock_pending_metrics(&pending_translation_metrics);
    Ok(Pdf2zhOutput {
        warmup_ms,
        process_ms: process_ms.max(invoke_started.elapsed().as_millis() as u64),
        prepare_cache_hit: prepared.cache_hit,
        prepare_cache_tier: prepared.cache_tier,
        prepare_timings_ms: prepared.timings_ms,
        render_ms: render_metrics.total_ms,
        render_call_count: render_metrics.call_count,
        translation_queue_capacity_units: pending_translation_metrics.queue_capacity_units,
        translation_queue_peak_units: pending_translation_metrics.queue_peak_units,
        translation_queue_peak_payload_bytes: pending_translation_metrics.queue_peak_payload_bytes,
        pending_translation_map_peak_units: pending_translation_metrics.map_peak_units,
        pending_translation_map_peak_chars: pending_translation_metrics.map_peak_chars,
        pending_translation_map_peak_payload_bytes: pending_translation_metrics
            .map_peak_payload_bytes,
        render_payload_peak_units: pending_translation_metrics.render_peak_units,
        render_payload_peak_chars: pending_translation_metrics.render_peak_chars,
        render_payload_peak_bytes: pending_translation_metrics.render_peak_payload_bytes,
        combined_pending_peak_units: pending_translation_metrics.combined_peak_units,
        combined_pending_peak_chars: pending_translation_metrics.combined_peak_chars,
        combined_pending_peak_payload_bytes: pending_translation_metrics
            .combined_peak_payload_bytes,
        rwkv_metrics: translation_result.metrics,
    })
}

fn unit_ids_by_page(pages: &[u32], units: &[PdfTranslationUnit]) -> BTreeMap<u32, Vec<String>> {
    let mut by_page = pages
        .iter()
        .copied()
        .map(|page| (page, Vec::<String>::new()))
        .collect::<BTreeMap<_, _>>();
    for unit in units {
        by_page
            .entry(unit.page_number)
            .or_default()
            .push(unit.unit_id.clone());
    }
    by_page
}

async fn render_ready_pages(
    app: &AppHandle,
    prepared_run_id: &str,
    output_dir: &Path,
    ordered_pages: &[u32],
    unit_ids_by_page: &mut BTreeMap<u32, Vec<String>>,
    translations_by_unit_id: &mut BTreeMap<String, String>,
    pending_translation_metrics: &SharedPendingTranslationMetrics,
    rendered_pages: &mut BTreeSet<u32>,
    render_metrics: &mut PdfRenderMetrics,
    on_stderr: &mut (dyn FnMut(&str) + Send),
    on_page_result: &mut (dyn FnMut(PdfPageResult) + Send),
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), PdfError> {
    for page_number in ordered_pages {
        if rendered_pages.contains(page_number) {
            continue;
        }
        let unit_ids = unit_ids_by_page
            .get(page_number)
            .cloned()
            .unwrap_or_default();
        if !unit_ids
            .iter()
            .all(|unit_id| translations_by_unit_id.contains_key(unit_id))
        {
            continue;
        }
        let page_translations = unit_ids
            .iter()
            .filter_map(|unit_id| {
                translations_by_unit_id
                    .get(unit_id)
                    .map(|text| (unit_id.clone(), text.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let render_units = page_translations.len() as u64;
        let render_chars = page_translations
            .values()
            .map(|text| text.chars().count() as u64)
            .sum();
        let render_payload = serde_json::json!({
            "preparedRunId": prepared_run_id,
            "outputDir": output_dir.to_string_lossy(),
            "translationsByUnitId": page_translations,
            "pages": [page_number],
        });
        let render_payload_bytes = render_payload.to_string().len() as u64;
        lock_pending_metrics(pending_translation_metrics).record_render_started(
            render_units,
            render_chars,
            render_payload_bytes,
        );
        let render_started = std::time::Instant::now();
        let outcome = crate::managed_pdf2zh::worker::render_pdf_window(
            app,
            render_payload,
            on_stderr,
            on_page_result,
            cancel_rx,
        )
        .await;
        lock_pending_metrics(pending_translation_metrics).record_render_finished();
        render_metrics.total_ms += render_started.elapsed().as_millis() as u64;
        render_metrics.call_count += 1;
        match outcome {
            crate::managed_pdf2zh::worker::WorkerTranslateOutcome::Completed => {
                rendered_pages.insert(*page_number);
                unit_ids_by_page.remove(page_number);
                release_page_translations(
                    &unit_ids,
                    translations_by_unit_id,
                    pending_translation_metrics,
                );
            }
            other => return Err(worker_outcome_to_pdf_error(other)),
        }
    }
    Ok(())
}

fn worker_outcome_to_pdf_error(
    outcome: crate::managed_pdf2zh::worker::WorkerTranslateOutcome,
) -> PdfError {
    match outcome {
        crate::managed_pdf2zh::worker::WorkerTranslateOutcome::Cancelled => PdfError::Cancelled,
        crate::managed_pdf2zh::worker::WorkerTranslateOutcome::JobFailed { message, .. }
        | crate::managed_pdf2zh::worker::WorkerTranslateOutcome::WorkerLost(message)
        | crate::managed_pdf2zh::worker::WorkerTranslateOutcome::Unavailable(message) => {
            PdfError::Pdf2zhFailed(message)
        }
        crate::managed_pdf2zh::worker::WorkerTranslateOutcome::Completed => PdfError::Pdf2zhFailed(
            "PDF worker returned an unexpected completed outcome.".to_string(),
        ),
    }
}

fn pdf2zh_lang(lang: &str) -> &str {
    match lang {
        "zh-CN" | "zh-TW" | "zh" => "zh",
        "en" => "en",
        other => other,
    }
}

pub(crate) fn emit_progress_phase(
    app: &AppHandle,
    job_id: &str,
    phase: &str,
    percent: Option<u8>,
    message: &str,
    total_pages: u32,
) {
    let _ = app.emit(
        PDF2ZH_PROGRESS_EVENT,
        Pdf2zhProgressPayload {
            job_id: job_id.to_string(),
            phase: phase.to_string(),
            percent,
            message: message.to_string(),
            current_page: None,
            total_pages: if total_pages > 0 {
                Some(total_pages)
            } else {
                None
            },
            completed_pages: if total_pages > 0 { Some(0) } else { None },
            translated_chars: if total_pages > 0 { Some(0) } else { None },
        },
    );
}

pub(crate) fn emit_completed_page_progress(
    app: &AppHandle,
    job_id: &str,
    total_pages: u32,
    completed_pages: u32,
    page_number: u32,
    translated_chars: Option<u64>,
) {
    let _ = app.emit(
        PDF2ZH_PROGRESS_EVENT,
        completed_page_progress_payload(
            job_id,
            total_pages,
            completed_pages,
            page_number,
            translated_chars,
        ),
    );
}

fn completed_page_progress_payload(
    job_id: &str,
    total_pages: u32,
    completed_pages: u32,
    page_number: u32,
    translated_chars: Option<u64>,
) -> Pdf2zhProgressPayload {
    Pdf2zhProgressPayload {
        job_id: job_id.to_string(),
        phase: "translate".to_string(),
        percent: None,
        message: format!("page {page_number} committed"),
        current_page: None,
        total_pages: if total_pages > 0 {
            Some(total_pages)
        } else {
            None
        },
        completed_pages: Some(completed_pages.min(total_pages)),
        translated_chars,
    }
}

fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    phase: &str,
    percent: Option<u8>,
    message: &str,
    ctx: Option<PageProgressContext>,
    pages_done_in_chunk: u32,
    translated_chars: Option<u64>,
) {
    let (current_page, total_pages) = match ctx {
        Some(ctx) => {
            let current_in_chunk = (pages_done_in_chunk + 1).min(ctx.chunk_len.max(1));
            (
                Some(ctx.completed_before + current_in_chunk),
                Some(ctx.total),
            )
        }
        None => (None, None),
    };
    let _ = app.emit(
        PDF2ZH_PROGRESS_EVENT,
        Pdf2zhProgressPayload {
            job_id: job_id.to_string(),
            phase: phase.to_string(),
            percent,
            message: message.to_string(),
            current_page,
            total_pages,
            completed_pages: None,
            translated_chars,
        },
    );
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use tokio::time::timeout;

    use super::{
        bounded_translation_queue, completed_page_progress_payload, lock_pending_metrics,
        pdf_prepare_cache_key, persistent_layout_cache_dir, release_page_translations,
        retain_translation, Pdf2zhInvokeOptions, PdfEngineScratchDir, PendingTranslationMetrics,
    };
    use crate::rosetta_jobs::formats::pdf::unit_translation::{
        LlamaCppPdfApiConfig, PdfUnitProviderConfig, PdfUnitTranslation,
    };

    fn scratch_test_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rosetta-pdf-engine-scratch-test-{}-{nanos}",
            std::process::id()
        ))
    }

    fn queued_translation(index: usize) -> PdfUnitTranslation {
        let text = format!("译文-{index}");
        PdfUnitTranslation {
            unit_id: format!("unit-{index}"),
            output_chars: text.chars().count() as u64,
            text,
        }
    }

    #[test]
    fn pdf_engine_scratch_dirs_are_unique_and_removed_on_drop() {
        let root = scratch_test_root();
        let first = PdfEngineScratchDir::create_in(&root).expect("create first scratch dir");
        let second = PdfEngineScratchDir::create_in(&root).expect("create second scratch dir");
        let first_path = first.path().to_path_buf();
        let second_path = second.path().to_path_buf();

        assert!(first_path.starts_with(&root));
        assert!(second_path.starts_with(&root));
        assert_ne!(first_path, second_path);
        assert!(first_path.is_dir());
        assert!(second_path.is_dir());

        drop(first);
        assert!(!first_path.exists());
        assert!(second_path.exists());

        drop(second);
        assert!(!second_path.exists());
        std::fs::remove_dir(&root).expect("remove scratch test root");
    }

    #[test]
    fn prepare_cache_key_changes_with_pages_and_language() {
        let root = scratch_test_root();
        std::fs::create_dir_all(&root).expect("create cache key test root");
        let source = root.join("source.pdf");
        std::fs::write(&source, b"pdf-cache-key-fixture").expect("write cache key fixture");
        let options = |pages, target_lang: &str| Pdf2zhInvokeOptions {
            job_id: "job-1".to_string(),
            provider: PdfUnitProviderConfig::LlamaCpp(LlamaCppPdfApiConfig {
                base_url: "http://127.0.0.1:1".to_string(),
                timeout_ms: 1,
            }),
            source_lang: "en".to_string(),
            target_lang: target_lang.to_string(),
            pages: Some(pages),
            page_progress: None,
        };

        let first_options = options(vec![1, 2], "zh-CN");
        let first = pdf_prepare_cache_key(
            &source,
            first_options.pages.as_deref(),
            &first_options.source_lang,
            &first_options.target_lang,
        )
        .expect("build first cache key");
        let same_options = options(vec![1, 2], "zh-CN");
        let same = pdf_prepare_cache_key(
            &source,
            same_options.pages.as_deref(),
            &same_options.source_lang,
            &same_options.target_lang,
        )
        .expect("build matching cache key");
        let other_pages_options = options(vec![2], "zh-CN");
        let other_pages = pdf_prepare_cache_key(
            &source,
            other_pages_options.pages.as_deref(),
            &other_pages_options.source_lang,
            &other_pages_options.target_lang,
        )
        .expect("build page-specific cache key");
        let other_language_options = options(vec![1, 2], "ja");
        let other_language = pdf_prepare_cache_key(
            &source,
            other_language_options.pages.as_deref(),
            &other_language_options.source_lang,
            &other_language_options.target_lang,
        )
        .expect("build language-specific cache key");

        assert_eq!(first, same);
        assert_ne!(first, other_pages);
        assert_ne!(first, other_language);
        std::fs::remove_dir_all(&root).expect("remove cache key test root");
    }

    #[test]
    fn persistent_layout_cache_is_versioned_and_keyed_below_the_job() {
        let root = scratch_test_root();
        std::fs::create_dir_all(&root).expect("create persistent cache test root");
        let source = root.join("source.pdf");
        std::fs::write(&source, b"pdf-cache-dir-fixture").expect("write cache dir fixture");
        let cache_key =
            pdf_prepare_cache_key(&source, Some(&[1, 2]), "en", "zh-CN").expect("build cache key");
        let first = persistent_layout_cache_dir(&source, &cache_key).expect("build cache dir");
        let second = persistent_layout_cache_dir(&source, &cache_key).expect("rebuild cache dir");

        assert_eq!(first, second);
        assert!(first.starts_with(root.join("pdf-prepare-cache").join("v1")));
        assert_eq!(
            first
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::len),
            Some(64)
        );

        std::fs::remove_dir_all(&root).expect("remove persistent cache test root");
    }

    #[test]
    fn completed_page_progress_carries_translated_chars() {
        let payload = completed_page_progress_payload("job-1", 7, 2, 3, Some(1_234));

        assert_eq!(payload.job_id, "job-1");
        assert_eq!(payload.phase, "translate");
        assert_eq!(payload.total_pages, Some(7));
        assert_eq!(payload.completed_pages, Some(2));
        assert_eq!(payload.translated_chars, Some(1_234));
    }

    #[test]
    fn completed_page_progress_clamps_completed_pages() {
        let payload = completed_page_progress_payload("job-1", 7, 9, 9, Some(0));

        assert_eq!(payload.total_pages, Some(7));
        assert_eq!(payload.completed_pages, Some(7));
        assert_eq!(payload.translated_chars, Some(0));
    }

    #[test]
    fn committed_page_translation_state_is_released_and_peak_is_retained() {
        let mut translations = BTreeMap::new();
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(2)));
        retain_translation(
            &mut translations,
            &metrics,
            PdfUnitTranslation {
                unit_id: "p0001-u0001".to_string(),
                text: "第一段译文".to_string(),
                output_chars: 5,
            },
        );
        retain_translation(
            &mut translations,
            &metrics,
            PdfUnitTranslation {
                unit_id: "p0001-u0002".to_string(),
                text: "第二段译文".to_string(),
                output_chars: 5,
            },
        );
        release_page_translations(
            &["p0001-u0001".to_string(), "p0001-u0002".to_string()],
            &mut translations,
            &metrics,
        );

        let metrics = *lock_pending_metrics(&metrics);
        assert!(translations.is_empty());
        assert_eq!(metrics.map_current_chars, 0);
        assert_eq!(metrics.map_peak_units, 2);
        assert_eq!(metrics.map_peak_chars, 10);
    }

    #[tokio::test]
    async fn slow_renderer_keeps_translation_queue_within_capacity_and_fifo_order() {
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(2)));
        let (sender, mut receiver) = bounded_translation_queue(2, Arc::clone(&metrics));
        let mut producer = tokio::spawn(async move {
            for index in 0..5 {
                sender.send(queued_translation(index)).await?;
            }
            Ok::<(), String>(())
        });

        for _ in 0..100 {
            if lock_pending_metrics(&metrics).queue_current_units == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(lock_pending_metrics(&metrics).queue_current_units, 2);
        assert!(timeout(Duration::from_millis(20), &mut producer)
            .await
            .is_err());

        let mut received = Vec::new();
        for _ in 0..5 {
            received.push(receiver.recv().await.expect("queued translation"));
        }
        producer
            .await
            .expect("producer task should join")
            .expect("producer should finish");

        assert_eq!(
            received
                .iter()
                .map(|translation| translation.unit_id.as_str())
                .collect::<Vec<_>>(),
            vec!["unit-0", "unit-1", "unit-2", "unit-3", "unit-4"]
        );
        let metrics = *lock_pending_metrics(&metrics);
        assert_eq!(metrics.queue_current_units, 0);
        assert_eq!(metrics.queue_peak_units, 2);
        assert!(metrics.queue_peak_units <= metrics.queue_capacity_units);
        assert!(metrics.queue_peak_payload_bytes > 0);
    }

    #[tokio::test]
    async fn combined_pending_peak_includes_queue_map_and_render_payload_copies() {
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(2)));
        let (sender, _receiver) = bounded_translation_queue(2, Arc::clone(&metrics));
        sender
            .send(queued_translation(0))
            .await
            .expect("queue translation");
        let mut translations = BTreeMap::new();
        retain_translation(&mut translations, &metrics, queued_translation(1));
        lock_pending_metrics(&metrics).record_render_started(1, 7, 100);

        let metrics = *lock_pending_metrics(&metrics);
        assert_eq!(metrics.queue_current_units, 1);
        assert_eq!(metrics.map_current_units, 1);
        assert_eq!(metrics.render_current_units, 1);
        assert_eq!(metrics.combined_peak_units, 3);
        assert_eq!(
            metrics.combined_peak_chars,
            metrics.queue_current_chars + metrics.map_current_chars + 7
        );
        assert_eq!(
            metrics.combined_peak_payload_bytes,
            metrics.queue_current_payload_bytes + metrics.map_current_payload_bytes + 100
        );
    }

    #[tokio::test]
    async fn renderer_failure_unblocks_a_producer_waiting_for_queue_capacity() {
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(1)));
        let (sender, receiver) = bounded_translation_queue(1, metrics);
        sender
            .send(queued_translation(0))
            .await
            .expect("fill queue");
        let blocked_sender = sender.clone();
        let blocked = tokio::spawn(async move { blocked_sender.send(queued_translation(1)).await });
        tokio::task::yield_now().await;

        drop(receiver);

        let error = timeout(Duration::from_secs(1), blocked)
            .await
            .expect("blocked producer should wake")
            .expect("producer task should join")
            .expect_err("renderer failure should close queue");
        assert!(error.contains("stopped receiving"));
    }

    #[tokio::test]
    async fn cancellation_aborts_a_producer_waiting_for_queue_capacity() {
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(1)));
        let (sender, mut receiver) = bounded_translation_queue(1, metrics);
        sender
            .send(queued_translation(0))
            .await
            .expect("fill queue");
        let blocked_sender = sender.clone();
        let blocked = tokio::spawn(async move { blocked_sender.send(queued_translation(1)).await });
        tokio::task::yield_now().await;

        blocked.abort();
        let cancelled = timeout(Duration::from_secs(1), blocked)
            .await
            .expect("cancelled producer should join")
            .expect_err("producer task should be cancelled");
        assert!(cancelled.is_cancelled());

        drop(sender);
        assert_eq!(
            receiver.recv().await.expect("first translation").unit_id,
            "unit-0"
        );
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn receiver_drop_is_reported_to_the_translation_callback() {
        let metrics = Arc::new(Mutex::new(PendingTranslationMetrics::new(1)));
        let (sender, receiver) = bounded_translation_queue(1, metrics);
        drop(receiver);

        let error = sender
            .send(queued_translation(0))
            .await
            .expect_err("closed receiver should reject translation");

        assert!(error.contains("stopped receiving"));
    }
}
