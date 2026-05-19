// Phase 0 spike: validate the high-fidelity PDF translation approach.
//
// Pipeline:
//   1. Generate a source PDF with English text at known positions (proxy for real input).
//   2. Open source, create a new destination PDF.
//   3. For each source page:
//        a. Walk `page.text().segments()` to capture bbox+text BEFORE we copy.
//        b. Use `copy_page_from_document` to clone the entire source page into
//           the destination as a *real* page — preserves images, vectors, etc.
//        c. Walk the destination page's objects and DELETE every Text object.
//           This wipes the original text from both the visible page AND the
//           text-extraction layer (so copy/paste returns only the translation).
//        d. Draw the translation on top using a custom-loaded CJK font.
//   4. Save destination PDF.
//
// Original spike used `copy_into_x_object_form_object` + white rectangles, but
// the Form XObject encapsulation kept original text reachable by text selection
// — copy/paste from the output returned English, not Chinese. Switching to
// page-copy + text-object-deletion fixes that.

use std::path::Path;

use anyhow::{Context, Result};
use pdfium_render::prelude::*;

const PDFIUM_LIB: &str = "vendor/lib/libpdfium.dylib";
const CJK_FONT: &str = "vendor/SourceHanSansCN-Regular.otf";
const SOURCE_PDF: &str = "output/spike-source.pdf";
const TRANSLATED_PDF: &str = "output/spike-translated.pdf";

const TRANSLATIONS: &[(&str, &str)] = &[
    ("Hello, world!", "你好，世界！"),
    ("The quick brown fox", "敏捷的棕色狐狸"),
    ("jumps over the lazy dog.", "跳过了懒狗。"),
    ("Rosetta translates PDFs.", "Rosetta 翻译 PDF 文件。"),
];

fn main() -> Result<()> {
    let spike_root = std::env::current_dir()?;
    println!("[spike] cwd = {}", spike_root.display());

    let pdfium = bind_pdfium(&spike_root)?;
    println!("[spike] pdfium bound ok");

    // CLI: no args = use bundled toy generator; one arg = use the given PDF.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (source_path, translated_path) = if let Some(user_pdf) = args.first() {
        let source = std::fs::canonicalize(user_pdf)
            .with_context(|| format!("failed to resolve {user_pdf}"))?;
        let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("input");
        let translated = spike_root.join(format!("output/{stem}.translated.pdf"));
        std::fs::create_dir_all(translated.parent().unwrap())?;
        println!("[spike] user-supplied source: {}", source.display());
        (source, translated)
    } else {
        let source = spike_root.join(SOURCE_PDF);
        std::fs::create_dir_all(source.parent().unwrap())?;
        if !source.exists() {
            generate_source_pdf(&pdfium, &source)?;
            println!("[spike] generated source PDF at {}", source.display());
        } else {
            println!("[spike] reusing existing source PDF at {}", source.display());
        }
        (source, spike_root.join(TRANSLATED_PDF))
    };

    translate_pdf(&pdfium, &source_path, &translated_path, &spike_root.join(CJK_FONT))?;
    println!("[spike] wrote translated PDF to {}", translated_path.display());

    // Verify what text-extraction (i.e. copy/paste) gets out of the translated PDF.
    println!("\n[spike] --- text-layer audit of translated PDF ---");
    let extracted = extract_text_from_pdf(&pdfium, &translated_path)?;
    let mut total_segments = 0usize;
    for (page_idx, segs) in extracted.iter().enumerate() {
        for s in segs {
            total_segments += 1;
            // Truncate long lines so the console output stays readable on
            // multi-page docs.
            let preview = if s.chars().count() > 80 {
                let truncated: String = s.chars().take(80).collect();
                format!("{truncated}…")
            } else {
                s.clone()
            };
            println!("[spike]   page {} extracted: {:?}", page_idx, preview);
        }
    }
    println!("[spike] {} text segments in the translated PDF text layer", total_segments);
    Ok(())
}

fn extract_text_from_pdf(pdfium: &Pdfium, path: &Path) -> Result<Vec<Vec<String>>> {
    let doc = pdfium.load_pdf_from_file(path.to_str().unwrap(), None)?;
    let mut all = Vec::new();
    let n = doc.pages().len();
    for i in 0..n {
        let page = doc.pages().get(i)?;
        let text = page.text()?;
        let segs: Vec<String> = text.segments().iter().map(|s| s.text()).collect();
        all.push(segs);
    }
    Ok(all)
}

fn bind_pdfium(spike_root: &Path) -> Result<Pdfium> {
    let lib_path = spike_root.join(PDFIUM_LIB);
    let bindings = Pdfium::bind_to_library(&lib_path)
        .with_context(|| format!("failed to bind pdfium at {}", lib_path.display()))?;
    Ok(Pdfium::new(bindings))
}

