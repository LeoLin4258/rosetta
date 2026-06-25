use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{
        apply_segment_translations_to_document, ensure_document_files,
        sync_document_default_language_from_files, sync_document_file_statuses, sync_job_counts,
        sync_job_source_files,
    },
    formats::{collect_supported_source_paths, document_format, parse_source, pdf, SourceFormat},
    model::{
        default_file_translation_status, RosettaDocument, RosettaJobBundle, RosettaJobDeleteResult,
        RosettaJobFileDeleteResult, RosettaJobSummary, RosettaSourceFile, Segment, SourceSnapshot,
        MAX_IMPORT_BYTES, MAX_PROJECT_FILES, SCHEMA_VERSION,
    },
    path::{
        checked_job_dir, cleanup_empty_dirs, jobs_root, new_job_id, path_from_relative,
        relative_path_string, timestamp_ms_string,
    },
    revisions::{archive_segment_translation, create_revision_snapshot},
    segmenter::{apply_file_id, renumber_blocks_and_segments},
    store::{
        load_job_bundle, read_index, read_json, read_translation_revisions, replace_index_job,
        write_index, write_job_bundle, write_job_bundle_pdf, write_job_bundle_sources, write_json,
        write_translation_files, write_translation_revisions,
    },
    translation_files::{read_or_migrate_translation_files, translation_segments_path},
};

const BLANK_TXT_DEFAULT_TITLE: &str = "未命名文档";
const DELETE_CLEANUP_TASKS_FILENAME: &str = "delete_cleanup_tasks.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteCleanupTask {
    job_id: String,
    path: String,
    created_at: String,
    last_error: Option<String>,
}

/// Fast path for PDF imports: synchronously creates the job directory + copies
/// the source PDF + writes a skeleton `document.json` (no blocks). Phase 3
/// delegates extraction + translation to pdf2zh when the user clicks translate.
pub(crate) async fn import_pdf_skeleton(
    app: &AppHandle,
    source_path: &Path,
) -> Result<RosettaJobBundle, String> {
    let metadata =
        fs::metadata(source_path).map_err(|error| format!("无法读取文件信息: {error}"))?;
    if !metadata.is_file() {
        return Err("只能导入文件。".to_string());
    }
    let format = document_format(source_path)?;
    if format != SourceFormat::Pdf {
        return Err("此命令仅支持 PDF 文件。".to_string());
    }

    // Cheap fast-fail BEFORE we create the sidebar entry — catches encrypted /
    // too-large / too-many-pages so users don't end up with a phantom job.
    pdf::extract::pre_flight(app, source_path).map_err(|error| error.user_message())?;

    let now = timestamp_ms_string();
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("untitled")
        .to_string();
    let job_id = new_job_id(source_path);
    let document_id = format!("document-{job_id}");

    let document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id.clone(),
        filename: filename.clone(),
        format: "pdf".to_string(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files: vec![RosettaSourceFile {
            id: "file-1".to_string(),
            filename: filename.clone(),
            relative_path: filename.clone(),
            format: "pdf".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: Some("zh-CN".to_string()),
            translation_status: default_file_translation_status(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids: vec![],
        }],
        blocks: vec![],
        extraction_status: Some("done".to_string()),
    };

    let job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: job_id.clone(),
        filename: filename.clone(),
        format: "pdf".to_string(),
        source_path: Some(source_path.to_string_lossy().to_string()),
        source_filename: filename.clone(),
        source_kind: "file".to_string(),
        file_count: 1,
        source_files: document.files.clone(),
        status: "ready".to_string(),
        created_at: now.clone(),
        updated_at: now,
        exported_at: None,
        last_error: None,
        target_lang: "zh-CN".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments: vec![],
        translation_files: vec![],
        translation_revisions: vec![],
    };
    write_job_bundle_pdf(app, &bundle, source_path)?;

    Ok(bundle)
}

