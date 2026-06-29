use std::{
    fs,
    path::{Component, Path},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

mod document;
mod export;
pub(crate) mod formats;
mod import;
pub(crate) mod model;
pub(crate) mod path;
mod revisions;
mod segmenter;
pub(crate) mod store;
#[cfg(test)]
mod tests;
pub(crate) mod translation_files;

use crate::managed_pdf2zh::openai_shim::{
    LightningApiConfig, LlamaCppApiConfig, ShimProviderConfig,
};
use crate::rwkv_providers::mobile_batch_chat::MobileBatchChatConfig;
use model::{
    RosettaExportKind, RosettaExportResult, RosettaJobBundle, RosettaJobDeleteResult,
    RosettaJobFileDeleteResult, RosettaJobSummary, RosettaTranslationFileBundle, Segment,
    TranslationRevisionReason, TranslationSegment,
};

/// Cancellation for the (single) active PDF translation run.
///
/// `cancelled` is level-triggered: it stays set until the run loop observes it,
/// so a stop request that lands in the gap between two page invocations (when
/// no oneshot sender is registered) is not lost. The oneshot sender is the
/// edge-trigger that interrupts the currently running pdf2zh process.
pub struct PdfTranslationCancelState {
    session_id: String,
    run_active: AtomicBool,
    active: Mutex<Option<ActivePdfRunCancel>>,
}

struct ActivePdfRunCancel {
    key: String,
    run_id: String,
    cancelled: bool,
    sender: Option<oneshot::Sender<()>>,
}

impl PdfTranslationCancelState {
    pub fn new() -> Self {
        Self {
            session_id: format!(
                "session-{}-{}",
                std::process::id(),
                path::timestamp_ms_string()
            ),
            run_active: AtomicBool::new(false),
            active: Mutex::new(None),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Marks a run as started. Returns `false` if another run is active.
    fn try_begin_run(&self) -> bool {
        self.try_begin_pdf_run("legacy".to_string(), "legacy".to_string())
    }

    fn try_begin_pdf_run(&self, key: String, run_id: String) -> bool {
        if self.run_active.swap(true, Ordering::SeqCst) {
            return false;
        }
        if let Ok(mut guard) = self.active.lock() {
            *guard = Some(ActivePdfRunCancel {
                key,
                run_id,
                cancelled: false,
                sender: None,
            });
            true
        } else {
            self.run_active.store(false, Ordering::SeqCst);
            false
        }
    }

    fn end_run(&self) {
        self.run_active.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.active.lock() {
            *guard = None;
        }
    }

    fn is_pdf_run_cancelled(&self, key: &str, run_id: &str) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|run| run.key == key && run.run_id == run_id && run.cancelled)
            })
            .unwrap_or(false)
    }

    fn register_sender(&self, tx: oneshot::Sender<()>) {
        self.register_sender_for_active(tx)
    }

    fn register_sender_for_active(&self, tx: oneshot::Sender<()>) {
        // If cancel already arrived, fire immediately instead of parking the
        // sender — the upcoming invocation should abort at once.
        let mut maybe_tx = Some(tx);
        if let Ok(mut guard) = self.active.lock() {
            let Some(run) = guard.as_mut() else {
                return;
            };
            if run.cancelled {
                if let Some(tx) = maybe_tx.take() {
                    let _ = tx.send(());
                }
                return;
            }
            run.sender = maybe_tx.take();
        }
        if let Some(tx) = maybe_tx {
            let _ = tx.send(());
        }
    }

    fn clear_sender(&self) {
        if let Ok(mut guard) = self.active.lock() {
            if let Some(run) = guard.as_mut() {
                run.sender = None;
            }
        }
    }

    pub fn request_cancel(&self) {
        if let Ok(mut guard) = self.active.lock() {
            if let Some(run) = guard.as_mut() {
                run.cancelled = true;
                if let Some(tx) = run.sender.take() {
                    let _ = tx.send(());
                }
            }
        }
    }

    pub fn request_cancel_for_run(&self, key: &str, run_id: Option<&str>) -> bool {
        let Ok(mut guard) = self.active.lock() else {
            return false;
        };
        let Some(run) = guard.as_mut() else {
            return false;
        };
        if run.key != key {
            return false;
        }
        if let Some(run_id) = run_id {
            if run.run_id != run_id {
                return false;
            }
        }
        run.cancelled = true;
        if let Some(tx) = run.sender.take() {
            let _ = tx.send(());
        }
        true
    }

    pub fn request_cancel_for_job(&self, job_id: &str) -> bool {
        let Ok(mut guard) = self.active.lock() else {
            return false;
        };
        let Some(run) = guard.as_mut() else {
            return false;
        };
        if !run.key.starts_with(&format!("{job_id}:")) {
            return false;
        }
        run.cancelled = true;
        if let Some(tx) = run.sender.take() {
            let _ = tx.send(());
        }
        true
    }

    fn active_run_matches(&self, key: &str, run_id: &str) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|run| run.key == key && run.run_id == run_id)
            })
            .unwrap_or(false)
    }
}

impl Default for PdfTranslationCancelState {
    fn default() -> Self {
        Self::new()
    }
}

pub use formats::pdf::runtime::PngCache as PdfPngCache;

#[tauri::command]
pub fn cancel_rosetta_translated_pdf(cancel_state: State<'_, PdfTranslationCancelState>) {
    cancel_state.request_cancel();
}