/// Build a deterministic 1-page source PDF with a few English lines.
fn generate_source_pdf(pdfium: &Pdfium, out: &Path) -> Result<()> {
    let mut doc = pdfium.create_new_pdf()?;
    let paper = PdfPagePaperSize::a4();
    let mut page = doc.pages_mut().create_page_at_end(paper)?;

    let helvetica = doc.fonts_mut().helvetica();
    let lines = [
        ("Hello, world!", 72.0, 760.0, 18.0),
        ("The quick brown fox", 72.0, 720.0, 14.0),
        ("jumps over the lazy dog.", 72.0, 700.0, 14.0),
        ("Rosetta translates PDFs.", 72.0, 640.0, 14.0),
    ];
    for (text, x, y, size) in lines {
        let mut obj = PdfPageTextObject::new(&doc, text, helvetica, PdfPoints::new(size))?;
        obj.translate(PdfPoints::new(x), PdfPoints::new(y))?;
        page.objects_mut().add_text_object(obj)?;
    }
    page.regenerate_content()?;
    doc.save_to_file(out)?;
    Ok(())
}

/// One reading-order text line on a page, after collapsing pdfium's
/// fine-grained segments by baseline.
#[derive(Debug)]
struct Line {
    text: String,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    baseline_y: f32,
    height: f32,
}

