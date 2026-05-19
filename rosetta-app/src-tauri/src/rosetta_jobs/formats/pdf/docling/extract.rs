//! HTTP client for the docling-serve sidecar: POST a PDF, parse the
//! structured response, and translate Docling's block roles into Rosetta IR.
//!
//! The flow is intentionally simple for v1 — synchronous POST to
//! `/v1/convert/file`, generous timeout, no progress streaming. The 117-page
//! GeoTopo textbook converts in ~35s end-to-end (verified in
//! `experiments/docling-probe`), so a 10-minute timeout covers anything a
//! real user is going to import. Phase 2 frontend can switch to the async
//! `/v1/convert/file/async` + `/v1/status/poll/{id}` flow for live progress.

use std::path::Path;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::json;

use crate::rosetta_jobs::{
    formats::pdf::errors::PdfError,
    model::{RosettaBlock, Segment},
    segmenter::{push_segments_for_block, translatable_block},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Convert a PDF via the running docling-serve sidecar and project the
/// response into the same `(blocks, segments)` shape the rest of the import
/// pipeline expects.
///
/// `document_id` is reused as the deterministic prefix for block/segment ids,
/// matching the convention in [`super::super::extract::parse_pdf`] so jobs
/// produced via either extraction backend look identical to downstream code.
pub(crate) async fn extract_via_docling(
    base_url: &str,
    document_id: &str,
    source_path: &Path,
) -> Result<(Vec<RosettaBlock>, Vec<Segment>), PdfError> {
    let bytes = std::fs::read(source_path)
        .map_err(|error| PdfError::Read(format!("读取 PDF 失败: {error}")))?;
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf")
        .to_string();

    let part = Part::bytes(bytes)
        .file_name(filename)
        .mime_str("application/pdf")
        .map_err(|error| PdfError::Parse(format!("构造 multipart 失败: {error}")))?;
    // Ask docling-serve for just the structured JSON. Skipping md/html/text
    // saves response size and post-processing on the Python side.
    let form = Form::new()
        .part("files", part)
        .text("to_formats", "json");

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| PdfError::Parse(format!("构造 HTTP 客户端失败: {error}")))?;

    let url = format!("{}/v1/convert/file", base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|error| PdfError::Parse(format!("docling-serve 请求失败: {error}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(PdfError::Parse(format!(
            "docling-serve 返回 {status}：{}",
            body.chars().take(400).collect::<String>()
        )));
    }

    let payload: DoclingResponse = response
        .json()
        .await
        .map_err(|error| PdfError::Parse(format!("解析 docling 响应失败: {error}")))?;

    map_response_to_blocks(payload, document_id)
}

