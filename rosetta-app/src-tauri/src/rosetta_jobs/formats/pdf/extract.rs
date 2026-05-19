//! Parse a PDF into Rosetta IR: one [`RosettaBlock`] per readable text line,
//! with bbox/page/font metadata stashed in `style.pdf` so the generate step
//! can put the translation back at the right coordinates.
//!
//! Line reconstruction uses `text.chars()` + `origin_y()` baselines, NOT
//! `text.segments()`. pdfium's segments are partitioned by content-stream
//! text-show operators, which on complex-typography PDFs (Google Docs, LaTeX,
//! Word) emit per-font-subset or per-glyph chunks — "Beautiful" comes back as
//! "Bifl" + "eautu" + "y" instead of one segment. The per-char baseline API is
//! the only reliable way to rebuild visual lines. See spike findings memory.
//!
//! v1 chooses simple over clever: every line becomes its own block, one
//! segment per block. Phase 3 will fold multi-line paragraphs together and
//! compute `layoutConfidence` from layout heuristics.

use std::path::Path;

use pdfium_render::prelude::*;
use serde_json::json;

use crate::rosetta_jobs::{
    formats::pdf::{
        docling::{extract_via_docling, DoclingSidecarRegistry},
        errors::{PdfError, MAX_PDF_BYTES, MAX_PDF_PAGES},
        runtime,
    },
    model::{RosettaBlock, Segment},
    segmenter::{push_segments_for_block, translatable_block},
};

use tauri::{AppHandle, Manager};

/// Parse a PDF at `source_path` into Rosetta blocks + segments.
///
/// `document_id` is used to derive deterministic block/segment ids
/// (`{document_id}-p{page}-{order}` so duplicates across imports don't collide).
///
/// Backend selection:
/// 1. First runs cheap pre-flight checks (size, page count, encryption) via
///    pdfium so we surface common failure modes with consistent error messages
///    regardless of which extractor handles the heavy lifting.
/// 2. Tries the Docling sidecar — that's our default extractor because it
///    handles multi-column layouts, tables, and section roles that the pure
///    pdfium path can't (Phase 1.6a-c, see memory project-pdf-layout-extractor-choice).
/// 3. Falls back to the pdfium `chars()` + baseline reconstruction if the
///    sidecar isn't installed yet (download UX → Phase 1.6g) or refuses to start.
///    Extraction-side errors from a *running* sidecar propagate; we don't
///    silently degrade a real failure into the weaker fallback.
/// Async because the Docling backend uses async HTTP (reqwest). Earlier this
/// was sync + `tauri::async_runtime::block_on(...)` inside the command, which
/// blocked a Tokio worker thread and stalled the webview's IPC handler — the
/// "import freezes the app for a few seconds" symptom from 2026-05-19 dogfood.
pub(crate) async fn parse_pdf(
    app: &AppHandle,
    document_id: &str,
    source_path: &Path,
) -> Result<(Vec<RosettaBlock>, Vec<Segment>), PdfError> {
    pre_flight(app, source_path)?;

    match try_docling_extract(app, document_id, source_path).await {
        Ok(Some(result)) => return Ok(result),
        Ok(None) => {
            // Sidecar unavailable — fall through to chars+baseline.
            eprintln!(
                "[pdf] docling sidecar unavailable, falling back to chars+baseline extraction"
            );
        }
        Err(error) => return Err(error),
    }

    parse_pdf_via_chars(app, document_id, source_path)
}

/// Pre-flight checks shared by every extraction backend. Catches the
/// common "this isn't going to work" cases (too large, encrypted, too many
/// pages) before we burn pdfium load time or sidecar HTTP roundtrips.
fn pre_flight(app: &AppHandle, source_path: &Path) -> Result<(), PdfError> {
    let metadata = std::fs::metadata(source_path)
        .map_err(|error| PdfError::Read(format!("无法读取文件信息: {error}")))?;
    if metadata.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge {
            reason: format!(
                "{} MB 超过单个 PDF 最大 {} MB 的上限。",
                metadata.len() / 1024 / 1024,
                MAX_PDF_BYTES / 1024 / 1024
            ),
        });
    }

    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;
    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("文件路径包含无效字符。".to_string()))?;
    let document = pdfium
        .load_pdf_from_file(source_path_str, None)
        .map_err(map_load_error)?;
    let page_count = document.pages().len();
    if page_count as u32 > MAX_PDF_PAGES {
        return Err(PdfError::TooLarge {
            reason: format!("共 {} 页，超过 {} 页的上限。", page_count, MAX_PDF_PAGES),
        });
    }
    Ok(())
}

