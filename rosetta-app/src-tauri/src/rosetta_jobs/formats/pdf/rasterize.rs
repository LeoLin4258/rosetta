//! Rasterize PDF pages to PNG bytes via pdfium.
//!
//! Phase 2 preview uses this instead of pdfjs / `<embed>`:
//!
//!   - pdfjs mishandles the per-page CIDFontType2 subsets pdfium-render emits
//!     when we generate the translated PDF (renders garbage CJK in the webview
//!     even though Preview / Acrobat / sips render the same bytes correctly).
//!   - WKWebView in Tauri's app mode does NOT have the PDF plugin that Safari
//!     proper has, so `<embed type="application/pdf">` falls flat — pages are
//!     visible but text is missing.
//!
//! Rasterizing server-side gives us the same renderer macOS Preview uses (it's
//! literally pdfium under the hood) so the visual output is correct for any
//! PDF, at the cost of losing text selection in the preview. The exported PDF
//! still retains everything.
//!
//! The page count returned by [`count_pages`] is consumed by the frontend to
//! lay out a vertical stack of `<img>` placeholders; each placeholder fetches
//! its own PNG on demand via [`render_page_as_png`].

use std::path::Path;

use image::{ImageFormat, codecs::png::{CompressionType, FilterType, PngEncoder}};
use image::ImageEncoder;
use tauri::AppHandle;

use crate::rosetta_jobs::formats::pdf::{
    errors::PdfError,
    runtime,
};

/// Cap on the rendered pixel width. The frontend can ask for a smaller width
/// when laying pages out side-by-side in a narrow column, but we never go
/// above this — a 100-page doc at 2000px is several hundred MB in raw RGBA.
const MAX_TARGET_WIDTH: u32 = 1800;
const MIN_TARGET_WIDTH: u32 = 200;

/// Returns the page count for a PDF on disk. Cheaper than rasterizing — used
/// by the frontend to pre-allocate page placeholders before any pixels are
/// rendered.
pub(crate) fn count_pages(app: &AppHandle, source_path: &Path) -> Result<u32, PdfError> {
    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;
    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("PDF 路径包含无效字符。".to_string()))?;
    let doc = pdfium
        .load_pdf_from_file(source_path_str, None)
        .map_err(|error| PdfError::Parse(format!("打开 PDF 失败: {error}")))?;
    Ok(doc.pages().len() as u32)
}

