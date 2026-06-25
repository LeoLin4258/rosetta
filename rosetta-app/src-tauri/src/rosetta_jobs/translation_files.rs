use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{document_files, effective_file_target_lang, ensure_document_files},
    import::normalize_required_lang,
    model::{
        RosettaDocument, RosettaTranslationFile, RosettaTranslationFileBundle, Segment,
        TranslationSegment, TRANSLATIONS_DIRNAME, TRANSLATION_FILES_FILENAME,
    },
    path::{checked_job_dir, is_safe_job_id, jobs_root, timestamp_ms_string, translation_file_id},
    store::{read_json, read_translation_files, write_json, write_translation_files},
};

pub(crate) fn translated_source_segments(
    source_segments: &[Segment],
    translation_segments: &[TranslationSegment],
    source_file_id: &str,
    target_lang: &str,
) -> Vec<Segment> {
    let translation_by_source_id = translation_segments
        .iter()
        .map(|segment| (segment.source_segment_id.as_str(), segment))
        .collect::<HashMap<_, _>>();

    source_segments
        .iter()
        .filter(|segment| segment.file_id.as_deref().unwrap_or("file-1") == source_file_id)
        .map(|segment| {
            let translation = translation_by_source_id.get(segment.id.as_str());
            let mut next = segment.clone();
            next.target_lang = target_lang.to_string();
            if let Some(translation) = translation {
                next.translated_text = translation.translated_text.clone();
                next.status = translation.status.clone();
                next.error = translation.error.clone();
                next.translation_history = translation.translation_history.clone();
            }
            next
        })
        .collect()
}

pub(crate) fn read_or_migrate_translation_files(
    dir: &Path,
    document: &RosettaDocument,
    segments: &[Segment],
) -> Result<Vec<RosettaTranslationFile>, String> {
    let existing = read_translation_files(dir)?;
    if !existing.is_empty() || dir.join(TRANSLATION_FILES_FILENAME).exists() {
        return Ok(existing);
    }

    let mut migrated = Vec::new();
    for source_file in document_files(document) {
        let source_file_segments = segments
            .iter()
            .filter(|segment| segment.file_id.as_deref().unwrap_or("file-1") == source_file.id)
            .collect::<Vec<_>>();
        let has_legacy_translation = source_file_segments.iter().any(|segment| {
            segment
                .translated_text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        });
        if !has_legacy_translation {
            continue;
        }

        let target_lang = effective_file_target_lang(&source_file, document);
        let translation_file = build_translation_file(
            &source_file.id,
            &target_lang,
            source_file_segments
                .iter()
                .map(|segment| legacy_translation_segment(segment, &target_lang))
                .collect(),
        );
        let translation_segments = source_file_segments
            .iter()
            .map(|segment| legacy_translation_segment(segment, &target_lang))
            .collect::<Vec<_>>();
        write_translation_segments(dir, &translation_file.id, &translation_segments)?;
        migrated.push(translation_file);
    }

    write_translation_files(dir, &migrated)?;
    Ok(migrated)
}

pub(crate) fn ensure_translation_file(
    app: &AppHandle,
    job_id: &str,
    source_file_id: &str,
    target_lang: &str,
) -> Result<RosettaTranslationFileBundle, String> {
    let target_lang = normalize_required_lang(target_lang.to_string())?;
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;

    if !document.files.iter().any(|file| file.id == source_file_id) {
        return Err("当前源文件不存在，无法创建译文。".to_string());
    }

    if let Some(translation_file) = translation_files
        .iter()
        .find(|file| file.source_file_id == source_file_id && file.target_lang == target_lang)
        .cloned()
    {
        let segments = read_translation_segments_or_repair_pdf(&dir, &document, &translation_file)?;
        return Ok(RosettaTranslationFileBundle {
            translation_file,
            segments,
        });
    }

    let source_segments = segments
        .iter()
        .filter(|segment| segment.file_id.as_deref().unwrap_or("file-1") == source_file_id)
        .collect::<Vec<_>>();
    let translation_segments = source_segments
        .iter()
        .map(|segment| empty_translation_segment(segment, &target_lang))
        .collect::<Vec<_>>();
    let translation_file =
        build_translation_file(source_file_id, &target_lang, translation_segments.clone());
    write_translation_segments(&dir, &translation_file.id, &translation_segments)?;
    translation_files.push(translation_file.clone());
    write_translation_files(&dir, &translation_files)?;

    Ok(RosettaTranslationFileBundle {
        translation_file,
        segments: translation_segments,
    })
}

pub(crate) fn load_translation_file_bundle(
    app: &AppHandle,
    job_id: &str,
    translation_file_id: &str,
) -> Result<RosettaTranslationFileBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let translation_file = read_translation_files(&dir)?
        .into_iter()
        .find(|file| file.id == translation_file_id)
        .ok_or_else(|| "译文文件不存在。".to_string())?;
    let segments = read_translation_segments_or_repair_pdf(&dir, &document, &translation_file)?;
    Ok(RosettaTranslationFileBundle {
        translation_file,
        segments,
    })
}

