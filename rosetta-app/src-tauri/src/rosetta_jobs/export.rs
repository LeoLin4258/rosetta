use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::rosetta_jobs::{
    document::{
        document_files, ensure_document_files, segments_by_block, sync_document_file_statuses,
        sync_job_counts, sync_job_source_files,
    },
    formats::pdf_markdown::{
        render::{
            asset_path as pdf_markdown_asset_path,
            declared_asset_path as declared_pdf_markdown_asset_path, is_pdf_markdown_block,
            join_blocks as join_pdf_markdown_blocks, render_blocks as render_pdf_markdown_blocks,
        },
        resolve_preview_asset as resolve_pdf_markdown_asset, MAX_IMAGE_BYTES,
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

#[derive(Debug)]
struct PdfMarkdownExportWrite {
    bytes_written: u64,
    files_written: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PdfMarkdownExportFault {
    None,
    #[cfg(test)]
    AfterMarkdownCommit,
}

struct StagedPdfMarkdownAsset {
    filename: String,
    source_path: PathBuf,
    digest: [u8; 32],
    bytes_len: u64,
}

fn write_pdf_markdown_export(
    job_dir: &Path,
    target_path: &Path,
    blocks: &[RosettaBlock],
    rendered_markdown: &str,
    output_format: &str,
) -> Result<PdfMarkdownExportWrite, String> {
    write_pdf_markdown_export_with_fault(
        job_dir,
        target_path,
        blocks,
        rendered_markdown,
        output_format,
        PdfMarkdownExportFault::None,
    )
}

fn write_pdf_markdown_export_with_fault(
    job_dir: &Path,
    target_path: &Path,
    blocks: &[RosettaBlock],
    rendered_markdown: &str,
    output_format: &str,
    fault: PdfMarkdownExportFault,
) -> Result<PdfMarkdownExportWrite, String> {
    if output_format != "markdown" {
        return Err("PDF Markdown 导出身份无效。".to_string());
    }
    let parent = target_path
        .parent()
        .ok_or_else(|| "导出路径无效。".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "无法创建导出目录。".to_string())?;
    if target_path.exists() && !target_path.is_file() {
        return Err("Markdown 导出目标不是文件。".to_string());
    }
    let assets_path = pdf_markdown_assets_path(target_path)?;
    if assets_path.exists() && !assets_path.is_dir() {
        return Err("Markdown 资源导出目标不是目录。".to_string());
    }

    let mut assets = Vec::<StagedPdfMarkdownAsset>::new();
    let mut canonical_by_hash = HashMap::<[u8; 32], String>::new();
    let mut exported_by_source = HashMap::<String, String>::new();
    let mut rewrites = Vec::<(String, String)>::new();
    for block in blocks {
        let Some(declared_path) = declared_pdf_markdown_asset_path(block) else {
            continue;
        };
        let validated_path = pdf_markdown_asset_path(block)
            .ok_or_else(|| "PDF Markdown 资源路径无效。".to_string())?;
        if exported_by_source.contains_key(validated_path) {
            continue;
        }
        let source_path = resolve_pdf_markdown_asset(job_dir, validated_path)
            .map_err(|_| "PDF Markdown 资源不可用。".to_string())?;
        let (digest, bytes_len) = hash_pdf_markdown_asset(&source_path)?;
        let logical_filename = Path::new(validated_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "PDF Markdown 资源路径无效。".to_string())?
            .to_string();
        let exported_filename = if let Some(existing) = canonical_by_hash.get(&digest) {
            existing.clone()
        } else {
            canonical_by_hash.insert(digest, logical_filename.clone());
            assets.push(StagedPdfMarkdownAsset {
                filename: logical_filename.clone(),
                source_path,
                digest,
                bytes_len,
            });
            logical_filename
        };
        exported_by_source.insert(validated_path.to_string(), exported_filename.clone());
        rewrites.push((declared_path.to_string(), exported_filename));
    }

    let assets_dir_name = assets_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Markdown 资源目录名无效。".to_string())?;
    let encoded_assets_dir = encode_markdown_path_component(assets_dir_name);
    let mut markdown = rendered_markdown.to_string();
    for (source, filename) in &rewrites {
        let needle = format!("]({source})");
        if !markdown.contains(&needle) {
            return Err("Markdown 资源链接与渲染结果不一致。".to_string());
        }
        let destination = format!(
            "]({}/{})",
            encoded_assets_dir,
            encode_markdown_path_component(filename)
        );
        markdown = markdown.replace(&needle, &destination);
    }
    if markdown.contains("](pdf-markdown/images/") {
        return Err("Markdown 资源链接未完整重写。".to_string());
    }

    let filename = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("translation.md");
    let nonce = export_nonce();
    let staged_markdown = parent.join(format!(".{filename}.{nonce}.markdown.tmp"));
    let staged_assets = parent.join(format!(".{filename}.{nonce}.assets.tmp"));
    let markdown_backup = parent.join(format!(".{filename}.{nonce}.markdown.bak"));
    let assets_backup = parent.join(format!(".{filename}.{nonce}.assets.bak"));

    let staged_result = (|| -> Result<(), String> {
        fs::create_dir(&staged_assets)
            .map_err(|_| "无法创建临时 Markdown 资源目录。".to_string())?;
        for asset in &assets {
            copy_pdf_markdown_asset_flushed(
                &asset.source_path,
                &staged_assets.join(&asset.filename),
                asset.digest,
                asset.bytes_len,
            )?;
        }
        write_file_flushed(
            &staged_markdown,
            markdown.as_bytes(),
            "无法写入临时 Markdown 文件。",
        )?;
        verify_staged_pdf_markdown_assets(&staged_assets, &rewrites)?;
        Ok(())
    })();
    if let Err(error) = staged_result {
        remove_export_path(&staged_markdown);
        remove_export_path(&staged_assets);
        return Err(error);
    }

    let commit_result = commit_pdf_markdown_export(
        target_path,
        &assets_path,
        &staged_markdown,
        &staged_assets,
        &markdown_backup,
        &assets_backup,
        fault,
    );
    remove_export_path(&staged_markdown);
    remove_export_path(&staged_assets);
    remove_export_path(&markdown_backup);
    remove_export_path(&assets_backup);
    commit_result?;

    let asset_bytes = assets.iter().try_fold(0u64, |total, asset| {
        total
            .checked_add(asset.bytes_len)
            .ok_or_else(|| "Markdown 导出资源总大小超出限制。".to_string())
    })?;
    Ok(PdfMarkdownExportWrite {
        bytes_written: markdown.len() as u64 + asset_bytes,
        files_written: 1 + assets.len(),
    })
}

fn pdf_markdown_assets_path(target_path: &Path) -> Result<PathBuf, String> {
    let parent = target_path
        .parent()
        .ok_or_else(|| "导出路径无效。".to_string())?;
    let stem = target_path
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| "Markdown 导出文件名无效。".to_string())?;
    let mut name = OsString::from(stem);
    name.push(".assets");
    Ok(parent.join(name))
}

fn encode_markdown_path_component(component: &str) -> String {
    let mut encoded = String::with_capacity(component.len());
    for byte in component.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn export_nonce() -> u128 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    ((std::process::id() as u128) << 96) ^ timestamp
}

fn write_file_flushed(path: &Path, bytes: &[u8], error: &str) -> Result<(), String> {
    let mut file = File::create(path).map_err(|_| error.to_string())?;
    file.write_all(bytes).map_err(|_| error.to_string())?;
    file.sync_all().map_err(|_| error.to_string())
}

fn hash_pdf_markdown_asset(path: &Path) -> Result<([u8; 32], u64), String> {
    let mut source = File::open(path).map_err(|_| "PDF Markdown 资源不可读。".to_string())?;
    let mut digest = Sha256::new();
    let mut bytes_len = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|_| "PDF Markdown 资源不可读。".to_string())?;
        if read == 0 {
            break;
        }
        bytes_len = bytes_len
            .checked_add(read as u64)
            .filter(|size| *size <= MAX_IMAGE_BYTES)
            .ok_or_else(|| "PDF Markdown 资源不可用。".to_string())?;
        digest.update(&buffer[..read]);
    }
    Ok((digest.finalize().into(), bytes_len))
}

