use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const SCHEMA_VERSION: u32 = 1;
const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_PROJECT_FILES: usize = 200;
const MAX_SEGMENT_CHARS: usize = 1_800;
const JOB_INDEX_FILENAME: &str = "index.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaDocument {
    schema_version: u32,
    id: String,
    filename: String,
    format: String,
    source_lang: Option<String>,
    target_lang: String,
    #[serde(default)]
    files: Vec<RosettaSourceFile>,
    blocks: Vec<RosettaBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaSourceFile {
    id: String,
    filename: String,
    relative_path: String,
    format: String,
    block_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaBlock {
    id: String,
    #[serde(default)]
    file_id: Option<String>,
    #[serde(rename = "type")]
    block_type: String,
    source_text: String,
    translated_text: Option<String>,
    should_translate: bool,
    order: usize,
    path: Option<String>,
    style: Option<Value>,
    status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    id: String,
    block_id: String,
    #[serde(default)]
    file_id: Option<String>,
    order: usize,
    source_text: String,
    translated_text: Option<String>,
    source_lang: Option<String>,
    target_lang: String,
    kind: String,
    preserve_whitespace: bool,
    status: String,
    block_order: Option<usize>,
    segment_index_in_block: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobSummary {
    schema_version: u32,
    id: String,
    filename: String,
    format: String,
    source_path: Option<String>,
    source_filename: String,
    #[serde(default = "default_source_kind")]
    source_kind: String,
    #[serde(default = "default_file_count")]
    file_count: usize,
    #[serde(default)]
    source_files: Vec<RosettaSourceFile>,
    status: String,
    created_at: String,
    updated_at: String,
    exported_at: Option<String>,
    last_error: Option<String>,
    target_lang: String,
    segment_count: usize,
    completed_segments: usize,
    failed_segments: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobBundle {
    schema_version: u32,
    job: RosettaJobSummary,
    document: RosettaDocument,
    segments: Vec<Segment>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaJobIndex {
    schema_version: u32,
    jobs: Vec<RosettaJobSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosettaExportResult {
    job: RosettaJobSummary,
    target_path: String,
    kind: String,
    bytes_written: u64,
    files_written: usize,
    message: String,
}

#[derive(Debug)]
struct SourceSnapshot {
    relative_path: String,
    contents: String,
}

fn default_source_kind() -> String {
    "file".to_string()
}

fn default_file_count() -> usize {
    1
}

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
pub async fn pick_rosetta_export_directory(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title("选择导出文件夹")
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
    import_project_from_directory(&app, Path::new(&path))
}

#[tauri::command]
pub fn export_rosetta_job_to_directory(
    app: AppHandle,
    job_id: String,
    kind: String,
    target_dir: String,
) -> Result<RosettaExportResult, String> {
    export_job_to_directory(&app, &job_id, &kind, Path::new(&target_dir))
}

#[tauri::command]
pub fn import_rosetta_document_from_path(
    app: AppHandle,
    path: String,
) -> Result<RosettaJobBundle, String> {
    import_document_from_path(&app, Path::new(&path))
}

#[tauri::command]
pub fn list_rosetta_jobs(app: AppHandle) -> Result<Vec<RosettaJobSummary>, String> {
    Ok(read_index(&jobs_root(&app)?)?.jobs)
}

#[tauri::command]
pub fn load_rosetta_job(app: AppHandle, job_id: String) -> Result<RosettaJobBundle, String> {
    load_job_bundle(&app, &job_id)
}

#[tauri::command]
pub fn save_rosetta_segments(
    app: AppHandle,
    job_id: String,
    segments: Vec<Segment>,
) -> Result<RosettaJobBundle, String> {
    save_segments(&app, &job_id, segments)
}

#[tauri::command]
pub fn update_rosetta_job_languages(
    app: AppHandle,
    job_id: String,
    source_lang: Option<String>,
    target_lang: String,
) -> Result<RosettaJobBundle, String> {
    update_job_languages(&app, &job_id, source_lang, target_lang)
}

#[tauri::command]
pub fn rename_rosetta_job(
    app: AppHandle,
    job_id: String,
    name: String,
) -> Result<Vec<RosettaJobSummary>, String> {
    rename_job(&app, &job_id, &name)
}

#[tauri::command]
pub fn delete_rosetta_job(
    app: AppHandle,
    job_id: String,
) -> Result<Vec<RosettaJobSummary>, String> {
    delete_job(&app, &job_id)
}

#[tauri::command]
pub fn export_rosetta_job(
    app: AppHandle,
    job_id: String,
    kind: String,
    target_path: String,
) -> Result<RosettaExportResult, String> {
    export_job(&app, &job_id, &kind, Path::new(&target_path))
}

fn import_document_from_path(
    app: &AppHandle,
    source_path: &Path,
) -> Result<RosettaJobBundle, String> {
    let metadata =
        fs::metadata(source_path).map_err(|error| format!("无法读取文件信息: {error}"))?;
    if !metadata.is_file() {
        return Err("只能导入 TXT 或 Markdown 文件。".to_string());
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err("文件超过 5 MB，当前原型暂不导入超大文件。".to_string());
    }

    let format = document_format(source_path)?;
    let source_contents = fs::read_to_string(source_path)
        .map_err(|error| format!("无法按 UTF-8 读取文件: {error}"))?;
    if source_contents.trim().is_empty() {
        return Err("文件没有可导入的文本内容。".to_string());
    }

    let now = timestamp_ms_string();
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("untitled")
        .to_string();
    let job_id = new_job_id(source_path);
    let document_id = format!("document-{job_id}");
    let (mut blocks, mut segments) = if format == "markdown" {
        parse_markdown(&document_id, &source_contents)
    } else {
        parse_txt(&document_id, &source_contents)
    };
    apply_file_id(&mut blocks, &mut segments, "file-1");

    if segments.is_empty() {
        return Err("文件没有可翻译的文本段落。".to_string());
    }
    let block_ids = blocks.iter().map(|block| block.id.clone()).collect();

    let document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: filename.clone(),
        format: format.clone(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files: vec![RosettaSourceFile {
            id: "file-1".to_string(),
            filename: filename.clone(),
            relative_path: filename.clone(),
            format: format.clone(),
            block_ids,
        }],
        blocks,
    };
    let source_files = document.files.clone();
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: job_id,
        filename: filename.clone(),
        format: format.clone(),
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
    sync_job_counts(&mut job, &segments);

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
    };
    write_job_bundle(app, &bundle, &source_contents)?;
    Ok(bundle)
}

fn import_project_from_directory(
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
        has_markdown = has_markdown || format == "markdown";
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
        let (mut file_blocks, mut file_segments) = if format == "markdown" {
            parse_markdown(&parser_document_id, &contents)
        } else {
            parse_txt(&parser_document_id, &contents)
        };

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
            format,
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
    let document = RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: document_id,
        filename: folder_name.clone(),
        format: document_format.clone(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files,
        blocks,
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
    sync_job_counts(&mut job, &segments);

    let bundle = RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
    };
    write_job_bundle_sources(app, &bundle, &source_snapshots)?;
    Ok(bundle)
}

fn parse_txt(document_id: &str, contents: &str) -> (Vec<RosettaBlock>, Vec<Segment>) {
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut block_order = 1;
    let mut segment_order = 1;

    for paragraph in split_txt_paragraphs(contents) {
        let block_id = format!("{document_id}-block-{block_order}");
        blocks.push(translatable_block(
            &block_id,
            "paragraph",
            &paragraph,
            block_order,
            None,
        ));
        push_segments_for_block(
            &mut segments,
            &block_id,
            "paragraph",
            block_order,
            &paragraph,
            &mut segment_order,
        );
        block_order += 1;
    }

    (blocks, segments)
}

fn apply_file_id(blocks: &mut [RosettaBlock], segments: &mut [Segment], file_id: &str) {
    for block in blocks {
        block.file_id = Some(file_id.to_string());
    }
    for segment in segments {
        segment.file_id = Some(file_id.to_string());
    }
}

fn renumber_blocks_and_segments(
    blocks: &mut [RosettaBlock],
    segments: &mut [Segment],
    next_block_order: &mut usize,
    next_segment_order: &mut usize,
) {
    let mut block_order_by_id = HashMap::new();
    for block in blocks {
        block.order = *next_block_order;
        block.path = Some(format!("blocks.{}", block.order));
        block_order_by_id.insert(block.id.clone(), block.order);
        *next_block_order += 1;
    }

    segments.sort_by_key(|segment| (segment.block_order.unwrap_or(0), segment.order));
    for segment in segments {
        segment.order = *next_segment_order;
        segment.block_order = block_order_by_id.get(&segment.block_id).copied();
        *next_segment_order += 1;
    }
}

fn parse_markdown(document_id: &str, contents: &str) -> (Vec<RosettaBlock>, Vec<Segment>) {
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut block_order = 1;
    let mut segment_order = 1;
    let mut paragraph_lines: Vec<String> = Vec::new();
    let mut code_lines: Vec<String> = Vec::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            code_lines.push(line.to_string());
            if in_code_block {
                push_skipped_markdown_block(
                    document_id,
                    &mut blocks,
                    &mut block_order,
                    "code",
                    &code_lines.join("\n"),
                    None,
                );
                code_lines.clear();
            }
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        if trimmed.is_empty() {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_skipped_markdown_block(
                document_id,
                &mut blocks,
                &mut block_order,
                "metadata",
                "",
                Some(json!({"markdownKind": "blank"})),
            );
            continue;
        }

        if let Some((marker, text)) = parse_heading(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "heading",
                text,
                json!({"marker": marker}),
            );
            continue;
        }

        if let Some((marker, text)) = parse_list_item(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "list_item",
                text,
                json!({"marker": marker}),
            );
            continue;
        }

        if let Some(text) = parse_blockquote(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "blockquote",
                text,
                json!({"marker": ">"}),
            );
            continue;
        }

        if is_plain_url(trimmed) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_skipped_markdown_block(
                document_id,
                &mut blocks,
                &mut block_order,
                "metadata",
                line,
                Some(json!({"markdownKind": "url"})),
            );
            continue;
        }

        paragraph_lines.push(line.to_string());
    }

    if in_code_block && !code_lines.is_empty() {
        push_skipped_markdown_block(
            document_id,
            &mut blocks,
            &mut block_order,
            "code",
            &code_lines.join("\n"),
            None,
        );
    }

    flush_markdown_paragraph(
        document_id,
        &mut blocks,
        &mut segments,
        &mut block_order,
        &mut segment_order,
        &mut paragraph_lines,
    );

    (blocks, segments)
}