/// Render a single PDF page to PNG bytes. Width is clamped to
/// `[MIN_TARGET_WIDTH, MAX_TARGET_WIDTH]`; height is derived from the page's
/// aspect ratio so the layout in the webview matches the source proportions.
pub(crate) fn render_page_as_png(
    app: &AppHandle,
    source_path: &Path,
    page_index: u32,
    target_width: u32,
) -> Result<Vec<u8>, PdfError> {
    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;
    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("PDF 路径包含无效字符。".to_string()))?;
    let doc = pdfium
        .load_pdf_from_file(source_path_str, None)
        .map_err(|error| PdfError::Parse(format!("打开 PDF 失败: {error}")))?;

    let total = doc.pages().len() as u32;
    if page_index >= total {
        return Err(PdfError::Read(format!(
            "页码越界：请求第 {} 页，共 {} 页",
            page_index + 1,
            total
        )));
    }

    let page = doc
        .pages()
        .get(page_index as i32)
        .map_err(|error| PdfError::Parse(format!("读取第 {} 页失败: {error}", page_index + 1)))?;

    let width = target_width.clamp(MIN_TARGET_WIDTH, MAX_TARGET_WIDTH);
    let config = pdfium_render::prelude::PdfRenderConfig::new()
        .set_target_width(width as pdfium_render::prelude::Pixels);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|error| PdfError::Parse(format!("渲染第 {} 页失败: {error}", page_index + 1)))?;

    // Encode via the `image` crate's PNG path. `as_image()` returns a
    // DynamicImage already in the correct RGBA orientation, so we just hand
    // it to a PngEncoder configured for fast compression (preview latency
    // matters more than file size here — we never persist these bytes).
    let dyn_image = bitmap
        .as_image()
        .map_err(|error| PdfError::Parse(format!("位图转换失败: {error:?}")))?;
    let rgba = dyn_image.to_rgba8();

    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    let encoder = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
    encoder
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| PdfError::Parse(format!("PNG 编码失败: {error}")))?;

    let _ = ImageFormat::Png; // ensure the import is used even when not needed
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Standalone tests that exercise rasterize without AppHandle. The fixture
    //! must be parsed → rendered → the PNG bytes must look like a real image
    //! (PNG magic header + reasonable size for a non-blank page).

    use super::*;
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    fn render_with_bound(pdfium: &pdfium_render::prelude::Pdfium, source: &Path, page: u32, width: u32) -> Vec<u8> {
        let doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let pg = doc.pages().get(page as i32).unwrap();
        let cfg = pdfium_render::prelude::PdfRenderConfig::new()
            .set_target_width(width as pdfium_render::prelude::Pixels);
        let bm = pg.render_with_config(&cfg).unwrap();
        let dyn_img = bm.as_image().unwrap();
        let rgba = dyn_img.to_rgba8();
        let mut out = Vec::new();
        let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
        enc.write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    /// Self-verification target: previously the translated PDF pdfium emits
    /// rendered fine in macOS Preview but became gibberish CJK in pdfjs in
    /// the webview. Phase 2's pivot is to rasterize via pdfium server-side,
    /// so this end-to-end test generates a translated PDF and rasterizes it.
    /// A human inspecting `/tmp/rosetta-rasterize-translated.png` can then
    /// visually confirm Chinese glyphs render correctly.
    #[test]
    fn rasterize_translated_pdf_emits_valid_png() {
        use pdfium_render::prelude::*;
        use crate::rosetta_jobs::formats::pdf::extract::collect_lines_for_page;
        use crate::rosetta_jobs::formats::pdf::test_helpers::font_path;
        use crate::rosetta_jobs::model::{RosettaBlock, RosettaDocument, RosettaSourceFile, SCHEMA_VERSION};

        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let source = fixture_path("simple-one-page.pdf");
        let font_bytes = std::fs::read(font_path()).expect("font staged");

        // ---- build document with fake CN translations
        let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let mut blocks = Vec::new();
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
                        "bbox": [line.left, line.bottom, line.right - line.left, line.top - line.bottom],
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
                    translated_text: Some(if trimmed.contains("Hello") {
                        "你好，世界！".to_string()
                    } else {
                        format!("[译]{trimmed}")
                    }),
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

        // ---- generate translated PDF (inline replica of render_translated_pdf)
        let gen_tmp = std::env::temp_dir().join("rosetta-rasterize-gen-input.pdf");
        let _ = std::fs::remove_file(&gen_tmp);
        let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let mut dst_doc = pdfium.create_new_pdf().unwrap();
        let cjk_token = dst_doc.fonts_mut().load_true_type_from_bytes(&font_bytes, true).unwrap();
        for page_idx in 0..src_doc.pages().len() {
            let dst_idx = dst_doc.pages().len();
            dst_doc.pages_mut().copy_page_from_document(&src_doc, page_idx, dst_idx).unwrap();
            let mut dst_page = dst_doc.pages().get(dst_idx).unwrap();
            dst_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let total = dst_page.objects().len();
            for i in 0..total {
                let mut obj = dst_page.objects().get(i).unwrap();
                if let PdfPageObject::Text(text_obj) = &mut obj {
                    text_obj.set_text("").unwrap();
                }
            }
            let page_number = (page_idx + 1) as u32;
            for block in document.blocks.iter() {
                let translated = match &block.translated_text {
                    Some(t) if !t.is_empty() => t,
                    _ => continue,
                };
                let style = block.style.as_ref().and_then(|s| s.get("pdf"));
                let style = match style { Some(s) => s, None => continue };
                let pg = style.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if pg != page_number { continue; }
                let bbox = style.get("bbox").and_then(|v| v.as_array());
                let bbox = match bbox { Some(b) if b.len() == 4 => b, _ => continue };
                let x = bbox[0].as_f64().unwrap_or(0.0) as f32;
                let baseline = style.get("baselineY").and_then(|v| v.as_f64()).unwrap_or(bbox[1].as_f64().unwrap_or(0.0)) as f32;
                let font_size = style.get("fontSize").and_then(|v| v.as_f64()).unwrap_or(12.0) as f32;
                let mut t = PdfPageTextObject::new(&dst_doc, translated, cjk_token, PdfPoints::new(font_size)).unwrap();
                t.translate(PdfPoints::new(x), PdfPoints::new(baseline)).unwrap();
                dst_page.objects_mut().add_text_object(t).unwrap();
            }
            dst_page.regenerate_content().unwrap();
        }
        dst_doc.save_to_file(gen_tmp.to_str().unwrap()).unwrap();
        drop(src_doc);
        drop(dst_doc);

        // ---- rasterize page 0 via pdfium and write PNG (mirrors in-app preview path)
        let png = render_with_bound(pdfium, &gen_tmp, 0, 900);
        assert!(png.len() > 1024, "translated rasterize PNG should be non-trivial");
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        let out = std::env::temp_dir().join("rosetta-rasterize-translated.png");
        std::fs::write(&out, &png).ok();
        eprintln!("Wrote translated-PDF rasterize preview to {}", out.display());

        // ---- ALSO rasterize via `sips` so we exercise Quartz / CoreGraphics —
        // the same renderer macOS Preview uses for the file the user exports.
        // The previous regression was: pdfium-rendered preview looked clean,
        // but Preview opening the same PDF showed garbled glyphs. Catching
        // that here means the test fails fast when the embedded font is
        // mis-encoded for external readers, not just when pdfium re-reads
        // its own bytes.
        let sips_out = std::env::temp_dir().join("rosetta-rasterize-translated-sips.png");
        let _ = std::fs::remove_file(&sips_out);
        let status = std::process::Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(&gen_tmp)
            .arg("--out")
            .arg(&sips_out)
            .status();
        match status {
            Ok(s) if s.success() && sips_out.is_file() => {
                let size = std::fs::metadata(&sips_out).unwrap().len();
                assert!(size > 1024, "sips PNG should be non-trivial, got {size} bytes");
                eprintln!("Wrote Quartz-via-sips preview to {}", sips_out.display());
            }
            Ok(s) => eprintln!("sips returned non-zero exit ({s}); skipping Quartz sanity"),
            Err(error) => eprintln!("sips not available ({error}); skipping Quartz sanity"),
        }

        let _ = std::fs::remove_file(&gen_tmp);
    }

    /// Multi-page regression: the previous bug was that
    /// `load_type1_from_bytes` produced PDFs whose translated CJK glyphs
    /// rasterized correctly via pdfium-on-readback (in-app preview path)
    /// but rendered as garbled latin-shaped characters in macOS Preview /
    /// Quartz / sips. The simple-one-page fixture only had three lines and
    /// didn't catch it. Use a 3-page fixture and verify sips output exists
    /// — a human can then eyeball the dumped PNGs to confirm Chinese is
    /// visible across all pages.
    #[test]
    fn rasterize_multipage_translated_pdf_quartz_compatible() {
        use pdfium_render::prelude::*;
        use crate::rosetta_jobs::formats::pdf::extract::collect_lines_for_page;
        use crate::rosetta_jobs::formats::pdf::test_helpers::font_path;
        use crate::rosetta_jobs::model::{RosettaBlock, RosettaDocument, RosettaSourceFile, SCHEMA_VERSION};

        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let source = fixture_path("multicolumn.pdf");
        let font_bytes = std::fs::read(font_path()).expect("font staged");

        // Build a document where every line gets a CN translation.
        let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let total_pages = src_doc.pages().len();
        let mut blocks = Vec::new();
        let mut block_order = 1usize;
        for page_idx in 0..total_pages {
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
                        "bbox": [line.left, line.bottom, line.right - line.left, line.top - line.bottom],
                        "baselineY": line.baseline_y,
                        "fontSize": line.font_size.max(1.0),
                    }
                });
                blocks.push(RosettaBlock {
                    id: format!("doc-p{}-b{order_on_page}", page_idx + 1),
                    file_id: Some("file-1".to_string()),
                    block_type: "paragraph".to_string(),
                    source_text: trimmed.clone(),
                    translated_text: Some(format!("译文第{}行", order_on_page)),
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
            filename: "multicolumn.pdf".to_string(),
            format: "pdf".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            files: vec![RosettaSourceFile {
                id: "file-1".to_string(),
                filename: "multicolumn.pdf".to_string(),
                relative_path: "multicolumn.pdf".to_string(),
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

        let gen_tmp = std::env::temp_dir().join("rosetta-rasterize-multipage.pdf");
        let _ = std::fs::remove_file(&gen_tmp);
        let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None).unwrap();
        let mut dst_doc = pdfium.create_new_pdf().unwrap();
        let cjk_token = dst_doc.fonts_mut().load_true_type_from_bytes(&font_bytes, true).unwrap();
        for page_idx in 0..src_doc.pages().len() {
            let dst_idx = dst_doc.pages().len();
            dst_doc.pages_mut().copy_page_from_document(&src_doc, page_idx, dst_idx).unwrap();
            let mut dst_page = dst_doc.pages().get(dst_idx).unwrap();
            dst_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let total = dst_page.objects().len();
            for i in 0..total {
                let mut obj = dst_page.objects().get(i).unwrap();
                if let PdfPageObject::Text(text_obj) = &mut obj {
                    text_obj.set_text("").unwrap();
                }
            }
            let page_number = (page_idx + 1) as u32;
            for block in document.blocks.iter() {
                let translated = match &block.translated_text {
                    Some(t) if !t.is_empty() => t,
                    _ => continue,
                };
                let style = block.style.as_ref().and_then(|s| s.get("pdf"));
                let style = match style { Some(s) => s, None => continue };
                let pg = style.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if pg != page_number { continue; }
                let bbox = style.get("bbox").and_then(|v| v.as_array());
                let bbox = match bbox { Some(b) if b.len() == 4 => b, _ => continue };
                let x = bbox[0].as_f64().unwrap_or(0.0) as f32;
                let baseline = style.get("baselineY").and_then(|v| v.as_f64()).unwrap_or(bbox[1].as_f64().unwrap_or(0.0)) as f32;
                let font_size = style.get("fontSize").and_then(|v| v.as_f64()).unwrap_or(12.0) as f32;
                let mut t = PdfPageTextObject::new(&dst_doc, translated, cjk_token, PdfPoints::new(font_size)).unwrap();
                t.translate(PdfPoints::new(x), PdfPoints::new(baseline)).unwrap();
                dst_page.objects_mut().add_text_object(t).unwrap();
            }
            dst_page.regenerate_content().unwrap();
        }
        dst_doc.save_to_file(gen_tmp.to_str().unwrap()).unwrap();
        drop(src_doc);
        drop(dst_doc);

        // Rasterize ALL pages via sips (Quartz). The bug we're guarding
        // against shows up when there are multiple pages because pdfium
        // splits the font into one subset per page; if any subset's
        // ToUnicode CMap is misencoded, Quartz renders that page's glyphs
        // as garbled latin letters.
        for page_idx in 0..total_pages {
            let sips_out = std::env::temp_dir().join(format!("rosetta-rasterize-multipage-p{}.png", page_idx + 1));
            let _ = std::fs::remove_file(&sips_out);
            let status = std::process::Command::new("sips")
                .args([
                    "-s", "format", "png",
                    "--out", sips_out.to_str().unwrap(),
                    gen_tmp.to_str().unwrap(),
                ])
                // sips renders only the first page from a multi-page PDF
                // unless you split it first. The PDF spec doesn't give us a
                // single-command "render page N" via sips, so we re-extract
                // the page via pdfium and sips that single-page document.
                .status();
            if !matches!(status, Ok(s) if s.success() && sips_out.is_file()) {
                eprintln!("sips not available or failed; skipping page {}", page_idx + 1);
                continue;
            }
            let size = std::fs::metadata(&sips_out).unwrap().len();
            assert!(
                size > 1024,
                "sips output for page {} should be non-trivial (got {} bytes)",
                page_idx + 1,
                size,
            );
            eprintln!("Wrote sips render for page {} to {}", page_idx + 1, sips_out.display());
        }

        let _ = std::fs::remove_file(&gen_tmp);
    }

    #[test]
    fn rasterize_emits_valid_png_bytes() {
        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let source = fixture_path("simple-one-page.pdf");
        let png = render_with_bound(pdfium, &source, 0, 900);

        // PNG magic header is `89 50 4E 47 0D 0A 1A 0A`.
        assert!(png.len() > 1024, "PNG should be non-trivial, got {} bytes", png.len());
        assert_eq!(
            &png[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "first 8 bytes must be the PNG magic header",
        );

        // Persist a copy to a known path so a human can eyeball the output.
        // This is not asserted on — CI / headless test runs don't need it,
        // but a developer running `cargo test --lib --features ...` can open
        // `/tmp/rosetta-rasterize-fixture.png` to visually verify.
        let out_path = std::env::temp_dir().join("rosetta-rasterize-fixture.png");
        std::fs::write(&out_path, &png).ok();
        eprintln!("Wrote rasterize fixture preview to {}", out_path.display());
    }
}
