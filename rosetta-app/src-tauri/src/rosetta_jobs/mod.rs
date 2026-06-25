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
    RosettaExportKind, RosettaExportResult, RosettaJobBundle, RosettaJobFileDeleteResult,
    RosettaJobSummary, RosettaTranslationFileBundle, Segment, TranslationRevisionReason,
    TranslationSegment,
};

/// Cancellation for the (single) active PDF translation run.
///
/// `cancelled` is level-triggered: it stays set until the run loop observes it,
/// so a stop request that lands in the gap between two page invocations (when
/// no oneshot sender is registered) is not lost. The oneshot sender is the
/// edge-trigger that interrupts the currently running pdf2zh process.
#[derive(Default)]
pub struct PdfTranslationCancelState {
    cancelled: AtomicBool,
    run_active: AtomicBool,
    sender: Mutex<Option<oneshot::Sender<()>>>,
}

impl PdfTranslationCancelState {
    /// Marks a run as started. Returns `false` if another run is active.
    fn try_begin_run(&self) -> bool {
        if self.run_active.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.cancelled.store(false, Ordering::SeqCst);
        true
    }

    fn end_run(&self) {
        self.run_active.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.sender.lock() {
            *guard = None;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn register_sender(&self, tx: oneshot::Sender<()>) {
        // If cancel already arrived, fire immediately instead of parking the
        // sender — the upcoming invocation should abort at once.
        if self.is_cancelled() {
            let _ = tx.send(());
            return;
        }
        if let Ok(mut guard) = self.sender.lock() {
            *guard = Some(tx);
        }
    }

    fn clear_sender(&self) {
        if let Ok(mut guard) = self.sender.lock() {
            *guard = None;
        }
    }

    pub fn request_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.sender.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
    }
}

pub use formats::pdf::runtime::PngCache as PdfPngCache;

#[tauri::command]
pub fn cancel_rosetta_translated_pdf(cancel_state: State<'_, PdfTranslationCancelState>) {
    cancel_state.request_cancel();
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
    page_number: u32,
    status: String,
}

#[tauri::command]
pub fn get_rosetta_pdf_page_status(
    app: AppHandle,
    job_id: String,
    target_lang: Option<String>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String> {
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

    formats::pdf::page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)
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
    if !cancel_state.try_begin_run() {
        return Err("已有 PDF 翻译正在进行，请先停止当前翻译。".to_string());
    }
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
    use formats::pdf::{diagnostics, page_state, pdf2zh_invoke};

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
        .join("pdf-pages")
        .join(page_state::pdf_page_language_dir(target_lang));
    fs::create_dir_all(&pages_dir).map_err(|error| format!("无法创建 PDF 页缓存目录: {error}"))?;
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

    // Pages that will actually be invoked (skip already-translated unless
    // `force`). The 1-based position in this filtered list drives the UI's
    // "第 X/Y 页" display: "3rd of 5 pages I asked for", not absolute numbers.
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
    if pages_to_process.is_empty() {
        sync_pdf_page_translation_summary(app, job_id, target_lang, &state)?;
        return Ok(state);
    }

    let run_started = std::time::Instant::now();
    let run_id = format!("run-pdf-{}", path::timestamp_ms_string());
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
        diagnostics::write_profile(&dir, profile);
    };

    let mut cancelled = false;

    if force {
        for page_number in &pages_to_process {
            clear_pdf_page_artifacts(&dir, &mut state, target_lang, *page_number);
        }
    }

    // Mark every targeted page as "translating" up front so the sidebar and
    // page grid flip to the spinner the moment the user clicks translate,
    // not when the first worker event lands.
    for page_number in &pages_to_process {
        page_state::upsert_pdf_page(&mut state, *page_number, "translating", None, None);
        emit_pdf_page_progress(app, job_id, *page_number, "translating");
    }
    page_state::write_pdf_page_translation_state(&dir, &state)?;

    // One invocation, all selected pages. The worker streams a `page` event
    // and a single-page PDF as each page finishes translating, so the UI
    // sees pages appear one by one without paying the per-invocation
    // overhead (model load + pymupdf preprocess + dual-PDF generation) more
    // than once. The previous "pre-split into single-page PDFs and invoke N
    // times" approach paid that overhead N times and ran ~5× slower.
    let invocation_output_dir = dir.join("pdf2zh-output");
    if invocation_output_dir.exists() {
        fs::remove_dir_all(&invocation_output_dir)
            .map_err(|error| format!("无法清理旧 PDF 输出: {error}"))?;
    }
    fs::create_dir_all(&invocation_output_dir)
        .map_err(|error| format!("无法创建 PDF 输出目录: {error}"))?;

    pdf2zh_invoke::emit_progress_phase(
        app,
        job_id,
        "warmup",
        Some(0),
        "正在准备翻译引擎…",
        total_pages_to_process,
    );

    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    cancel_state.register_sender(cancel_tx);

