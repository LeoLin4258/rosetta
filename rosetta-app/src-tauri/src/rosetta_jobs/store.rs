use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{
        ensure_document_files, sync_document_file_statuses, sync_job_counts, sync_job_source_files,
    },
    model::{
        RosettaDocument, RosettaJobBundle, RosettaJobIndex, RosettaJobSummary,
        RosettaTranslationFile, Segment, SourceSnapshot, TranslationRevision, JOB_INDEX_FILENAME,
        SCHEMA_VERSION, TRANSLATION_FILES_FILENAME, TRANSLATION_REVISIONS_FILENAME,
    },
    path::{checked_job_dir, jobs_root, path_from_relative},
    translation_files::read_or_migrate_translation_files,
};

pub fn list_rosetta_jobs(app: AppHandle) -> Result<Vec<RosettaJobSummary>, String> {
    let root = jobs_root(&app)?;
    let mut index = read_index(&root)?;
    for job in &mut index.jobs {
        let dir = checked_job_dir(&root, &job.id)?;
        if !dir.exists() {
            continue;
        }
        let Ok(mut document) = read_json::<RosettaDocument>(&dir.join("document.json")) else {
            continue;
        };
        let Ok(segments) = read_json::<Vec<Segment>>(&dir.join("segments.json")) else {
            continue;
        };
        ensure_document_files(&mut document);
        sync_document_file_statuses(&mut document, &segments);
        sync_job_counts(job, &segments);
        sync_job_source_files(job, &document);
    }
    write_index(&root, &index)?;
    Ok(index.jobs)
}

pub(crate) fn write_job_bundle(
    app: &AppHandle,
    bundle: &RosettaJobBundle,
    source_contents: &str,
) -> Result<(), String> {
    let source_filename = if bundle.document.format == "markdown" {
        "source.md"
    } else {
        "source.txt"
    };
    write_job_bundle_sources(
        app,
        bundle,
        &[SourceSnapshot {
            relative_path: source_filename.to_string(),
            contents: source_contents.to_string(),
        }],
    )
}

pub(crate) fn write_job_bundle_sources(
    app: &AppHandle,
    bundle: &RosettaJobBundle,
    sources: &[SourceSnapshot],
) -> Result<(), String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, &bundle.job.id)?;
    fs::create_dir_all(dir.join("exports"))
        .map_err(|error| format!("无法创建项目目录: {error}"))?;

    for source in sources {
        let relative_path = path_from_relative(&source.relative_path)?;
        let source_path = if sources.len() == 1 {
            dir.join(relative_path)
        } else {
            dir.join("sources").join(relative_path)
        };
        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建源文件缓存目录: {error}"))?;
        }
        fs::write(&source_path, &source.contents)
            .map_err(|error| format!("无法写入源文件缓存: {error}"))?;
    }
    write_json(&dir.join("document.json"), &bundle.document)?;
    write_json(&dir.join("segments.json"), &bundle.segments)?;
    write_json(
        &dir.join(TRANSLATION_REVISIONS_FILENAME),
        &bundle.translation_revisions,
    )?;
    write_translation_files(&dir, &bundle.translation_files)?;
    upsert_index_job(&root, bundle.job.clone())
}

pub(crate) fn load_job_bundle(app: &AppHandle, job_id: &str) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let index = read_index(&root)?;
    let mut job = index
        .jobs
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| "项目不存在。".to_string())?;
    let mut document = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    let translation_revisions = read_translation_revisions(&dir)?;
    let translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files,
        translation_revisions,
    })
}

pub(crate) fn read_index(root: &Path) -> Result<RosettaJobIndex, String> {
    let path = root.join(JOB_INDEX_FILENAME);
    if !path.exists() {
        return Ok(RosettaJobIndex {
            schema_version: SCHEMA_VERSION,
            jobs: Vec::new(),
        });
    }
    read_json(&path)
}

pub(crate) fn write_index(root: &Path, index: &RosettaJobIndex) -> Result<(), String> {
    write_json(&root.join(JOB_INDEX_FILENAME), index)
}

pub(crate) fn read_translation_revisions(dir: &Path) -> Result<Vec<TranslationRevision>, String> {
    let path = dir.join(TRANSLATION_REVISIONS_FILENAME);
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(&path)
}

pub(crate) fn write_translation_revisions(
    dir: &Path,
    revisions: &[TranslationRevision],
) -> Result<(), String> {
    write_json(&dir.join(TRANSLATION_REVISIONS_FILENAME), &revisions)
}

pub(crate) fn read_translation_files(dir: &Path) -> Result<Vec<RosettaTranslationFile>, String> {
    let path = dir.join(TRANSLATION_FILES_FILENAME);
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(&path)
}

pub(crate) fn write_translation_files(
    dir: &Path,
    translation_files: &[RosettaTranslationFile],
) -> Result<(), String> {
    write_json(&dir.join(TRANSLATION_FILES_FILENAME), &translation_files)
}

pub(crate) fn upsert_index_job(root: &Path, job: RosettaJobSummary) -> Result<(), String> {
    let mut index = read_index(root)?;
    replace_index_job(&mut index, job);
    write_index(root, &index)
}

pub(crate) fn replace_index_job(index: &mut RosettaJobIndex, job: RosettaJobSummary) {
    index.jobs.retain(|existing| existing.id != job.id);
    index.jobs.push(job);
    index
        .jobs
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 JSON 文件 {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("JSON 文件格式错误 {}: {error}", path.display()))
}

pub(crate) fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let contents =
        serde_json::to_string_pretty(value).map_err(|error| format!("无法序列化 JSON: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("无法写入 JSON 文件 {}: {error}", path.display()))
}
