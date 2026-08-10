//! Checkpoint 3 storage and deterministic normalization for PDF Markdown.

pub(crate) mod render;

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::{
    managed_pdf_markdown,
    rosetta_jobs::{
        document::{block_translation, segments_by_block, sync_document_file_statuses},
        formats::pdf::source_state,
        model::{RosettaBlock, RosettaDocument, RosettaJobBundle, RosettaSourceFile, Segment},
        path::checked_job_dir,
        segmenter::split_long_text,
        store::{load_job_bundle, read_json},
        translation_files::{load_translation_file_bundle, translated_source_segments},
    },
};

use self::render::{render_blocks, RenderedMarkdownBlock};

pub const EXTRACTION_SCHEMA: &str = "rosetta-pdf-markdown-extraction/1";
pub const POLICY_VERSION: &str = "rosetta-pdf-markdown-normalizer/2";
const WINDOW_SIZE: usize = 8;
const MAX_WINDOW_SIZE: usize = 10;
const MAX_SHARD_COMPRESSED: u64 = 64 * 1024 * 1024;
const MAX_SHARD_DECOMPRESSED: usize = 16 * 1024 * 1024;
const MAX_PAGE_BLOCKS: usize = 10_000;
const MAX_PAGE_CHARS: usize = 2_000_000;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineIdentity {
    pub pymupdf4llm: String,
    pub pymupdf_layout: String,
    pub pymupdf: String,
}