fn copy_pdf_markdown_asset_flushed(
    source_path: &Path,
    target_path: &Path,
    expected_digest: [u8; 32],
    expected_len: u64,
) -> Result<(), String> {
    let mut source =
        File::open(source_path).map_err(|_| "PDF Markdown 资源不可读。".to_string())?;
    let mut target =
        File::create(target_path).map_err(|_| "无法写入临时 Markdown 资源。".to_string())?;
    let mut digest = Sha256::new();
    let mut bytes_len = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|_| "PDF Markdown 资源不可读。".to_string())?;
        if read == 0 {
            break;
        }
        bytes_len = bytes_len
            .checked_add(read as u64)
            .filter(|size| *size <= MAX_IMAGE_BYTES)
            .ok_or_else(|| "PDF Markdown 资源不可用。".to_string())?;
        target
            .write_all(&buffer[..read])
            .map_err(|_| "无法写入临时 Markdown 资源。".to_string())?;
        digest.update(&buffer[..read]);
    }
    target
        .sync_all()
        .map_err(|_| "无法刷写临时 Markdown 资源。".to_string())?;
    let actual_digest: [u8; 32] = digest.finalize().into();
    if bytes_len != expected_len || actual_digest != expected_digest {
        return Err("PDF Markdown 资源在导出期间发生变化。".to_string());
    }
    Ok(())
}