fn flush_markdown_paragraph(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    segments: &mut Vec<Segment>,
    block_order: &mut usize,
    segment_order: &mut usize,
    paragraph_lines: &mut Vec<String>,
) {
    if paragraph_lines.is_empty() {
        return;
    }

    let text = paragraph_lines.join("\n");
    paragraph_lines.clear();
    push_markdown_translatable(
        document_id,
        blocks,
        segments,
        block_order,
        segment_order,
        "paragraph",
        &text,
        json!({"markdownKind": "paragraph"}),
    );
}

fn push_markdown_translatable(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    segments: &mut Vec<Segment>,
    block_order: &mut usize,
    segment_order: &mut usize,
    block_type: &str,
    text: &str,
    style: Value,
) {
    let block_id = format!("{document_id}-block-{block_order}");
    blocks.push(translatable_block(
        &block_id,
        block_type,
        text,
        *block_order,
        Some(style),
    ));
    push_segments_for_block(
        segments,
        &block_id,
        block_type,
        *block_order,
        text,
        segment_order,
    );
    *block_order += 1;
}

fn push_skipped_markdown_block(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    block_order: &mut usize,
    block_type: &str,
    source_text: &str,
    style: Option<Value>,
) {
    let block_id = format!("{document_id}-block-{block_order}");
    blocks.push(RosettaBlock {
        id: block_id,
        file_id: None,
        block_type: block_type.to_string(),
        source_text: source_text.to_string(),
        translated_text: None,
        should_translate: false,
        order: *block_order,
        path: Some(format!("blocks.{}", *block_order)),
        style,
        status: "skipped".to_string(),
    });
    *block_order += 1;
}

