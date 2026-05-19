//! Render a translated PDF from a source PDF + Rosetta blocks holding the
//! translations.
//!
//! The pipeline ported from `experiments/pdf-spike`:
//!
//!   1. Open source via pdfium, create a fresh destination doc.
//!   2. Embed Source Han Sans CN OTF into the destination doc (it becomes a
//!      CID font in the output PDF, viewable on any third-party reader).
//!   3. For each source page:
//!        a. `copy_page_from_document` so images / vectors / background are
//!           preserved byte-for-byte.
//!        b. Set the destination page's regen strategy to Manual (else
//!           pdfium-render 0.9.1 segfaults on the first set_text("") of a
//!           freshly-imported page — see memory: project-pdf-spike-findings).
//!        c. Walk the page's objects; for each `PdfPageObject::Text`, call
//!           `set_text("")` to wipe original glyphs. We deliberately do NOT
//!           use `remove_object_at_index` — same segfault path.
//!        d. For each block whose `style.pdf.page` matches this page, draw
//!           the translated text at the recorded bbox.
//!        e. `regenerate_content` once at the end.
//!   4. Save destination.

use std::path::Path;

use pdfium_render::prelude::*;
use serde_json::Value;
use tauri::AppHandle;

use crate::rosetta_jobs::{
    formats::pdf::{
        errors::PdfError,
        runtime,
    },
    model::{RosettaBlock, RosettaDocument},
};

/// One translation row to draw on a page. Captured up front so we don't
/// re-borrow `document.blocks` while mutating pdfium state.
///
/// `baseline_y` is the typographic origin Y for the row — what pdfium's
/// `PdfPageTextObject::translate` expects so the new text sits exactly where
/// the original sat. `bbox_x` is the left edge of the original line.
/// `bbox_width` is used to auto-shrink the font when a translation is wider
/// than the source span (very common for CN→EN where English is ~1.5-2× the
/// width of compact Chinese glyphs at the same font size).
#[derive(Debug)]
struct Placement {
    page: u32,
    bbox_x: f32,
    bbox_width: f32,
    baseline_y: f32,
    font_size: f32,
    text: String,
}