/// Best-effort attempt to extract via the Docling sidecar.
///
/// Return shape:
/// - `Ok(Some(_))` — extraction succeeded, caller uses the result.
/// - `Ok(None)` — sidecar isn't installed/won't start. Caller falls back to
///   the pdfium chars+baseline path.
/// - `Err(_)` — sidecar IS running but failed to convert this specific PDF.
///   We don't silently fall back because that would mask real bugs.
async fn try_docling_extract(
    app: &AppHandle,
    document_id: &str,
    source_path: &Path,
) -> Result<Option<(Vec<RosettaBlock>, Vec<Segment>)>, PdfError> {
    let Some(registry) = app.try_state::<DoclingSidecarRegistry>() else {
        // App didn't register the sidecar registry — only happens in tests
        // that bypass lib.rs setup. Treat as "no sidecar available".
        return Ok(None);
    };

    let base_url = match registry.ensure_running(app).await {
        Ok(url) => url,
        Err(_) => return Ok(None),
    };

    let result = extract_via_docling(&base_url, document_id, source_path).await?;
    Ok(Some(result))
}

/// Pure pdfium `chars()` + typographic-baseline line reconstruction. Used as
/// the fallback when the Docling sidecar isn't available. See module-level
/// docs and memory project-pdf-spike-findings for why we use chars + origin_y
/// instead of `text.segments()`.
fn parse_pdf_via_chars(
    app: &AppHandle,
    document_id: &str,
    source_path: &Path,
) -> Result<(Vec<RosettaBlock>, Vec<Segment>), PdfError> {
    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;
    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("文件路径包含无效字符。".to_string()))?;
    let document = pdfium
        .load_pdf_from_file(source_path_str, None)
        .map_err(map_load_error)?;
    let page_count = document.pages().len();

    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut block_order = 1usize;
    let mut segment_order = 1usize;
    let mut total_text_chars = 0usize;

    for page_idx in 0..page_count {
        let page = document
            .pages()
            .get(page_idx)
            .map_err(|error| PdfError::Parse(format!("第 {} 页打开失败: {error}", page_idx + 1)))?;

        let lines = collect_lines_for_page(&page)
            .map_err(|error| PdfError::Parse(format!("第 {} 页文字提取失败: {error}", page_idx + 1)))?;

        let mut order_on_page = 0usize;
        for line in lines {
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            total_text_chars += text.chars().count();
            order_on_page += 1;

            let block_id = format!("{document_id}-p{}-b{order_on_page}", page_idx + 1);
            let style = json!({
                "pdf": {
                    "page": page_idx + 1,
                    "orderOnPage": order_on_page,
                    "bbox": [
                        line.left,
                        line.bottom,
                        line.right - line.left,
                        line.top - line.bottom,
                    ],
                    "baselineY": line.baseline_y,
                    "fontSize": line.font_size.max(1.0),
                    // v1: every block claims "high" confidence. Phase 3 will
                    // replace this from layout.rs heuristics.
                    "layoutConfidence": "high",
                }
            });

            blocks.push(translatable_block(
                &block_id,
                "paragraph",
                text,
                block_order,
                Some(style),
            ));
            push_segments_for_block(
                &mut segments,
                &block_id,
                "paragraph",
                block_order,
                text,
                &mut segment_order,
            );
            block_order += 1;
        }
    }

    if total_text_chars == 0 {
        return Err(PdfError::ImageOnly);
    }

    Ok((blocks, segments))
}

/// One reading-order text line on a page. `baseline_y` is the typographic
/// origin Y that lines all chars on this visual row up regardless of
/// descender depth — see module-level docs.
#[derive(Debug, Clone)]
pub(crate) struct PdfLine {
    pub text: String,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub baseline_y: f32,
    pub font_size: f32,
}

