use std::{
    collections::HashMap,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use serde_json::Value;
use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{
        document_files, ensure_document_files, segments_by_block, sync_document_file_statuses,
        sync_job_counts, sync_job_source_files,
    },
    formats::pdf_markdown::render::{
        is_pdf_markdown_block, join_blocks as join_pdf_markdown_blocks,
        render_blocks as render_pdf_markdown_blocks,
    },
    model::{
        RosettaBlock, RosettaDocument, RosettaExportKind, RosettaExportResult, Segment,
        TranslationSegment,
    },
    path::{checked_job_dir, jobs_root, timestamp_ms_string},
    store::{read_index, read_json, replace_index_job, write_index, write_translation_files},
    translation_files::{
        read_or_migrate_translation_files, read_translation_segments, translated_source_segments,
    },
};

pub(crate) fn export_job_file(
    app: &AppHandle,
    job_id: &str,
    file_id: &str,
    kind: RosettaExportKind,
    target_path: &Path,
) -> Result<RosettaExportResult, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法导出。".to_string())?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    sync_document_file_statuses(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
    sync_job_source_files(&mut job, &document);
    let source_file = document_files(&document)
        .into_iter()
        .find(|file| file.id == file_id)
        .ok_or_else(|| "当前文件不存在，无法导出。".to_string())?;
    let file_blocks = document
        .blocks
        .iter()
        .filter(|block| block.file_id.as_deref().unwrap_or("file-1") == source_file.id.as_str())
        .cloned()
        .collect::<Vec<_>>();
    let file_segments = segments
        .iter()
        .filter(|segment| segment.file_id.as_deref().unwrap_or("file-1") == source_file.id.as_str())
        .cloned()
        .collect::<Vec<_>>();

    ensure_file_ready_for_export(&file_segments)?;

    let output = render_export_blocks(
        &document,
        &file_blocks,
        &file_segments,
        kind.as_str(),
        &source_file.format,
    );

    write_export_atomically(target_path, output.as_bytes())?;

    job.exported_at = Some(timestamp_ms_string());
    job.updated_at = timestamp_ms_string();
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaExportResult {
        job,
        target_path: target_path.to_string_lossy().to_string(),
        kind: kind.to_string(),
        bytes_written: output.len() as u64,
        files_written: 1,
        message: "导出完成。".to_string(),
    })
}

fn write_export_atomically(target_path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target_path
        .parent()
        .ok_or_else(|| "导出路径无效。".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "无法创建导出目录。".to_string())?;
    let filename = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("translation");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let staged = parent.join(format!(".{filename}.{}.{}.tmp", std::process::id(), nonce));
    fs::write(&staged, bytes).map_err(|_| "无法写入临时导出文件。".to_string())?;
    fs::OpenOptions::new()
        .write(true)
        .open(&staged)
        .and_then(|file| file.sync_all())
        .map_err(|_| "无法刷写临时导出文件。".to_string())?;
    if let Err(error) = replace_export_file(&staged, target_path) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_export_file(staged: &Path, target_path: &Path) -> Result<(), String> {
    fs::rename(staged, target_path).map_err(|_| "无法提交导出文件。".to_string())
}

#[cfg(windows)]
fn replace_export_file(staged: &Path, target_path: &Path) -> Result<(), String> {
    let staged_wide = staged
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target_wide = target_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            staged_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err("无法提交导出文件。".to_string());
    }
    Ok(())
}