/// Reconstruct reading-order lines by walking every character on the page and
/// bucketing them by typographic baseline.
///
/// Why per-char and not per-segment: pdfium's `text.segments()` partitions the
/// content stream by text-show operator, which for PDFs from sophisticated
/// typography pipelines (Google Docs, LaTeX, Word) can mean per-glyph or
/// per-font-subset chunks. On a Google Docs export the word "Beautiful" comes
/// back as separate segments for "Bifl" (no-descender subset) + "eautu" (another
/// subset) + "y" (descender subset) — totally scrambled relative to visual
/// order. The per-char API doesn't have this problem: each char carries its
/// own bbox and we can rebuild lines from raw geometry.
fn collect_lines_from_page(src_doc: &PdfDocument, page_idx: PdfPageIndex) -> Result<Vec<Line>> {
    // Bucket size in points. Because we use the *baseline* (origin_y), all
    // chars on the same visual line share a Y value to sub-pt precision, so a
    // 1.5pt bucket is plenty.
    const Y_BUCKET: f32 = 1.5;
    // Split a bucket into multiple lines when adjacent chars sit more than this
    // multiple of font height apart horizontally — that's a table-cell gutter
    // or a hard column break, not a word space.
    const X_GAP_BREAK_FACTOR: f32 = 3.5;

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

    let page = src_doc.pages().get(page_idx)?;
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
        // Use the typographic baseline (origin_y) instead of the glyph's
        // tight-bbox bottom for bucketing. This is the line the font sits on;
        // descender chars (g, p, y) share the same baseline as non-descender
        // chars (u, l, .) on the same visual line even though their bboxes
        // end at different Y values.
        let baseline_y = ch.origin_y().map(|p| p.value).unwrap_or(bounds.bottom().value);
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

    let mut buckets: std::collections::BTreeMap<i32, Vec<CharBox>> =
        std::collections::BTreeMap::new();
    for cb in all_chars {
        let key = (cb.baseline_y / Y_BUCKET).round() as i32;
        buckets.entry(key).or_default().push(cb);
    }

    let mut lines: Vec<Line> = Vec::new();
    for (_, mut bucket) in buckets.into_iter().rev() {
        bucket.sort_by(|a, b| {
            a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut current: Option<Line> = None;
        for cb in bucket {
            let gap_break = cb.font_size.max(8.0) * X_GAP_BREAK_FACTOR;
            let extend = current
                .as_ref()
                .is_some_and(|cur| (cb.left - cur.right) < gap_break);

            if extend {
                let cur = current.as_mut().unwrap();
                if cb.left - cur.right > cb.font_size * 0.25 {
                    cur.text.push(' ');
                }
                cur.text.push(cb.ch);
                cur.right = cur.right.max(cb.right);
                cur.top = cur.top.max(cb.top);
                cur.bottom = cur.bottom.min(cb.bottom);
                cur.height = cur.height.max(cb.font_size);
            } else {
                if let Some(done) = current.take() {
                    lines.push(done);
                }
                current = Some(Line {
                    text: cb.ch.to_string(),
                    left: cb.left,
                    right: cb.right,
                    top: cb.top,
                    bottom: cb.bottom,
                    baseline_y: cb.baseline_y,
                    height: cb.font_size,
                });
            }
        }
        if let Some(done) = current.take() {
            lines.push(done);
        }
    }

    // Drop lines that are pure punctuation / one-char artifacts — they're
    // usually accents or dots that didn't make it into the main bucket.
    lines.retain(|l| l.text.trim().chars().count() >= 1);
    Ok(lines)
}

fn translate_pdf(pdfium: &Pdfium, source: &Path, dest: &Path, cjk_font: &Path) -> Result<()> {
    // The CJK font must be loaded into the destination doc so pdfium embeds it
    // (subsetted) into the output file. Without this, viewers without the font
    // installed render empty boxes.
    let cjk_bytes = std::fs::read(cjk_font)
        .with_context(|| format!("failed to read CJK font at {}", cjk_font.display()))?;
    println!("[spike] CJK font: {} bytes", cjk_bytes.len());

    let src_doc = pdfium.load_pdf_from_file(source.to_str().unwrap(), None)?;
    let mut dst_doc = pdfium.create_new_pdf()?;
    let cjk_token = dst_doc
        .fonts_mut()
        .load_true_type_from_bytes(&cjk_bytes, true)?;

    let n_pages = src_doc.pages().len();
    println!("[spike] source has {} page(s)", n_pages);

    for page_idx in 0..n_pages {
        // Capture text bbox+content from source BEFORE we copy the page (we want
        // the original text positions, not the post-deletion state).
        //
        // CRITICAL: pdfium's `PdfPageText::segments()` returns one entry per
        // text-show operator in the content stream. For PDFs generated by
        // sophisticated typography pipelines (Google Docs, LaTeX, Word)
        // segments are typically per-glyph or per-word — NOT per-line. Drawing
        // a translation at each fine-grained segment's bbox makes the output
        // collide into an illegible pile. So we merge segments into lines by
        // baseline before treating them as translation units.
        let lines = collect_lines_from_page(&src_doc, page_idx)?;
        println!(
            "[spike]   page {}: {} segments merged into {} lines",
            page_idx,
            "?",
            lines.len()
        );
        let segments_to_draw: Vec<(PdfRect, String, f32)> = lines
            .into_iter()
            .map(|line| {
                let translation = lookup_translation(&line.text);
                // Anchor translation at the baseline X-min — pdfium's
                // PdfPageTextObject.translate() places the text origin (which
                // is the baseline) at (x, y), so passing baseline_y for y
                // makes the new text sit on the same baseline as the original.
                let bounds = PdfRect::new(
                    PdfPoints::new(line.baseline_y),
                    PdfPoints::new(line.left),
                    PdfPoints::new(line.top),
                    PdfPoints::new(line.right),
                );
                println!(
                    "[spike]     line '{}' x={:.1} baseline={:.1} w={:.1} h={:.1} → '{}'",
                    line.text,
                    line.left,
                    line.baseline_y,
                    line.right - line.left,
                    line.height,
                    translation,
                );
                (bounds, translation, line.height)
            })
            .collect();

        // Copy the source page into the destination as a real page (not as an
        // opaque Form XObject). This keeps individual page objects accessible
        // so we can mutate the text layer.
        let dst_idx = dst_doc.pages().len();
        println!("[spike]   about to copy_page_from_document page {} to dst_idx {}", page_idx, dst_idx);
        dst_doc
            .pages_mut()
            .copy_page_from_document(&src_doc, page_idx, dst_idx)?;
        println!("[spike]   copy ok, dst has {} pages", dst_doc.pages().len());

        // Wipe original text objects. Walk back-to-front so deletions don't
        // invalidate indices in front of the cursor. NOTE: must use pages()
        // (immutable accessor) not pages_mut() — combining pages_mut().get()
        // with FPDFPage_GetObject right after FPDF_ImportPages segfaults in
        // pdfium-render 0.9.1. The returned PdfPage is owned, so we can still
        // call objects_mut() on it.
        // Clear original text by setting each text object's text to "".
        // We avoid `remove_object_at_index` here because in pdfium-render 0.9.1
        // it segfaults when called on a page produced by FPDF_ImportPages
        // (the FPDFPage_RemoveObject path appears to deref a stale handle).
        // Setting empty text is safer and achieves the same visual + text-layer
        // result: the glyphs are gone, copy/paste returns nothing for them.
        let mut text_cleared = 0usize;
        {
            let mut dst_page = dst_doc.pages().get(dst_idx)?;
            dst_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let total_objs = dst_page.objects().len();
            for i in 0..total_objs {
                let mut obj = dst_page.objects().get(i)?;
                if let PdfPageObject::Text(text_obj) = &mut obj {
                    text_obj.set_text("")?;
                    text_cleared += 1;
                }
            }
            dst_page.regenerate_content()?;
        }
        println!("[spike]   page {} cleared {} text objects", page_idx, text_cleared);

        // Draw translations.
        {
            let mut dst_page = dst_doc.pages().get(dst_idx)?;
            for (bounds, translation, line_height) in segments_to_draw {
                let mut text_obj = PdfPageTextObject::new(
                    &dst_doc,
                    &translation,
                    cjk_token,
                    PdfPoints::new(line_height.max(10.0)),
                )?;
                text_obj.translate(bounds.left(), bounds.bottom())?;
                dst_page.objects_mut().add_text_object(text_obj)?;
            }
            dst_page.regenerate_content()?;
        }
    }

    dst_doc.save_to_file(dest)?;
    Ok(())
}

fn lookup_translation(s: &str) -> String {
    let key = s.trim();
    for (en, cn) in TRANSLATIONS {
        if key == *en {
            return (*cn).to_string();
        }
    }
    format!("「未译」{}", key)
}