pub(crate) async fn import_document_from_path(
    app: &AppHandle,
    source_path: &Path,
) -> Result<RosettaJobBundle, String> {
    let metadata =
        fs::metadata(source_path).map_err(|error| format!("无法读取文件信息: {error}"))?;
    if !metadata.is_file() {
        return Err("只能导入文件。".to_string());
    }

    let format = document_format(source_path)?;
    let format_name = format.as_str().to_string();

    // Size cap differs by format: PDF gets its own 100 MB limit checked by
    // the pdfium pre-flight; text formats stay at 5 MB.
    if format != SourceFormat::Pdf && metadata.len() > MAX_IMPORT_BYTES {
        return Err("文件超过 5 MB，当前原型暂不导入超大文件。".to_string());
    }

    let now = timestamp_ms_string();
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("untitled")
        .to_string();
    let job_id = new_job_id(source_path);
    let document_id = format!("document-{job_id}");

    // Branch: PDF is imported as a black-box source file; text formats stay UTF-8.
    let (blocks, segments, source_contents) = match format {
        SourceFormat::Pdf => {
            pdf::extract::pre_flight(app, source_path).map_err(|error| error.user_message())?;
            (Vec::new(), Vec::new(), None)
        }
        _ => {
            let contents = fs::read_to_string(source_path)
                .map_err(|error| format!("无法按 UTF-8 读取文件: {error}"))?;
            if contents.trim().is_empty() {
                return Err("文件没有可导入的文本内容。".to_string());
            }
            let parsed = parse_source(format, &document_id, &contents);
            (parsed.blocks, parsed.segments, Some(contents))
        }
    };

    let mut blocks = blocks;
    let mut segments = segments;
    apply_file_id(&mut blocks, &mut segments, "file-1");

    if format != SourceFormat::Pdf && segments.is_empty() {
        return Err("文件没有可翻译的文本段落。".to_string());
    }
    let block_ids = blocks.iter().map(|block| block.id.clone()).collect();

    let mut document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: filename.clone(),
        format: format_name.clone(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files: vec![RosettaSourceFile {
            id: "file-1".to_string(),
            filename: filename.clone(),
            relative_path: filename.clone(),
            format: format_name.clone(),
            source_lang: Some("en".to_string()),
            target_lang: Some("zh-CN".to_string()),
            translation_status: default_file_translation_status(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids,
        }],
        blocks,
        // Synchronous path always returns a fully-extracted document — only
        // the skeleton flow uses "pending".
        extraction_status: Some("done".to_string()),
    };
    let source_files = document.files.clone();
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: job_id,
        filename: filename.clone(),
        format: format_name.clone(),
        source_path: Some(source_path.to_string_lossy().to_string()),
        source_filename: filename.clone(),
        source_kind: "file".to_string(),
        file_count: 1,
        source_files,
        status: "ready".to_string(),
        created_at: now.clone(),
        updated_at: now,
        exported_at: None,
        last_error: None,
        target_lang: "zh-CN".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files: Vec::new(),
        translation_revisions: Vec::new(),
    };
    match format {
        SourceFormat::Pdf => write_job_bundle_pdf(app, &bundle, source_path)?,
        _ => write_job_bundle(app, &bundle, source_contents.as_deref().unwrap_or(""))?,
    }
    Ok(bundle)
}