/// Pure projection: docling-serve response → Rosetta blocks + segments.
/// Split out from the HTTP path so we can unit-test the mapping without
/// spinning up a real sidecar.
pub(crate) fn map_response_to_blocks(
    payload: DoclingResponse,
    document_id: &str,
) -> Result<(Vec<RosettaBlock>, Vec<Segment>), PdfError> {
    if payload.status != "success" {
        return Err(PdfError::Parse(format!(
            "docling 返回非 success 状态：{}",
            payload.status
        )));
    }

    let Some(json_content) = payload.document.and_then(|doc| doc.json_content) else {
        return Err(PdfError::ImageOnly);
    };

    if json_content.texts.is_empty() && json_content.tables.is_empty() {
        return Err(PdfError::ImageOnly);
    }

    // Build a page-number → page-height map so we can convert TOPLEFT bboxes
    // (which Docling uses for tables) into BOTTOMLEFT (which pdfium-render
    // wants for `PdfPageTextObject::translate`).
    let page_heights: std::collections::HashMap<u32, f64> = json_content
        .pages
        .iter()
        .filter_map(|(key, info)| key.parse::<u32>().ok().map(|n| (n, info.size.height)))
        .collect();

    // Collect raw items (text spans + table cells) into a single list so we
    // can sort by reading order (page, then descending Y) before assigning
    // block ids and ordering.
    let mut raws: Vec<RawItem> = Vec::new();

    for item in &json_content.texts {
        let role = classify_role(&item.label);
        let trimmed = item.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(prov) = item.prov.first() else {
            continue;
        };
        let page_height = page_heights.get(&prov.page_no).copied();
        let bbox = &prov.bbox;
        let (bbox_x, bbox_y, width, height, baseline_y) = normalize_bbox(
            bbox.l,
            bbox.t,
            bbox.r,
            bbox.b,
            &bbox.coord_origin,
            page_height,
        );
        raws.push(RawItem {
            page: prov.page_no,
            bbox_x,
            bbox_y,
            width,
            height,
            baseline_y,
            text: trimmed.to_string(),
            role,
            docling_label: item.label.clone(),
        });
    }

    // Each table contributes one block per cell. Table-level role lets the
    // renderer treat cells as paragraphs (so they get translated), while
    // the `doclingLabel: "table_cell"` hint is preserved for Phase 3 layout
    // heuristics.
    //
    // IMPORTANT: a table cell's bbox height is the CELL rect (with padding),
    // NOT the height of the text inside. Using cell height as font size makes
    // small numbers of text rendered at 26pt+. We anchor the font size at a
    // conservative body-text default and let `generate.rs` shrink further if
    // the translation doesn't fit horizontally.
    const DEFAULT_TABLE_CELL_FONT_PT: f32 = 10.0;
    for table in &json_content.tables {
        let Some(prov) = table.prov.first() else {
            continue;
        };
        let page_height = page_heights.get(&prov.page_no).copied();
        for cell in &table.data.table_cells {
            let trimmed = cell.text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let (bbox_x, bbox_y, width, height, _baseline_y) = normalize_bbox(
                cell.bbox.l,
                cell.bbox.t,
                cell.bbox.r,
                cell.bbox.b,
                &cell.bbox.coord_origin,
                page_height,
            );
            // Use the cell-bottom + a few points padding as the baseline so
            // text renders inside the cell, not pinned to the cell bottom.
            let baseline_y = bbox_y + (height - DEFAULT_TABLE_CELL_FONT_PT).max(0.0) * 0.4;
            raws.push(RawItem {
                page: prov.page_no,
                bbox_x,
                bbox_y,
                width,
                // Override the cell height with a body-text default so we
                // don't render small numbers at heading-sized font.
                height: DEFAULT_TABLE_CELL_FONT_PT.min(height),
                baseline_y,
                text: trimmed.to_string(),
                role: DoclingRole::TableCell,
                docling_label: "table_cell".to_string(),
            });
        }
    }

    // Sort into top-to-bottom, left-to-right reading order. We do this AFTER
    // mixing texts + table cells so a table inserted between two paragraphs
    // gets the right order_on_page sequence.
    raws.sort_by(|a, b| {
        a.page.cmp(&b.page).then_with(|| {
            // PDF y increases upward; higher y = earlier in reading order.
            b.bbox_y
                .partial_cmp(&a.bbox_y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.bbox_x
                        .partial_cmp(&b.bbox_x)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        })
    });

    let mut blocks: Vec<RosettaBlock> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut block_order = 1usize;
    let mut segment_order = 1usize;
    let mut per_page_orders: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::new();

    for raw in raws {
        let order_on_page = per_page_orders
            .entry(raw.page)
            .and_modify(|n| *n += 1)
            .or_insert(1);

        let style = json!({
            "pdf": {
                "page": raw.page,
                "orderOnPage": *order_on_page,
                "bbox": [raw.bbox_x, raw.bbox_y, raw.width, raw.height],
                "baselineY": raw.baseline_y,
                "fontSize": raw.height.max(1.0),
                "doclingLabel": &raw.docling_label,
                "layoutConfidence": "high",
            }
        });

        let block_id = format!("{document_id}-p{}-b{}", raw.page, *order_on_page);
        let should_translate = role_should_translate(&raw.role);
        let block_type = role_block_type(&raw.role);

        let mut block = translatable_block(
            &block_id,
            block_type,
            &raw.text,
            block_order,
            Some(style),
        );
        block.should_translate = should_translate;
        if !should_translate {
            block.status = "skipped".to_string();
        }
        blocks.push(block);

        if should_translate {
            push_segments_for_block(
                &mut segments,
                &block_id,
                block_type,
                block_order,
                &raw.text,
                &mut segment_order,
            );
        }
        block_order += 1;
    }

    if segments.is_empty() {
        return Err(PdfError::ImageOnly);
    }

    Ok((blocks, segments))
}

/// Intermediate representation used to mix `texts[]` and `tables[].cells` into
/// a single sortable list before emitting blocks.
struct RawItem {
    page: u32,
    bbox_x: f32,
    bbox_y: f32,
    width: f32,
    height: f32,
    baseline_y: f32,
    text: String,
    role: DoclingRole,
    docling_label: String,
}

/// Docling labels we may see in `json_content.texts[*].label`. Anything else
/// falls into `Other` and is treated as a regular paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoclingRole {
    SectionHeader,
    Paragraph,
    ListItem,
    TableCell,
    Caption,
    Footnote,
    Formula,
    Code,
    PageHeader,
    PageFooter,
    Other,
}

