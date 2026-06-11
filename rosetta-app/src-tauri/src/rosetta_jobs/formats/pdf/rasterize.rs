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

use image::ImageEncoder;
use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    ImageFormat,
};
use tauri::{AppHandle, Manager};

use crate::rosetta_jobs::formats::pdf::{
    errors::PdfError,
    runtime::{self, PngCache},
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
    let width = target_width.clamp(MIN_TARGET_WIDTH, MAX_TARGET_WIDTH);
    let source_path_str = source_path
        .to_str()
        .ok_or_else(|| PdfError::Read("PDF 路径包含无效字符。".to_string()))?;

    // Check cache first — avoids re-loading the PDF and re-rasterizing on
    // repeated requests (e.g. scroll back, PDF switch, cacheKey bump).
    let cache_key = (source_path_str.to_string(), page_index, width);
    if let Ok(mut cache) = app.state::<PngCache>().0.lock() {
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    let pdfium = runtime::get_pdfium(app).map_err(PdfError::RuntimeMissing)?;
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
    let encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
    encoder
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| PdfError::Parse(format!("PNG 编码失败: {error}")))?;

    let _ = ImageFormat::Png; // ensure the import is used even when not needed

    // Store in cache for subsequent requests (resize, PDF switch, scroll back).
    if let Ok(mut cache) = app.state::<PngCache>().0.lock() {
        cache.put(cache_key, out.clone());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    fn render_with_bound(
        pdfium: &pdfium_render::prelude::Pdfium,
        source: &Path,
        page: u32,
        width: u32,
    ) -> Vec<u8> {
        let doc = pdfium
            .load_pdf_from_file(source.to_str().unwrap(), None)
            .unwrap();
        let pg = doc.pages().get(page as i32).unwrap();
        let cfg = pdfium_render::prelude::PdfRenderConfig::new()
            .set_target_width(width as pdfium_render::prelude::Pixels);
        let bm = pg.render_with_config(&cfg).unwrap();
        let dyn_img = bm.as_image().unwrap();
        let rgba = dyn_img.to_rgba8();
        let mut out = Vec::new();
        let enc =
            PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
        enc.write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        out
    }

    #[test]
    fn rasterize_source_pdf_emits_valid_png() {
        let _guard = pdfium_test_lock();
        let pdfium = shared_pdfium();
        let source = fixture_path("simple-one-page.pdf");
        let png = render_with_bound(pdfium, &source, 0, 900);

        assert!(
            png.len() > 1024,
            "PNG should be non-trivial, got {} bytes",
            png.len()
        );
        assert_eq!(
            &png[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "first 8 bytes must be the PNG magic header",
        );
    }
}
