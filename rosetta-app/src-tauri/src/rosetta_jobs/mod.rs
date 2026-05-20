use std::{fs, path::Path, str::FromStr, sync::Mutex};

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

use model::{
    RosettaExportKind, RosettaExportResult, RosettaJobBundle, RosettaJobFileDeleteResult,
    RosettaJobSummary, RosettaTranslationFileBundle, Segment, TranslationRevisionReason,
    TranslationSegment,
};

#[derive(Default)]
pub struct PdfTranslationCancelState(pub Mutex<Option<oneshot::Sender<()>>>);

#[tauri::command]
pub fn cancel_rosetta_translated_pdf(
    cancel_state: State<'_, PdfTranslationCancelState>,
) {
    if let Ok(mut guard) = cancel_state.0.lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
    }
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
pub fn get_rosetta_pdf_assets(
    app: AppHandle,
    job_id: String,
) -> Result<RosettaPdfAssets, String> {
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
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("读取 PDF 失败: {error}"))?;
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
    let page_count = formats::pdf::count_pages(&app, &source_path)
        .map_err(|error| error.user_message())?;
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
    source_lang: Option<String>,
    timeout_ms: Option<u64>,
    force: Option<bool>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String> {
    let bundle = store::load_job_bundle(&app, &job_id)?;
    if bundle.document.format != "pdf" {
        return Err("当前文档不是 PDF，无法按页翻译。".to_string());
    }

    let source_path = store::cached_pdf_source_path(&app, &job_id)?;
    let page_count = formats::pdf::count_pages(&app, &source_path)
        .map_err(|error| error.user_message())?;
    let pages = formats::pdf::page_state::parse_pdf_page_selection(&page_selection, page_count)?;
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let pages_dir = dir.join("pdf-pages");
    fs::create_dir_all(&pages_dir)
        .map_err(|error| format!("无法创建 PDF 页缓存目录: {error}"))?;
    let mut state =
        formats::pdf::page_state::read_pdf_page_translation_state(&dir, page_count, &target_lang)?;
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
        return Err("PDF 翻译需要一个本地 RWKV base URL。请先启动本地运行时。".to_string());
    }
    let force = force.unwrap_or(false);

    for page_number in pages {
        let already_translated = state.pages.iter().any(|page| {
            page.page_number == page_number
                && page.status == "translated"
                && page.translated_pdf_path.is_some()
        });
        if already_translated && !force {
            continue;
        }

        formats::pdf::page_state::upsert_pdf_page(
            &mut state,
            page_number,
            "translating",
            None,
            None,
        );
        formats::pdf::page_state::write_pdf_page_translation_state(&dir, &state)?;
        emit_pdf_page_progress(&app, &job_id, page_number, "translating");

        let output_dir = dir
            .join("pdf2zh-output")
            .join(formats::pdf::page_state::pdf_page_filename(page_number));
        if output_dir.exists() {
            fs::remove_dir_all(&output_dir)
                .map_err(|error| format!("无法清理旧 PDF 页输出: {error}"))?;
        }
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("无法创建 PDF 页输出目录: {error}"))?;

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        {
            let mut guard = cancel_state
                .0
                .lock()
                .map_err(|_| "取消状态锁定失败。".to_string())?;
            *guard = Some(cancel_tx);
        }

        let invoke_result = formats::pdf::pdf2zh_invoke::invoke_pdf2zh(
            &app,
            &source_path,
            &output_dir,
            formats::pdf::pdf2zh_invoke::Pdf2zhInvokeOptions {
                job_id: job_id.clone(),
                rwkv_base_url: rwkv_base_url.clone(),
                source_lang: source_lang.clone(),
                target_lang: target_lang.clone(),
                timeout_ms: timeout_ms.unwrap_or(120_000),
                ignore_cache: false,
                pages: Some(vec![page_number]),
            },
            cancel_rx,
        )
        .await;

        {
            let mut guard = cancel_state
                .0
                .lock()
                .map_err(|_| "取消状态锁定失败。".to_string())?;
            *guard = None;
        }

        match invoke_result {
            Ok(output) => {
                let relative_path = formats::pdf::page_state::pdf_page_relative_path(page_number);
                let target = dir.join(&relative_path);
                fs::copy(&output.mono_pdf, &target)
                    .map_err(|error| format!("无法缓存第 {page_number} 页译文 PDF: {error}"))?;
                formats::pdf::page_state::upsert_pdf_page(
                    &mut state,
                    page_number,
                    "translated",
                    Some(relative_path),
                    None,
                );
                formats::pdf::page_state::write_pdf_page_translation_state(&dir, &state)?;
                emit_pdf_page_progress(&app, &job_id, page_number, "translated");
            }
            Err(formats::pdf::errors::PdfError::Cancelled) => {
                formats::pdf::page_state::upsert_pdf_page(
                    &mut state,
                    page_number,
                    "pending",
                    None,
                    None,
                );
                formats::pdf::page_state::write_pdf_page_translation_state(&dir, &state)?;
                emit_pdf_page_progress(&app, &job_id, page_number, "pending");
                return Err("已取消 PDF 翻译。".to_string());
            }
            Err(error) => {
                formats::pdf::page_state::upsert_pdf_page(
                    &mut state,
                    page_number,
                    "failed",
                    None,
                    Some(error.user_message()),
                );
                formats::pdf::page_state::write_pdf_page_translation_state(&dir, &state)?;
                emit_pdf_page_progress(&app, &job_id, page_number, "failed");
            }
        }
    }

    Ok(state)
}