fn classify_role(label: &str) -> DoclingRole {
    // Docling's label vocabulary as of docling-core 2.75 — see
    // https://docling-project.github.io/docling/concepts/docling_document/
    match label {
        "section_header" | "title" => DoclingRole::SectionHeader,
        "text" | "paragraph" => DoclingRole::Paragraph,
        "list_item" => DoclingRole::ListItem,
        "table_cell" => DoclingRole::TableCell,
        "caption" => DoclingRole::Caption,
        "footnote" => DoclingRole::Footnote,
        "formula" => DoclingRole::Formula,
        "code" => DoclingRole::Code,
        "page_header" => DoclingRole::PageHeader,
        "page_footer" => DoclingRole::PageFooter,
        _ => DoclingRole::Other,
    }
}

/// Map Docling roles to the `block_type` field used by markdown.rs / txt.rs.
fn role_block_type(role: &DoclingRole) -> &'static str {
    match role {
        DoclingRole::SectionHeader => "heading",
        DoclingRole::ListItem => "list_item",
        DoclingRole::Code => "code",
        DoclingRole::PageHeader | DoclingRole::PageFooter | DoclingRole::Formula => "metadata",
        _ => "paragraph",
    }
}

/// Skip translation for content that's either non-linguistic (formulas, page
/// numbers) or where translating would do more harm than good.
fn role_should_translate(role: &DoclingRole) -> bool {
    !matches!(
        role,
        DoclingRole::Formula | DoclingRole::PageHeader | DoclingRole::PageFooter
    )
}

/// Convert Docling bbox (which may use either origin convention) into
/// PDF-points with BOTTOMLEFT origin — that's what pdfium-render's
/// `PdfPageTextObject::translate` expects in generate.rs.
///
/// `page_height` is only needed for the TOPLEFT case (tables in Docling's
/// output) where we have to flip the Y axis around the page midline.
fn normalize_bbox(
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    coord_origin: &str,
    page_height: Option<f64>,
) -> (f32, f32, f32, f32, f32) {
    let bbox_x = l as f32;
    let width = (r - l) as f32;
    let (bbox_y, height) = if coord_origin.eq_ignore_ascii_case("BOTTOMLEFT") {
        // Docling's BOTTOMLEFT: t > b (t is "top in PDF space" = upper Y).
        (b as f32, (t - b) as f32)
    } else if let Some(ph) = page_height {
        // TOPLEFT (commonly used for table cells): t < b in Docling. Flip Y
        // around the page midline so BOTTOMLEFT-origin code downstream can
        // treat it identically to text bboxes.
        let h = (b - t) as f32;
        let bottom_y_pdf = (ph - b) as f32;
        (bottom_y_pdf, h)
    } else {
        // Last-ditch fallback when we don't know the page height. Treat the
        // numbers as if they were BOTTOMLEFT; the resulting placement will be
        // wrong but the renderer won't crash. Phase 3 layout-confidence
        // flagging surfaces this to the user.
        (b as f32, (t - b).abs() as f32)
    };
    // Approximate typographic baseline ≈ bbox bottom for body text.
    let baseline_y = bbox_y;
    (bbox_x, bbox_y, width, height, baseline_y)
}

// -------------------- response types --------------------

#[derive(Debug, Deserialize)]
pub(crate) struct DoclingResponse {
    status: String,
    document: Option<DoclingResponseDocument>,
}

#[derive(Debug, Deserialize)]
struct DoclingResponseDocument {
    json_content: Option<DoclingJsonContent>,
}