fn translatable_block(
    id: &str,
    block_type: &str,
    source_text: &str,
    order: usize,
    style: Option<Value>,
) -> RosettaBlock {
    RosettaBlock {
        id: id.to_string(),
        file_id: None,
        block_type: block_type.to_string(),
        source_text: source_text.to_string(),
        translated_text: None,
        should_translate: true,
        order,
        path: Some(format!("blocks.{order}")),
        style,
        status: "pending".to_string(),
    }
}

fn push_segments_for_block(
    segments: &mut Vec<Segment>,
    block_id: &str,
    kind: &str,
    block_order: usize,
    source_text: &str,
    segment_order: &mut usize,
) {
    for (index, chunk) in split_long_text(source_text).into_iter().enumerate() {
        segments.push(Segment {
            id: format!("{block_id}-segment-{}", index + 1),
            block_id: block_id.to_string(),
            file_id: None,
            order: *segment_order,
            source_text: chunk,
            translated_text: None,
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            kind: kind.to_string(),
            preserve_whitespace: true,
            status: "pending".to_string(),
            block_order: Some(block_order),
            segment_index_in_block: Some(index),
            error: None,
        });
        *segment_order += 1;
    }
}

fn split_txt_paragraphs(contents: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();

    for line in contents.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join("\n").trim().to_string());
                current.clear();
            }
        } else {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        paragraphs.push(current.join("\n").trim().to_string());
    }

    paragraphs
}