pub(crate) fn render_translated_pdf(
    app: &AppHandle,
    document: &RosettaDocument,
    source_path: &Path,
    output_path: &Path,
) -> Result<(), PdfError> {
    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;

    let font_path = runtime::locate_cjk_font(app)
        .ok_or_else(|| PdfError::RuntimeMissing("找不到 CJK 字体文件。".to_string()))?;
    let font_bytes = std::fs::read(&font_path)
        .map_err(|error| PdfError::RuntimeMissing(format!("读取 CJK 字体失败: {error}")))?;

    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("源文件路径包含无效字符。".to_string()))?;
    let src_doc = pdfium
        .load_pdf_from_file(source_path_str, None)
        .map_err(|error| PdfError::Parse(format!("打开源 PDF 失败: {error}")))?;

    let mut dst_doc = pdfium
        .create_new_pdf()
        .map_err(|error| PdfError::Parse(format!("创建目标 PDF 失败: {error}")))?;
    // Source Han Sans CN is OpenType/CFF (PostScript outlines in an OTF
    // container). We tried `load_type1_from_bytes` (the CFF path in
    // pdfium-render, which emits a single Type 0 / CIDFontType0 font) hoping
    // pdfjs would handle the resulting subset cleanly. It didn't, AND macOS
    // Preview / Quartz also rendered garbled glyphs for longer documents.
    // `load_true_type_from_bytes` produces multiple CIDFontType2 subsets
    // (one per page) but each subset is well-formed enough that Preview /
    // Quartz / pdfium-on-readback all render it correctly. Since the in-app
    // preview now rasterizes via pdfium (see [rasterize.rs]), pdfjs
    // compatibility is no longer a constraint — only "real PDF readers must
    // open the exported file" matters.
    let cjk_token = dst_doc
        .fonts_mut()
        .load_true_type_from_bytes(&font_bytes, true)
        .map_err(|error| PdfError::Parse(format!("嵌入 CJK 字体失败: {error}")))?;

    let placements = collect_placements(document);

    let total_pages = src_doc.pages().len();
    for page_idx in 0..total_pages {
        let dst_idx = dst_doc.pages().len();
        dst_doc
            .pages_mut()
            .copy_page_from_document(&src_doc, page_idx, dst_idx)
            .map_err(|error| {
                PdfError::Parse(format!("复制第 {} 页失败: {error}", page_idx + 1))
            })?;

        // IMPORTANT: pages() not pages_mut() here — see spike findings memory.
        let mut dst_page = dst_doc
            .pages()
            .get(dst_idx)
            .map_err(|error| PdfError::Parse(format!("打开新页失败: {error}")))?;
        dst_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

        clear_text_objects(&mut dst_page)?;

        let page_number = (page_idx + 1) as u32;
        for placement in placements.iter().filter(|p| p.page == page_number) {
            if placement.text.is_empty() {
                continue;
            }
            let fitted = fit_font_size(&placement.text, placement.font_size, placement.bbox_width);
            let mut text_obj = PdfPageTextObject::new(
                &dst_doc,
                &placement.text,
                cjk_token,
                PdfPoints::new(fitted),
            )
            .map_err(|error| PdfError::Parse(format!("创建译文对象失败: {error}")))?;
            text_obj
                .translate(
                    PdfPoints::new(placement.bbox_x),
                    PdfPoints::new(placement.baseline_y),
                )
                .map_err(|error| PdfError::Parse(format!("译文定位失败: {error}")))?;
            dst_page
                .objects_mut()
                .add_text_object(text_obj)
                .map_err(|error| PdfError::Parse(format!("添加译文失败: {error}")))?;
        }

        dst_page
            .regenerate_content()
            .map_err(|error| PdfError::Parse(format!("第 {} 页内容生成失败: {error}", page_number)))?;
    }

    let output_path_str = output_path
        .to_str()
        .ok_or_else(|| PdfError::Read("输出路径包含无效字符。".to_string()))?;
    dst_doc
        .save_to_file(output_path_str)
        .map_err(|error| PdfError::Parse(format!("写入译文 PDF 失败: {error}")))?;

    Ok(())
}

/// Wipe every text object on the page. We use `set_text("")` instead of
/// `remove_object_at_index` because the latter crashes pdfium on a page that
/// was just imported via FPDF_ImportPages.
fn clear_text_objects(dst_page: &mut PdfPage) -> Result<(), PdfError> {
    let total = dst_page.objects().len();
    for i in 0..total {
        let mut obj = dst_page
            .objects()
            .get(i)
            .map_err(|error| PdfError::Parse(format!("读取页对象 {} 失败: {error}", i)))?;
        if let PdfPageObject::Text(text_obj) = &mut obj {
            text_obj
                .set_text("")
                .map_err(|error| PdfError::Parse(format!("清空原文字失败: {error}")))?;
        }
    }
    Ok(())
}

fn collect_placements(document: &RosettaDocument) -> Vec<Placement> {
    document
        .blocks
        .iter()
        .filter_map(placement_from_block)
        .collect()
}

fn placement_from_block(block: &RosettaBlock) -> Option<Placement> {
    let translated = block.translated_text.as_ref()?;
    if translated.trim().is_empty() {
        return None;
    }
    let style = block.style.as_ref()?;
    let pdf = style.get("pdf")?;
    let page = pdf.get("page").and_then(Value::as_u64)? as u32;
    let bbox = pdf.get("bbox").and_then(Value::as_array)?;
    if bbox.len() != 4 {
        return None;
    }
    let bbox_x = bbox[0].as_f64()? as f32;
    let bbox_y = bbox[1].as_f64()? as f32;
    let bbox_width = bbox[2].as_f64()? as f32;
    let height = bbox[3].as_f64()? as f32;
    let baseline_y = pdf
        .get("baselineY")
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(bbox_y);
    let font_size = pdf
        .get("fontSize")
        .and_then(Value::as_f64)
        .unwrap_or(height as f64) as f32;
    Some(Placement {
        page,
        bbox_x,
        bbox_width,
        baseline_y,
        font_size,
        text: translated.clone(),
    })
}