pub(crate) fn import_project_from_directory(
    app: &AppHandle,
    source_dir: &Path,
) -> Result<RosettaJobBundle, String> {
    let metadata =
        fs::metadata(source_dir).map_err(|error| format!("无法读取文件夹信息: {error}"))?;
    if !metadata.is_dir() {
        return Err("请选择一个文件夹。".to_string());
    }

    let mut source_paths = Vec::new();
    collect_supported_source_paths(source_dir, source_dir, &mut source_paths)?;
    source_paths.sort();

    if source_paths.is_empty() {
        return Err("这个文件夹里没有 TXT 或 Markdown 文件。".to_string());
    }
    if source_paths.len() > MAX_PROJECT_FILES {
        return Err(format!(
            "这个文件夹包含超过 {MAX_PROJECT_FILES} 个可导入文件，请先拆分项目。"
        ));
    }

    let now = timestamp_ms_string();
    let folder_name = source_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("folder")
        .to_string();
    let job_id = new_job_id(source_dir);
    let document_id = format!("document-{job_id}");
    let mut files = Vec::new();
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut source_snapshots = Vec::new();
    let mut next_block_order = 1;
    let mut next_segment_order = 1;
    let mut has_markdown = false;

    for (file_index, source_path) in source_paths.iter().enumerate() {
        let metadata =
            fs::metadata(source_path).map_err(|error| format!("无法读取文件信息: {error}"))?;
        if metadata.len() > MAX_IMPORT_BYTES {
            return Err(format!(
                "文件 {} 超过 5 MB，当前原型暂不导入超大文件。",
                source_path.display()
            ));
        }

        let format = document_format(source_path)?;
        let format_name = format.as_str().to_string();
        has_markdown = has_markdown || format_name == "markdown";
        let contents = fs::read_to_string(source_path)
            .map_err(|error| format!("无法按 UTF-8 读取文件 {}: {error}", source_path.display()))?;
        let relative_path = relative_path_string(source_dir, source_path)?;
        let filename = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        let file_id = format!("file-{}", file_index + 1);
        let parser_document_id = format!("{document_id}-{file_id}");
        let parsed = parse_source(format, &parser_document_id, &contents);
        let (mut file_blocks, mut file_segments) = (parsed.blocks, parsed.segments);

        apply_file_id(&mut file_blocks, &mut file_segments, &file_id);
        renumber_blocks_and_segments(
            &mut file_blocks,
            &mut file_segments,
            &mut next_block_order,
            &mut next_segment_order,
        );
        let block_ids = file_blocks.iter().map(|block| block.id.clone()).collect();

        files.push(RosettaSourceFile {
            id: file_id,
            filename,
            relative_path: relative_path.clone(),
            format: format_name.clone(),
            source_lang: Some("en".to_string()),
            target_lang: Some("zh-CN".to_string()),
            translation_status: default_file_translation_status(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids,
        });
        blocks.extend(file_blocks);
        segments.extend(file_segments);
        source_snapshots.push(SourceSnapshot {
            relative_path,
            contents,
        });
    }

    if segments.is_empty() {
        return Err("这个文件夹里没有可翻译的文本段落。".to_string());
    }

    let document_format = if has_markdown { "markdown" } else { "txt" }.to_string();
    let mut document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: folder_name.clone(),
        format: document_format.clone(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files,
        blocks,
        extraction_status: Some("done".to_string()),
    };
    let source_files = document.files.clone();
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: job_id,
        filename: folder_name.clone(),
        format: document_format,
        source_path: Some(source_dir.to_string_lossy().to_string()),
        source_filename: folder_name,
        source_kind: "directory".to_string(),
        file_count: source_snapshots.len(),
        source_files,
        status: "ready".to_string(),
        created_at: now.clone(),
        updated_at: now,
        exported_at: None,
        last_error: None,
        target_lang: "zh-CN".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files: Vec::new(),
        translation_revisions: Vec::new(),
    };
    write_job_bundle_sources(app, &bundle, &source_snapshots)?;
    Ok(bundle)
}

pub(crate) fn create_blank_txt_document(
    app: &AppHandle,
    filename: String,
) -> Result<RosettaJobBundle, String> {
    let now = timestamp_ms_string();
    let job_id = format!("job-{now}-txt");
    let bundle = build_blank_txt_bundle(&job_id, &now, &filename)?;
    write_job_bundle(app, &bundle, "")?;
    Ok(bundle)
}

pub(crate) fn build_blank_txt_bundle(
    job_id: &str,
    now: &str,
    title: &str,
) -> Result<RosettaJobBundle, String> {
    let display_title = sanitize_txt_filename_stem(title);
    let filename = format!("{display_title}.txt");
    let document_id = format!("document-{job_id}");
    let mut document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: filename.clone(),
        format: "txt".to_string(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files: vec![RosettaSourceFile {
            id: "file-1".to_string(),
            filename: filename.clone(),
            relative_path: filename.clone(),
            format: "txt".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: Some("zh-CN".to_string()),
            translation_status: default_file_translation_status(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids: vec![],
        }],
        blocks: vec![],
        extraction_status: Some("done".to_string()),
    };
    let source_files = document.files.clone();
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: job_id.to_string(),
        filename: filename.clone(),
        format: "txt".to_string(),
        source_path: None,
        source_filename: filename,
        source_kind: "file".to_string(),
        file_count: 1,
        source_files,
        status: "ready".to_string(),
        created_at: now.to_string(),
        updated_at: now.to_string(),
        exported_at: None,
        last_error: None,
        target_lang: "zh-CN".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };
    let segments = Vec::new();
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files: Vec::new(),
        translation_revisions: Vec::new(),
    })
}