fn split_long_text(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_SEGMENT_CHARS {
        return vec![text.trim().to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in split_sentence_like(text) {
        let next_len = current.chars().count() + sentence.chars().count();
        if !current.is_empty() && next_len > MAX_SEGMENT_CHARS {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sentence.trim());
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks
    }
}

fn split_sentence_like(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;

    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '?' | '!' | ';' | '。' | '？' | '！' | '；') {
            let end = index + character.len_utf8();
            if start < end {
                sentences.push(&text[start..end]);
            }
            start = end;
        }
    }

    if start < text.len() {
        sentences.push(&text[start..]);
    }

    sentences
}

fn parse_heading(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some(("#".repeat(hashes), rest.trim()))
}

fn parse_list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = trimmed.strip_prefix(marker) {
            return Some((marker.trim_end().to_string(), text.trim()));
        }
    }

    let dot_index = trimmed.find(". ")?;
    if trimmed[..dot_index]
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return Some((
            trimmed[..=dot_index].to_string(),
            trimmed[dot_index + 2..].trim(),
        ));
    }

    None
}

fn parse_blockquote(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix('>').map(str::trim)
}

fn is_plain_url(text: &str) -> bool {
    (text.starts_with("http://") || text.starts_with("https://"))
        && !text.chars().any(char::is_whitespace)
}

fn document_format(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "txt" => Ok("txt".to_string()),
        "md" | "markdown" => Ok("markdown".to_string()),
        _ => Err("当前只支持导入 .txt、.md、.markdown 文件。".to_string()),
    }
}

fn collect_supported_source_paths(
    root: &Path,
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|error| format!("无法读取文件夹 {}: {error}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取文件夹条目: {error}"))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法读取文件类型 {}: {error}", path.display()))?;
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            collect_supported_source_paths(root, &path, output)?;
            continue;
        }

        if file_type.is_file() && document_format(&path).is_ok() {
            relative_path_string(root, &path)?;
            output.push(path);
            if output.len() > MAX_PROJECT_FILES {
                return Err(format!(
                    "这个文件夹包含超过 {MAX_PROJECT_FILES} 个可导入文件，请先拆分项目。"
                ));
            }
        }
    }

    Ok(())
}