impl Default for EngineIdentity {
    fn default() -> Self {
        Self {
            pymupdf4llm: "1.28.0".into(),
            pymupdf_layout: "1.28.0".into(),
            pymupdf: "1.28.0".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionManifest {
    pub schema: String,
    pub source_fingerprint: String,
    pub page_count: u32,
    pub engine: EngineIdentity,
    pub policy_version: String,
    pub use_ocr: bool,
    pub force_text: bool,
    pub write_images: bool,
    pub committed_pages: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageShard {
    pub schema: String,
    pub source_fingerprint: String,
    pub policy_version: String,
    pub page_number: u32,
    pub vendor: Value,
    #[serde(default)]
    pub images: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownExtractionStatus {
    pub job_id: String,
    pub state: String,
    pub completed_pages: u32,
    pub page_count: u32,
    pub error_code: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownPreview {
    pub source_blocks: Vec<RenderedMarkdownBlock>,
    pub translation_blocks: Option<Vec<RenderedMarkdownBlock>>,
}

#[derive(Default)]
pub struct PdfMarkdownExtractionState {
    active: Mutex<Option<ActiveExtraction>>,
    last: Mutex<Option<PdfMarkdownExtractionStatus>>,
}

struct ActiveExtraction {
    job_id: String,
    run_id: String,
    status: PdfMarkdownExtractionStatus,
}

impl PdfMarkdownExtractionState {
    fn snapshot(&self, app: &AppHandle, job_id: &str) -> PdfMarkdownExtractionStatus {
        if let Ok(guard) = self.active.lock() {
            if let Some(active) = guard.as_ref().filter(|a| a.job_id == job_id) {
                return active.status.clone();
            }
        }
        let Ok(root) = crate::rosetta_jobs::path::jobs_root(app) else {
            return PdfMarkdownExtractionStatus {
                job_id: job_id.into(),
                state: "idle".into(),
                completed_pages: 0,
                page_count: 0,
                error_code: Some("job-storage-unavailable".into()),
                run_id: None,
            };
        };
        let Ok(dir) = checked_job_dir(&root, job_id) else {
            return PdfMarkdownExtractionStatus {
                job_id: job_id.into(),
                state: "idle".into(),
                completed_pages: 0,
                page_count: 0,
                error_code: None,
                run_id: None,
            };
        };
        let manifest = read_manifest(&dir).ok().flatten();
        let page_count = manifest.as_ref().map(|m| m.page_count).unwrap_or(0);
        let completed_pages = manifest
            .as_ref()
            .map(|m| m.committed_pages.len() as u32)
            .unwrap_or(0);
        let source_current = dir.join("source.pdf").is_file()
            && source_state::fingerprint_file(&dir.join("source.pdf"))
                .ok()
                .zip(manifest.as_ref())
                .is_some_and(|(fingerprint, m)| manifest_is_current(m, &fingerprint, page_count));
        if !(source_current && page_count > 0 && completed_pages == page_count) {
            if let Ok(last) = self.last.lock() {
                if let Some(status) = last.as_ref().filter(|s| {
                    s.job_id == job_id && matches!(s.state.as_str(), "failed" | "cancelled")
                }) {
                    return status.clone();
                }
            }
        }
        PdfMarkdownExtractionStatus {
            job_id: job_id.into(),
            state: if source_current && page_count > 0 && completed_pages == page_count {
                "ready"
            } else if manifest.is_some() && !source_current {
                "stale"
            } else {
                "idle"
            }
            .into(),
            completed_pages,
            page_count,
            error_code: None,
            run_id: None,
        }
    }
}

pub fn extraction_root(job_dir: &Path) -> PathBuf {
    job_dir.join("pdf-markdown")
}
pub fn manifest_path(job_dir: &Path) -> PathBuf {
    extraction_root(job_dir).join("manifest.json")
}
fn pages_root(job_dir: &Path) -> PathBuf {
    extraction_root(job_dir).join("extraction").join("pages")
}
fn images_root(job_dir: &Path) -> PathBuf {
    extraction_root(job_dir).join("images")
}

/// Copy vendor images into deterministic job-relative names. Vendor paths are
/// untrusted derivatives and may not escape the per-run temp root.
pub fn canonicalize_images(
    job_dir: &Path,
    temp_root: &Path,
    page_number: u32,
    vendor: &mut Value,
) -> Result<Vec<String>, String> {
    let mut copied = Vec::new();
    let page = vendor
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .and_then(|pages| pages.first_mut())
        .ok_or_else(|| "page-shard-invalid-page".to_string())?;
    let boxes = page
        .get_mut("boxes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "page-shard-invalid-boxes".to_string())?;
    let root = temp_root
        .canonicalize()
        .map_err(|_| "invalid-temp-root".to_string())?;
    for (index, box_value) in boxes.iter_mut().enumerate() {
        let Some(raw) = box_value
            .get("image")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let p = PathBuf::from(&raw);
        let candidate = if p.is_absolute() {
            p
        } else {
            let direct = temp_root.join(&p);
            if direct.exists() {
                direct
            } else {
                temp_root
                    .join(format!("page-{page_number:04}-images"))
                    .join(p)
            }
        };
        let canonical = candidate
            .canonicalize()
            .map_err(|_| "image-path-invalid".to_string())?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err("image-path-outside-temp".into());
        }
        let meta = fs::metadata(&canonical).map_err(|_| "image-unreadable".to_string())?;
        if meta.len() > MAX_IMAGE_BYTES {
            return Err("image-too-large".into());
        }
        let ext = canonical
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp"))
            .ok_or_else(|| "image-format-unsupported".to_string())?;
        let filename = format!("page-{page_number:04}-picture-{:02}.{ext}", index + 1);
        atomic_replace(
            &images_root(job_dir).join(&filename),
            &fs::read(&canonical).map_err(|_| "image-unreadable".to_string())?,
        )?;
        let reference = format!("pdf-markdown/images/{filename}");
        if let Some(object) = box_value.as_object_mut() {
            object.insert("image".into(), Value::String(reference.clone()));
        }
        copied.push(reference);
    }
    Ok(copied)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "invalid-persistence-path".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "unable-to-create-persistence-dir".to_string())?;
    let nonce = format!("{}.{}", std::process::id(), now_nonce());
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("file"),
        nonce
    ));
    fs::write(&temp, bytes).map_err(|_| "unable-to-stage-persistence".to_string())?;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&temp)
        .map_err(|_| "unable-to-stage-persistence".to_string())?;
    file.sync_all()
        .map_err(|_| "unable-to-flush-persistence".to_string())?;
    let backup = parent.join(format!(
        ".{}.previous",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("file")
    ));
    let _ = fs::remove_file(&backup);
    if path.exists() {
        fs::rename(path, &backup).map_err(|_| "unable-to-stage-replacement".to_string())?;
    }
    if let Err(_) = fs::rename(&temp, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temp);
        return Err("unable-to-commit-persistence".into());
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

fn now_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

pub fn write_manifest(job_dir: &Path, manifest: &ExtractionManifest) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(manifest).map_err(|_| "unable-to-encode-manifest".to_string())?;
    atomic_replace(&manifest_path(job_dir), &bytes)
}

pub fn read_manifest(job_dir: &Path) -> Result<Option<ExtractionManifest>, String> {
    let path = manifest_path(job_dir);
    if !path.is_file() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

pub fn manifest_is_current(
    manifest: &ExtractionManifest,
    fingerprint: &str,
    page_count: u32,
) -> bool {
    manifest.schema == EXTRACTION_SCHEMA
        && manifest.source_fingerprint == fingerprint
        && manifest.page_count == page_count
        && manifest.policy_version == POLICY_VERSION
        && manifest.engine == EngineIdentity::default()
        && !manifest.use_ocr
        && !manifest.force_text
        && manifest.write_images
}

pub fn page_shard_path(job_dir: &Path, page_number: u32) -> PathBuf {
    pages_root(job_dir).join(format!("page-{page_number:04}.json.gz"))
}

pub fn write_page_shard(job_dir: &Path, shard: &PageShard) -> Result<(), String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let data = serde_json::to_vec(shard).map_err(|_| "unable-to-encode-page-shard".to_string())?;
    encoder
        .write_all(&data)
        .map_err(|_| "unable-to-compress-page-shard".to_string())?;
    let compressed = encoder
        .finish()
        .map_err(|_| "unable-to-compress-page-shard".to_string())?;
    atomic_replace(&page_shard_path(job_dir, shard.page_number), &compressed)
}

pub fn read_page_shard(job_dir: &Path, page_number: u32) -> Result<PageShard, String> {
    let path = page_shard_path(job_dir, page_number);
    let meta = fs::metadata(&path).map_err(|_| "page-shard-missing".to_string())?;
    if meta.len() > MAX_SHARD_COMPRESSED {
        return Err("page-shard-too-large".into());
    }
    let file = fs::File::open(&path).map_err(|_| "page-shard-unreadable".to_string())?;
    let decoder = GzDecoder::new(file);
    let mut data = Vec::new();
    decoder
        .take((MAX_SHARD_DECOMPRESSED + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| "page-shard-invalid-gzip".to_string())?;
    if data.len() > MAX_SHARD_DECOMPRESSED {
        return Err("page-shard-decompressed-too-large".into());
    }
    let shard: PageShard =
        serde_json::from_slice(&data).map_err(|_| "page-shard-invalid-json".to_string())?;
    if shard.schema != EXTRACTION_SCHEMA
        || shard.page_number != page_number
        || shard.policy_version != POLICY_VERSION
    {
        return Err("page-shard-stale".into());
    }
    let root = job_dir.join("pdf-markdown").join("images");
    for reference in &shard.images {
        if !reference.starts_with("pdf-markdown/images/") || reference.contains("..") {
            return Err("image-reference-invalid".into());
        }
        let relative = reference
            .strip_prefix("pdf-markdown/images/")
            .unwrap_or_default();
        let image = root.join(relative);
        let canonical_root = root
            .canonicalize()
            .map_err(|_| "image-reference-invalid".to_string())?;
        let canonical = image
            .canonicalize()
            .map_err(|_| "image-reference-missing".to_string())?;
        if !canonical.starts_with(&canonical_root)
            || fs::metadata(&canonical)
                .map_err(|_| "image-reference-missing".to_string())?
                .len()
                > MAX_IMAGE_BYTES
        {
            return Err("image-reference-invalid".into());
        }
    }
    validate_vendor_page(&shard.vendor, page_number)?;
    Ok(shard)
}

fn validate_vendor_page(vendor: &Value, page_number: u32) -> Result<(), String> {
    let pages = vendor
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| "page-shard-invalid-page".to_string())?;
    if pages.len() != 1 {
        return Err("page-shard-invalid-page".into());
    }
    let page = &pages[0];
    let number = page
        .get("page_number")
        .and_then(Value::as_u64)
        .or_else(|| page.get("pageIndex").and_then(Value::as_u64).map(|n| n + 1))
        .ok_or_else(|| "page-shard-invalid-page".to_string())? as u32;
    if number != page_number {
        return Err("page-shard-page-identity-mismatch".into());
    }
    let boxes = page
        .get("boxes")
        .and_then(Value::as_array)
        .ok_or_else(|| "page-shard-invalid-boxes".to_string())?;
    if boxes.len() > MAX_PAGE_BLOCKS {
        return Err("page-shard-too-many-blocks".into());
    }
    let mut chars = 0usize;
    let mut normalized_blocks = 0usize;
    for b in boxes {
        for key in ["x0", "y0", "x1", "y1"] {
            if let Some(n) = b.get(key).and_then(Value::as_f64) {
                if !n.is_finite() {
                    return Err("page-shard-invalid-coordinate".into());
                }
            }
        }
        if let Some(rows) = b
            .get("table")
            .and_then(table_matrix)
            .or_else(|| table_matrix(b))
        {
            for row in rows {
                let cells = row
                    .as_array()
                    .ok_or_else(|| "page-shard-invalid-table".to_string())?;
                normalized_blocks = normalized_blocks.saturating_add(cells.len());
                for cell in cells {
                    chars = chars.saturating_add(
                        cell.as_str()
                            .map(|text| text.chars().count())
                            .unwrap_or_else(|| extract_box_text(cell).chars().count()),
                    );
                }
            }
        } else {
            normalized_blocks = normalized_blocks.saturating_add(1);
            chars = chars.saturating_add(extract_box_text(b).chars().count());
        }
        if normalized_blocks > MAX_PAGE_BLOCKS {
            return Err("page-shard-too-many-blocks".into());
        }
        if chars > MAX_PAGE_CHARS {
            return Err("page-shard-too-many-characters".into());
        }
    }
    Ok(())
}

fn extract_box_text(box_value: &Value) -> String {
    if let Some(text) = box_value.get("text").and_then(Value::as_str) {
        return normalize_text(text);
    }
    box_value
        .get("textlines")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.get("spans").and_then(Value::as_array))
                .flatten()
                .filter_map(|span| span.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .map(|v| normalize_text(&v))
        .unwrap_or_default()
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn box_style(page: u32, class: &str, bbox: [f64; 4], extra: Value) -> Value {
    json!({"pdfMarkdown":{"version":1,"page":page,"boxClass":class,"bbox":bbox,"extra":extra}})
}

fn stable_id(page: u32, ordinal: usize, suffix: &str) -> String {
    format!("pdfmd-v1-p{page:04}-b{ordinal:04}{suffix}")
}

fn value_usize(value: &Value, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
        .and_then(|value| usize::try_from(value).ok())
}

fn value_bool(value: &Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_bool))
}

fn value_bbox(value: &Value) -> Option<[f64; 4]> {
    let values = value.as_array()?;
    if values.len() != 4 {
        return None;
    }
    Some([
        values[0].as_f64()?,
        values[1].as_f64()?,
        values[2].as_f64()?,
        values[3].as_f64()?,
    ])
}

fn list_item_levels(boxes: &[Value]) -> HashMap<usize, usize> {
    let mut levels = HashMap::new();
    let mut segment = Vec::new();

    let flush = |segment: &mut Vec<(usize, f64)>, levels: &mut HashMap<usize, usize>| {
        if segment.is_empty() {
            return;
        }
        let mut indents = segment.iter().map(|(_, x0)| *x0).collect::<Vec<_>>();
        indents.sort_by(f64::total_cmp);
        indents.dedup_by(|left, right| (*left - *right).abs() <= 10.0);
        for (index, x0) in segment.drain(..) {
            let level = indents
                .iter()
                .position(|indent| (x0 - *indent).abs() <= 10.0)
                .unwrap_or(0)
                + 1;
            levels.insert(index, level.min(6));
        }
    };

    for (index, value) in boxes.iter().enumerate() {
        if value.get("boxclass").and_then(Value::as_str) == Some("list-item") {
            segment.push((
                index,
                value.get("x0").and_then(Value::as_f64).unwrap_or(0.0),
            ));
        } else {
            flush(&mut segment, &mut levels);
        }
    }
    flush(&mut segment, &mut levels);
    levels
}

fn normalize_list_item(text: &str) -> (String, String, bool) {
    let trimmed = text.trim();
    let Some(first) = trimmed.chars().next() else {
        return (String::new(), "-".into(), false);
    };
    let first_len = first.len_utf8();
    let suffix = &trimmed[first_len..];
    let rest = suffix.trim_start();
    let ascii_marker =
        matches!(first, '-' | '*' | '+') && suffix.chars().next().is_some_and(char::is_whitespace);
    let graphic_marker = matches!(
        first,
        '\u{2022}' | '\u{2023}' | '\u{25e6}' | '\u{25aa}' | '\u{25cf}' | '\u{25cb}'
    ) || ('\u{e000}'..='\u{f8ff}').contains(&first);
    if ascii_marker || graphic_marker {
        let marker = if matches!(first, '-' | '*' | '+') {
            first.to_string()
        } else {
            "-".into()
        };
        return (rest.to_string(), marker, false);
    }

    if let Some((token, rest)) = trimmed.split_once(char::is_whitespace) {
        let number = token.trim_end_matches(['.', ')']);
        if !number.is_empty()
            && number.chars().all(|character| character.is_ascii_digit())
            && token.len() == number.len() + 1
        {
            return (rest.trim().to_string(), format!("{number}."), true);
        }
    }

    (trimmed.to_string(), "-".into(), false)
}

fn table_matrix(table: &Value) -> Option<&Vec<Value>> {
    table.get("extract").and_then(Value::as_array)
}

fn table_cell_bbox(table: &Value, row: usize, column: usize) -> Option<[f64; 4]> {
    table
        .get("cells")
        .and_then(Value::as_array)?
        .get(row)?
        .as_array()?
        .get(column)
        .and_then(value_bbox)
}

pub fn normalize_shard(
    shard: &PageShard,
    block_order: &mut usize,
    segment_order: &mut usize,
    source_lang: Option<String>,
    target_lang: &str,
) -> Result<(Vec<RosettaBlock>, Vec<Segment>), String> {
    let page = shard
        .vendor
        .get("pages")
        .and_then(Value::as_array)
        .and_then(|p| p.first())
        .ok_or_else(|| "page-shard-invalid-page".to_string())?;
    let boxes = page
        .get("boxes")
        .and_then(Value::as_array)
        .ok_or_else(|| "page-shard-invalid-boxes".to_string())?;
    let list_levels = list_item_levels(boxes);
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    for (index, b) in boxes.iter().enumerate() {
        let class = b.get("boxclass").and_then(Value::as_str).unwrap_or("text");
        if matches!(class, "page-header" | "page-footer") {
            continue;
        }
        let mut text = extract_box_text(b);
        let bbox = [
            b.get("x0").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("y0").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("x1").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("y1").and_then(Value::as_f64).unwrap_or(0.0),
        ];
        if class == "table" {
            let table = b.get("table").unwrap_or(b);
            if let Some(rows) = table_matrix(table) {
                let row_count =
                    value_usize(table, &["row_count", "rowCount"]).unwrap_or(rows.len());
                let column_count = value_usize(table, &["col_count", "columnCount"])
                    .unwrap_or_else(|| {
                        rows.iter()
                            .filter_map(Value::as_array)
                            .map(Vec::len)
                            .max()
                            .unwrap_or(0)
                    });
                let table_id = stable_id(shard.page_number, index + 1, "");
                let mut cell_index = 0usize;
                for (row, row_value) in rows.iter().enumerate() {
                    let Some(cells) = row_value.as_array() else {
                        continue;
                    };
                    for (column, cell) in cells.iter().enumerate() {
                        cell_index += 1;
                        let cell_text = cell
                            .as_str()
                            .map(normalize_text)
                            .unwrap_or_else(|| extract_box_text(cell));
                        let should_translate = !cell_text.is_empty();
                        let cell_bbox = table_cell_bbox(table, row, column).unwrap_or(bbox);
                        let row_span = value_usize(cell, &["row_span", "rowSpan", "rowspan"])
                            .unwrap_or(1)
                            .max(1);
                        let column_span = value_usize(
                            cell,
                            &[
                                "column_span",
                                "columnSpan",
                                "col_span",
                                "colSpan",
                                "colspan",
                            ],
                        )
                        .unwrap_or(1)
                        .max(1);
                        let header = value_bool(cell, &["header", "is_header", "isHeader"])
                            .unwrap_or(row == 0);
                        let id =
                            stable_id(shard.page_number, index + 1, &format!("-c{cell_index:04}"));
                        let order = *block_order;
                        *block_order += 1;
                        let block = RosettaBlock {
                            id: id.clone(),
                            file_id: Some("file-1".into()),
                            block_type: "table_cell".into(),
                            source_text: cell_text.clone(),
                            translated_text: None,
                            should_translate,
                            order,
                            path: Some(format!(
                                "pdf-markdown.pages.{}.boxes.{}.cells.{}",
                                shard.page_number,
                                index + 1,
                                cell_index
                            )),
                            style: Some(box_style(
                                shard.page_number,
                                class,
                                cell_bbox,
                                json!({
                                    "tableId": table_id,
                                    "row": row,
                                    "column": column,
                                    "rowSpan": row_span,
                                    "columnSpan": column_span,
                                    "header": header,
                                    "rowCount": row_count,
                                    "columnCount": column_count
                                }),
                            )),
                            status: if should_translate {
                                "pending".into()
                            } else {
                                "skipped".into()
                            },
                        };
                        if should_translate {
                            for (part, chunk) in split_long_text(&cell_text).into_iter().enumerate()
                            {
                                segments.push(Segment {
                                    id: format!("{id}-segment-{}", part + 1),
                                    block_id: id.clone(),
                                    file_id: Some("file-1".into()),
                                    order: *segment_order,
                                    source_text: chunk,
                                    translated_text: None,
                                    source_lang: source_lang.clone(),
                                    target_lang: target_lang.into(),
                                    kind: "table_cell".into(),
                                    preserve_whitespace: true,
                                    status: "pending".into(),
                                    block_order: Some(order),
                                    segment_index_in_block: Some(part),
                                    error: None,
                                    translation_history: Vec::new(),
                                });
                                *segment_order += 1;
                            }
                        }
                        blocks.push(block);
                    }
                }
                continue;
            }
        }
        let (block_type, should_translate) = match class {
            "title" => ("heading", true),
            "section-header" => ("heading", true),
            "text" => ("paragraph", true),
            "list-item" => ("list_item", true),
            "caption" => ("caption", true),
            "footnote" => ("footnote", true),
            "picture" => ("metadata", false),
            "formula" => ("code", false),
            _ => ("paragraph", true),
        };
        let id = stable_id(shard.page_number, index + 1, "");
        let extra = match class {
            "title" => json!({"headingLevel": 1}),
            "section-header" => json!({
                "headingLevel": value_usize(b, &["header_level", "headerLevel"])
                    .unwrap_or(2)
                    .clamp(2, 6)
            }),
            "list-item" => {
                let (normalized, marker, ordered) = normalize_list_item(&text);
                text = normalized;
                json!({
                    "listLevel": list_levels.get(&index).copied().unwrap_or(1),
                    "listMarker": marker,
                    "ordered": ordered
                })
            }
            "picture" | "formula" => {
                text.clear();
                json!({
                    "assetPath": b.get("image").and_then(Value::as_str),
                    "width": (bbox[2] - bbox[0]).max(0.0),
                    "height": (bbox[3] - bbox[1]).max(0.0)
                })
            }
            _ => json!({}),
        };
        let order = *block_order;
        *block_order += 1;
        let style = box_style(shard.page_number, class, bbox, extra);
        blocks.push(RosettaBlock {
            id: id.clone(),
            file_id: Some("file-1".into()),
            block_type: block_type.into(),
            source_text: text.clone(),
            translated_text: None,
            should_translate,
            order,
            path: Some(format!(
                "pdf-markdown.pages.{}.boxes.{}",
                shard.page_number,
                index + 1
            )),
            style: Some(style),
            status: if should_translate {
                "pending".into()
            } else {
                "skipped".into()
            },
        });
        if should_translate && !text.is_empty() {
            for (part, chunk) in split_long_text(&text).into_iter().enumerate() {
                segments.push(Segment {
                    id: format!("{id}-segment-{}", part + 1),
                    block_id: id.clone(),
                    file_id: Some("file-1".into()),
                    order: *segment_order,
                    source_text: chunk,
                    translated_text: None,
                    source_lang: source_lang.clone(),
                    target_lang: target_lang.into(),
                    kind: block_type.into(),
                    preserve_whitespace: true,
                    status: "pending".into(),
                    block_order: Some(order),
                    segment_index_in_block: Some(part),
                    error: None,
                    translation_history: Vec::new(),
                });
                *segment_order += 1;
            }
        }
    }
    Ok((blocks, segments))
}

pub fn project_ir(
    job_dir: &Path,
    mut document: RosettaDocument,
    mut blocks: Vec<RosettaBlock>,
    segments: Vec<Segment>,
) -> Result<(), String> {
    let file = document
        .files
        .first()
        .cloned()
        .unwrap_or_else(|| RosettaSourceFile {
            id: "file-1".into(),
            filename: document.filename.clone(),
            relative_path: document.filename.clone(),
            format: "pdf".into(),
            source_lang: document.source_lang.clone(),
            target_lang: Some(document.target_lang.clone()),
            translation_status: "untranslated".into(),
            segment_count: 0,
            completed_segments: 0,
            failed_segments: 0,
            translating_segments: 0,
            block_ids: Vec::new(),
        });
    let mut file = file;
    file.block_ids = blocks.iter().map(|b| b.id.clone()).collect();
    document.files = vec![file];
    document.blocks = std::mem::take(&mut blocks);
    document.extraction_status = Some("done".into());
    sync_document_file_statuses(&mut document, &segments);
    let doc_bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| "unable-to-encode-document".to_string())?;
    let seg_bytes = serde_json::to_vec_pretty(&segments)
        .map_err(|_| "unable-to-encode-segments".to_string())?;
    let tmp = job_dir
        .join(".tmp")
        .join(format!("projection-{}", now_nonce()));
    fs::create_dir_all(&tmp).map_err(|_| "unable-to-stage-projection".to_string())?;
    let result = (|| {
        atomic_replace(&tmp.join("document.json"), &doc_bytes)?;
        atomic_replace(&tmp.join("segments.json"), &seg_bytes)?;
        let old_doc = job_dir.join("document.json");
        let old_seg = job_dir.join("segments.json");
        let backup_doc = job_dir.join(".document.json.previous");
        let backup_seg = job_dir.join(".segments.json.previous");
        let _ = fs::remove_file(&backup_doc);
        let _ = fs::remove_file(&backup_seg);
        if old_doc.exists() {
            fs::rename(&old_doc, &backup_doc)
                .map_err(|_| "unable-to-stage-projection".to_string())?;
        }
        if old_seg.exists() {
            fs::rename(&old_seg, &backup_seg)
                .map_err(|_| "unable-to-stage-projection".to_string())?;
        }
        if fs::rename(tmp.join("document.json"), &old_doc).is_err()
            || fs::rename(tmp.join("segments.json"), &old_seg).is_err()
        {
            let _ = fs::remove_file(&old_doc);
            let _ = fs::remove_file(&old_seg);
            if backup_doc.exists() {
                let _ = fs::rename(&backup_doc, &old_doc);
            }
            if backup_seg.exists() {
                let _ = fs::rename(&backup_seg, &old_seg);
            }
            return Err("unable-to-commit-projection".into());
        }
        let _ = fs::remove_file(backup_doc);
        let _ = fs::remove_file(backup_seg);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn preserve_existing_translations(
    old_document: &RosettaDocument,
    old_segments: &[Segment],
    blocks: &mut [RosettaBlock],
    segments: &mut [Segment],
) {
    for block in blocks {
        if let Some(old) = old_document.blocks.iter().find(|candidate| {
            candidate.id == block.id && candidate.source_text == block.source_text
        }) {
            block.translated_text = old.translated_text.clone();
            block.status = old.status.clone();
        }
    }
    for segment in segments {
        if let Some(old) = old_segments.iter().find(|candidate| {
            candidate.id == segment.id && candidate.source_text == segment.source_text
        }) {
            segment.translated_text = old.translated_text.clone();
            segment.status = old.status.clone();
            segment.error = old.error.clone();
            segment.translation_history = old.translation_history.clone();
        }
    }
}

fn run_id() -> String {
    format!("run-{}", now_nonce())
}

#[tauri::command]
pub fn get_pdf_markdown_extraction_status(
    app: AppHandle,
    state: State<'_, PdfMarkdownExtractionState>,
    job_id: String,
) -> Result<PdfMarkdownExtractionStatus, String> {
    Ok(state.snapshot(&app, &job_id))
}

#[tauri::command]
pub fn render_pdf_markdown_preview(
    app: AppHandle,
    job_id: String,
    source_file_id: String,
    translation_file_id: Option<String>,
) -> Result<PdfMarkdownPreview, String> {
    let bundle =
        load_job_bundle(&app, &job_id).map_err(|_| "pdf-markdown-job-unavailable".to_string())?;
    let source_file = bundle
        .document
        .files
        .iter()
        .find(|file| file.id == source_file_id && file.format == "pdf")
        .ok_or_else(|| "pdf-markdown-source-invalid".to_string())?;
    let blocks = bundle
        .document
        .blocks
        .iter()
        .filter(|block| {
            block.file_id.as_deref().unwrap_or("file-1") == source_file.id
                && render::is_pdf_markdown_block(block)
        })
        .cloned()
        .collect::<Vec<_>>();
    let source_text = blocks
        .iter()
        .map(|block| (block.id.clone(), block.source_text.clone()))
        .collect::<HashMap<_, _>>();
    let source_blocks = render_blocks(&blocks, &source_text);

    let translation_blocks = translation_file_id
        .map(|translation_file_id| {
            let translation = load_translation_file_bundle(&app, &job_id, &translation_file_id)
                .map_err(|_| "pdf-markdown-translation-unavailable".to_string())?;
            if translation.translation_file.source_file_id != source_file.id
                || translation.translation_file.output_format != "markdown"
            {
                return Err("pdf-markdown-translation-identity-invalid".to_string());
            }
            let translated_segments = translated_source_segments(
                &bundle.segments,
                &translation.segments,
                &source_file.id,
                &translation.translation_file.target_lang,
            );
            let by_block = segments_by_block(&translated_segments);
            let translated_text = blocks
                .iter()
                .map(|block| {
                    let text = if block.should_translate {
                        block_translation(
                            block,
                            &by_block,
                            &translation.translation_file.target_lang,
                        )
                    } else {
                        block.source_text.clone()
                    };
                    (block.id.clone(), text)
                })
                .collect::<HashMap<_, _>>();
            Ok(render_blocks(&blocks, &translated_text))
        })
        .transpose()?;

    Ok(PdfMarkdownPreview {
        source_blocks,
        translation_blocks,
    })
}

#[tauri::command]
pub fn read_pdf_markdown_asset(
    app: AppHandle,
    job_id: String,
    asset_path: String,
) -> Result<tauri::ipc::Response, String> {
    let root = crate::rosetta_jobs::path::jobs_root(&app)?;
    let job_dir = checked_job_dir(&root, &job_id)?;
    let path = resolve_preview_asset(&job_dir, &asset_path)?;
    let bytes = fs::read(path).map_err(|_| "pdf-markdown-asset-unreadable".to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn resolve_preview_asset(job_dir: &Path, asset_path: &str) -> Result<PathBuf, String> {
    let relative = asset_path
        .strip_prefix("pdf-markdown/images/")
        .filter(|relative| {
            !relative.is_empty() && !relative.contains('/') && !relative.contains('\\')
        })
        .ok_or_else(|| "pdf-markdown-asset-path-invalid".to_string())?;
    let extension = Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "pdf-markdown-asset-type-invalid".to_string())?;
    if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Err("pdf-markdown-asset-type-invalid".into());
    }
    let image_root = images_root(job_dir)
        .canonicalize()
        .map_err(|_| "pdf-markdown-asset-root-missing".to_string())?;
    let candidate = image_root.join(relative);
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "pdf-markdown-asset-missing".to_string())?;
    let metadata =
        fs::metadata(&canonical).map_err(|_| "pdf-markdown-asset-missing".to_string())?;
    if !canonical.starts_with(&image_root)
        || !metadata.is_file()
        || metadata.len() > MAX_IMAGE_BYTES
    {
        return Err("pdf-markdown-asset-invalid".into());
    }
    Ok(canonical)
}

#[tauri::command]
pub async fn start_pdf_markdown_extraction(
    app: AppHandle,
    state: State<'_, PdfMarkdownExtractionState>,
    job_id: String,
) -> Result<PdfMarkdownExtractionStatus, String> {
    let run = run_id();
    {
        let mut guard = state
            .active
            .lock()
            .map_err(|_| "extraction-state-lock".to_string())?;
        if guard.is_some() {
            return Err("pdf-markdown-extraction-busy".into());
        }
        *guard = Some(ActiveExtraction {
            job_id: job_id.clone(),
            run_id: run.clone(),
            status: PdfMarkdownExtractionStatus {
                job_id: job_id.clone(),
                state: "extracting".into(),
                completed_pages: 0,
                page_count: 0,
                error_code: None,
                run_id: Some(run.clone()),
            },
        });
    }
    let result = run_extraction(&app, &state, &job_id, &run).await;
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            if let Ok(mut g) = state.active.lock() {
                *g = None;
            }
            if let Ok(mut last) = state.last.lock() {
                *last = Some(PdfMarkdownExtractionStatus {
                    job_id: job_id.clone(),
                    state: if error == "worker-protocol-closed" || error == "worker-stopping" {
                        "cancelled".into()
                    } else {
                        "failed".into()
                    },
                    completed_pages: 0,
                    page_count: 0,
                    error_code: Some(error.clone()),
                    run_id: Some(run.clone()),
                });
            }
            return Err(error);
        }
    };
    if let Ok(mut g) = state.active.lock() {
        *g = None;
    }
    if let Ok(mut last) = state.last.lock() {
        *last = Some(status.clone());
    }
    Ok(status)
}