    // The streaming callback owns the per-page artifact pipeline: copy the
    // worker's single-page output into the page cache, flip status to
    // translated, persist state, and fire the UI event. Anything that
    // mutates `state` flows through here, which is why the failure branches
    // in invoke_result below don't have to second-guess per-page status —
    // pages we never heard about stay in "translating" until the cancel /
    // error sweep at the end runs.
    let dir_for_cb = dir.clone();
    let app_for_cb = app.clone();
    let job_id_for_cb = job_id.to_string();
    let target_lang_for_cb = target_lang.to_string();
    let state_for_cb: &mut formats::pdf::page_state::PdfPageTranslationState = &mut state;
    let mut on_page_done = move |page_number: u32, worker_file: std::path::PathBuf| {
        let assembly_started = std::time::Instant::now();
        let relative_path =
            page_state::pdf_page_relative_path_for_lang(&target_lang_for_cb, page_number);
        let target_path = dir_for_cb.join(&relative_path);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let outcome = fs::copy(&worker_file, &target_path)
            .map_err(|error| format!("无法缓存译文页: {error}"));
        let _ = assembly_started.elapsed();

        match outcome {
            Ok(_) => {
                page_state::upsert_pdf_page(
                    state_for_cb,
                    page_number,
                    "translated",
                    Some(relative_path),
                    None,
                );
                let _ = page_state::write_pdf_page_translation_state(&dir_for_cb, state_for_cb);
                let _ = app_for_cb.emit(
                    PDF_PAGE_PROGRESS_EVENT,
                    PdfPageProgressPayload {
                        job_id: job_id_for_cb.clone(),
                        page_number,
                        status: "translated".to_string(),
                    },
                );
            }
            Err(error) => {
                page_state::upsert_pdf_page(state_for_cb, page_number, "failed", None, Some(error));
                let _ = page_state::write_pdf_page_translation_state(&dir_for_cb, state_for_cb);
                let _ = app_for_cb.emit(
                    PDF_PAGE_PROGRESS_EVENT,
                    PdfPageProgressPayload {
                        job_id: job_id_for_cb.clone(),
                        page_number,
                        status: "failed".to_string(),
                    },
                );
            }
        }
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
            pages: Some(pages_to_process.clone()),
            page_progress: Some(pdf2zh_invoke::PageProgressContext {
                completed_before: 0,
                chunk_len: total_pages_to_process,
                total: total_pages_to_process,
            }),
            translated_chars_offset: 0,
        },
        cancel_rx,
        Some(&mut on_page_done),
    )
    .await;

    cancel_state.clear_sender();
    drop(on_page_done);
    profile.invocation_count = 1;

    let mut failure_message: Option<String> = None;
    match invoke_result {
        Ok(output) => {
            profile.durations_ms.pdf2zh_warmup = output.warmup_ms;
            profile.durations_ms.pdf2zh_process = output.process_ms;
            rwkv_aggregate.add(&output.rwkv_metrics);
            if force && total_pages_to_process > 0 && output.rwkv_metrics.request_count == 0 {
                let message = "PDF 重翻没有向翻译模型发送任何文本，已拒绝复用旧译文。请确认该页包含可提取文本后再试。"
                    .to_string();
                for page_number in &pages_to_process {
                    clear_pdf_page_artifacts(&dir, &mut state, target_lang, *page_number);
                    page_state::upsert_pdf_page(
                        &mut state,
                        *page_number,
                        "failed",
                        None,
                        Some(message.clone()),
                    );
                    emit_pdf_page_progress(app, job_id, *page_number, "failed");
                }
                let _ = page_state::write_pdf_page_translation_state(&dir, &state);
                failure_message = Some(message);
            }
        }
        Err(formats::pdf::errors::PdfError::Cancelled) => {
            cancelled = true;
        }
        Err(error) => {
            failure_message = Some(error.user_message());
        }
    }

    // Any page still flagged "translating" after the invocation finishes
    // never produced a worker event — pending on cancel, failed on error.
    let resolved_status = if cancelled || failure_message.is_some() {
        if failure_message.is_some() {
            "failed"
        } else {
            "pending"
        }
    } else {
        // Successful completion with no event for a page means the worker
        // silently skipped it; mark failed so the UI surfaces it.
        "failed"
    };
    let mut state_dirty = false;
    for page in state.pages.iter_mut() {
        if page.status == "translating" {
            page.status = resolved_status.to_string();
            if resolved_status == "failed" {
                page.error = Some(
                    failure_message
                        .clone()
                        .unwrap_or_else(|| "翻译未完成".to_string()),
                );
            }
            emit_pdf_page_progress(app, job_id, page.page_number, resolved_status);
            state_dirty = true;
        }
    }
    if state_dirty {
        page_state::write_pdf_page_translation_state(&dir, &state)?;
    }

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
        finish_profile(&mut profile, "failed", &rwkv_aggregate);
        return Err(message);
    }
    if cancelled {
        finish_profile(&mut profile, "cancelled", &rwkv_aggregate);
        return Err("已取消 PDF 翻译。".to_string());
    }
    finish_profile(&mut profile, "completed", &rwkv_aggregate);
    Ok(state)
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

    candidates.into_iter().find(|path| path.is_file())
}

fn emit_pdf_page_progress(app: &AppHandle, job_id: &str, page_number: u32, status: &str) {
    let _ = app.emit(
        PDF_PAGE_PROGRESS_EVENT,
        PdfPageProgressPayload {
            job_id: job_id.to_string(),
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
        None, // no streaming callback — this path is whole-doc only
    )
    .await;

    cancel_state.end_run();

    let output = invoke_result.map_err(|error| error.user_message())?;
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
pub fn list_rosetta_jobs(app: AppHandle) -> Result<Vec<RosettaJobSummary>, String> {
    store::list_rosetta_jobs(app)
}

#[tauri::command]
pub fn load_rosetta_job(app: AppHandle, job_id: String) -> Result<RosettaJobBundle, String> {
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
    job_id: String,
) -> Result<Vec<RosettaJobSummary>, String> {
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