fn relative_path_string(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "文件路径不在所选文件夹内。".to_string())?;
    let mut parts = Vec::new();

    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err("文件夹里包含不安全的相对路径。".to_string());
        };
        let Some(part) = part.to_str() else {
            return Err("文件路径包含无法识别的字符。".to_string());
        };
        parts.push(part.to_string());
    }

    if parts.is_empty() {
        return Err("文件路径为空。".to_string());
    }

    Ok(parts.join("/"))
}

fn path_from_relative(relative_path: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in relative_path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err("文件相对路径不安全。".to_string());
        }
        path.push(part);
    }
    Ok(path)
}

fn write_job_bundle(
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

fn write_job_bundle_sources(
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
    upsert_index_job(&root, bundle.job.clone())
}

fn save_segments(
    app: &AppHandle,
    job_id: &str,
    segments: Vec<Segment>,
) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目索引不存在，无法保存翻译结果。".to_string())?;

    apply_segment_translations_to_document(&mut document, &segments);
    sync_job_counts(&mut job, &segments);
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
    })
}

fn update_job_languages(
    app: &AppHandle,
    job_id: &str,
    source_lang: Option<String>,
    target_lang: String,
) -> Result<RosettaJobBundle, String> {
    let normalized_source_lang = normalize_optional_lang(source_lang);
    let normalized_target_lang = normalize_required_lang(target_lang)?;
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut document: RosettaDocument = read_json(&dir.join("document.json"))?;
    let mut segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目索引不存在，无法保存语言设置。".to_string())?;

    let changed = document.source_lang != normalized_source_lang
        || document.target_lang != normalized_target_lang;
    document.source_lang = normalized_source_lang.clone();
    document.target_lang = normalized_target_lang.clone();
    job.target_lang = normalized_target_lang.clone();

    for segment in &mut segments {
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
            if block.should_translate {
                block.translated_text = None;
                block.status = "pending".to_string();
            }
        }
    }

    sync_job_counts(&mut job, &segments);
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
    })
}

fn rename_job(app: &AppHandle, job_id: &str, name: &str) -> Result<Vec<RosettaJobSummary>, String> {
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

fn normalize_optional_lang(language: Option<String>) -> Option<String> {
    language
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "auto")
}

fn normalize_required_lang(language: String) -> Result<String, String> {
    let normalized = language.trim().to_string();
    if normalized.is_empty() || normalized == "auto" {
        return Err("请选择目标语言。".to_string());
    }
    Ok(normalized)
}

fn load_job_bundle(app: &AppHandle, job_id: &str) -> Result<RosettaJobBundle, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let index = read_index(&root)?;
    let job = index
        .jobs
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| "项目不存在。".to_string())?;
    let document = read_json(&dir.join("document.json"))?;
    let segments = read_json(&dir.join("segments.json"))?;

    Ok(RosettaJobBundle {
        schema_version: SCHEMA_VERSION,
        job,
        document,
        segments,
    })
}

fn delete_job(app: &AppHandle, job_id: &str) -> Result<Vec<RosettaJobSummary>, String> {
    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;

    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| format!("无法删除项目缓存: {error}"))?;
    }

    let mut index = read_index(&root)?;
    index.jobs.retain(|job| job.id != job_id);
    write_index(&root, &index)?;
    Ok(index.jobs)
}

fn export_job(
    app: &AppHandle,
    job_id: &str,
    kind: &str,
    target_path: &Path,
) -> Result<RosettaExportResult, String> {
    if kind != "translation" && kind != "bilingual" {
        return Err("导出类型必须是 translation 或 bilingual。".to_string());
    }

    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法导出。".to_string())?;
    let document: RosettaDocument = read_json(&dir.join("document.json"))?;
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let output = render_export(&document, &segments, kind);

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建导出目录: {error}"))?;
    }
    fs::write(target_path, output.as_bytes())
        .map_err(|error| format!("无法写入导出文件: {error}"))?;

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

fn render_export(document: &RosettaDocument, segments: &[Segment], kind: &str) -> String {
    render_export_blocks(document, &document.blocks, segments, kind)
}