/// Pick a font size that fits the translation inside the source bbox width.
///
/// Source Han Sans CN at N pt → Latin glyphs avg ~0.55*N pt wide, CJK glyphs
/// ~N pt wide (full-width). When CN→EN translation makes a 60pt-wide block
/// hold 15 Latin chars, the natural-size text would overflow into the next
/// block on the same line. This shrinks the font *down* to fit. Never grows
/// beyond the source-block's font, and never goes below MIN_FONT_PT so the
/// output stays readable.
fn fit_font_size(text: &str, source_font: f32, bbox_width: f32) -> f32 {
    const MIN_FONT_PT: f32 = 5.0;
    const LATIN_WIDTH_FACTOR: f32 = 0.55;
    const CJK_WIDTH_FACTOR: f32 = 1.0;
    const SAFETY: f32 = 0.95;

    if bbox_width <= 0.0 || text.is_empty() {
        return source_font;
    }
    let est_em_units: f32 = text
        .chars()
        .map(|c| {
            if is_full_width(c) {
                CJK_WIDTH_FACTOR
            } else {
                LATIN_WIDTH_FACTOR
            }
        })
        .sum();
    let est_width = source_font * est_em_units;
    if est_width <= bbox_width {
        return source_font;
    }
    let scale = (bbox_width / est_width) * SAFETY;
    (source_font * scale).max(MIN_FONT_PT)
}

fn is_full_width(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x303F     // CJK punctuation
        | 0x3400..=0x4DBF   // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0xF900..=0xFAFF   // CJK Compatibility Ideographs
        | 0xFF00..=0xFFEF   // Halfwidth/Fullwidth Forms
        | 0x3040..=0x30FF   // Hiragana, Katakana
        | 0xAC00..=0xD7AF   // Hangul Syllables
    )
}

#[cfg(test)]
mod tests {
    //! Integration test: parse fixture → fake-translate → render → reopen
    //! and confirm the translated PDF's text layer holds only Chinese (the
    //! same check the spike runs).