#[derive(Debug, Deserialize)]
struct DoclingJsonContent {
    #[serde(default)]
    texts: Vec<DoclingTextItem>,
    #[serde(default)]
    tables: Vec<DoclingTable>,
    /// Docling indexes pages by `"1"`-style string keys, not numerically.
    #[serde(default)]
    pages: std::collections::HashMap<String, DoclingPage>,
}

#[derive(Debug, Deserialize)]
struct DoclingTextItem {
    label: String,
    text: String,
    #[serde(default)]
    prov: Vec<DoclingProvenance>,
}

#[derive(Debug, Deserialize)]
struct DoclingTable {
    #[serde(default)]
    prov: Vec<DoclingProvenance>,
    data: DoclingTableData,
}

#[derive(Debug, Deserialize)]
struct DoclingTableData {
    #[serde(default)]
    table_cells: Vec<DoclingTableCell>,
}

#[derive(Debug, Deserialize)]
struct DoclingTableCell {
    #[serde(default)]
    text: String,
    bbox: DoclingBbox,
}

#[derive(Debug, Deserialize)]
struct DoclingPage {
    size: DoclingPageSize,
}

#[derive(Debug, Deserialize)]
struct DoclingPageSize {
    #[allow(dead_code)]
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct DoclingProvenance {
    page_no: u32,
    bbox: DoclingBbox,
}

#[derive(Debug, Deserialize)]
struct DoclingBbox {
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    #[serde(default = "default_coord_origin")]
    coord_origin: String,
}

fn default_coord_origin() -> String {
    "BOTTOMLEFT".to_string()
}

#[cfg(test)]
mod tests {
    //! Pure parsing tests — exercise the JSON → block mapping without spinning
    //! up an actual docling-serve. Integration with a live sidecar is covered
    //! end-to-end in Phase 1.6f.

    use super::*;

