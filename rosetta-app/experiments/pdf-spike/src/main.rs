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

    let source_path = spike_root.join(SOURCE_PDF);
    std::fs::create_dir_all(source_path.parent().unwrap())?;
    if !source_path.exists() {
        generate_source_pdf(&pdfium, &source_path)?;
        println!("[spike] generated source PDF at {}", source_path.display());
    } else {
        println!("[spike] reusing existing source PDF at {}", source_path.display());
    }

    let translated_path = spike_root.join(TRANSLATED_PDF);
    translate_pdf(&pdfium, &source_path, &translated_path, &spike_root.join(CJK_FONT))?;
    println!("[spike] wrote translated PDF to {}", translated_path.display());

    // Verify what text-extraction (i.e. copy/paste) gets out of the translated PDF.
    println!("\n[spike] --- text-layer audit of translated PDF ---");
    let extracted = extract_text_from_pdf(&pdfium, &translated_path)?;
    for (page_idx, segs) in extracted.iter().enumerate() {
        for s in segs {
            println!("[spike]   page {} extracted: {:?}", page_idx, s);
        }
    }
    let any_english_residue = extracted.iter().any(|page| {
        page.iter().any(|s| {
            s.contains("Hello, world!")
                || s.contains("brown fox")
                || s.contains("lazy dog")
                || s.contains("translates PDFs")
        })
    });
    if any_english_residue {
        println!("[spike] ❌ ENGLISH RESIDUE detected in text layer — copy/paste will still leak source text");
    } else {
        println!("[spike] ✅ text layer clean of English source");
    }
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
        let segments_to_draw: Vec<(PdfRect, String, f32)> = {
            let src_page = src_doc.pages().get(page_idx)?;
            let page_text = src_page.text()?;
            page_text
                .segments()
                .iter()
                .map(|seg| {
                    let text = seg.text();
                    let bounds = seg.bounds();
                    let translation = lookup_translation(&text);
                    println!(
                        "[spike]   page {} seg '{}' bbox=({:.1},{:.1},{:.1},{:.1}) → '{}'",
                        page_idx,
                        text,
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                        translation,
                    );
                    (bounds, translation, seg.height().value)
                })
                .collect()
        };

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
