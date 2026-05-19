use std::path::Path;
use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

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
    let extensions = if format == "markdown" {
        vec!["md"]
    } else {
        vec!["txt"]
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
    // Async because the PDF backend calls into the Docling sidecar over HTTP.
    // Keeping this sync + block_on would tie up a Tokio worker for the whole
    // import (~10s on a 1-page doc), starving the webview's IPC handler and
    // freezing the UI. Tauri 2 awaits async commands natively without extra
    // glue, so propagating async upward is the cheapest correct fix.
    import::import_document_from_path(&app, Path::new(&path)).await
}

/// Smoke test: confirm the bundled pdfium dylib and CJK font can be located
/// and that pdfium binds successfully. Phase 2 frontend uses this to surface
/// "PDF support unavailable" early before the user tries to import a PDF.
#[tauri::command]
pub fn probe_pdf_runtime(app: AppHandle) -> formats::pdf::PdfRuntimeStatus {
    formats::pdf::probe_status(&app)
}

/// Generate a translated PDF for a job by reading the cached source PDF and
/// each block's translated text. Returns the absolute path of the rendered
/// PDF so the frontend can show it side-by-side with the source.
///
/// Translation data lives in `<job_dir>/translations/<tr-file-id>.json`, NOT
/// in `document.json` / `segments.json` — the latter only hold the source-side
/// state. So before handing the document to the renderer we merge the
/// translation segments back onto the source segments and apply them to the
/// document's blocks (same flow `export.rs` uses for .md/.txt exports).
#[tauri::command]
pub fn generate_rosetta_translated_pdf(app: AppHandle, job_id: String) -> Result<String, String> {
    let bundle = store::load_job_bundle(&app, &job_id)?;
    if bundle.document.format != "pdf" {
        return Err("当前文档不是 PDF，无法生成翻译后 PDF。".to_string());
    }
    let source_path = store::cached_pdf_source_path(&app, &job_id)?;
    if !source_path.is_file() {
        return Err("项目缓存里找不到源 PDF，请重新导入。".to_string());
    }

    // Pick the translation file for the (single) source file. PDF v1 always
    // has exactly one source file per job — see import.rs.
    let translation_file = bundle
        .translation_files
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| "尚未生成任何译文，请先完成翻译。".to_string())?;
    let source_file_id = translation_file.source_file_id.clone();

    let root = path::jobs_root(&app)?;
    let dir = path::checked_job_dir(&root, &job_id)?;
    let translation_segments =
        translation_files::read_translation_segments(&dir, &translation_file.id)?;
    let merged_segments = translation_files::translated_source_segments(
        &bundle.segments,
        &translation_segments,
        &source_file_id,
        &translation_file.target_lang,
    );

    let mut document = bundle.document.clone();
    document::apply_segment_translations_to_document(&mut document, &merged_segments);

    let output_path = store::translated_pdf_output_path(&app, &job_id)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建导出目录: {error}"))?;
    }
    formats::pdf::render_translated_pdf(&app, &document, &source_path, &output_path)
        .map_err(|error| error.user_message())?;
    Ok(output_path.to_string_lossy().to_string())
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