fn render_export_blocks(
    document: &RosettaDocument,
    blocks: &[RosettaBlock],
    segments: &[Segment],
    kind: &str,
) -> String {
    let by_block = segments_by_block(segments);
    if document.format == "markdown" {
        return render_markdown_export_blocks(document, blocks, &by_block, kind);
    }

    let output_blocks = blocks
        .iter()
        .map(|block| render_export_block(document, block, &by_block, kind))
        .collect::<Vec<_>>();
    trim_excess_blank_blocks(output_blocks).join("\n\n")
}

fn render_markdown_export_blocks(
    document: &RosettaDocument,
    blocks: &[RosettaBlock],
    by_block: &HashMap<String, Vec<Segment>>,
    kind: &str,
) -> String {
    let mut output = String::new();
    let mut previous_type: Option<&str> = None;

    for block in blocks {
        let rendered = render_export_block(document, block, by_block, kind);
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
) -> String {
    if !block.should_translate {
        return block.source_text.clone();
    }

    let translation = block_translation(block, by_block);
    if kind == "bilingual" {
        render_bilingual_block(document, block, &translation)
    } else {
        render_translation_block(document, block, &translation)
    }
}

fn export_job_to_directory(
    app: &AppHandle,
    job_id: &str,
    kind: &str,
    target_dir: &Path,
) -> Result<RosettaExportResult, String> {
    if kind != "translation" && kind != "bilingual" {
        return Err("导出类型必须是 translation 或 bilingual。".to_string());
    }

    let root = jobs_root(app)?;
    let dir = checked_job_dir(&root, job_id)?;
    let mut index = read_index(&root)?;
    let mut job = index
        .jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "项目不存在，无法导出。".to_string())?;
    let document: RosettaDocument = read_json(&dir.join("document.json"))?;
    let segments: Vec<Segment> = read_json(&dir.join("segments.json"))?;
    let mut bytes_written = 0_u64;
    let mut files_written = 0_usize;

    fs::create_dir_all(target_dir).map_err(|error| format!("无法创建导出目录: {error}"))?;

    for source_file in document_files(&document) {
        let file_blocks = document
            .blocks
            .iter()
            .filter(|block| block.file_id.as_deref().unwrap_or("file-1") == source_file.id.as_str())
            .cloned()
            .collect::<Vec<_>>();
        let file_segments = segments
            .iter()
            .filter(|segment| {
                segment.file_id.as_deref().unwrap_or("file-1") == source_file.id.as_str()
            })
            .cloned()
            .collect::<Vec<_>>();
        let output = render_export_blocks(&document, &file_blocks, &file_segments, kind);
        let relative_output_path = export_relative_path(&source_file, kind)?;
        let target_path = target_dir.join(relative_output_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建导出目录: {error}"))?;
        }
        fs::write(&target_path, output.as_bytes())
            .map_err(|error| format!("无法写入导出文件 {}: {error}", target_path.display()))?;
        bytes_written += output.len() as u64;
        files_written += 1;
    }

    job.exported_at = Some(timestamp_ms_string());
    job.updated_at = timestamp_ms_string();
    replace_index_job(&mut index, job.clone());
    write_index(&root, &index)?;

    Ok(RosettaExportResult {
        job,
        target_path: target_dir.to_string_lossy().to_string(),
        kind: kind.to_string(),
        bytes_written,
        files_written,
        message: format!("导出完成，写入 {files_written} 个文件。"),
    })
}

fn document_files(document: &RosettaDocument) -> Vec<RosettaSourceFile> {
    if !document.files.is_empty() {
        return document.files.clone();
    }

    vec![RosettaSourceFile {
        id: "file-1".to_string(),
        filename: document.filename.clone(),
        relative_path: document.filename.clone(),
        format: document.format.clone(),
        block_ids: document
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect(),
    }]
}

fn export_relative_path(file: &RosettaSourceFile, kind: &str) -> Result<PathBuf, String> {
    let input_path = path_from_relative(&file.relative_path)?;
    let extension = if file.format == "markdown" {
        "md"
    } else {
        "txt"
    };
    let suffix = if kind == "bilingual" {
        "bilingual"
    } else {
        "zh"
    };
    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document");
    let filename = format!("{stem}.{suffix}.{extension}");
    Ok(input_path.with_file_name(filename))
}