pub(crate) fn rebuild_txt_source_file(
    document: &mut RosettaDocument,
    segments: &mut Vec<Segment>,
    file_id: &str,
    contents: &str,
) -> Result<(), String> {
    let Some(file_index) = document.files.iter().position(|file| file.id == file_id) else {
        return Err("当前文件不存在，无法编辑原文。".to_string());
    };
    if document.files[file_index].format != "txt" {
        return Err("只有 TXT 文件支持直接编辑原文。".to_string());
    }
    if contents.len() as u64 > MAX_IMPORT_BYTES {
        return Err("文本超过 5 MB，当前原型暂不保存超长文本。".to_string());
    }

    document
        .blocks
        .retain(|block| block.file_id.as_deref().unwrap_or("file-1") != file_id);
    segments.retain(|segment| segment.file_id.as_deref().unwrap_or("file-1") != file_id);

    let parser_document_id = format!("{}-{file_id}", document.id);
    let parsed = if contents.trim().is_empty() {
        crate::rosetta_jobs::formats::ParsedSource {
            blocks: Vec::new(),
            segments: Vec::new(),
        }
    } else {
        parse_source(SourceFormat::Txt, &parser_document_id, contents)
    };
    let (mut next_blocks, mut next_segments) = (parsed.blocks, parsed.segments);
    apply_file_id(&mut next_blocks, &mut next_segments, file_id);

    document.blocks.extend(next_blocks);
    segments.extend(next_segments);
    let mut next_block_order = 1;
    let mut next_segment_order = 1;
    renumber_blocks_and_segments(
        &mut document.blocks,
        segments,
        &mut next_block_order,
        &mut next_segment_order,
    );
    document.files[file_index].block_ids = document
        .blocks
        .iter()
        .filter(|block| (block.file_id.as_deref().unwrap_or("file-1")) == file_id)
        .map(|block| block.id.clone())
        .collect();
    sync_document_file_statuses(document, segments);
    Ok(())
}

pub(crate) fn update_txt_source_file(
    app: &AppHandle,
    job_id: &str,
    file_id: &str,
    contents: String,
) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法编辑原文。".to_string())?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    if document.files.len() != 1 {
        return Err("当前只支持编辑单文件 TXT 文档。".to_string());
    }
    let mut segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;

    rebuild_txt_source_file(&mut document, &mut segments, file_id, &contents)?;
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    job.updated_at = timestamp_ms_string();
    job.last_error = None;

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files: Vec::new(),
        translation_revisions: Vec::new(),
    };
    write_job_bundle(app, &bundle, &contents)?;
    Ok(bundle)
}

fn sanitize_txt_filename_stem(title: &str) -> String {
    let title = title.trim().trim_end_matches(".txt").trim();
    let sanitized = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let clipped = sanitized.chars().take(80).collect::<String>();
    if clipped.is_empty() {
        BLANK_TXT_DEFAULT_TITLE.to_string()
    } else {
        clipped
    }
}

pub(crate) fn save_segments(
    app: &AppHandle,
    job_id: &str,
    segments: Vec<Segment>,
) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let translation_revisions = read_translation_revisions(&dir)?;
    let translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目索引不存在，无法保存翻译结果。".to_string())?;

    apply_segment_translations_to_document(&mut document, &segments);
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    job.updated_at = timestamp_ms_string();
    job.last_error = None;

    write_json(&dir.join("document.json"), &document)?;
    write_json(&dir.join("segments.json"), &segments)?;
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files,
        translation_revisions,
    })
}