#[tauri::command]
pub fn pause_rosetta_pdf_run(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    target_lang: String,
    run_id: Option<String>,
) -> Result<Option<formats::pdf::run_state::PdfTranslationRun>, String> {
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let key = pdf_run_key(&job_id, &target_lang);
    let cancelled = cancel_state.request_cancel_for_run(&key, run_id.as_deref());
    if let Some(mut run) = formats::pdf::run_state::read_pdf_run_state(&dir, &target_lang)? {
        let run_matches = match run_id.as_ref() {
            Some(expected) => expected == &run.run_id,
            None => true,
        };
        if run_matches {
            run.state = if cancelled {
                "pausing".to_string()
            } else if run.state == "running" {
                "paused".to_string()
            } else {
                run.state
            };
            run.cancel_requested = cancelled;
            run.updated_at = path::timestamp_ms_string();
            formats::pdf::run_state::write_pdf_run_state(&dir, &run)?;
            return Ok(Some(run));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn pick_rosetta_import_path(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("选择 TXT、Markdown 或 PDF 文件")
        .add_filter("TXT / Markdown / PDF", &["txt", "md", "markdown", "pdf"])
        .pick_file(move |path| {
            let _ = tx.blocking_send(path.map(|path| path.to_string()));
        });

    Ok(rx.recv().await.flatten())
}

#[tauri::command]
pub async fn pick_rosetta_import_directory(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("选择项目文件夹")
        .pick_folder(move |path| {
            let _ = tx.blocking_send(path.map(|path| path.to_string()));
        });

    Ok(rx.recv().await.flatten())
}

#[tauri::command]
pub async fn pick_rosetta_export_path(
    app: AppHandle,
    default_filename: String,
    format: String,
) -> Result<Option<String>, String> {
    let extensions = match format.as_str() {
        "markdown" => vec!["md"],
        "pdf" => vec!["pdf"],
        _ => vec!["txt"],
    };

    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("导出 Rosetta 翻译结果")
        .set_file_name(default_filename)
        .add_filter("Rosetta export", &extensions)
        .save_file(move |path| {
            let _ = tx.blocking_send(path.map(|path| path.to_string()));
        });

    Ok(rx.recv().await.flatten())
}

#[tauri::command]
pub fn import_rosetta_project_from_directory(
    app: AppHandle,
    path: String,
) -> Result<RosettaJobBundle, String> {
    import::import_project_from_directory(&app, Path::new(&path))
}

#[tauri::command]
pub async fn import_rosetta_document_from_path(
    app: AppHandle,
    path: String,
) -> Result<RosettaJobBundle, String> {
    // PDFs go through the skeleton path: fast pre-flight + copy + sidebar
    // entry, with the heavy Docling extraction handed off to a background
    // task that emits an event when finished. Text formats stay synchronous
    // (parsing them is in-process and cheap).
    let source_path = Path::new(&path);
    match formats::document_format(source_path)? {
        formats::SourceFormat::Pdf => import::import_pdf_skeleton(&app, source_path).await,
        _ => import::import_document_from_path(&app, source_path).await,
    }
}

#[tauri::command]
pub fn create_welcome_document(app: AppHandle) -> Result<RosettaJobBundle, String> {
    import::create_welcome_document(&app)
}

#[tauri::command]
pub fn create_blank_txt_document(
    app: AppHandle,
    filename: String,
) -> Result<RosettaJobBundle, String> {
    import::create_blank_txt_document(&app, filename)
}

#[tauri::command]
pub fn update_txt_source_file(
    app: AppHandle,
    job_id: String,
    file_id: String,
    contents: String,
) -> Result<RosettaJobBundle, String> {
    import::update_txt_source_file(&app, &job_id, &file_id, contents)
}

/// Smoke test: confirm the bundled pdfium dylib and CJK font can be located
/// and that pdfium binds successfully. Phase 2 frontend uses this to surface
/// "PDF support unavailable" early before the user tries to import a PDF.
#[tauri::command]
pub fn probe_pdf_runtime(app: AppHandle) -> formats::pdf::PdfRuntimeStatus {
    formats::pdf::probe_status(&app)
}

/// Source PDF + (optional) translated PDF absolute paths for a PDF-format job.
/// The frontend wraps these with `convertFileSrc()` to get asset:// URLs that
/// react-pdf / pdfjs can load directly via XHR.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaPdfAssets {
    pub source_pdf: String,
    pub translated_pdf: Option<String>,
}

/// Resolve the cached source PDF and (if present) the generated translated PDF
/// for a job. Returns absolute filesystem paths.
///
/// On macOS the path itself isn't useful to the webview — `convertFileSrc`
/// returns `asset://localhost/<path>` which WebKit refuses to XHR from the
/// `tauri://localhost` origin (cross-protocol restriction), and forcing the
/// `http` arg returns `http://localhost/<path>` which doesn't route to Tauri's
/// asset handler at all. So callers actually use this only for existence
/// checks (e.g. "translated PDF already generated?"); to render a PDF they
/// pull bytes via [`read_rosetta_pdf_bytes`].
#[tauri::command]
pub fn get_rosetta_pdf_assets(app: AppHandle, job_id: String) -> Result<RosettaPdfAssets, String> {
    let source_pdf = store::cached_pdf_source_path(&app, &job_id)?;
    if !source_pdf.is_file() {
        return Err("项目缓存里找不到源 PDF。".to_string());
    }
    let translated_pdf_path = store::translated_pdf_output_path(&app, &job_id)?;
    let translated_pdf = if translated_pdf_path.is_file() {
        Some(translated_pdf_path.to_string_lossy().to_string())
    } else {
        None
    };
    Ok(RosettaPdfAssets {
        source_pdf: source_pdf.to_string_lossy().to_string(),
        translated_pdf,
    })
}

/// Read either the source PDF or the generated translated PDF for a job into
/// a binary IPC response so the webview can hand it directly to react-pdf as
/// `Uint8Array`. This is the canonical Phase 2 path; see the comment on
/// [`get_rosetta_pdf_assets`] for why URL-based loading was abandoned on
/// macOS.
#[tauri::command]
pub fn read_rosetta_pdf_bytes(
    app: AppHandle,
    job_id: String,
    kind: String,
) -> Result<tauri::ipc::Response, String> {
    let path = match kind.as_str() {
        "source" => store::cached_pdf_source_path(&app, &job_id)?,
        "translated" => store::translated_pdf_output_path(&app, &job_id)?,
        other => return Err(format!("未知的 PDF 类型: {other}")),
    };
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("读取 PDF 失败: {error}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Total page count of the source or translated PDF for a job. The frontend
/// uses this to pre-allocate page placeholders before any pixels are
/// rendered, so layout doesn't jump as PNGs arrive.
#[tauri::command]
pub fn count_rosetta_pdf_pages(
    app: AppHandle,
    job_id: String,
    kind: String,
) -> Result<u32, String> {
    let path = pdf_path_for_kind(&app, &job_id, &kind)?;
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    formats::pdf::count_pages(&app, &path).map_err(|error| error.user_message())
}

/// Render one page of the source or translated PDF to a PNG byte array.
/// Returned via `tauri::ipc::Response` so it crosses the IPC boundary as raw
/// bytes (avoiding a base64 round-trip). The frontend wraps these bytes into
/// a Blob URL and feeds them to `<img>`.
///
/// `target_width` is the requested pixel width; the renderer clamps it to a
/// sane range and derives the height from the page's aspect ratio.
#[tauri::command]
pub fn render_rosetta_pdf_page_as_png(
    app: AppHandle,
    job_id: String,
    kind: String,
    page_index: u32,
    target_width: u32,
) -> Result<tauri::ipc::Response, String> {
    let path = pdf_path_for_kind(&app, &job_id, &kind)?;
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    let bytes = formats::pdf::render_page_as_png(&app, &path, page_index, target_width)
        .map_err(|error| error.user_message())?;
    Ok(tauri::ipc::Response::new(bytes))
}

const PDF_PAGE_PROGRESS_EVENT: &str = "rosetta-pdf-page-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfPageProgressPayload {
    job_id: String,
    target_lang: Option<String>,
    run_id: Option<String>,
    page_number: u32,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfPageSummary {
    total_pages: u32,
    completed_pages: usize,
    failed_pages: usize,
    pending_pages: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfRepairResult {
    job_id: String,
    repaired: bool,
    recoverable: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfJobSnapshot {
    source: Option<formats::pdf::source_state::PdfSourceMetadata>,
    pages: formats::pdf::page_state::PdfPageTranslationState,
    run: Option<formats::pdf::run_state::PdfTranslationRun>,
    summary: PdfPageSummary,
    repair_warnings: Vec<String>,
}

#[tauri::command]
pub fn get_rosetta_pdf_page_status(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    target_lang: Option<String>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String> {
    Ok(get_rosetta_pdf_snapshot(app, cancel_state, job_id, target_lang)?.pages)
}

#[tauri::command]
pub fn get_rosetta_pdf_snapshot(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    target_lang: Option<String>,
) -> Result<PdfJobSnapshot, String> {
    let repair = repair_pdf_job_inner(&app, &job_id, cancel_state.session_id())?;
    let source_path = store::cached_pdf_source_path(&app, &job_id)?;
    let page_count =
        formats::pdf::count_pages(&app, &source_path).map_err(|error| error.user_message())?;
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let bundle = store::load_job_bundle(&app, &job_id)?;
    let target_lang = target_lang
        .or_else(|| {
            bundle
                .document
                .files
                .first()
                .and_then(|file| file.target_lang.clone())
        })
        .unwrap_or_else(|| bundle.document.target_lang.clone());
    let state =
        formats::pdf::page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)?;
    let run =
        formats::pdf::run_state::recover_stale_run(&dir, &target_lang, cancel_state.session_id())?;
    let effective = effective_pdf_page_state(
        state,
        run.as_ref(),
        cancel_state.session_id(),
        &pdf_run_key(&job_id, &target_lang),
        &cancel_state,
    );
    let source = formats::pdf::source_state::read_pdf_source_metadata(&dir)?;
    let summary = pdf_page_summary(&effective);
    Ok(PdfJobSnapshot {
        source,
        pages: effective,
        run,
        summary,
        repair_warnings: repair.warnings,
    })
}

#[tauri::command]
pub fn repair_rosetta_pdf_job(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
) -> Result<PdfRepairResult, String> {
    repair_pdf_job_inner(&app, &job_id, cancel_state.session_id())
}

fn repair_pdf_job_inner(
    app: &AppHandle,
    job_id: &str,
    session_id: &str,
) -> Result<PdfRepairResult, String> {
    use formats::pdf::{page_state, run_state, source_state};

    let root = path::jobs_root(app)?;
    let dir = path::checked_job_dir(&root, job_id)?;
    let mut warnings = Vec::new();
    let mut repaired = false;

    if !dir.exists() {
        return Ok(PdfRepairResult {
            job_id: job_id.to_string(),
            repaired: false,
            recoverable: false,
            warnings: vec!["项目目录不存在，只能从列表移除。".to_string()],
        });
    }

    let mut index = store::read_index(&root)?;
    let Some(job) = index.jobs.iter().find(|job| job.id == job_id).cloned() else {
        return Ok(PdfRepairResult {
            job_id: job_id.to_string(),
            repaired: false,
            recoverable: false,
            warnings: vec!["项目索引不存在。".to_string()],
        });
    };
    if job.format != "pdf" {
        return Ok(PdfRepairResult {
            job_id: job_id.to_string(),
            repaired: false,
            recoverable: true,
            warnings,
        });
    }

    let source_path = store::cached_pdf_source_path(app, job_id)?;
    if !source_path.is_file() {
        warnings.push("项目缓存里找不到 source.pdf，无法自动恢复 PDF 内容。".to_string());
        return Ok(PdfRepairResult {
            job_id: job_id.to_string(),
            repaired: false,
            recoverable: false,
            warnings,
        });
    }

    let page_count =
        formats::pdf::count_pages(app, &source_path).map_err(|error| error.user_message())?;
    if !dir.join("segments.json").is_file() {
        store::write_json(&dir.join("segments.json"), &Vec::<model::Segment>::new())?;
        repaired = true;
    }
    if !dir.join("document.json").is_file() {
        let document = repair_pdf_document_from_job(&job);
        store::write_json(&dir.join("document.json"), &document)?;
        warnings.push("document.json 缺失，已从索引和 source.pdf 重建 PDF 文档记录。".to_string());
        repaired = true;
    }

    let source_metadata = source_state::build_pdf_source_metadata(
        &source_path,
        page_count,
        job.filename.clone(),
        job.source_path.clone(),
        Some(job.created_at.clone()),
    )?;
    let existing_source = source_state::read_pdf_source_metadata(&dir)?;
    let source_changed = match existing_source.as_ref() {
        Some(existing) => {
            existing.page_count != source_metadata.page_count
                || existing.source_fingerprint != source_metadata.source_fingerprint
                || existing.filename != source_metadata.filename
                || existing.original_path != source_metadata.original_path
        }
        None => true,
    };
    if source_changed {
        source_state::write_pdf_source_metadata(&dir, &source_metadata)?;
        repaired = true;
    }

    let target_langs = pdf_target_langs_for_repair(&dir, &job);
    let mut active_run_ids = std::collections::BTreeSet::new();
    for target_lang in target_langs {
        if let Some(run) = run_state::recover_stale_run(&dir, &target_lang, session_id)? {
            if run.state == "paused" {
                repaired = true;
            }
            if run.is_live_state() && run.owner_session_id == session_id {
                active_run_ids.insert(run.run_id.clone());
            }
        }
        let mut state =
            page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)?;
        if repair_pdf_page_artifacts(&dir, &mut state, &target_lang)? {
            repaired = true;
        }
        page_state::write_pdf_page_translation_state(&dir, &state)?;
        sync_pdf_page_translation_summary(app, job_id, &target_lang, &state)?;
    }
    if cleanup_stale_pdf_run_tmp_dirs(&dir, &active_run_ids)? {
        repaired = true;
    }

    index = store::read_index(&root)?;
    store::write_index(&root, &index)?;

    Ok(PdfRepairResult {
        job_id: job_id.to_string(),
        repaired,
        recoverable: true,
        warnings,
    })
}

fn repair_all_pdf_jobs(app: &AppHandle, session_id: &str) -> Result<(), String> {
    let root = path::jobs_root(app)?;
    let index = store::read_index(&root)?;
    for job in index.jobs.iter().filter(|job| job.format == "pdf") {
        let _ = repair_pdf_job_inner(app, &job.id, session_id);
    }
    Ok(())
}

fn cleanup_stale_pdf_run_tmp_dirs(
    dir: &Path,
    active_run_ids: &std::collections::BTreeSet<String>,
) -> Result<bool, String> {
    let runs_root = dir.join(".tmp").join("pdf-runs");
    if !runs_root.is_dir() {
        return Ok(false);
    }

    let mut cleaned = false;
    for entry in
        fs::read_dir(&runs_root).map_err(|error| format!("无法读取 PDF run 临时目录: {error}"))?
    {
        let entry = entry.map_err(|error| format!("无法读取 PDF run 临时项: {error}"))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if active_run_ids.contains(name) {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("无法清理旧 PDF run 临时目录: {error}"))?;
            cleaned = true;
        } else if path.is_file() {
            fs::remove_file(&path)
                .map_err(|error| format!("无法清理旧 PDF run 临时文件: {error}"))?;
            cleaned = true;
        }
    }

    if fs::read_dir(&runs_root)
        .map_err(|error| format!("无法检查 PDF run 临时目录: {error}"))?
        .next()
        .is_none()
    {
        let _ = fs::remove_dir(&runs_root);
        if let Some(tmp_root) = runs_root.parent() {
            let _ = fs::remove_dir(tmp_root);
        }
    }

    Ok(cleaned)
}

fn repair_pdf_document_from_job(job: &model::RosettaJobSummary) -> model::RosettaDocument {
    model::RosettaDocument {
        schema_version: model::SCHEMA_VERSION,
        id: format!("document-{}", job.id),
        filename: job.filename.clone(),
        format: "pdf".to_string(),
        source_lang: job
            .source_files
            .first()
            .and_then(|file| file.source_lang.clone())
            .or_else(|| Some("en".to_string())),
        target_lang: job.target_lang.clone(),
        files: vec![model::RosettaSourceFile {
            id: "file-1".to_string(),
            filename: job.source_filename.clone(),
            relative_path: job.source_filename.clone(),
            format: "pdf".to_string(),
            source_lang: job
                .source_files
                .first()
                .and_then(|file| file.source_lang.clone())
                .or_else(|| Some("en".to_string())),
            target_lang: Some(job.target_lang.clone()),
            translation_status: model::default_file_translation_status(),
            segment_count: job.segment_count,
            completed_segments: job.completed_segments,
            failed_segments: job.failed_segments,
            translating_segments: 0,
            block_ids: Vec::new(),
        }],
        blocks: Vec::new(),
        extraction_status: Some("done".to_string()),
    }
}

fn pdf_target_langs_for_repair(dir: &Path, job: &model::RosettaJobSummary) -> Vec<String> {
    use formats::pdf::page_state;
    let mut langs = std::collections::BTreeSet::new();
    langs.insert(job.target_lang.clone());
    for file in &job.source_files {
        if let Some(lang) = &file.target_lang {
            langs.insert(lang.clone());
        }
    }
    if let Ok(translation_files) = store::read_translation_files(dir) {
        for translation_file in translation_files {
            langs.insert(translation_file.target_lang);
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if let Some(slug) = name
                .strip_prefix("pdf_pages.")
                .and_then(|rest| rest.strip_suffix(".json"))
            {
                langs.insert(slug.to_string());
            }
            if let Some(slug) = name
                .strip_prefix("pdf_page_translations.")
                .and_then(|rest| rest.strip_suffix(".json"))
            {
                langs.insert(slug.to_string());
            }
        }
    }
    langs
        .into_iter()
        .filter(|lang| !lang.trim().is_empty())
        .map(|lang| {
            let slug = page_state::pdf_page_language_dir(&lang);
            if slug == "unknown" {
                job.target_lang.clone()
            } else {
                lang
            }
        })
        .collect()
}

fn repair_pdf_page_artifacts(
    dir: &Path,
    state: &mut formats::pdf::page_state::PdfPageTranslationState,
    target_lang: &str,
) -> Result<bool, String> {
    use formats::pdf::page_state;
    let mut repaired = false;
    let mut known_pages = std::collections::BTreeSet::new();

    for page in &mut state.pages {
        known_pages.insert(page.page_number);
        if page.status != "translated" {
            continue;
        }
        let Some(path) = find_existing_pdf_page_artifact(
            dir,
            target_lang,
            page.page_number,
            page.translated_pdf_path.as_deref(),
        ) else {
            page.status = "pending".to_string();
            page.translated_pdf_path = None;
            page.artifact_version = None;
            page.error = None;
            page.updated_at = path::timestamp_ms_string();
            repaired = true;
            continue;
        };
        let canonical_relative =
            page_state::pdf_page_relative_path_for_lang(target_lang, page.page_number);
        let canonical = dir.join(&canonical_relative);
        if path != canonical {
            if let Some(parent) = canonical.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("无法创建 PDF 页译文目录: {error}"))?;
            }
            fs::copy(&path, &canonical)
                .map_err(|error| format!("无法迁移 PDF 页译文产物: {error}"))?;
            repaired = true;
        }
        if page.translated_pdf_path.as_deref() != Some(canonical_relative.as_str()) {
            page.translated_pdf_path = Some(canonical_relative);
            page.artifact_version = Some(path::timestamp_ms_string());
            page.error = None;
            page.updated_at = path::timestamp_ms_string();
            repaired = true;
        }
    }

    let translated_dir = dir
        .join("translated-pages")
        .join(page_state::pdf_page_language_dir(target_lang));
    if let Ok(entries) = fs::read_dir(&translated_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(page_number) = page_number_from_pdf_filename(&path) else {
                continue;
            };
            if page_number == 0
                || page_number > state.source_page_count
                || known_pages.contains(&page_number)
            {
                continue;
            }
            let relative = page_state::pdf_page_relative_path_for_lang(target_lang, page_number);
            page_state::upsert_pdf_page(state, page_number, "translated", Some(relative), None);
            repaired = true;
        }
    }
    Ok(repaired)
}

fn find_existing_pdf_page_artifact(
    dir: &Path,
    target_lang: &str,
    page_number: u32,
    stored_relative: Option<&str>,
) -> Option<std::path::PathBuf> {
    use formats::pdf::page_state;
    let mut candidates = Vec::new();
    if let Some(relative) = stored_relative {
        if let Some(path) = safe_relative_cache_path(dir, relative) {
            candidates.push(path);
        }
    }
    candidates.push(dir.join(page_state::pdf_page_relative_path_for_lang(
        target_lang,
        page_number,
    )));
    candidates.push(dir.join(page_state::legacy_pdf_page_relative_path_for_lang(
        target_lang,
        page_number,
    )));
    candidates.push(dir.join(page_state::legacy_pdf_page_relative_path(page_number)));
    candidates.into_iter().find(|path| path.is_file())
}

fn page_number_from_pdf_filename(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let number = name
        .strip_prefix("page-")?
        .strip_suffix(".pdf")?
        .parse::<u32>()
        .ok()?;
    Some(number)
}

fn effective_pdf_page_state(
    mut state: formats::pdf::page_state::PdfPageTranslationState,
    run: Option<&formats::pdf::run_state::PdfTranslationRun>,
    session_id: &str,
    run_key: &str,
    cancel_state: &PdfTranslationCancelState,
) -> formats::pdf::page_state::PdfPageTranslationState {
    let Some(run) = run else {
        return state;
    };
    if !run.is_live_state()
        || run.owner_session_id != session_id
        || !cancel_state.active_run_matches(run_key, &run.run_id)
    {
        return state;
    }
    let current_chunk: std::collections::BTreeSet<u32> =
        run.current_chunk.iter().copied().collect();
    for page_number in &run.requested_pages {
        let Some(index) = state
            .pages
            .iter()
            .position(|page| page.page_number == *page_number)
        else {
            state
                .pages
                .push(formats::pdf::page_state::PdfPageTranslation {
                    page_number: *page_number,
                    status: if current_chunk.contains(page_number) {
                        "translating".to_string()
                    } else {
                        "queued".to_string()
                    },
                    translated_pdf_path: None,
                    artifact_version: None,
                    error: None,
                    updated_at: run.updated_at.clone(),
                    last_run_id: Some(run.run_id.clone()),
                });
            continue;
        };
        if state.pages[index].status == "translated" {
            continue;
        }
        state.pages[index].status = if current_chunk.contains(page_number) {
            "translating".to_string()
        } else {
            "queued".to_string()
        };
        state.pages[index].last_run_id = Some(run.run_id.clone());
    }
    state.pages.sort_by_key(|page| page.page_number);
    state
}

fn pdf_page_summary(state: &formats::pdf::page_state::PdfPageTranslationState) -> PdfPageSummary {
    let completed_pages = state
        .pages
        .iter()
        .filter(|page| page.status == "translated")
        .count();
    let failed_pages = state
        .pages
        .iter()
        .filter(|page| page.status == "failed")
        .count();
    PdfPageSummary {
        total_pages: state.source_page_count,
        completed_pages,
        failed_pages,
        pending_pages: (state.source_page_count as usize)
            .saturating_sub(completed_pages)
            .saturating_sub(failed_pages),
    }
}

fn pdf_run_key(job_id: &str, target_lang: &str) -> String {
    format!(
        "{job_id}:{}",
        formats::pdf::page_state::pdf_page_language_dir(target_lang)
    )
}

#[tauri::command]
pub async fn translate_rosetta_pdf_pages(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    page_selection: String,
    target_lang: String,
    rwkv_base_url: String,
    provider_id: Option<String>,
    provider_endpoint: Option<String>,
    provider_internal_token: Option<String>,
    provider_body_password: Option<String>,
    source_lang: Option<String>,
    timeout_ms: Option<u64>,
    force: Option<bool>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String> {
    eprintln!(
        "[pdf-translate] command received: job_id={job_id}, target_lang={target_lang}, page_selection={page_selection}, provider_id={}, force={}",
        provider_id.as_deref().unwrap_or("default"),
        force.unwrap_or(false)
    );
    let result = translate_pdf_pages_inner(
        &app,
        &cancel_state,
        &job_id,
        &page_selection,
        &target_lang,
        rwkv_base_url,
        provider_id,
        provider_endpoint,
        provider_internal_token,
        provider_body_password,
        source_lang,
        timeout_ms,
        force,
    )
    .await;
    cancel_state.end_run();
    match &result {
        Ok(state) => eprintln!(
            "[pdf-translate] command completed: job_id={job_id}, target_lang={target_lang}, pages={}",
            state.pages.len()
        ),
        Err(error) => eprintln!(
            "[pdf-translate] command failed: job_id={job_id}, target_lang={target_lang}, error={error}"
        ),
    }
    result
}

fn shim_provider_from_parts(
    base_url: &str,
    provider_id: Option<&str>,
    provider_endpoint: &str,
    provider_internal_token: Option<String>,
    provider_body_password: Option<String>,
    timeout_ms: u64,
) -> ShimProviderConfig {
    match provider_id {
        Some("llama-cpp-chat-completions") => ShimProviderConfig::LlamaCpp(LlamaCppApiConfig {
            base_url: base_url.to_string(),
            timeout_ms,
        }),
        Some("rwkv-mobile-batch-chat") => ShimProviderConfig::MobileBatch(MobileBatchChatConfig {
            base_url: base_url.to_string(),
            timeout_ms,
        }),
        Some("rwkv-lightning-contents") => ShimProviderConfig::Lightning(LightningApiConfig {
            base_url: base_url.to_string(),
            endpoint: if provider_endpoint.trim().is_empty() {
                "/v1/batch/completions".to_string()
            } else {
                provider_endpoint.to_string()
            },
            internal_token: provider_internal_token.unwrap_or_default(),
            body_password: provider_body_password.unwrap_or_default(),
            timeout_ms,
        }),
        _ if provider_endpoint.trim().is_empty() => {
            ShimProviderConfig::MobileBatch(MobileBatchChatConfig {
                base_url: base_url.to_string(),
                timeout_ms,
            })
        }
        _ => ShimProviderConfig::Lightning(LightningApiConfig {
            base_url: base_url.to_string(),
            endpoint: provider_endpoint.to_string(),
            internal_token: provider_internal_token.unwrap_or_default(),
            body_password: provider_body_password.unwrap_or_default(),
            timeout_ms,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn translate_pdf_pages_inner(
    app: &AppHandle,
    cancel_state: &PdfTranslationCancelState,
    job_id: &str,
    page_selection: &str,
    target_lang: &str,
    rwkv_base_url: String,
    provider_id: Option<String>,
    provider_endpoint: Option<String>,
    provider_internal_token: Option<String>,
    provider_body_password: Option<String>,
    source_lang: Option<String>,
    timeout_ms: Option<u64>,
    force: Option<bool>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String> {
    use formats::pdf::{diagnostics, page_state, pdf2zh_invoke, run_state};

    let _ = repair_pdf_job_inner(app, job_id, cancel_state.session_id());
    let bundle = store::load_job_bundle(app, job_id)?;
    if bundle.document.format != "pdf" {
        return Err("当前文档不是 PDF，无法按页翻译。".to_string());
    }

    let source_path = store::cached_pdf_source_path(app, job_id)?;
    let page_count =
        formats::pdf::count_pages(app, &source_path).map_err(|error| error.user_message())?;
    let pages = page_state::parse_pdf_page_selection(page_selection, page_count)?;
    let root = path::jobs_root(app)?;
    let dir = path::checked_job_dir(&root, job_id)?;
    let pages_dir = dir
        .join("translated-pages")
        .join(page_state::pdf_page_language_dir(target_lang));
    fs::create_dir_all(&pages_dir).map_err(|error| format!("无法创建 PDF 页译文目录: {error}"))?;
    let mut state = page_state::read_pdf_page_translation_state(&dir, page_count, target_lang)?;
    let source_lang = source_lang
        .filter(|lang| lang != "auto")
        .or_else(|| {
            bundle
                .document
                .files
                .first()
                .and_then(|file| file.source_lang.clone())
        })
        .unwrap_or_else(|| "en".to_string());
    let rwkv_base_url = rwkv_base_url.trim().to_string();
    if rwkv_base_url.is_empty() {
        return Err("PDF 翻译需要配置 API 地址。请先启动本地运行时或配置远程 API。".to_string());
    }
    let timeout = timeout_ms.unwrap_or(120_000);
    let provider_endpoint = provider_endpoint
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let provider = shim_provider_from_parts(
        &rwkv_base_url,
        provider_id.as_deref(),
        &provider_endpoint,
        provider_internal_token,
        provider_body_password,
        timeout,
    );
    let force = force.unwrap_or(false);
    let mode = if force && pages.len() as u32 == page_count {
        "retranslate-all"
    } else if force {
        "retranslate-selected"
    } else {
        "continue"
    };

    let pages_to_process: Vec<u32> = pages
        .iter()
        .copied()
        .filter(|page_number| {
            if force {
                return true;
            }
            !state.pages.iter().any(|page| {
                page.page_number == *page_number
                    && page.status == "translated"
                    && page.translated_pdf_path.is_some()
            })
        })
        .collect();
    let total_pages_to_process = pages_to_process.len() as u32;
    diagnostics::append_timeline_event(
        &dir,
        diagnostics::PdfTimelineEvent::new(job_id, "translation", "translation.requested")
            .target_lang(target_lang)
            .details(json!({
                "pageSelection": page_selection,
                "sourcePageCount": page_count,
                "selectedPageCount": pages.len(),
                "pagesToProcess": total_pages_to_process,
                "force": force,
                "mode": mode,
                "providerId": provider_id.as_deref().unwrap_or("default"),
                "sourceLang": source_lang.clone(),
            })),
    );
    if pages_to_process.is_empty() {
        diagnostics::append_timeline_event(
            &dir,
            diagnostics::PdfTimelineEvent::new(job_id, "translation", "translation.skipped")
                .target_lang(target_lang)
                .details(json!({
                    "reason": "all-requested-pages-already-translated",
                    "pageSelection": page_selection,
                    "selectedPageCount": pages.len(),
                })),
        );
        sync_pdf_page_translation_summary(app, job_id, target_lang, &state)?;
        return Ok(state);
    }

    let run_started = std::time::Instant::now();
    let run_id = format!("run-pdf-{}", path::timestamp_ms_string());
    let run_key = pdf_run_key(job_id, target_lang);
    if !cancel_state.try_begin_pdf_run(run_key.clone(), run_id.clone()) {
        return Err("已有 PDF 翻译正在进行，请先停止当前翻译。".to_string());
    }
    let mut run = run_state::PdfTranslationRun::new(
        run_id.clone(),
        job_id.to_string(),
        target_lang.to_string(),
        mode.to_string(),
        pages_to_process.clone(),
        cancel_state.session_id().to_string(),
    );
    run_state::write_pdf_run_state(&dir, &run)?;
    diagnostics::append_timeline_event(
        &dir,
        diagnostics::PdfTimelineEvent::new(job_id, "translation", "run.started")
            .run_id(&run_id)
            .target_lang(target_lang)
            .details(json!({
                "mode": mode,
                "requestedPages": pages_to_process.clone(),
                "requestedPageCount": total_pages_to_process,
                "chunkSize": run_state::PDF_RUN_CHUNK_SIZE,
                "ownerSessionId": cancel_state.session_id(),
            })),
    );

    let mut profile = diagnostics::new_profile(
        &run_id,
        job_id,
        &source_lang,
        target_lang,
        page_selection,
        total_pages_to_process,
        path::timestamp_ms_string(),
    );
    let mut rwkv_aggregate = diagnostics::RwkvAggregate::default();
    let finish_profile = |profile: &mut diagnostics::PdfTranslationProfile,
                          status: &str,
                          rwkv: &diagnostics::RwkvAggregate| {
        profile.status = status.to_string();
        profile.ended_at = path::timestamp_ms_string();
        profile.durations_ms.total = run_started.elapsed().as_millis() as u64;
        if rwkv.request_count > 0 {
            profile.rwkv = Some(rwkv.clone());
        }
        let event_name = match status {
            "completed" => "run.completed",
            "cancelled" => "run.cancelled",
            "failed" => "run.failed",
            _ => "run.finished",
        };
        diagnostics::append_timeline_event(
            &dir,
            diagnostics::PdfTimelineEvent::new(job_id, "translation", event_name)
                .run_id(&profile.run_id)
                .target_lang(&profile.target_lang)
                .duration_ms(profile.durations_ms.total)
                .details(json!({
                    "status": status,
                    "sourceLang": profile.source_lang.clone(),
                    "pageSelection": profile.page_selection.clone(),
                    "pagesRequested": profile.pages_requested,
                    "pagesTranslated": profile.pages_translated,
                    "pagesFailed": profile.pages_failed,
                    "invocationCount": profile.invocation_count,
                    "durationsMs": {
                        "total": profile.durations_ms.total,
                        "pdf2zhWarmup": profile.durations_ms.pdf2zh_warmup,
                        "pdf2zhProcess": profile.durations_ms.pdf2zh_process,
                        "pageArtifactAssembly": profile.durations_ms.page_artifact_assembly,
                    },
                    "rwkv": diagnostics::rwkv_aggregate_details(rwkv),
                })),
        );
        diagnostics::write_profile(&dir, profile);
    };

    if force {
        for page_number in &pages {
            clear_pdf_page_artifacts(&dir, &mut state, target_lang, *page_number);
        }
        page_state::write_pdf_page_translation_state(&dir, &state)?;
    }

    pdf2zh_invoke::emit_progress_phase(
        app,
        job_id,
        "warmup",
        Some(0),
        "正在准备翻译引擎…",
        total_pages_to_process,
    );

    let run_tmp_root = dir.join(".tmp").join("pdf-runs").join(&run_id);
    if run_tmp_root.exists() {
        fs::remove_dir_all(&run_tmp_root)
            .map_err(|error| format!("无法清理旧 PDF run 临时目录: {error}"))?;
    }
    fs::create_dir_all(&run_tmp_root)
        .map_err(|error| format!("无法创建 PDF run 临时目录: {error}"))?;

    let mut processed_before = 0u32;
    let mut translated_chars_offset = 0u64;
    let mut failure_message: Option<String> = None;
    let mut cancelled = false;

    for (chunk_index, chunk) in pages_to_process
        .chunks(run_state::PDF_RUN_CHUNK_SIZE)
        .enumerate()
    {
        if cancel_state.is_pdf_run_cancelled(&run_key, &run_id) {
            cancelled = true;
            break;
        }
        let chunk_pages = chunk.to_vec();
        run.current_chunk = chunk_pages.clone();
        run.touch_lease();
        run_state::write_pdf_run_state(&dir, &run)?;
        let chunk_started = std::time::Instant::now();
        diagnostics::append_timeline_event(
            &dir,
            diagnostics::PdfTimelineEvent::new(job_id, "translation", "chunk.started")
                .run_id(&run_id)
                .target_lang(target_lang)
                .details(json!({
                    "chunkIndex": chunk_index,
                    "pages": chunk_pages.clone(),
                    "pageCount": chunk_pages.len(),
                    "completedBefore": processed_before,
                    "totalPagesToProcess": total_pages_to_process,
                })),
        );
        for page_number in &chunk_pages {
            emit_pdf_page_progress_for_run(
                app,
                job_id,
                target_lang,
                &run_id,
                *page_number,
                "translating",
            );
        }

        let invocation_output_dir = run_tmp_root.join(format!("chunk-{chunk_index:04}"));
        if invocation_output_dir.exists() {
            fs::remove_dir_all(&invocation_output_dir)
                .map_err(|error| format!("无法清理 PDF 批次临时目录: {error}"))?;
        }
        fs::create_dir_all(&invocation_output_dir)
            .map_err(|error| format!("无法创建 PDF 批次临时目录: {error}"))?;

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        cancel_state.register_sender(cancel_tx);

        let dir_for_cb = dir.clone();
        let app_for_cb = app.clone();
        let job_id_for_cb = job_id.to_string();
        let target_lang_for_cb = target_lang.to_string();
        let run_id_for_cb = run_id.clone();
        let state_for_cb: &mut formats::pdf::page_state::PdfPageTranslationState = &mut state;
        let mut on_page_done = move |page_number: u32, worker_file: std::path::PathBuf| {
            let relative_path =
                page_state::pdf_page_relative_path_for_lang(&target_lang_for_cb, page_number);
            let target_path = dir_for_cb.join(&relative_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            let outcome = commit_pdf_page_artifact(&worker_file, &target_path);

            match outcome {
                Ok(_) => {
                    let output_bytes = fs::metadata(&target_path)
                        .map(|metadata| metadata.len())
                        .ok();
                    page_state::upsert_pdf_page_with_run(
                        state_for_cb,
                        page_number,
                        "translated",
                        Some(relative_path),
                        None,
                        Some(&run_id_for_cb),
                    );
                    let _ = page_state::write_pdf_page_translation_state(&dir_for_cb, state_for_cb);
                    let _ = app_for_cb.emit(
                        PDF_PAGE_PROGRESS_EVENT,
                        PdfPageProgressPayload {
                            job_id: job_id_for_cb.clone(),
                            target_lang: Some(target_lang_for_cb.clone()),
                            run_id: Some(run_id_for_cb.clone()),
                            page_number,
                            status: "translated".to_string(),
                        },
                    );
                    diagnostics::append_timeline_event(
                        &dir_for_cb,
                        diagnostics::PdfTimelineEvent::new(
                            &job_id_for_cb,
                            "translation",
                            "page.committed",
                        )
                        .run_id(&run_id_for_cb)
                        .target_lang(&target_lang_for_cb)
                        .page_number(page_number)
                        .details(json!({
                            "translatedPdfPath": page_state::pdf_page_relative_path_for_lang(
                                &target_lang_for_cb,
                                page_number,
                            ),
                            "outputBytes": output_bytes,
                        })),
                    );
                }
                Err(error) => {
                    let error_for_timeline = error.clone();
                    page_state::upsert_pdf_page_with_run(
                        state_for_cb,
                        page_number,
                        "failed",
                        None,
                        Some(error),
                        Some(&run_id_for_cb),
                    );
                    let _ = page_state::write_pdf_page_translation_state(&dir_for_cb, state_for_cb);
                    let _ = app_for_cb.emit(
                        PDF_PAGE_PROGRESS_EVENT,
                        PdfPageProgressPayload {
                            job_id: job_id_for_cb.clone(),
                            target_lang: Some(target_lang_for_cb.clone()),
                            run_id: Some(run_id_for_cb.clone()),
                            page_number,
                            status: "failed".to_string(),
                        },
                    );
                    diagnostics::append_timeline_event(
                        &dir_for_cb,
                        diagnostics::PdfTimelineEvent::new(
                            &job_id_for_cb,
                            "translation",
                            "page.commitFailed",
                        )
                        .run_id(&run_id_for_cb)
                        .target_lang(&target_lang_for_cb)
                        .page_number(page_number)
                        .details(json!({
                            "error": error_for_timeline,
                        })),
                    );
                }
            }
        };

        let dir_for_stage = dir.clone();
        let job_id_for_stage = job_id.to_string();
        let target_lang_for_stage = target_lang.to_string();
        let run_id_for_stage = run_id.clone();
        let mut on_worker_stage = move |stage: crate::managed_pdf2zh::worker::WorkerStageEvent| {
            let mut event =
                diagnostics::PdfTimelineEvent::new(&job_id_for_stage, "worker", "worker.stage")
                    .run_id(&run_id_for_stage)
                    .target_lang(&target_lang_for_stage)
                    .details(json!({
                        "stage": stage.stage,
                        "status": stage.status,
                        "pageNumber": stage.page_number,
                        "durationMs": stage.duration_ms,
                        "stageDetails": stage.details,
                    }));
            if let Some(duration_ms) = stage.duration_ms {
                event = event.duration_ms(duration_ms);
            }
            diagnostics::append_timeline_event(&dir_for_stage, event);
        };

        let invoke_result = pdf2zh_invoke::invoke_pdf2zh(
            app,
            &source_path,
            &invocation_output_dir,
            pdf2zh_invoke::Pdf2zhInvokeOptions {
                job_id: job_id.to_string(),
                provider: provider.clone(),
                source_lang: source_lang.clone(),
                target_lang: target_lang.to_string(),
                ignore_cache: force,
                pages: Some(chunk_pages.clone()),
                page_progress: Some(pdf2zh_invoke::PageProgressContext {
                    completed_before: processed_before,
                    chunk_len: chunk_pages.len() as u32,
                    total: total_pages_to_process,
                }),
                translated_chars_offset,
            },
            cancel_rx,
            Some(&mut on_page_done),
            Some(&mut on_worker_stage),
        )
        .await;

        cancel_state.clear_sender();
        drop(on_page_done);
        profile.invocation_count += 1;

        match invoke_result {
            Ok(output) => {
                profile.durations_ms.pdf2zh_warmup += output.warmup_ms;
                profile.durations_ms.pdf2zh_process += output.process_ms;
                translated_chars_offset += output.rwkv_metrics.total_output_chars;
                diagnostics::append_timeline_event(
                    &dir,
                    diagnostics::PdfTimelineEvent::new(job_id, "translation", "chunk.completed")
                        .run_id(&run_id)
                        .target_lang(target_lang)
                        .duration_ms(chunk_started.elapsed().as_millis() as u64)
                        .details(json!({
                            "chunkIndex": chunk_index,
                            "pages": chunk_pages.clone(),
                            "warmupMs": output.warmup_ms,
                            "processMs": output.process_ms,
                            "rwkv": diagnostics::rwkv_snapshot_details(&output.rwkv_metrics),
                        })),
                );
                rwkv_aggregate.add(&output.rwkv_metrics);
                if let Some(message) = pdf_rwkv_failure_message(&output.rwkv_metrics) {
                    diagnostics::append_timeline_event(
                        &dir,
                        diagnostics::PdfTimelineEvent::new(
                            job_id,
                            "translation",
                            "chunk.rwkvFailed",
                        )
                        .run_id(&run_id)
                        .target_lang(target_lang)
                        .details(json!({
                            "chunkIndex": chunk_index,
                            "pages": chunk_pages.clone(),
                            "error": message.clone(),
                            "rwkv": diagnostics::rwkv_snapshot_details(&output.rwkv_metrics),
                        })),
                    );
                    failure_message = Some(message);
                    for page_number in &chunk_pages {
                        clear_pdf_page_artifacts(&dir, &mut state, target_lang, *page_number);
                    }
                } else if force && output.rwkv_metrics.request_count == 0 {
                    failure_message = Some(
                        "PDF 重翻没有向翻译模型发送任何文本，已拒绝复用旧译文。请确认该页包含可提取文本后再试。"
                            .to_string(),
                    );
                    for page_number in &chunk_pages {
                        clear_pdf_page_artifacts(&dir, &mut state, target_lang, *page_number);
                    }
                }
            }
            Err(formats::pdf::errors::PdfError::Cancelled) => {
                diagnostics::append_timeline_event(
                    &dir,
                    diagnostics::PdfTimelineEvent::new(job_id, "translation", "chunk.cancelled")
                        .run_id(&run_id)
                        .target_lang(target_lang)
                        .duration_ms(chunk_started.elapsed().as_millis() as u64)
                        .details(json!({
                            "chunkIndex": chunk_index,
                            "pages": chunk_pages.clone(),
                        })),
                );
                cancelled = true;
            }
            Err(error) => {
                let message = error.user_message();
                diagnostics::append_timeline_event(
                    &dir,
                    diagnostics::PdfTimelineEvent::new(job_id, "translation", "chunk.failed")
                        .run_id(&run_id)
                        .target_lang(target_lang)
                        .duration_ms(chunk_started.elapsed().as_millis() as u64)
                        .details(json!({
                            "chunkIndex": chunk_index,
                            "pages": chunk_pages.clone(),
                            "error": message.clone(),
                        })),
                );
                failure_message = Some(message);
            }
        }

        let chunk_failed_message = failure_message
            .clone()
            .unwrap_or_else(|| "PDF 批次未生成译文页。".to_string());
        let chunk_had_failure = failure_message.is_some();
        for page_number in &chunk_pages {
            let translated = state.pages.iter().any(|page| {
                page.page_number == *page_number
                    && page.status == "translated"
                    && page.translated_pdf_path.is_some()
            });
            if translated {
                run_state::append_unique_page(&mut run.completed_pages, *page_number);
                continue;
            }
            if cancelled {
                page_state::upsert_pdf_page_with_run(
                    &mut state,
                    *page_number,
                    "pending",
                    None,
                    None,
                    Some(&run_id),
                );
                emit_pdf_page_progress_for_run(
                    app,
                    job_id,
                    target_lang,
                    &run_id,
                    *page_number,
                    "pending",
                );
            } else {
                page_state::upsert_pdf_page_with_run(
                    &mut state,
                    *page_number,
                    "failed",
                    None,
                    Some(chunk_failed_message.clone()),
                    Some(&run_id),
                );
                run_state::append_unique_page(&mut run.failed_pages, *page_number);
                emit_pdf_page_progress_for_run(
                    app,
                    job_id,
                    target_lang,
                    &run_id,
                    *page_number,
                    "failed",
                );
            }
        }
        page_state::write_pdf_page_translation_state(&dir, &state)?;
        run.current_chunk.clear();
        run.touch_lease();
        if cancelled {
            run.state = "paused".to_string();
            run.cancel_requested = false;
        } else if chunk_had_failure {
            run.state = "failed".to_string();
            run.last_error = failure_message.clone();
        }
        run_state::write_pdf_run_state(&dir, &run)?;
        sync_pdf_page_translation_summary(app, job_id, target_lang, &state)?;
        let _ = fs::remove_dir_all(&invocation_output_dir);

        processed_before += chunk_pages.len() as u32;
        if cancelled || failure_message.is_some() {
            break;
        }
    }

    let _ = fs::remove_dir_all(&run_tmp_root);

    profile.pages_translated = state
        .pages
        .iter()
        .filter(|page| page.status == "translated")
        .count() as u32;
    profile.pages_failed = state
        .pages
        .iter()
        .filter(|page| page.status == "failed")
        .count() as u32;

    sync_pdf_page_translation_summary(app, job_id, target_lang, &state)?;

    if let Some(message) = failure_message {
        run.set_state("failed");
        run.last_error = Some(message.clone());
        run.current_chunk.clear();
        run_state::write_pdf_run_state(&dir, &run)?;
        finish_profile(&mut profile, "failed", &rwkv_aggregate);
        return Err(message);
    }
    if cancelled {
        run.set_state("paused");
        run.cancel_requested = false;
        run.current_chunk.clear();
        run_state::write_pdf_run_state(&dir, &run)?;
        finish_profile(&mut profile, "cancelled", &rwkv_aggregate);
        return Err("已取消 PDF 翻译。".to_string());
    }
    run.set_state("completed");
    run.current_chunk.clear();
    run_state::write_pdf_run_state(&dir, &run)?;
    finish_profile(&mut profile, "completed", &rwkv_aggregate);
    Ok(state)
}

fn pdf_rwkv_failure_message(
    snapshot: &crate::managed_pdf2zh::openai_shim::ShimRwkvMetricsSnapshot,
) -> Option<String> {
    (snapshot.failed_request_count > 0).then(|| {
        format!(
            "PDF 翻译检测到底层模型请求失败（failedRequestCount={}），已拒绝提交本批次页面，避免截断或空译文被标记为成功。",
            snapshot.failed_request_count
        )
    })
}

fn commit_pdf_page_artifact(worker_file: &Path, target_path: &Path) -> Result<(), String> {
    let parent = target_path
        .parent()
        .ok_or_else(|| "译文页目标路径无父目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建译文页目录: {error}"))?;
    let filename = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("page.pdf");
    let temp_path = parent.join(format!(
        ".{filename}.{}.tmp.pdf",
        path::timestamp_ms_string()
    ));
    fs::copy(worker_file, &temp_path)
        .map_err(|error| format!("无法保存译文页临时文件: {error}"))?;

    let cleanup_temp = || {
        let _ = fs::remove_file(&temp_path);
    };
    let page_count = match formats::pdf::page_assemble::count_pdf_pages_lopdf(&temp_path) {
        Ok(count) => count,
        Err(error) => {
            cleanup_temp();
            return Err(format!("译文页 PDF 无法打开: {error}"));
        }
    };
    if page_count != 1 {
        cleanup_temp();
        return Err(format!(
            "译文页 PDF 页数异常: 期望 1 页，实际 {page_count} 页。"
        ));
    }

    if target_path.exists() {
        fs::remove_file(target_path).map_err(|error| format!("无法替换旧译文页: {error}"))?;
    }
    fs::rename(&temp_path, target_path).map_err(|error| {
        cleanup_temp();
        format!("无法提交译文页: {error}")
    })
}

fn clear_pdf_page_artifacts(
    job_dir: &Path,
    state: &mut formats::pdf::page_state::PdfPageTranslationState,
    target_lang: &str,
    page_number: u32,
) {
    use formats::pdf::page_state;

    let mut candidates = vec![job_dir.join(page_state::pdf_page_relative_path_for_lang(
        target_lang,
        page_number,
    ))];
    candidates.push(
        job_dir.join(page_state::legacy_pdf_page_relative_path_for_lang(
            target_lang,
            page_number,
        )),
    );
    candidates.push(job_dir.join(page_state::legacy_pdf_page_relative_path(page_number)));
    if let Some(page) = state
        .pages
        .iter()
        .find(|page| page.page_number == page_number)
    {
        if let Some(path) = page
            .translated_pdf_path
            .as_deref()
            .and_then(|relative| safe_relative_cache_path(job_dir, relative))
        {
            candidates.push(path);
        }
    }
    for path in candidates {
        let _ = fs::remove_file(path);
    }
    page_state::upsert_pdf_page(state, page_number, "pending", None, None);
}

fn safe_relative_cache_path(job_dir: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(relative);
    let is_safe = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if is_safe {
        Some(job_dir.join(path))
    } else {
        None
    }
}

#[tauri::command]
pub fn render_rosetta_pdf_translated_page_as_png(
    app: AppHandle,
    job_id: String,
    page_number: u32,
    target_lang: Option<String>,
    target_width: u32,
) -> Result<tauri::ipc::Response, String> {
    if page_number == 0 {
        return Err("页码必须从 1 开始。".to_string());
    }
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let bundle = store::load_job_bundle(&app, &job_id)?;
    let target_lang = resolve_pdf_target_lang(&bundle, target_lang);
    let source_path = store::cached_pdf_source_path(&app, &job_id)?;
    let page_count =
        formats::pdf::count_pages(&app, &source_path).map_err(|error| error.user_message())?;
    let state =
        formats::pdf::page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)?;
    let page_path = translated_page_artifact_path(&dir, &state, &target_lang, page_number);
    let Some(page_path) = page_path else {
        return Err(format!("第 {page_number} 页还没有译文 PDF。"));
    };
    let bytes = formats::pdf::render_page_as_png(&app, &page_path, 0, target_width)
        .map_err(|error| error.user_message())?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn resolve_pdf_target_lang(
    bundle: &model::RosettaJobBundle,
    target_lang: Option<String>,
) -> String {
    target_lang
        .filter(|lang| !lang.trim().is_empty())
        .or_else(|| {
            bundle
                .document
                .files
                .first()
                .and_then(|file| file.target_lang.clone())
        })
        .unwrap_or_else(|| bundle.document.target_lang.clone())
}

fn translated_page_artifact_path(
    job_dir: &Path,
    state: &formats::pdf::page_state::PdfPageTranslationState,
    target_lang: &str,
    page_number: u32,
) -> Option<std::path::PathBuf> {
    use formats::pdf::page_state;

    let page = state.pages.iter().find(|page| {
        page.page_number == page_number
            && page.status == "translated"
            && page.translated_pdf_path.is_some()
    })?;
    let mut candidates = Vec::new();
    if let Some(relative_path) = page.translated_pdf_path.as_ref() {
        candidates.push(job_dir.join(relative_path));
    }
    candidates.push(job_dir.join(page_state::pdf_page_relative_path_for_lang(
        target_lang,
        page_number,
    )));
    candidates.push(job_dir.join(page_state::pdf_page_relative_path(page_number)));
    candidates.push(
        job_dir.join(page_state::legacy_pdf_page_relative_path_for_lang(
            target_lang,
            page_number,
        )),
    );
    candidates.push(job_dir.join(page_state::legacy_pdf_page_relative_path(page_number)));

    candidates.into_iter().find(|path| path.is_file())
}

fn emit_pdf_page_progress_for_run(
    app: &AppHandle,
    job_id: &str,
    target_lang: &str,
    run_id: &str,
    page_number: u32,
    status: &str,
) {
    let _ = app.emit(
        PDF_PAGE_PROGRESS_EVENT,
        PdfPageProgressPayload {
            job_id: job_id.to_string(),
            target_lang: Some(target_lang.to_string()),
            run_id: Some(run_id.to_string()),
            page_number,
            status: status.to_string(),
        },
    );
}

fn pdf_path_for_kind(
    app: &AppHandle,
    job_id: &str,
    kind: &str,
) -> Result<std::path::PathBuf, String> {
    match kind {
        "source" => store::cached_pdf_source_path(app, job_id),
        "translated" => store::translated_pdf_output_path(app, job_id),
        other => Err(format!("未知的 PDF 类型: {other}")),
    }
}

/// Export a complete PDF by substituting any page-level translated PDFs into
/// the original source PDF. Pages without translated artifacts are preserved
/// from the source document.
#[tauri::command]
pub fn export_rosetta_translated_pdf(
    app: AppHandle,
    job_id: String,
    target_lang: Option<String>,
    target_path: String,
) -> Result<model::RosettaExportResult, String> {
    let source_pdf = store::cached_pdf_source_path(&app, &job_id)?;
    let page_count =
        formats::pdf::count_pages(&app, &source_pdf).map_err(|error| error.user_message())?;
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let bundle = store::load_job_bundle(&app, &job_id)?;
    let target_lang = resolve_pdf_target_lang(&bundle, target_lang);
    let state =
        formats::pdf::page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)?;
    let assembled_path = store::translated_pdf_output_path(&app, &job_id)?;
    formats::pdf::page_assemble::assemble_pdf_with_page_translations(
        &source_pdf,
        &dir,
        &state,
        &assembled_path,
    )?;
    let target = std::path::PathBuf::from(&target_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("无法创建导出目录: {error}"))?;
    }
    let bytes_written = std::fs::copy(&assembled_path, &target)
        .map_err(|error| format!("复制翻译后 PDF 失败: {error}"))?;

    // Mirror the txt/md export bookkeeping (timestamp updates) so the UI can
    // show "上次导出于…". The job summary lives in index.json.
    let mut index = store::read_index(&root)?;
    let now = path::timestamp_ms_string();
    let export_job = {
        let job = index
            .jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| "项目索引不存在，无法记录导出状态。".to_string())?;
        job.exported_at = Some(now.clone());
        job.updated_at = now;
        job.clone()
    };
    store::write_index(&root, &index)?;

    Ok(model::RosettaExportResult {
        job: export_job,
        target_path: target.to_string_lossy().to_string(),
        kind: "translation".to_string(),
        bytes_written,
        files_written: 1,
        message: "翻译后 PDF 已导出。".to_string(),
    })
}

#[tauri::command]
pub async fn generate_rosetta_translated_pdf(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    rwkv_base_url: Option<String>,
    provider_id: Option<String>,
    provider_endpoint: Option<String>,
    provider_internal_token: Option<String>,
    provider_body_password: Option<String>,
    source_lang: Option<String>,
    target_lang: Option<String>,
    timeout_ms: Option<u64>,
    ignore_cache: Option<bool>,
) -> Result<String, String> {
    let bundle = store::load_job_bundle(&app, &job_id)?;
    if bundle.document.format != "pdf" {
        return Err("当前文档不是 PDF，无法生成翻译后 PDF。".to_string());
    }
    let source_path = store::cached_pdf_source_path(&app, &job_id)?;
    if !source_path.is_file() {
        return Err("项目缓存里找不到源 PDF，请重新导入。".to_string());
    }

    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let output_path = store::translated_pdf_output_path(&app, &job_id)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("无法创建导出目录: {error}"))?;
    }
    let pdf2zh_output_dir = dir.join("pdf2zh-output");
    if pdf2zh_output_dir.exists() {
        std::fs::remove_dir_all(&pdf2zh_output_dir)
            .map_err(|error| format!("无法清理旧 PDF 译文生成输出: {error}"))?;
    }
    std::fs::create_dir_all(&pdf2zh_output_dir)
        .map_err(|error| format!("无法创建 PDF 译文生成输出目录: {error}"))?;

    let target_lang = target_lang
        .or_else(|| {
            bundle
                .document
                .files
                .first()
                .and_then(|file| file.target_lang.clone())
        })
        .unwrap_or_else(|| bundle.document.target_lang.clone());
    let source_lang = source_lang
        .filter(|lang| lang != "auto")
        .or_else(|| {
            bundle
                .document
                .files
                .first()
                .and_then(|file| file.source_lang.clone())
        })
        .unwrap_or_else(|| "en".to_string());
    let base_url = rwkv_base_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .ok_or_else(|| {
            "PDF 翻译需要配置 API 地址。请先启动本地运行时或配置远程 API。".to_string()
        })?;
    let timeout = timeout_ms.unwrap_or(120_000);
    let ep = provider_endpoint
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let provider = shim_provider_from_parts(
        &base_url,
        provider_id.as_deref(),
        &ep,
        provider_internal_token,
        provider_body_password,
        timeout,
    );

    if !cancel_state.try_begin_run() {
        return Err("已有 PDF 翻译正在进行，请先停止当前翻译。".to_string());
    }
    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    cancel_state.register_sender(cancel_tx);

    let invoke_result = formats::pdf::pdf2zh_invoke::invoke_pdf2zh(
        &app,
        &source_path,
        &pdf2zh_output_dir,
        formats::pdf::pdf2zh_invoke::Pdf2zhInvokeOptions {
            job_id: job_id.clone(),
            provider,
            source_lang,
            target_lang: target_lang.clone(),
            ignore_cache: ignore_cache.unwrap_or(false),
            pages: None,
            // Whole-document fallback: no per-page progress to report.
            page_progress: None,
            translated_chars_offset: 0,
        },
        cancel_rx,
        None,
        None, // no streaming callback — this path is whole-doc only
    )
    .await;

    cancel_state.end_run();

    let output = invoke_result.map_err(|error| error.user_message())?;
    if let Some(message) = pdf_rwkv_failure_message(&output.rwkv_metrics) {
        return Err(message);
    }
    let mono_pdf = output
        .mono_pdf
        .ok_or_else(|| "翻译完成但未生成完整 PDF。".to_string())?;
    std::fs::copy(&mono_pdf, &output_path)
        .map_err(|error| format!("无法缓存 pdf2zh 译文 PDF: {error}"))?;
    mark_pdf_translation_ready(&app, &job_id, &target_lang)?;
    Ok(output_path.to_string_lossy().to_string())
}

fn mark_pdf_translation_ready(
    app: &AppHandle,
    job_id: &str,
    target_lang: &str,
) -> Result<(), String> {
    let root = path::jobs_root(app)?;
    let dir = path::checked_job_dir(&root, job_id)?;
    let mut translation_files = store::read_translation_files(&dir)?;
    let source_file_id = "file-1";
    let id = path::translation_file_id(source_file_id, target_lang);
    if let Some(file) = translation_files.iter_mut().find(|file| file.id == id) {
        file.status = "translated".to_string();
        file.segment_count = 1;
        file.completed_segments = 1;
        file.failed_segments = 0;
        file.updated_at = path::timestamp_ms_string();
    } else {
        translation_files.push(model::RosettaTranslationFile {
            id,
            source_file_id: source_file_id.to_string(),
            target_lang: target_lang.to_string(),
            status: "translated".to_string(),
            segment_count: 1,
            completed_segments: 1,
            failed_segments: 0,
            updated_at: path::timestamp_ms_string(),
            exported_at: None,
        });
    }
    store::write_translation_files(&dir, &translation_files)?;

    let mut index = store::read_index(&root)?;
    if let Some(job) = index.jobs.iter_mut().find(|job| job.id == job_id) {
        job.status = "completed".to_string();
        job.segment_count = 1;
        job.completed_segments = 1;
        job.failed_segments = 0;
        job.target_lang = target_lang.to_string();
        job.last_error = None;
        job.updated_at = path::timestamp_ms_string();
    }
    store::write_index(&root, &index)?;
    Ok(())
}

fn sync_pdf_page_translation_summary(
    app: &AppHandle,
    job_id: &str,
    target_lang: &str,
    state: &formats::pdf::page_state::PdfPageTranslationState,
) -> Result<(), String> {
    let root = path::jobs_root(app)?;
    let dir = path::checked_job_dir(&root, job_id)?;
    let mut translation_files = store::read_translation_files(&dir)?;
    let source_file_id = "file-1";
    let id = path::translation_file_id(source_file_id, target_lang);
    let (segment_count, completed_segments, failed_segments, status) =
        formats::pdf::page_state::pdf_page_status_summary(state);
    let now = path::timestamp_ms_string();

    if let Some(file) = translation_files.iter_mut().find(|file| file.id == id) {
        file.status = status.clone();
        file.segment_count = segment_count;
        file.completed_segments = completed_segments;
        file.failed_segments = failed_segments;
        file.updated_at = now.clone();
    } else {
        translation_files.push(model::RosettaTranslationFile {
            id,
            source_file_id: source_file_id.to_string(),
            target_lang: target_lang.to_string(),
            status: status.clone(),
            segment_count,
            completed_segments,
            failed_segments,
            updated_at: now.clone(),
            exported_at: None,
        });
    }
    store::write_translation_files(&dir, &translation_files)?;

    let mut index = store::read_index(&root)?;
    if let Some(job) = index.jobs.iter_mut().find(|job| job.id == job_id) {
        job.status = if status == "translated" {
            "completed".to_string()
        } else {
            status.clone()
        };
        job.segment_count = segment_count;
        job.completed_segments = completed_segments;
        job.failed_segments = failed_segments;
        job.target_lang = target_lang.to_string();
        job.last_error = None;
        job.updated_at = now;
    }
    store::write_index(&root, &index)?;
    Ok(())
}

#[tauri::command]
pub fn list_rosetta_jobs(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
) -> Result<Vec<RosettaJobSummary>, String> {
    let _ = import::cleanup_pending_job_deletions(&app);
    repair_all_pdf_jobs(&app, cancel_state.session_id())?;
    store::list_rosetta_jobs(app)
}

#[tauri::command]
pub fn load_rosetta_job(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
) -> Result<RosettaJobBundle, String> {
    let _ = repair_pdf_job_inner(&app, &job_id, cancel_state.session_id());
    store::load_job_bundle(&app, &job_id)
}

#[tauri::command]
pub fn save_rosetta_segments(
    app: AppHandle,
    job_id: String,
    segments: Vec<Segment>,
) -> Result<RosettaJobBundle, String> {
    import::save_segments(&app, &job_id, segments)
}

#[tauri::command]
pub fn ensure_rosetta_translation_file(
    app: AppHandle,
    job_id: String,
    source_file_id: String,
    target_lang: String,
) -> Result<RosettaTranslationFileBundle, String> {
    translation_files::ensure_translation_file(&app, &job_id, &source_file_id, &target_lang)
}

#[tauri::command]
pub fn load_rosetta_translation_file(
    app: AppHandle,
    job_id: String,
    translation_file_id: String,
) -> Result<RosettaTranslationFileBundle, String> {
    translation_files::load_translation_file_bundle(&app, &job_id, &translation_file_id)
}

#[tauri::command]
pub fn save_rosetta_translation_segments(
    app: AppHandle,
    job_id: String,
    translation_file_id: String,
    segments: Vec<TranslationSegment>,
) -> Result<RosettaTranslationFileBundle, String> {
    translation_files::save_translation_segments(&app, &job_id, &translation_file_id, segments)
}

#[tauri::command]
pub fn update_rosetta_job_file_languages(
    app: AppHandle,
    job_id: String,
    file_id: String,
    source_lang: Option<String>,
    target_lang: String,
) -> Result<RosettaJobBundle, String> {
    import::update_job_file_languages(&app, &job_id, &file_id, source_lang, target_lang)
}

#[tauri::command]
pub fn create_rosetta_translation_revision(
    app: AppHandle,
    job_id: String,
    file_id: String,
    reason: String,
    scope_block_ids: Option<Vec<String>>,
) -> Result<RosettaJobBundle, String> {
    let reason = TranslationRevisionReason::from_str(&reason)?;
    revisions::create_translation_revision(&app, &job_id, &file_id, reason, scope_block_ids)
}

#[tauri::command]
pub fn rename_rosetta_job(
    app: AppHandle,
    job_id: String,
    name: String,
) -> Result<Vec<RosettaJobSummary>, String> {
    import::rename_job(&app, &job_id, &name)
}

#[tauri::command]
pub fn delete_rosetta_job(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
) -> Result<RosettaJobDeleteResult, String> {
    cancel_state.request_cancel_for_job(&job_id);
    import::delete_job(&app, &job_id)
}

#[tauri::command]
pub fn delete_rosetta_job_file(
    app: AppHandle,
    job_id: String,
    file_id: String,
) -> Result<RosettaJobFileDeleteResult, String> {
    import::delete_job_file(&app, &job_id, &file_id)
}

#[tauri::command]
pub fn export_rosetta_job_file(
    app: AppHandle,
    job_id: String,
    file_id: String,
    kind: String,
    target_path: String,
) -> Result<RosettaExportResult, String> {
    let kind = RosettaExportKind::from_str(&kind)?;
    export::export_job_file(&app, &job_id, &file_id, kind, Path::new(&target_path))
}

#[tauri::command]
pub fn export_rosetta_translation_file(
    app: AppHandle,
    job_id: String,
    translation_file_id: String,
    kind: String,
    target_path: String,
) -> Result<RosettaExportResult, String> {
    let kind = RosettaExportKind::from_str(&kind)?;
    export::export_translation_file(
        &app,
        &job_id,
        &translation_file_id,
        kind,
        Path::new(&target_path),
    )
}