    /// Minimal docling-serve response captured from a real run against
    /// `simple-one-page.pdf` in experiments/docling-probe.
    fn synthetic_response() -> &'static str {
        r#"{
          "status": "success",
          "document": {
            "json_content": {
              "texts": [
                {
                  "label": "section_header",
                  "text": "Hello, world!",
                  "prov": [{"page_no": 1, "bbox": {"l": 72.0, "t": 772.92, "r": 171.0, "b": 756.27, "coord_origin": "BOTTOMLEFT"}}]
                },
                {
                  "label": "text",
                  "text": "The quick brown fox jumps over the lazy dog.",
                  "prov": [{"page_no": 1, "bbox": {"l": 72.0, "t": 730.0, "r": 223.7, "b": 697.1, "coord_origin": "BOTTOMLEFT"}}]
                },
                {
                  "label": "formula",
                  "text": "E = mc^2",
                  "prov": [{"page_no": 1, "bbox": {"l": 100.0, "t": 680.0, "r": 200.0, "b": 660.0, "coord_origin": "BOTTOMLEFT"}}]
                }
              ]
            }
          }
        }"#
    }

    #[test]
    fn classify_role_matches_docling_vocabulary() {
        assert_eq!(classify_role("section_header"), DoclingRole::SectionHeader);
        assert_eq!(classify_role("text"), DoclingRole::Paragraph);
        assert_eq!(classify_role("list_item"), DoclingRole::ListItem);
        assert_eq!(classify_role("formula"), DoclingRole::Formula);
        assert_eq!(classify_role("totally_made_up"), DoclingRole::Other);
    }

    #[test]
    fn formulas_are_not_translated() {
        assert!(!role_should_translate(&DoclingRole::Formula));
        assert!(role_should_translate(&DoclingRole::Paragraph));
        assert!(role_should_translate(&DoclingRole::SectionHeader));
    }

    #[test]
    fn json_parses_into_expected_response_shape() {
        let parsed: DoclingResponse = serde_json::from_str(synthetic_response()).unwrap();
        assert_eq!(parsed.status, "success");
        let texts = parsed
            .document
            .unwrap()
            .json_content
            .unwrap()
            .texts;
        assert_eq!(texts.len(), 3);
        assert_eq!(texts[0].label, "section_header");
        assert_eq!(texts[2].text, "E = mc^2");
    }

    /// End-to-end mapping test: synthetic docling-serve response → Rosetta
    /// blocks + segments with the same `style.pdf` shape generate.rs expects.
    /// This is the substitute for a full sidecar round-trip; the HTTP path
    /// itself is exercised manually in Phase 1.6 cleanup.
    #[test]
    fn map_response_produces_blocks_with_correct_style_pdf() {
        let payload: DoclingResponse = serde_json::from_str(synthetic_response()).unwrap();
        let (blocks, segments) =
            map_response_to_blocks(payload, "test-doc").expect("mapping should succeed");

        // 3 texts total but `formula` is not-translatable → only 2 segments.
        assert_eq!(blocks.len(), 3);
        assert_eq!(segments.len(), 2);

        // section_header → "heading" block_type, translatable
        assert_eq!(blocks[0].block_type, "heading");
        assert_eq!(blocks[0].source_text, "Hello, world!");
        assert!(blocks[0].should_translate);

        // text → "paragraph"
        assert_eq!(blocks[1].block_type, "paragraph");
        assert!(blocks[1].should_translate);

        // formula → "metadata", not translated, status=skipped
        assert_eq!(blocks[2].block_type, "metadata");
        assert!(!blocks[2].should_translate);
        assert_eq!(blocks[2].status, "skipped");

        // style.pdf has all the fields generate.rs reads.
        let style = blocks[0].style.as_ref().unwrap();
        let pdf = style.get("pdf").unwrap();
        assert_eq!(pdf["page"].as_u64(), Some(1));
        assert_eq!(pdf["orderOnPage"].as_u64(), Some(1));
        assert!(pdf["bbox"].is_array());
        assert!(pdf["baselineY"].is_number());
        assert!(pdf["fontSize"].is_number());
        assert_eq!(pdf["doclingLabel"].as_str(), Some("section_header"));
        assert_eq!(pdf["layoutConfidence"].as_str(), Some("high"));

        // Block ids include page + order — must be stable across re-runs.
        assert_eq!(blocks[0].id, "test-doc-p1-b1");
        assert_eq!(blocks[1].id, "test-doc-p1-b2");
        assert_eq!(blocks[2].id, "test-doc-p1-b3");
    }

    #[test]
    fn map_response_image_only_returns_image_only_error() {
        let empty = r#"{
          "status": "success",
          "document": { "json_content": { "texts": [] } }
        }"#;
        let payload: DoclingResponse = serde_json::from_str(empty).unwrap();
        let result = map_response_to_blocks(payload, "test-doc");
        assert!(matches!(result, Err(PdfError::ImageOnly)));
    }

    #[test]
    fn map_response_non_success_status_propagates() {
        let failed = r#"{ "status": "failure", "document": null }"#;
        let payload: DoclingResponse = serde_json::from_str(failed).unwrap();
        let result = map_response_to_blocks(payload, "test-doc");
        assert!(matches!(result, Err(PdfError::Parse(_))));
    }

    /// Real-sidecar integration test. Skipped by default — run with
    /// `cargo test -- --ignored` AFTER starting docling-serve manually:
    ///
    ///     cd experiments/docling-probe
    ///     ROSETTA_DOCLING_SERVE_BASE_URL=http://127.0.0.1:5005 \
    ///         .venv/bin/docling-serve run --port 5005 &
    ///     # wait ~10s for warmup, then in another terminal:
    ///     ROSETTA_DOCLING_SERVE_BASE_URL=http://127.0.0.1:5005 \
    ///         cargo test --lib --test-threads=1 -- --ignored extract_via_docling
    #[test]
    #[ignore]
    fn extract_via_docling_round_trips_against_running_sidecar() {
        let Some(base_url) = std::env::var("ROSETTA_DOCLING_SERVE_BASE_URL").ok() else {
            eprintln!("ROSETTA_DOCLING_SERVE_BASE_URL not set; skipping");
            return;
        };
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/pdf/google-doc-document.pdf");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (blocks, segments) = runtime
            .block_on(extract_via_docling(&base_url, "manual-test", &fixture))
            .expect("docling-serve must be reachable and convert successfully");
        assert!(!blocks.is_empty(), "expected at least one block");
        assert!(!segments.is_empty(), "expected at least one segment");
        let first = &blocks[0];
        assert_eq!(first.block_type, "heading", "first block: {:?}", first);
        assert_eq!(first.source_text, "Example document");
    }
}
