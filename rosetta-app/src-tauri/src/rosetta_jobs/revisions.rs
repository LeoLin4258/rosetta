use std::collections::HashMap;

use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{
        document_files, effective_file_source_lang, effective_file_target_lang,
        ensure_document_files, sync_document_file_statuses, sync_job_counts, sync_job_source_files,
    },
    model::{
        RosettaDocument, RosettaJobBundle, Segment, TranslationHistoryEntry, TranslationRevision,
        TranslationRevisionReason, SCHEMA_VERSION,
    },
    path::{checked_job_dir, jobs_root, timestamp_ms_string},
    store::{
        read_index, read_json, read_translation_revisions, replace_index_job, write_index,
        write_translation_revisions,
    },
    translation_files::read_or_migrate_translation_files,
};

pub(crate) fn create_translation_revision(
    app: &AppHandle,
    job_id: &str,
    file_id: &str,
    reason: TranslationRevisionReason,
    scope_block_ids: Option<Vec<String>>,
) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目索引不存在，无法保存历史版本。".to_string())?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    let mut translation_revisions = read_translation_revisions(&dir)?;
    let translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;

    if let Some(revision) = create_revision_snapshot(
        job_id,
        file_id,
        reason.as_str(),
        scope_block_ids,
        &document,
        &segments,
    )? {
        translation_revisions.push(revision);
        write_translation_revisions(&dir, &translation_revisions)?;
        replace_index_job(&mut index, job.clone());
        write_index(&root, &index)?;
    }

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files,
        translation_revisions,
    })
}

pub(crate) fn archive_segment_translation(segment: &mut Segment, reason: &str, run_id: &str) {
    let Some(translated_text) = segment
        .translated_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return;
    };
    let created_at = timestamp_ms_string();
    segment.translation_history.push(TranslationHistoryEntry {
        id: format!("{}-history-{created_at}", segment.id),
        run_id: Some(run_id.to_string()),
        translated_text: translated_text.to_string(),
        created_at,
        source_lang: segment.source_lang.clone(),
        target_lang: segment.target_lang.clone(),
        reason: reason.to_string(),
    });
}

pub(crate) fn create_revision_snapshot(
    job_id: &str,
    file_id: &str,
    reason: &str,
    scope_block_ids: Option<Vec<String>>,
    document: &RosettaDocument,
    segments: &[Segment],
) -> Result<Option<TranslationRevision>, String> {
    let source_file = document_files(document)
        .into_iter()
        .find(|source_file| source_file.id == file_id)
        .ok_or_else(|| "当前文件不存在，无法保存历史版本。".to_string())?;

    let segment_translations = segments
        .iter()
        .filter(|segment| {
            segment.file_id.as_deref().unwrap_or("file-1") == file_id
                && segment.status != "skipped"
                && !segment.source_text.trim().is_empty()
        })
        .filter_map(|segment| {
            segment
                .translated_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| (segment.id.clone(), text.to_string()))
        })
        .collect::<HashMap<_, _>>();

    if segment_translations.is_empty() {
        return Ok(None);
    }

    let created_at = timestamp_ms_string();
    Ok(Some(TranslationRevision {
        id: format!("{file_id}-{reason}-{created_at}"),
        job_id: job_id.to_string(),
        file_id: file_id.to_string(),
        created_at,
        source_lang: effective_file_source_lang(&source_file, document),
        target_lang: effective_file_target_lang(&source_file, document),
        reason: reason.to_string(),
        scope_block_ids,
        segment_translations,
    }))
}