pub(crate) fn update_job_file_languages(
    app: &AppHandle,
    job_id: &str,
    file_id: &str,
    source_lang: Option<String>,
    target_lang: String,
) -> Result<RosettaJobBundle, String> {
    let normalized_source_lang = normalize_optional_lang(source_lang);
    let normalized_target_lang = normalize_required_lang(target_lang)?;
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let mut segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_revisions = read_translation_revisions(&dir)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目索引不存在，无法保存语言设置。".to_string())?;

    let Some(file_index) = document.files.iter().position(|file| file.id == file_id) else {
        return Err("当前文件不存在，无法保存语言设置。".to_string());
    };

    let current_source_lang = document.files[file_index]
        .source_lang
        .clone()
        .or_else(|| document.source_lang.clone());
    let current_target_lang = document.files[file_index]
        .target_lang
        .clone()
        .unwrap_or_else(|| document.target_lang.clone());
    let changed = current_source_lang != normalized_source_lang
        || current_target_lang != normalized_target_lang;

    if changed {
        if let Some(revision) = create_revision_snapshot(
            job_id,
            file_id,
            "language-change",
            None,
            &document,
            &segments,
        )? {
            translation_revisions.push(revision);
        }
    }

    document.files[file_index].source_lang = normalized_source_lang.clone();
    document.files[file_index].target_lang = Some(normalized_target_lang.clone());

    if document.files.len() == 1 {
        document.source_lang = normalized_source_lang.clone();
        document.target_lang = normalized_target_lang.clone();
    }

    sync_document_default_language_from_files(&mut document);
    job.target_lang = document.target_lang.clone();
    let history_run_id = format!("run-{}", timestamp_ms_string());

    for segment in &mut segments {
        if segment.file_id.as_deref().unwrap_or("file-1") != file_id {
            continue;
        }

        if changed {
            archive_segment_translation(segment, "language-change", &history_run_id);
        }
        segment.source_lang = normalized_source_lang.clone();
        segment.target_lang = normalized_target_lang.clone();
        if changed {
            segment.translated_text = None;
            segment.error = None;
            if segment.status != "skipped" {
                segment.status = "pending".to_string();
            }
        }
    }

    if changed {
        for block in &mut document.blocks {
            if block.file_id.as_deref().unwrap_or("file-1") != file_id {
                continue;
            }
            if block.should_translate {
                block.translated_text = None;
                block.status = "pending".to_string();
            }
        }
    }

    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    job.updated_at = timestamp_ms_string();
    job.last_error = None;

    write_json(&dir.join("document.json"), &document)?;
    write_json(&dir.join("segments.json"), &segments)?;
    write_translation_revisions(&dir, &translation_revisions)?;
    let translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files,
        translation_revisions,
    })
}

pub(crate) fn rename_job(
    app: &AppHandle,
    job_id: &str,
    name: &str,
) -> Result<Vec<RosettaJobSummary>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名不能为空。".to_string());
    }
    if name.chars().count() > 80 {
        return Err("项目名不能超过 80 个字符。".to_string());
    }

    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法重命名。".to_string())?;

    document.filename = name.to_string();
    job.filename = name.to_string();
    job.updated_at = timestamp_ms_string();

    write_json(&dir.join("document.json"), &document)?;
    replace_index_job(&mut index, job);
    write_index(&root, &index)?;
    Ok(index.jobs)
}

pub(crate) fn normalize_optional_lang(language: Option<String>) -> Option<String> {
    language
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "auto")
}

pub(crate) fn normalize_required_lang(language: String) -> Result<String, String> {
    let normalized = language.trim().to_string();
    if normalized.is_empty() || normalized == "auto" {
        return Err("请选择目标语言。".to_string());
    }
    Ok(normalized)
}

pub(crate) fn delete_job(app: &AppHandle, job_id: &str) -> Result<RosettaJobDeleteResult, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;

    let mut index = read_index(&root)?;
    let before = index.jobs.len();
    index.jobs.retain(|job| job.id != job_id);
    if before == index.jobs.len() && !dir.exists() {
        return Ok(RosettaJobDeleteResult {
            jobs: index.jobs,
            cleanup_status: "not-found".to_string(),
            warning: None,
        });
    }
    write_index(&root, &index)?;

    let (cleanup_status, warning) = if dir.exists() {
        match move_job_dir_to_trash(&root, &dir, job_id) {
            Ok(trash_dir) => match fs::remove_dir_all(&trash_dir) {
                Ok(_) => ("deleted".to_string(), None),
                Err(error) => {
                    record_delete_cleanup_task(&root, job_id, &trash_dir, Some(error.to_string()))?;
                    (
                        "pending-cleanup".to_string(),
                        Some(format!("文档已从列表移除，磁盘缓存稍后重试清理: {error}")),
                    )
                }
            },
            Err(error) => {
                record_delete_cleanup_task(&root, job_id, &dir, Some(error.clone()))?;
                (
                    "pending-cleanup".to_string(),
                    Some(format!("文档已从列表移除，磁盘缓存稍后重试清理: {error}")),
                )
            }
        }
    } else {
        ("no-cache".to_string(), None)
    };

    Ok(RosettaJobDeleteResult {
        jobs: index.jobs,
        cleanup_status,
        warning,
    })
}