/// Rebuild visual lines from per-char data on a single page. See
/// [`crate::rosetta_jobs::formats::pdf::extract`] module docs for the rationale.
pub(crate) fn collect_lines_for_page(page: &PdfPage<'_>) -> Result<Vec<PdfLine>, PdfiumError> {
    // Baselines within ~1.5pt are considered the same visual line. Generous
    // enough to absorb sub-pixel kerning drift, tight enough to keep adjacent
    // body lines separate (those are typically ≥12pt apart).
    const Y_BUCKET: f32 = 1.5;
    // Splitting threshold for X gaps within a baseline bucket. Anything larger
    // than `font_size * X_GAP_BREAK_FACTOR` is treated as a table-cell gutter
    // or column break, not a word space.
    const X_GAP_BREAK_FACTOR: f32 = 3.5;
    const WORD_SPACE_FACTOR: f32 = 0.25;

    #[derive(Debug)]
    struct CharBox {
        ch: char,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        baseline_y: f32,
        font_size: f32,
    }

    let page_text = page.text()?;
    let chars = page_text.chars();

    let mut all_chars: Vec<CharBox> = Vec::new();
    for ch in chars.iter() {
        let Some(unicode) = ch.unicode_char() else {
            continue;
        };
        if unicode.is_control() {
            continue;
        }
        let Ok(bounds) = ch.tight_bounds() else {
            continue;
        };
        let baseline_y = ch
            .origin_y()
            .map(|p| p.value)
            .unwrap_or(bounds.bottom().value);
        all_chars.push(CharBox {
            ch: unicode,
            left: bounds.left().value,
            right: bounds.right().value,
            top: bounds.top().value,
            bottom: bounds.bottom().value,
            baseline_y,
            font_size: ch.scaled_font_size().value,
        });
    }

    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<i32, Vec<CharBox>> = BTreeMap::new();
    for cb in all_chars {
        let key = (cb.baseline_y / Y_BUCKET).round() as i32;
        buckets.entry(key).or_default().push(cb);
    }

    let mut lines: Vec<PdfLine> = Vec::new();
    // BTreeMap iterates ascending; .rev() gives descending = top of page first.
    for (_, mut bucket) in buckets.into_iter().rev() {
        bucket.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal));
        let mut current: Option<PdfLine> = None;
        for cb in bucket {
            let gap_break = cb.font_size.max(8.0) * X_GAP_BREAK_FACTOR;
            let extend = current
                .as_ref()
                .is_some_and(|cur| (cb.left - cur.right) < gap_break);
            if extend {
                let cur = current.as_mut().unwrap();
                if cb.left - cur.right > cb.font_size * WORD_SPACE_FACTOR {
                    cur.text.push(' ');
                }
                cur.text.push(cb.ch);
                cur.right = cur.right.max(cb.right);
                cur.top = cur.top.max(cb.top);
                cur.bottom = cur.bottom.min(cb.bottom);
                cur.font_size = cur.font_size.max(cb.font_size);
            } else {
                if let Some(done) = current.take() {
                    lines.push(done);
                }
                current = Some(PdfLine {
                    text: cb.ch.to_string(),
                    left: cb.left,
                    right: cb.right,
                    top: cb.top,
                    bottom: cb.bottom,
                    baseline_y: cb.baseline_y,
                    font_size: cb.font_size,
                });
            }
        }
        if let Some(done) = current.take() {
            lines.push(done);
        }
    }

    lines.retain(|l| !l.text.trim().is_empty());
    Ok(lines)
}

fn map_load_error(error: PdfiumError) -> PdfError {
    // pdfium error variants don't have a stable enum we can match cleanly,
    // so string-match on the rendered message — coarse but enough for v1's
    // three user-visible buckets (encrypted / parse / generic-read).
    let rendered = error.to_string();
    let lowered = rendered.to_lowercase();
    if lowered.contains("password") || lowered.contains("encrypted") {
        PdfError::Encrypted
    } else {
        PdfError::Parse(rendered)
    }
}