pub(crate) fn export_translation_file(
    app: &AppHandle,
    job_id: &str,
    translation_file_id: &str,
    kind: RosettaExportKind,
    target_path: &Path,
) -> Result<RosettaExportResult, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法导出。".to_string())?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    ensure_document_files(&mut document);
    let source_segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut translation_files =
        read_or_migrate_translation_files(&dir, &document, &source_segments)?;
    let Some(translation_file_index) = translation_files
        .iter()
        .position(|file| file.id == translation_file_id)
    else {
        return Err("译文文件不存在，无法导出。".to_string());
    };
    let translation_file = translation_files[translation_file_index].clone();
    let source_file = document_files(&document)
        .into_iter()
        .find(|file| file.id == translation_file.source_file_id)
        .ok_or_else(|| "当前源文件不存在，无法导出。".to_string())?;
    if source_file.format.eq_ignore_ascii_case("pdf")
        && translation_file
            .output_format
            .eq_ignore_ascii_case("markdown")
        && kind == RosettaExportKind::Bilingual
    {
        return Err("PDF Markdown 暂不支持双语导出。".to_string());
    }
    let translation_segments = read_translation_segments(&dir, translation_file_id)?;
    ensure_translation_file_ready_for_export(&translation_segments)?;

    let file_blocks = document
        .blocks
        .iter()
        .filter(|block| block.file_id.as_deref().unwrap_or("file-1") == source_file.id.as_str())
        .cloned()
        .collect::<Vec<_>>();
    let file_segments = translated_source_segments(
        &source_segments,
        &translation_segments,
        &source_file.id,
        &translation_file.target_lang,
    );

    let output = render_export_blocks(
        &document,
        &file_blocks,
        &file_segments,
        kind.as_str(),
        &translation_file.output_format,
    );

    write_export_atomically(target_path, output.as_bytes())?;

    let now = timestamp_ms_string();
    translation_files[translation_file_index].exported_at = Some(now.clone());
    translation_files[translation_file_index].updated_at = now.clone();
    write_translation_files(&dir, &translation_files)?;
    job.exported_at = Some(now);
    job.updated_at = timestamp_ms_string();
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaExportResult {
        job,
        target_path: target_path.to_string_lossy().to_string(),
        kind: kind.to_string(),
        bytes_written: output.len() as u64,
        files_written: 1,
        message: "导出完成。".to_string(),
    })
}

pub(crate) fn ensure_translation_file_ready_for_export(
    segments: &[TranslationSegment],
) -> Result<(), String> {
    let translatable_segments = segments
        .iter()
        .filter(|segment| segment.status != "skipped")
        .collect::<Vec<_>>();

    if translatable_segments.is_empty() {
        return Err("当前译文文件没有可导出的译文。".to_string());
    }

    if translatable_segments
        .iter()
        .any(|segment| !matches!(segment.status.as_str(), "done" | "edited"))
    {
        return Err("当前译文文件还没有完成翻译，不能导出。".to_string());
    }

    if translatable_segments.iter().any(|segment| {
        segment
            .translated_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    }) {
        return Err("当前译文文件存在空译文，不能导出。".to_string());
    }

    Ok(())
}