fn segments_by_block(segments: &[Segment]) -> HashMap<String, Vec<Segment>> {
    let mut grouped: HashMap<String, Vec<Segment>> = HashMap::new();
    for segment in segments {
        grouped
            .entry(segment.block_id.clone())
            .or_default()
            .push(segment.clone());
    }
    for grouped_segments in grouped.values_mut() {
        grouped_segments.sort_by_key(|segment| segment.segment_index_in_block.unwrap_or(0));
    }
    grouped
}

fn block_translation(block: &RosettaBlock, by_block: &HashMap<String, Vec<Segment>>) -> String {
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
        .join(" ");

    if translated.trim().is_empty() {
        block.source_text.clone()
    } else {
        translated
    }
}

fn render_translation_block(
    document: &RosettaDocument,
    block: &RosettaBlock,
    translation: &str,
) -> String {
    if document.format != "markdown" {
        return translation.to_string();
    }

    match block.block_type.as_str() {
        "heading" => format!("{} {translation}", style_marker(block)),
        "list_item" => format!("{} {translation}", style_marker(block)),
        "blockquote" => format!("> {translation}"),
        _ => translation.to_string(),
    }
}

fn render_bilingual_block(
    document: &RosettaDocument,
    block: &RosettaBlock,
    translation: &str,
) -> String {
    if document.format == "markdown" {
        return format!(
            "> Original: {}\n\n{}",
            block.source_text,
            render_translation_block(document, block, translation)
        );
    }

    format!(
        "Original:\n{}\n\nChinese:\n{}",
        block.source_text, translation
    )
}