pub(crate) fn delete_job_file(
    app: &AppHandle,
    job_id: &str,
    file_id: &str,
) -> Result<RosettaJobFileDeleteResult, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法删除文件。".to_string())?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let mut segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_revisions = read_translation_revisions(&dir)?;
    let Some(file_index) = document.files.iter().position(|file| file.id == file_id) else {
        return Err("当前文件不存在，无法删除。".to_string());
    };

    if document.files.len() <= 1 {
        let result = delete_job(app, job_id)?;
        return Ok(RosettaJobFileDeleteResult {
            deleted_job: true,
            jobs: result.jobs,
            bundle: None,
            message: "项目只包含一个文件，已删除整个项目。".to_string(),
        });
    }

    let removed_file = document.files.remove(file_index);
    document
        .blocks
        .retain(|block| block.file_id.as_deref().unwrap_or("file-1") != removed_file.id.as_str());
    segments.retain(|segment| {
        segment.file_id.as_deref().unwrap_or("file-1") != removed_file.id.as_str()
    });
    translation_revisions.retain(|revision| revision.file_id != removed_file.id);
    let mut next_block_order = 1;
    let mut next_segment_order = 1;
    renumber_blocks_and_segments(
        &mut document.blocks,
        &mut segments,
        &mut next_block_order,
        &mut next_segment_order,
    );
    apply_segment_translations_to_document(&mut document, &segments);
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    job.updated_at = timestamp_ms_string();
    job.last_error = None;

    let source_path = dir
        .join("sources")
        .join(path_from_relative(&removed_file.relative_path)?);
    if source_path.exists() {
        fs::remove_file(&source_path)
            .map_err(|error| format!("无法删除源文件缓存 {}: {error}", source_path.display()))?;
    }
    if let Some(parent) = source_path.parent() {
        cleanup_empty_dirs(parent, &dir.join("sources"))?;
    }

    write_json(&dir.join("document.json"), &document)?;
    write_json(&dir.join("segments.json"), &segments)?;
    write_translation_revisions(&dir, &translation_revisions)?;
    let mut translation_files = read_or_migrate_translation_files(&dir, &document, &segments)?;
    for translation_file in translation_files
        .iter()
        .filter(|translation_file| translation_file.source_file_id == removed_file.id)
    {
        let path = translation_segments_path(&dir, &translation_file.id)?;
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("无法删除译文文件缓存: {error}"))?;
        }
    }
    translation_files.retain(|translation_file| translation_file.source_file_id != removed_file.id);
    write_translation_files(&dir, &translation_files)?;
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaJobFileDeleteResult {
        deleted_job: false,
        jobs: index.jobs,
        bundle: Some(RosettaJobBundle {
            schema_version: SCHEMA_VERSION,
            job,
            document,
            segments,
            translation_files,
            translation_revisions,
        }),
        message: "文件已删除。".to_string(),
    })
}