fn verify_staged_pdf_markdown_assets(
    staged_assets: &Path,
    rewrites: &[(String, String)],
) -> Result<(), String> {
    let canonical_root = staged_assets
        .canonicalize()
        .map_err(|_| "无法验证临时 Markdown 资源目录。".to_string())?;
    for (_, filename) in rewrites {
        let candidate = staged_assets.join(filename);
        let canonical = candidate
            .canonicalize()
            .map_err(|_| "临时 Markdown 资源不完整。".to_string())?;
        if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
            return Err("临时 Markdown 资源路径无效。".to_string());
        }
    }
    Ok(())
}

fn commit_pdf_markdown_export(
    target_markdown: &Path,
    target_assets: &Path,
    staged_markdown: &Path,
    staged_assets: &Path,
    markdown_backup: &Path,
    assets_backup: &Path,
    fault: PdfMarkdownExportFault,
) -> Result<(), String> {
    let had_markdown = target_markdown.is_file();
    let had_assets = target_assets.is_dir();

    if had_markdown {
        fs::rename(target_markdown, markdown_backup)
            .map_err(|_| "无法暂存已有 Markdown 导出。".to_string())?;
    }
    if had_assets {
        if fs::rename(target_assets, assets_backup).is_err() {
            if had_markdown {
                let _ = fs::rename(markdown_backup, target_markdown);
            }
            return Err("无法暂存已有 Markdown 资源。".to_string());
        }
    }

    let commit_result = (|| -> Result<(), String> {
        fs::rename(staged_markdown, target_markdown)
            .map_err(|_| "无法提交 Markdown 导出。".to_string())?;
        #[cfg(test)]
        if fault == PdfMarkdownExportFault::AfterMarkdownCommit {
            return Err("injected-pdf-markdown-export-failure".to_string());
        }
        let _ = fault;
        fs::rename(staged_assets, target_assets)
            .map_err(|_| "无法提交 Markdown 资源。".to_string())?;
        Ok(())
    })();
    if let Err(error) = commit_result {
        remove_export_path(target_markdown);
        remove_export_path(target_assets);
        if had_markdown {
            let _ = fs::rename(markdown_backup, target_markdown);
        }
        if had_assets {
            let _ = fs::rename(assets_backup, target_assets);
        }
        return Err(error);
    }
    Ok(())
}