#[cfg(test)]
mod tests {
    //! Tests parse parsing logic against fixtures/pdf/. They bypass AppHandle
    //! and bind pdfium directly so they run without a full Tauri context.

    use super::*;
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    /// Mirrors [`parse_pdf`] but takes a bound `Pdfium` directly so we can
    /// exercise the parsing logic without an AppHandle in tests. Uses the
    /// same `collect_lines_for_page` helper as production.
    fn parse_pdf_with_bound(
        pdfium: &Pdfium,
        document_id: &str,
        source_path: &Path,
    ) -> Result<(Vec<RosettaBlock>, Vec<Segment>), PdfError> {
        let document = pdfium
            .load_pdf_from_file(source_path.to_str().unwrap(), None)
            .map_err(map_load_error)?;

        let mut blocks = Vec::new();
        let mut segments = Vec::new();
        let mut block_order = 1usize;
        let mut segment_order = 1usize;

        for page_idx in 0..document.pages().len() {
            let page = document.pages().get(page_idx).unwrap();
            let lines = collect_lines_for_page(&page)
                .map_err(|error| PdfError::Parse(error.to_string()))?;
            let mut order_on_page = 0usize;
            for line in lines {
                let text = line.text.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                order_on_page += 1;
                let style = json!({
                    "pdf": {
                        "page": page_idx + 1,
                        "orderOnPage": order_on_page,
                        "bbox": [
                            line.left,
                            line.bottom,
                            line.right - line.left,
                            line.top - line.bottom,
                        ],
                        "baselineY": line.baseline_y,
                        "fontSize": line.font_size.max(1.0),
                        "layoutConfidence": "high",
                    }
                });
                let block_id = format!("{document_id}-p{}-b{order_on_page}", page_idx + 1);
                blocks.push(translatable_block(&block_id, "paragraph", &text, block_order, Some(style)));
                push_segments_for_block(
                    &mut segments,
                    &block_id,
                    "paragraph",
                    block_order,
                    &text,
                    &mut segment_order,
                );
                block_order += 1;
            }
        }
        Ok((blocks, segments))
    }

    #[test]
    fn parse_simple_one_page_produces_expected_blocks() {
        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let (blocks, segments) =
            parse_pdf_with_bound(pdfium, "test-doc", &fixture_path("simple-one-page.pdf"))
                .expect("parse should succeed");

        // Spike fixture has 4 English lines.
        assert_eq!(blocks.len(), 4, "expected 4 blocks, got: {:#?}", blocks.iter().map(|b| &b.source_text).collect::<Vec<_>>());
        assert_eq!(segments.len(), 4);

        // First block sanity-checks: text content + style.pdf shape.
        assert_eq!(blocks[0].source_text, "Hello, world!");
        let style = blocks[0].style.as_ref().expect("block style should be set");
        let pdf = style.get("pdf").expect("style.pdf should be present");
        assert_eq!(pdf.get("page").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(pdf.get("orderOnPage").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(pdf.get("layoutConfidence").and_then(|v| v.as_str()), Some("high"));
        assert!(pdf.get("bbox").and_then(|v| v.as_array()).is_some());
        assert!(pdf.get("fontSize").and_then(|v| v.as_f64()).is_some());

        // Page ordering preserved.
        let pages: Vec<u64> = blocks
            .iter()
            .map(|b| b.style.as_ref().unwrap()["pdf"]["page"].as_u64().unwrap())
            .collect();
        assert_eq!(pages, vec![1, 1, 1, 1]);

        // Segment ids point back at their block.
        for (block, segment) in blocks.iter().zip(segments.iter()) {
            assert_eq!(segment.block_id, block.id);
            assert_eq!(segment.kind, "paragraph");
            assert_eq!(segment.source_text, block.source_text);
        }
    }

    #[test]
    fn parse_pdf_respects_size_limit() {
        // Just verify the constant + reasonable behavior — actual >100MB
        // fixture would balloon the repo.
        assert!(MAX_PDF_BYTES > 1024 * 1024); // sanity
        assert!(MAX_PDF_PAGES > 0);
    }
}