pub(crate) fn ensure_file_ready_for_export(segments: &[Segment]) -> Result<(), String> {
    let translatable_segments = segments
        .iter()
        .filter(|segment| !segment.source_text.trim().is_empty() && segment.status != "skipped")
        .collect::<Vec<_>>();

    if translatable_segments.is_empty() {
        return Err("当前文件没有可导出的译文。".to_string());
    }

    if translatable_segments
        .iter()
        .any(|segment| !matches!(segment.status.as_str(), "done" | "edited"))
    {
        return Err("当前文件还没有完成翻译，不能导出。".to_string());
    }

    if translatable_segments.iter().any(|segment| {
        segment
            .translated_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    }) {
        return Err("当前文件存在空译文，不能导出。".to_string());
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn render_export(
    document: &RosettaDocument,
    segments: &[Segment],
    kind: &str,
) -> String {
    render_export_blocks(document, &document.blocks, segments, kind, &document.format)
}

pub(crate) fn render_export_blocks(
    document: &RosettaDocument,
    blocks: &[RosettaBlock],
    segments: &[Segment],
    kind: &str,
    output_format: &str,
) -> String {
    let by_block = segments_by_block(segments);
    if output_format == "markdown" && blocks.iter().any(is_pdf_markdown_block) {
        let text_by_block = blocks
            .iter()
            .map(|block| {
                let text = if block.should_translate {
                    block_translation(block, &by_block, &document.target_lang)
                } else {
                    block.source_text.clone()
                };
                (block.id.clone(), text)
            })
            .collect::<HashMap<_, _>>();
        return join_pdf_markdown_blocks(&render_pdf_markdown_blocks(blocks, &text_by_block));
    }
    if output_format == "markdown" {
        return render_markdown_export_blocks(document, blocks, &by_block, kind, output_format);
    }

    let output_blocks = blocks
        .iter()
        .map(|block| render_export_block(document, block, &by_block, kind, output_format))
        .collect::<Vec<_>>();
    trim_excess_blank_blocks(output_blocks).join("\n\n")
}

pub(crate) fn render_markdown_export_blocks(
    document: &RosettaDocument,
    blocks: &[RosettaBlock],
    by_block: &HashMap<String, Vec<Segment>>,
    kind: &str,
    output_format: &str,
) -> String {
    let mut output = String::new();
    let mut previous_type: Option<&str> = None;

    for block in blocks {
        let rendered = render_export_block(document, block, by_block, kind, output_format);
        let rendered = rendered.trim_matches('\n');

        if rendered.trim().is_empty() {
            if !output.is_empty() && !output.ends_with("\n\n") {
                output.push_str("\n\n");
            }
            previous_type = None;
            continue;
        }

        if !output.is_empty() && !output.ends_with("\n\n") {
            let separator = if previous_type == Some("list_item") && block.block_type == "list_item"
            {
                "\n"
            } else {
                "\n\n"
            };
            output.push_str(separator);
        }

        output.push_str(rendered);
        previous_type = Some(block.block_type.as_str());
    }

    output.trim().to_string()
}

fn render_export_block(
    document: &RosettaDocument,
    block: &RosettaBlock,
    by_block: &HashMap<String, Vec<Segment>>,
    kind: &str,
    output_format: &str,
) -> String {
    if !block.should_translate {
        return block.source_text.clone();
    }

    let translation = block_translation(block, by_block, &document.target_lang);
    if kind == "bilingual" {
        render_bilingual_block(block, &translation, output_format)
    } else {
        render_translation_block(block, &translation, output_format)
    }
}

pub(crate) fn block_translation(
    block: &RosettaBlock,
    by_block: &HashMap<String, Vec<Segment>>,
    target_lang: &str,
) -> String {
    let Some(segments) = by_block.get(&block.id) else {
        return block.source_text.clone();
    };
    let translated = segments
        .iter()
        .map(|segment| {
            segment
                .translated_text
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or(&segment.source_text)
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(segment_joiner(
            segments
                .first()
                .map(|segment| segment.target_lang.as_str())
                .unwrap_or(target_lang),
        ));

    if translated.trim().is_empty() {
        block.source_text.clone()
    } else {
        translated
    }
}

pub(crate) fn render_translation_block(
    block: &RosettaBlock,
    translation: &str,
    output_format: &str,
) -> String {
    if output_format != "markdown" {
        return translation.to_string();
    }

    match block.block_type.as_str() {
        "heading" => format!("{} {translation}", style_marker(block)),
        "list_item" => format!("{} {translation}", style_marker(block)),
        "blockquote" => format!("> {translation}"),
        _ => translation.to_string(),
    }
}

pub(crate) fn render_bilingual_block(
    block: &RosettaBlock,
    translation: &str,
    output_format: &str,
) -> String {
    if output_format == "markdown" {
        return format!(
            "> Original: {}\n\n{}",
            block.source_text,
            render_translation_block(block, translation, output_format)
        );
    }

    format!(
        "Original:\n{}\n\nChinese:\n{}",
        block.source_text, translation
    )
}

pub(crate) fn style_marker(block: &RosettaBlock) -> String {
    block
        .style
        .as_ref()
        .and_then(|style| style.get("marker"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub(crate) fn segment_joiner(target_lang: &str) -> &'static str {
    if is_compact_language(target_lang) {
        ""
    } else {
        " "
    }
}

pub(crate) fn is_compact_language(target_lang: &str) -> bool {
    let normalized = target_lang.to_ascii_lowercase();
    normalized.starts_with("zh") || normalized.starts_with("ja") || normalized.starts_with("ko")
}

pub(crate) fn trim_excess_blank_blocks(blocks: Vec<String>) -> Vec<String> {
    let mut trimmed = Vec::new();
    let mut previous_blank = false;

    for block in blocks {
        let blank = block.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        previous_blank = blank;
        trimmed.push(block);
    }

    while trimmed.first().is_some_and(|block| block.trim().is_empty()) {
        trimmed.remove(0);
    }
    while trimmed.last().is_some_and(|block| block.trim().is_empty()) {
        trimmed.pop();
    }

    trimmed
}
