use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

use crate::{
    managed_pdf2zh::{
        self,
        worker::{PdfPageResult, PdfTranslationUnit},
    },
    rosetta_jobs::formats::pdf::{
        errors::PdfError,
        unit_translation::{
            translate_pdf_units_with_events, PdfUnitProviderConfig, PdfUnitTranslation,
            PdfUnitTranslationBatchResult, PdfUnitTranslationMetrics,
        },
    },
};

const PDF2ZH_PROGRESS_EVENT: &str = "rosetta-pdf2zh-progress";
const PDF_ENGINE_SCRATCH_ROOT: &str = "pdf-engine-scratch";
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
    pub rwkv_metrics: PdfUnitTranslationMetrics,
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
    let prepared = crate::managed_pdf2zh::worker::prepare_pdf_window(
        app,
        serde_json::json!({
            "file": source_path.to_string_lossy(),
            "outputDir": output_dir.to_string_lossy(),
            "tmpDir": scratch_dir.path().to_string_lossy(),
            "pages": options.pages.clone(),
            "langIn": pdf2zh_lang(&options.source_lang),
            "langOut": pdf2zh_lang(&options.target_lang),
            "thread": 1,
            "options": {
                "cleanupScratchDir": true,
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
    let unit_ids_by_page = unit_ids_by_page(&ordered_pages, &prepared.units);
    let mut translations_by_unit_id = BTreeMap::<String, String>::new();
    let mut rendered_pages = BTreeSet::<u32>::new();
    let (unit_tx, mut unit_rx) = mpsc::unbounded_channel::<PdfUnitTranslation>();
    let provider_for_task = options.provider.clone();
    let source_lang_for_task = options.source_lang.clone();
    let target_lang_for_task = options.target_lang.clone();
    let units_for_task = prepared.units.clone();
    let cancel_for_task = Arc::clone(&cancel_flag);
    let mut translate_task = tokio::spawn(async move {
        let mut on_unit_translation = move |translation: PdfUnitTranslation| {
            let _ = unit_tx.send(translation);
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
        &unit_ids_by_page,
        &translations_by_unit_id,
        &mut rendered_pages,
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
                    translations_by_unit_id.insert(translation.unit_id, translation.text);
                    match render_ready_pages(
                        app,
                        &prepared.prepared_run.prepared_run_id,
                        output_dir,
                        &ordered_pages,
                        &unit_ids_by_page,
                        &translations_by_unit_id,
                        &mut rendered_pages,
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
                    translations_by_unit_id.insert(translation.unit_id, translation.text);
                    match render_ready_pages(
                        app,
                        &prepared.prepared_run.prepared_run_id,
                        output_dir,
                        &ordered_pages,
                        &unit_ids_by_page,
                        &translations_by_unit_id,
                        &mut rendered_pages,
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
        &unit_ids_by_page,
        &translations_by_unit_id,
        &mut rendered_pages,
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

    crate::managed_pdf2zh::worker::dispose_pdf_window(app, &prepared.prepared_run.prepared_run_id)
        .await;

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
    Ok(Pdf2zhOutput {
        warmup_ms,
        process_ms: process_ms.max(invoke_started.elapsed().as_millis() as u64),
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
    unit_ids_by_page: &BTreeMap<u32, Vec<String>>,
    translations_by_unit_id: &BTreeMap<String, String>,
    rendered_pages: &mut BTreeSet<u32>,
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
        let render_payload = serde_json::json!({
            "preparedRunId": prepared_run_id,
            "outputDir": output_dir.to_string_lossy(),
            "translationsByUnitId": page_translations,
            "pages": [page_number],
        });
        match crate::managed_pdf2zh::worker::render_pdf_window(
            app,
            render_payload,
            on_stderr,
            on_page_result,
            cancel_rx,
        )
        .await
        {
            crate::managed_pdf2zh::worker::WorkerTranslateOutcome::Completed => {
                rendered_pages.insert(*page_number);
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
        crate::managed_pdf2zh::worker::WorkerTranslateOutcome::JobFailed(message)
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
    use std::path::PathBuf;

    use super::{completed_page_progress_payload, PdfEngineScratchDir};

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
}
