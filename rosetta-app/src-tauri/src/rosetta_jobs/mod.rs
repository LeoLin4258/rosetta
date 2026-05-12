use std::path::Path;
use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

mod document;
mod export;
mod formats;
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
        .set_title("选择 TXT 或 Markdown 文件")
        .add_filter("TXT / Markdown", &["txt", "md", "markdown"])
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
pub fn import_rosetta_document_from_path(
    app: AppHandle,
    path: String,
) -> Result<RosettaJobBundle, String> {
    import::import_document_from_path(&app, Path::new(&path))
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