    use super::*;
    use crate::rosetta_jobs::formats::pdf::extract::collect_lines_for_page;
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, font_path, pdfium_test_lock, shared_pdfium,
    };
    use crate::rosetta_jobs::model::{RosettaBlock, RosettaSourceFile, SCHEMA_VERSION};

    /// Standalone version of [`render_translated_pdf`] that takes an already-
    /// bound `Pdfium` so we can exercise the generate logic without an
    /// AppHandle in tests.
    #[allow(clippy::too_many_lines)]
    fn render_with_bound(
        pdfium: &Pdfium,
        font_bytes: &[u8],
        document: &RosettaDocument,
        source_path: &Path,
        output_path: &Path,
    ) -> Result<(), PdfError> {
        let src_doc = pdfium
            .load_pdf_from_file(source_path.to_str().unwrap(), None)
            .map_err(|error| PdfError::Parse(format!("open source: {error}")))?;
        let mut dst_doc = pdfium
            .create_new_pdf()
            .map_err(|error| PdfError::Parse(format!("create dst: {error}")))?;
        let cjk_token = dst_doc
            .fonts_mut()
            .load_true_type_from_bytes(font_bytes, true)
            .map_err(|error| PdfError::Parse(format!("load font: {error}")))?;

        let placements = collect_placements(document);

        for page_idx in 0..src_doc.pages().len() {
            let dst_idx = dst_doc.pages().len();
            dst_doc
                .pages_mut()
                .copy_page_from_document(&src_doc, page_idx, dst_idx)
                .map_err(|error| PdfError::Parse(format!("copy page: {error}")))?;
            let mut dst_page = dst_doc.pages().get(dst_idx).unwrap();
            dst_page
                .set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            clear_text_objects(&mut dst_page)?;

            let page_number = (page_idx + 1) as u32;
            for placement in placements.iter().filter(|p| p.page == page_number) {
                if placement.text.is_empty() {
                    continue;
                }
                let fitted = fit_font_size(&placement.text, placement.font_size, placement.bbox_width);
                let mut t = PdfPageTextObject::new(
                    &dst_doc,
                    &placement.text,
                    cjk_token,
                    PdfPoints::new(fitted),
                )
                .map_err(|error| PdfError::Parse(format!("text obj: {error}")))?;
                t.translate(
                    PdfPoints::new(placement.bbox_x),
                    PdfPoints::new(placement.baseline_y),
                )
                .unwrap();
                dst_page.objects_mut().add_text_object(t).unwrap();
            }
            dst_page.regenerate_content().unwrap();
        }

        dst_doc
            .save_to_file(output_path.to_str().unwrap())
            .map_err(|error| PdfError::Parse(format!("save: {error}")))?;
        Ok(())
    }

    #[test]
    fn full_pipeline_parses_then_renders_then_text_layer_is_chinese() {
        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let font_bytes = std::fs::read(font_path()).expect("font must be staged");
        let source = fixture_path("simple-one-page.pdf");

        // ---- 1. Parse via the production line-reconstruction helper.
        let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let mut blocks: Vec<RosettaBlock> = Vec::new();
        let mut block_order = 1usize;
        for page_idx in 0..src_doc.pages().len() {
            let page = src_doc.pages().get(page_idx).unwrap();
            let lines = collect_lines_for_page(&page).unwrap();
            let mut order_on_page = 0usize;
            for line in lines {
                let trimmed = line.text.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                order_on_page += 1;
                let style = serde_json::json!({
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
                blocks.push(RosettaBlock {
                    id: format!("doc-p{}-b{order_on_page}", page_idx + 1),
                    file_id: Some("file-1".to_string()),
                    block_type: "paragraph".to_string(),
                    source_text: trimmed.clone(),
                    translated_text: Some(fake_translate(&trimmed)),
                    should_translate: true,
                    order: block_order,
                    path: Some(format!("blocks.{block_order}")),
                    style: Some(style),
                    status: "translated".to_string(),
                });
                block_order += 1;
            }
        }
        drop(src_doc);

        let document = RosettaDocument {
            schema_version: SCHEMA_VERSION,
            id: "doc".to_string(),
            filename: "simple-one-page.pdf".to_string(),
            format: "pdf".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            files: vec![RosettaSourceFile {
                id: "file-1".to_string(),
                filename: "simple-one-page.pdf".to_string(),
                relative_path: "simple-one-page.pdf".to_string(),
                format: "pdf".to_string(),
                source_lang: Some("en".to_string()),
                target_lang: Some("zh-CN".to_string()),
                translation_status: "completed".to_string(),
                segment_count: blocks.len(),
                completed_segments: blocks.len(),
                failed_segments: 0,
                translating_segments: 0,
                block_ids: blocks.iter().map(|b| b.id.clone()).collect(),
            }],
            blocks,
            extraction_status: None,
        };

        // ---- 2. Render translated PDF into a tempfile.
        let tmp = std::env::temp_dir().join("rosetta-pdf-gen-test.pdf");
        let _ = std::fs::remove_file(&tmp);
        render_with_bound(pdfium, &font_bytes, &document, &source, &tmp)
            .expect("render should succeed");
        assert!(tmp.is_file(), "output PDF should exist");
        let size = std::fs::metadata(&tmp).unwrap().len();
        assert!(size > 1024, "output PDF should be non-trivial, got {size} bytes");

        // ---- 3. Re-open + audit text layer.
        let translated_doc = pdfium.load_pdf_from_file(tmp.to_str().unwrap(), None).unwrap();
        let mut extracted_text = String::new();
        for page_idx in 0..translated_doc.pages().len() {
            let page = translated_doc.pages().get(page_idx).unwrap();
            let text = page.text().unwrap();
            for seg in text.segments().iter() {
                extracted_text.push_str(&seg.text());
                extracted_text.push('\n');
            }
        }
        // Confirm copy/paste gets Chinese, not English.
        assert!(
            extracted_text.contains("你好") || extracted_text.contains("译"),
            "text layer should contain Chinese; got: {extracted_text:?}"
        );
        assert!(
            !extracted_text.contains("Hello, world!"),
            "text layer should not leak source English; got: {extracted_text:?}"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// Stub "translation": prefix every line with `[译]` so we can verify the
    /// generate path round-trips text without relying on the real RWKV.
    fn fake_translate(source: &str) -> String {
        if source.contains("Hello") {
            "你好，世界！".to_string()
        } else {
            format!("[译]{source}")
        }
    }
}
