// Phase 0 spike: validate the high-fidelity PDF translation approach.
//
// Pipeline:
//   1. Generate a source PDF with English text at known positions (proxy for real input).
//   2. Open source, create a new destination PDF.
//   3. For each source page:
//        a. Use pdfium-render's `copy_into_x_object_form_object` to import the
//           entire source page as a Form XObject into the destination — preserves
//           all images, vectors, original fonts byte-for-byte.
//        b. Walk `page.text().segments()` to get bbox+text for each line.
//        c. On the destination page, draw a white rect over each original segment
//           bbox, then draw the translation in a custom-loaded CJK font.
//   4. Save destination PDF.
//
// Hypothesis being tested: this combo produces a translated PDF whose layout
// matches the source's, with selectable real text, in any PDF viewer.

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
    println!("[spike] DONE — diff the two PDFs visually to evaluate fidelity.");
    Ok(())
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
        // Two scopes so we can borrow src_doc / dst_doc separately and drop refs.
        let (size_width, size_height, segments_to_draw) = {
            let src_page = src_doc.pages().get(page_idx)?;
            let size = src_page.page_size();
            let page_text = src_page.text()?;

            let mut segs = Vec::new();
            for seg in page_text.segments().iter() {
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
                segs.push((bounds, translation, seg.height().value));
            }
            (size.width(), size.height(), segs)
        };

        let paper = PdfPagePaperSize::from_points(size_width, size_height);
        let mut dst_page = dst_doc.pages_mut().create_page_at_end(paper)?;

        // Import source page as Form XObject — this is the key high-fidelity step.
        // copy_into_x_object_form_object handles FPDF_NewXObjectFromPage +
        // FPDF_NewFormObjectFromXObject + FPDF_CloseXObject internally.
        {
            let src_page = src_doc.pages().get(page_idx)?;
            let xobj_obj = src_page.objects().copy_into_x_object_form_object(&mut dst_doc)?;
            dst_page.objects_mut().add_object(xobj_obj)?;
        }

        // White rectangles to mask original text, then translated text on top.
        for (bounds, translation, line_height) in segments_to_draw {
            // Pad the mask a hair to avoid leaving stroke edges from the original.
            let padded = PdfRect::new(
                PdfPoints::new(bounds.bottom().value - 1.5),
                PdfPoints::new(bounds.left().value - 1.0),
                PdfPoints::new(bounds.top().value + 1.5),
                PdfPoints::new(bounds.right().value + 2.0),
            );
            let rect = PdfPagePathObject::new_rect(
                &dst_doc,
                padded,
                None,
                None,
                Some(PdfColor::WHITE),
            )?;
            dst_page.objects_mut().add_path_object(rect)?;

            // Translated text. Use the original line's height as a font-size proxy.
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