fn style_marker(block: &RosettaBlock) -> String {
    block
        .style
        .as_ref()
        .and_then(|style| style.get("marker"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn trim_excess_blank_blocks(blocks: Vec<String>) -> Vec<String> {
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

fn apply_segment_translations_to_document(document: &mut RosettaDocument, segments: &[Segment]) {
    let by_block = segments_by_block(segments);
    for block in &mut document.blocks {
        if !block.should_translate {
            continue;
        }
        block.translated_text = Some(block_translation(block, &by_block));
        block.status = block_status(block, &by_block);
    }
}

fn block_status(block: &RosettaBlock, by_block: &HashMap<String, Vec<Segment>>) -> String {
    let Some(segments) = by_block.get(&block.id) else {
        return "pending".to_string();
    };
    if segments.iter().any(|segment| segment.status == "failed") {
        "failed".to_string()
    } else if segments.iter().all(|segment| segment.status == "done") {
        "done".to_string()
    } else if segments
        .iter()
        .any(|segment| segment.status == "translating")
    {
        "translating".to_string()
    } else {
        "pending".to_string()
    }
}

fn sync_job_counts(job: &mut RosettaJobSummary, segments: &[Segment]) {
    job.segment_count = segments.len();
    job.completed_segments = segments
        .iter()
        .filter(|segment| matches!(segment.status.as_str(), "done" | "edited" | "skipped"))
        .count();
    job.failed_segments = segments
        .iter()
        .filter(|segment| segment.status == "failed")
        .count();
    job.status = if segments
        .iter()
        .any(|segment| segment.status == "translating")
    {
        "translating".to_string()
    } else if job.failed_segments > 0 {
        "failed".to_string()
    } else if job.completed_segments == job.segment_count {
        "completed".to_string()
    } else {
        "ready".to_string()
    };
}

fn jobs_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法读取 Rosetta app data 目录: {error}"))?
        .join("jobs");
    fs::create_dir_all(&root).map_err(|error| format!("无法创建 jobs 目录: {error}"))?;
    Ok(root)
}

fn checked_job_dir(root: &Path, job_id: &str) -> Result<PathBuf, String> {
    if !is_safe_job_id(job_id) {
        return Err("项目 id 不安全。".to_string());
    }
    let dir = root.join(job_id);
    if !dir.starts_with(root) {
        return Err("项目路径越界。".to_string());
    }
    Ok(dir)
}

fn is_safe_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

fn read_index(root: &Path) -> Result<RosettaJobIndex, String> {
    let path = root.join(JOB_INDEX_FILENAME);
    if !path.exists() {
        return Ok(RosettaJobIndex {
            schema_version: SCHEMA_VERSION,
            jobs: Vec::new(),
        });
    }
    read_json(&path)
}

fn write_index(root: &Path, index: &RosettaJobIndex) -> Result<(), String> {
    write_json(&root.join(JOB_INDEX_FILENAME), index)
}

fn upsert_index_job(root: &Path, job: RosettaJobSummary) -> Result<(), String> {
    let mut index = read_index(root)?;
    replace_index_job(&mut index, job);
    write_index(root, &index)
}

fn replace_index_job(index: &mut RosettaJobIndex, job: RosettaJobSummary) {
    index.jobs.retain(|existing| existing.id != job.id);
    index.jobs.push(job);
    index
        .jobs
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 JSON 文件 {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("JSON 文件格式错误 {}: {error}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let contents =
        serde_json::to_string_pretty(value).map_err(|error| format!("无法序列化 JSON: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("无法写入 JSON 文件 {}: {error}", path.display()))
}

fn new_job_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let stem = if stem.is_empty() {
        "document".to_string()
    } else {
        stem
    };
    format!("job-{}-{stem}", timestamp_ms_string())
}

fn timestamp_ms_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_parser_splits_blank_line_paragraphs() {
        let (_document, segments) = parse_txt("doc", "One.\n\nTwo.\nStill two.");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source_text, "One.");
        assert_eq!(segments[1].source_text, "Two.\nStill two.");
        assert_eq!(segments[0].order, 1);
        assert_eq!(segments[1].order, 2);
    }

    #[test]
    fn markdown_parser_recognizes_basic_blocks_and_skips_code() {
        let (blocks, segments) = parse_markdown(
            "doc",
            "# Title\n\nParagraph.\n\n- Item\n\n```rust\nfn main() {}\n```",
        );

        assert!(blocks.iter().any(|block| block.block_type == "heading"));
        assert!(blocks.iter().any(|block| block.block_type == "list_item"));
        assert!(blocks
            .iter()
            .any(|block| block.block_type == "code" && block.status == "skipped"));
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn long_segmenter_splits_sentence_like_text() {
        let text = format!("{}.", "a".repeat(MAX_SEGMENT_CHARS));
        let text = format!("{text} {}.", "b".repeat(50));
        let chunks = split_long_text(&text);

        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn export_translation_uses_original_for_untranslated_segments() {
        let (document, segments) = parse_txt("doc", "Hello.");
        let output = render_export(
            &RosettaDocument {
                schema_version: SCHEMA_VERSION,
                id: "doc".to_string(),
                filename: "demo.txt".to_string(),
                format: "txt".to_string(),
                source_lang: Some("en".to_string()),
                target_lang: "zh-CN".to_string(),
                files: Vec::new(),
                blocks: document,
            },
            &segments,
            "translation",
        );

        assert_eq!(output, "Hello.");
    }

    #[test]
    fn export_markdown_preserves_heading_marker() {
        let (document, mut segments) = parse_markdown("doc", "# Title");
        segments[0].translated_text = Some("标题".to_string());
        segments[0].status = "done".to_string();
        let output = render_export(
            &RosettaDocument {
                schema_version: SCHEMA_VERSION,
                id: "doc".to_string(),
                filename: "demo.md".to_string(),
                format: "markdown".to_string(),
                source_lang: Some("en".to_string()),
                target_lang: "zh-CN".to_string(),
                files: Vec::new(),
                blocks: document,
            },
            &segments,
            "translation",
        );

        assert_eq!(output, "# 标题");
    }

    #[test]
    fn path_safety_rejects_traversal_job_id() {
        assert!(is_safe_job_id("job-123_demo"));
        assert!(!is_safe_job_id("../job"));
        assert!(!is_safe_job_id("job/123"));
    }
}