pub(crate) fn save_translation_segments(
    app: &AppHandle,
    job_id: &str,
    translation_file_id: &str,
    segments: Vec<TranslationSegment>,
) -> Result<RosettaTranslationFileBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut translation_files = read_translation_files(&dir)?;
    let Some(index) = translation_files
        .iter()
        .position(|file| file.id == translation_file_id)
    else {
        return Err("译文文件不存在，无法保存。".to_string());
    };

    write_translation_segments(&dir, translation_file_id, &segments)?;
    let source_file_id = translation_files[index].source_file_id.clone();
    let target_lang = translation_files[index].target_lang.clone();
    translation_files[index] =
        build_translation_file(&source_file_id, &target_lang, segments.clone());
    write_translation_files(&dir, &translation_files)?;

    Ok(RosettaTranslationFileBundle {
        translation_file: translation_files[index].clone(),
        segments,
    })
}

pub(crate) fn read_translation_segments(
    dir: &Path,
    translation_file_id: &str,
) -> Result<Vec<TranslationSegment>, String> {
    read_json(&translation_segments_path(dir, translation_file_id)?)
}

pub(crate) fn read_translation_segments_or_repair_pdf(
    dir: &Path,
    document: &RosettaDocument,
    translation_file: &RosettaTranslationFile,
) -> Result<Vec<TranslationSegment>, String> {
    let path = translation_segments_path(dir, &translation_file.id)?;
    if path.exists() {
        return read_json(&path);
    }

    if !is_pdf_source_file(document, &translation_file.source_file_id) {
        return read_json(&path);
    }

    let segments = Vec::<TranslationSegment>::new();
    write_translation_segments(dir, &translation_file.id, &segments)?;
    eprintln!(
        "[rosetta-jobs] repaired missing PDF translation segment file: {}",
        path.display()
    );
    Ok(segments)
}

pub(crate) fn write_translation_segments(
    dir: &Path,
    translation_file_id: &str,
    segments: &[TranslationSegment],
) -> Result<(), String> {
    let translations_dir = dir.join(TRANSLATIONS_DIRNAME);
    fs::create_dir_all(&translations_dir).map_err(|error| format!("无法创建译文目录: {error}"))?;
    write_json(
        &translation_segments_path(dir, translation_file_id)?,
        segments,
    )
}

pub(crate) fn translation_segments_path(
    dir: &Path,
    translation_file_id: &str,
) -> Result<PathBuf, String> {
    if !is_safe_job_id(translation_file_id) {
        return Err("译文文件 id 不安全。".to_string());
    }
    let path = dir
        .join(TRANSLATIONS_DIRNAME)
        .join(format!("{translation_file_id}.json"));
    if !path.starts_with(dir.join(TRANSLATIONS_DIRNAME)) {
        return Err("译文文件路径越界。".to_string());
    }
    Ok(path)
}

fn is_pdf_source_file(document: &RosettaDocument, source_file_id: &str) -> bool {
    document
        .files
        .iter()
        .any(|file| file.id == source_file_id && file.format.eq_ignore_ascii_case("pdf"))
}

pub(crate) fn build_translation_file(
    source_file_id: &str,
    target_lang: &str,
    segments: Vec<TranslationSegment>,
) -> RosettaTranslationFile {
    let segment_count = segments
        .iter()
        .filter(|segment| segment.status != "skipped")
        .count();
    let completed_segments = segments
        .iter()
        .filter(|segment| matches!(segment.status.as_str(), "done" | "edited"))
        .count();
    let failed_segments = segments
        .iter()
        .filter(|segment| segment.status == "failed")
        .count();
    let status = if segments
        .iter()
        .any(|segment| segment.status == "translating")
    {
        "translating"
    } else if failed_segments > 0 {
        "failed"
    } else if segment_count > 0 && completed_segments == segment_count {
        "translated"
    } else {
        "untranslated"
    };

    RosettaTranslationFile {
        id: translation_file_id(source_file_id, target_lang),
        source_file_id: source_file_id.to_string(),
        target_lang: target_lang.to_string(),
        status: status.to_string(),
        segment_count,
        completed_segments,
        failed_segments,
        updated_at: timestamp_ms_string(),
        exported_at: None,
    }
}

pub(crate) fn empty_translation_segment(
    segment: &Segment,
    target_lang: &str,
) -> TranslationSegment {
    TranslationSegment {
        source_segment_id: segment.id.clone(),
        translated_text: None,
        target_lang: target_lang.to_string(),
        status: if segment.status == "skipped" || segment.source_text.trim().is_empty() {
            "skipped".to_string()
        } else {
            "pending".to_string()
        },
        error: None,
        translation_history: Vec::new(),
    }
}

pub(crate) fn legacy_translation_segment(
    segment: &Segment,
    target_lang: &str,
) -> TranslationSegment {
    TranslationSegment {
        source_segment_id: segment.id.clone(),
        translated_text: segment.translated_text.clone(),
        target_lang: target_lang.to_string(),
        status: segment.status.clone(),
        error: segment.error.clone(),
        translation_history: segment.translation_history.clone(),
    }
}