#[tauri::command]
pub fn render_rosetta_pdf_translated_page_as_png(
    app: AppHandle,
    job_id: String,
    page_number: u32,
    target_width: u32,
) -> Result<tauri::ipc::Response, String> {
    if page_number == 0 {
        return Err("页码必须从 1 开始。".to_string());
    }
    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let page_path = dir.join(formats::pdf::page_state::pdf_page_relative_path(page_number));
    if !page_path.is_file() {
        return Err(format!("第 {page_number} 页还没有译文 PDF。"));
    }
    let bytes = formats::pdf::render_page_as_png(&app, &page_path, 0, target_width)
        .map_err(|error| error.user_message())?;
    Ok(tauri::ipc::Response::new(bytes))
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

/// Export the already-generated translated PDF for a job by copying it from
/// `<job_dir>/exports/translated.pdf` to the user-chosen `target_path`.
/// Re-rendering is unnecessary — the bytes on disk are exactly what we'd
/// produce again, and a re-render of a 100-page doc would block the UI for
/// ~10s when a plain copy takes milliseconds.
#[tauri::command]
pub fn export_rosetta_translated_pdf(
    app: AppHandle,
    job_id: String,
    target_path: String,
) -> Result<model::RosettaExportResult, String> {
    let source_path = store::translated_pdf_output_path(&app, &job_id)?;
    if !source_path.is_file() {
        return Err("尚未生成翻译后 PDF，请先翻译完成后再导出。".to_string());
    }
    let target = std::path::PathBuf::from(&target_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建导出目录: {error}"))?;
    }
    let bytes_written = std::fs::copy(&source_path, &target)
        .map_err(|error| format!("复制翻译后 PDF 失败: {error}"))?;

    // Mirror the txt/md export bookkeeping (timestamp updates) so the UI can
    // show "上次导出于…". The job summary lives in index.json.
    let root = path::jobs_root(&app)?;
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
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建导出目录: {error}"))?;
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
    let rwkv_base_url = rwkv_base_url
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "PDF 翻译需要一个本地 RWKV base URL。请先启动本地运行时。".to_string())?;

    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    {
        let mut guard = cancel_state.0.lock().map_err(|_| "取消状态锁定失败。".to_string())?;
        *guard = Some(cancel_tx);
    }

    let invoke_result = formats::pdf::pdf2zh_invoke::invoke_pdf2zh(
        &app,
        &source_path,
        &pdf2zh_output_dir,
        formats::pdf::pdf2zh_invoke::Pdf2zhInvokeOptions {
            job_id: job_id.clone(),
            rwkv_base_url,
            source_lang,
            target_lang: target_lang.clone(),
            timeout_ms: timeout_ms.unwrap_or(120_000),
            ignore_cache: ignore_cache.unwrap_or(false),
            pages: None,
        },
        cancel_rx,
    )
    .await;

    {
        let mut guard = cancel_state.0.lock().map_err(|_| "取消状态锁定失败。".to_string())?;
        *guard = None;
    }

    let output = invoke_result.map_err(|error| error.user_message())?;
    std::fs::copy(&output.mono_pdf, &output_path)
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