#[tauri::command]
pub async fn cancel_pdf_markdown_extraction(
    app: AppHandle,
    state: State<'_, PdfMarkdownExtractionState>,
    job_id: String,
) -> Result<bool, String> {
    let active = state
        .active
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|a| a.job_id == job_id))
        .unwrap_or(false);
    if active {
        Ok(managed_pdf_markdown::cancel(&app).await)
    } else {
        Ok(false)
    }
}

pub async fn cancel_for_job(
    app: &AppHandle,
    state: &PdfMarkdownExtractionState,
    job_id: &str,
) -> bool {
    let active = state
        .active
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|a| a.job_id == job_id))
        .unwrap_or(false);
    if active {
        managed_pdf_markdown::cancel(app).await
    } else {
        false
    }
}

async fn run_extraction(
    app: &AppHandle,
    state: &PdfMarkdownExtractionState,
    job_id: &str,
    run: &str,
) -> Result<PdfMarkdownExtractionStatus, String> {
    let bundle: RosettaJobBundle =
        load_job_bundle(app, job_id).map_err(|_| "job-bundle-invalid".to_string())?;
    let job_dir = checked_job_dir(&crate::rosetta_jobs::path::jobs_root(app)?, job_id)?;
    let source = job_dir.join("source.pdf");
    if !source.is_file() {
        return Err("pdf-source-missing".into());
    }
    let metadata = source_state::read_pdf_source_metadata(&job_dir)
        .map_err(|_| "pdf-source-metadata-invalid".to_string())?
        .ok_or_else(|| "pdf-source-metadata-missing".to_string())?;
    let fingerprint = source_state::fingerprint_file(&source)
        .map_err(|_| "source-fingerprint-failed".to_string())?;
    let page_count = metadata.page_count;
    let mut manifest = read_manifest(&job_dir)?
        .filter(|m| manifest_is_current(m, &fingerprint, page_count))
        .unwrap_or(ExtractionManifest {
            schema: EXTRACTION_SCHEMA.into(),
            source_fingerprint: fingerprint.clone(),
            page_count,
            engine: EngineIdentity::default(),
            policy_version: POLICY_VERSION.into(),
            use_ocr: false,
            force_text: false,
            write_images: true,
            committed_pages: Vec::new(),
        });
    manifest.committed_pages.retain(|p| {
        read_page_shard(&job_dir, *p)
            .map(|s| s.source_fingerprint == fingerprint)
            .unwrap_or(false)
    });
    write_manifest(&job_dir, &manifest)?;
    let temp_root = extraction_root(&job_dir).join(".tmp").join(run);
    fs::create_dir_all(&temp_root).map_err(|_| "unable-to-stage-extraction".to_string())?;
    let completed = manifest.committed_pages.len() as u32;
    update_active_status(
        state,
        job_id,
        run,
        "extracting",
        completed,
        page_count,
        None,
    );
    emit_progress(app, job_id, run, "extracting", completed, page_count, None);
    for start in 0..page_count {
        let page = start + 1;
        if manifest.committed_pages.contains(&page) {
            continue;
        }
        let pages: Vec<u32> = ((page - 1)..page_count.min(page - 1 + WINDOW_SIZE as u32))
            .filter(|p| !manifest.committed_pages.contains(&(p + 1)))
            .take(MAX_WINDOW_SIZE)
            .collect();
        let values = managed_pdf_markdown::extract_window(app, &source, &pages, &temp_root).await?;
        for value in values {
            let page_index = value
                .get("pageIndex")
                .and_then(Value::as_u64)
                .ok_or_else(|| "worker-page-identity-invalid".to_string())?
                as u32;
            let mut vendor = value
                .get("json")
                .cloned()
                .ok_or_else(|| "worker-page-result-invalid".to_string())?;
            let page_number = page_index + 1;
            validate_vendor_page(&vendor, page_number)?;
            let images = canonicalize_images(&job_dir, &temp_root, page_number, &mut vendor)?;
            let shard = PageShard {
                schema: EXTRACTION_SCHEMA.into(),
                source_fingerprint: fingerprint.clone(),
                policy_version: POLICY_VERSION.into(),
                page_number,
                vendor,
                images,
            };
            write_page_shard(&job_dir, &shard)?;
            if !manifest.committed_pages.contains(&page_number) {
                manifest.committed_pages.push(page_number);
            }
        }
        manifest.committed_pages.sort_unstable();
        write_manifest(&job_dir, &manifest)?;
        let completed = manifest.committed_pages.len() as u32;
        update_active_status(
            state,
            job_id,
            run,
            "extracting",
            completed,
            page_count,
            None,
        );
        emit_progress(app, job_id, run, "extracting", completed, page_count, None);
    }
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut bo = 1usize;
    let mut so = 1usize;
    for page in 1..=page_count {
        let shard = read_page_shard(&job_dir, page)?;
        let (mut b, mut s) = normalize_shard(
            &shard,
            &mut bo,
            &mut so,
            bundle.document.source_lang.clone(),
            &bundle.document.target_lang,
        )?;
        blocks.append(&mut b);
        segments.append(&mut s);
    }
    let mut blocks = blocks;
    let mut segments = segments;
    preserve_existing_translations(
        &bundle.document,
        &bundle.segments,
        &mut blocks,
        &mut segments,
    );
    project_ir(&job_dir, bundle.document, blocks, segments)?;
    update_active_status(state, job_id, run, "ready", page_count, page_count, None);
    emit_progress(app, job_id, run, "ready", page_count, page_count, None);
    let _ = fs::remove_dir_all(temp_root);
    Ok(PdfMarkdownExtractionStatus {
        job_id: job_id.into(),
        state: "ready".into(),
        completed_pages: page_count,
        page_count,
        error_code: None,
        run_id: Some(run.into()),
    })
}

fn update_active_status(
    state: &PdfMarkdownExtractionState,
    job_id: &str,
    run: &str,
    status: &str,
    completed: u32,
    total: u32,
    error: Option<String>,
) {
    if let Ok(mut guard) = state.active.lock() {
        if let Some(active) = guard
            .as_mut()
            .filter(|a| a.job_id == job_id && a.run_id == run)
        {
            active.status = PdfMarkdownExtractionStatus {
                job_id: job_id.into(),
                state: status.into(),
                completed_pages: completed,
                page_count: total,
                error_code: error,
                run_id: Some(run.into()),
            };
        }
    }
}
fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    run: &str,
    status: &str,
    completed: u32,
    total: u32,
    error: Option<String>,
) {
    let _ = app.emit("rosetta-pdf-markdown-progress", json!({"jobId":job_id,"runId":run,"state":status,"completedPages":completed,"pageCount":total,"errorCode":error}));
}

#[cfg(test)]
mod tests;