fn remove_export_path(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
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
    let is_pdf_markdown = source_file.format.eq_ignore_ascii_case("pdf")
        && translation_file
            .output_format
            .eq_ignore_ascii_case("markdown");
    if is_pdf_markdown && translation_file.output_format != "markdown" {
        return Err("PDF Markdown 导出身份无效。".to_string());
    }
    if is_pdf_markdown && kind == RosettaExportKind::Bilingual {
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

    let pdf_markdown_write = if is_pdf_markdown {
        Some(write_pdf_markdown_export(
            &dir,
            target_path,
            &file_blocks,
            &output,
            &translation_file.output_format,
        )?)
    } else {
        write_export_atomically(target_path, output.as_bytes())?;
        None
    };

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
        bytes_written: pdf_markdown_write
            .as_ref()
            .map(|write| write.bytes_written)
            .unwrap_or(output.len() as u64),
        files_written: pdf_markdown_write
            .as_ref()
            .map(|write| write.files_written)
            .unwrap_or(1),
        message: if pdf_markdown_write.is_some() {
            "Markdown 及资源已导出。".to_string()
        } else {
            "导出完成。".to_string()
        },
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

#[cfg(test)]
mod pdf_markdown_export_tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rosetta-{name}-{}", export_nonce()))
    }

    fn asset_block(id: &str, class: &str, asset: &str) -> RosettaBlock {
        RosettaBlock {
            id: id.to_string(),
            file_id: Some("file-1".to_string()),
            block_type: if class == "formula" {
                "code"
            } else {
                "metadata"
            }
            .to_string(),
            source_text: String::new(),
            translated_text: None,
            should_translate: false,
            order: 1,
            path: None,
            style: Some(json!({
                "pdfMarkdown": {
                    "version": 1,
                    "page": 1,
                    "boxClass": class,
                    "bbox": [0, 0, 10, 10],
                    "extra": {"assetPath": asset}
                }
            })),
            status: "skipped".to_string(),
        }
    }

    fn write_job_asset(job_dir: &Path, filename: &str, bytes: &[u8]) {
        let images = job_dir.join("pdf-markdown").join("images");
        fs::create_dir_all(&images).expect("create image root");
        fs::write(images.join(filename), bytes).expect("write image");
    }

    #[test]
    fn pdf_markdown_export_requires_exact_output_identity() {
        let root = temp_dir("pdf-markdown-export-identity");
        let target = root.join("document.md");
        let error = write_pdf_markdown_export(&root, &target, &[], "text", "Markdown")
            .expect_err("mixed-case identity must be rejected");

        assert_eq!(error, "PDF Markdown 导出身份无效。");
        assert!(!target.exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pdf_markdown_export_rewrites_link_and_reports_aggregate_counts() {
        let root = temp_dir("pdf-markdown-export-one");
        let job_dir = root.join("job");
        let export_dir = root.join("exports");
        write_job_asset(&job_dir, "page-0001-picture-01.png", b"image-one");
        let source = "pdf-markdown/images/page-0001-picture-01.png";
        let blocks = vec![asset_block("picture-1", "picture", source)];
        let markdown = format!("# Title\n\n![Figure]({source})");
        let target = export_dir.join("translated document.zh-CN.md");

        let result = write_pdf_markdown_export(&job_dir, &target, &blocks, &markdown, "markdown")
            .expect("export markdown assets");

        let output = fs::read_to_string(&target).expect("read markdown");
        let expected =
            "# Title\n\n![Figure](translated%20document.zh-CN.assets/page-0001-picture-01.png)";
        assert_eq!(output, expected);
        assert_eq!(result.files_written, 2);
        assert_eq!(result.bytes_written, expected.len() as u64 + 9);
        assert_eq!(
            fs::read(
                export_dir
                    .join("translated document.zh-CN.assets")
                    .join("page-0001-picture-01.png")
            )
            .expect("read exported image"),
            b"image-one"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pdf_markdown_export_deduplicates_assets_and_replaces_stale_directory() {
        let root = temp_dir("pdf-markdown-export-dedupe");
        let job_dir = root.join("job");
        write_job_asset(&job_dir, "page-0001-picture-01.png", b"same-image");
        write_job_asset(&job_dir, "page-0002-picture-01.png", b"same-image");
        let first = "pdf-markdown/images/page-0001-picture-01.png";
        let second = "pdf-markdown/images/page-0002-picture-01.png";
        let blocks = vec![
            asset_block("picture-1", "picture", first),
            asset_block("picture-2", "picture", second),
        ];
        let target = root.join("document.zh-CN.md");
        let assets = root.join("document.zh-CN.assets");
        fs::create_dir_all(&assets).expect("create prior assets");
        fs::write(&target, "old markdown").expect("write old markdown");
        fs::write(assets.join("stale.png"), b"stale").expect("write stale asset");
        let markdown = format!("![Figure]({first})\n\n![Figure]({second})");

        let result = write_pdf_markdown_export(&job_dir, &target, &blocks, &markdown, "markdown")
            .expect("deduplicated export");

        let output = fs::read_to_string(&target).expect("read markdown");
        assert_eq!(output.matches("page-0001-picture-01.png").count(), 2);
        assert!(!output.contains("page-0002-picture-01.png"));
        assert_eq!(result.files_written, 2);
        assert_eq!(fs::read_dir(&assets).expect("list assets").count(), 1);
        assert!(!assets.join("stale.png").exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pdf_markdown_export_rejects_unsafe_missing_and_oversized_assets() {
        let root = temp_dir("pdf-markdown-export-invalid");
        let job_dir = root.join("job");
        write_job_asset(&job_dir, "valid.png", b"valid");
        let invalid = [
            "pdf-markdown/images/../outside.png",
            "pdf-markdown/images/nested/picture.png",
            "pdf-markdown/images/picture.svg",
            "pdf-markdown/images/missing.png",
        ];
        for (index, source) in invalid.iter().enumerate() {
            let block = asset_block(&format!("asset-{index}"), "picture", source);
            let target = root.join(format!("invalid-{index}.md"));
            let error = write_pdf_markdown_export(
                &job_dir,
                &target,
                &[block],
                &format!("![Figure]({source})"),
                "markdown",
            )
            .expect_err("invalid asset must be rejected");
            assert!(error.starts_with("PDF Markdown 资源"));
            assert!(!target.exists());
        }

        let oversized = job_dir
            .join("pdf-markdown")
            .join("images")
            .join("oversized.webp");
        File::create(&oversized)
            .and_then(|file| file.set_len(32 * 1024 * 1024 + 1))
            .expect("create sparse oversized image");
        let source = "pdf-markdown/images/oversized.webp";
        let error = write_pdf_markdown_export(
            &job_dir,
            &root.join("oversized.md"),
            &[asset_block("oversized", "picture", source)],
            &format!("![Figure]({source})"),
            "markdown",
        )
        .expect_err("oversized asset must be rejected");
        assert_eq!(error, "PDF Markdown 资源不可用。");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pdf_markdown_export_rolls_back_both_outputs_and_cleans_staging() {
        let root = temp_dir("pdf-markdown-export-rollback");
        let job_dir = root.join("job");
        write_job_asset(&job_dir, "page-0001-picture-01.png", b"new-image");
        let source = "pdf-markdown/images/page-0001-picture-01.png";
        let target = root.join("document.md");
        let assets = root.join("document.assets");
        fs::create_dir_all(&assets).expect("create old assets");
        fs::write(&target, b"old markdown").expect("write old markdown");
        fs::write(assets.join("old.png"), b"old-image").expect("write old image");

        let error = write_pdf_markdown_export_with_fault(
            &job_dir,
            &target,
            &[asset_block("picture", "picture", source)],
            &format!("![Figure]({source})"),
            "markdown",
            PdfMarkdownExportFault::AfterMarkdownCommit,
        )
        .expect_err("injected commit failure");

        assert_eq!(error, "injected-pdf-markdown-export-failure");
        assert_eq!(
            fs::read(&target).expect("restored markdown"),
            b"old markdown"
        );
        assert_eq!(
            fs::read(assets.join("old.png")).expect("restored asset"),
            b"old-image"
        );
        let leftovers = fs::read_dir(&root)
            .expect("list export root")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| name.starts_with(".document.md."))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "leftover staging paths: {leftovers:?}"
        );
        fs::remove_dir_all(root).ok();
    }
}