pub(crate) fn cleanup_pending_job_deletions(app: &AppHandle) -> Result<(), String> {
    let root = jobs_root(app)?;
    let tasks_path = root.join(DELETE_CLEANUP_TASKS_FILENAME);
    if !tasks_path.is_file() {
        return Ok(());
    }

    let tasks: Vec<DeleteCleanupTask> = read_json(&tasks_path)?;
    let mut remaining = Vec::new();
    for mut task in tasks {
        let path = PathBuf::from(&task.path);
        if !path.starts_with(&root) {
            task.last_error = Some("清理路径不在 Rosetta jobs 目录内，已跳过。".to_string());
            remaining.push(task);
            continue;
        }
        if !path.exists() {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(&path) {
            task.last_error = Some(error.to_string());
            remaining.push(task);
        }
    }

    if remaining.is_empty() {
        let _ = fs::remove_file(tasks_path);
    } else {
        write_json(&tasks_path, &remaining)?;
    }
    Ok(())
}

fn move_job_dir_to_trash(root: &Path, dir: &Path, job_id: &str) -> Result<PathBuf, String> {
    let trash_root = root.join(".trash");
    fs::create_dir_all(&trash_root).map_err(|error| format!("无法创建项目回收目录: {error}"))?;
    let trash_dir = trash_root.join(format!("{job_id}-{}", timestamp_ms_string()));
    fs::rename(dir, &trash_dir).map_err(|error| format!("无法移动项目缓存到回收目录: {error}"))?;
    Ok(trash_dir)
}

fn record_delete_cleanup_task(
    root: &Path,
    job_id: &str,
    path: &Path,
    last_error: Option<String>,
) -> Result<(), String> {
    let tasks_path = root.join(DELETE_CLEANUP_TASKS_FILENAME);
    let mut tasks: Vec<DeleteCleanupTask> = if tasks_path.is_file() {
        read_json(&tasks_path)?
    } else {
        Vec::new()
    };
    let path_string = path.to_string_lossy().to_string();
    tasks.retain(|task| task.path != path_string);
    tasks.push(DeleteCleanupTask {
        job_id: job_id.to_string(),
        path: path_string,
        created_at: timestamp_ms_string(),
        last_error,
    });
    write_json(&tasks_path, &tasks)
}

const WELCOME_JOB_ID: &str = "job-welcome";

const WELCOME_CONTENT: &str = r#"# 欢迎使用 Rosetta

Rosetta 是一款专为文档翻译设计的桌面应用，支持 Markdown、TXT 和 PDF 格式，让你的内容跨越语言边界。

## 如何开始

- **拖拽文件**到窗口，或点击「打开文件」导入文档
- 在顶部工具栏选择**目标语言**，然后点击「翻译」
- 翻译完成后，通过「导出」保存翻译结果

## 主要功能

### 多格式支持
支持导入 `.md`、`.txt` 和 `.pdf` 文件，自动识别文档结构，逐段翻译。

### 段落级精准翻译
每个段落独立翻译，可以选中任意段落**单独重翻**，精细调整翻译结果。

### 双语对照
翻译时左侧显示原文，右侧实时展示译文，便于对照校对。

### 本地 AI 模型
内置 RWKV 模型，支持本地离线运行，无需联网，保护文档隐私。也可以连接外部 API 使用云端模型。

### 导出灵活
支持导出**纯译文**或**双语对照**两种格式，满足不同使用场景。

---

现在就拖入一份文档，开始你的第一次翻译吧！
"#;

pub(crate) fn create_welcome_document(app: &AppHandle) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let job_dir = checked_job_dir(&root, WELCOME_JOB_ID)?;

    if job_dir.exists() {
        return load_job_bundle(app, WELCOME_JOB_ID);
    }

    let filename = "欢迎使用 Rosetta.md".to_string();
    let format = SourceFormat::Markdown;
    let format_name = format.as_str().to_string();
    let now = timestamp_ms_string();
    let document_id = format!("document-{WELCOME_JOB_ID}");

    let parsed = parse_source(format, &document_id, WELCOME_CONTENT);
    let mut blocks = parsed.blocks;
    let mut segments = parsed.segments;
    apply_file_id(&mut blocks, &mut segments, "file-1");

    let block_ids = blocks.iter().map(|block| block.id.clone()).collect();

    let mut document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: filename.clone(),
        format: format_name.clone(),
        source_lang: Some("zh-CN".to_string()),
        target_lang: "en".to_string(),
        files: vec![RosettaSourceFile {
            id: "file-1".to_string(),
            filename: filename.clone(),
            relative_path: filename.clone(),
            format: format_name.clone(),
            source_lang: Some("zh-CN".to_string()),
            target_lang: Some("en".to_string()),
            translation_status: default_file_translation_status(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids,
        }],
        blocks,
        extraction_status: Some("done".to_string()),
    };
    let source_files = document.files.clone();
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: WELCOME_JOB_ID.to_string(),
        filename: filename.clone(),
        format: format_name.clone(),
        source_path: None,
        source_filename: filename.clone(),
        source_kind: "file".to_string(),
        file_count: 1,
        source_files,
        status: "ready".to_string(),
        created_at: now.clone(),
        updated_at: now,
        exported_at: None,
        last_error: None,
        target_lang: "en".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
        translation_files: Vec::new(),
        translation_revisions: Vec::new(),
    };
    write_job_bundle(app, &bundle, WELCOME_CONTENT)?;
    Ok(bundle)
}
