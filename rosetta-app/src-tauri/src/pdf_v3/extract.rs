use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    time::Instant,
};

use pdfium_render::prelude::{
    PdfColor, PdfFontWeight, PdfPageObject, PdfPageObjectsCommon, PdfPageText, PdfPageTextChar,
    PdfRect, Pdfium,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    document::{DocumentHandle, DocumentHandleError},
    page_set::PageSet,
    types::{
        PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageReconciliationSummary,
        PageStyle, PAGE_GRAPH_SCHEMA_VERSION, PDF_V3_CONTRACT_VERSION,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfV3ExtractionError {
    Open(DocumentHandleError),
    PageOutOfBounds { page: u32, page_count: u32 },
    PageRead { page: u32, message: String },
    TextRead { page: u32, message: String },
}

impl fmt::Display for PdfV3ExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => error.fmt(formatter),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::PageRead { page, message } => {
                write!(formatter, "failed to read PDF page {page}: {message}")
            }
            Self::TextRead { page, message } => {
                write!(
                    formatter,
                    "failed to read text from PDF page {page}: {message}"
                )
            }
        }
    }
}

impl std::error::Error for PdfV3ExtractionError {}

impl From<DocumentHandleError> for PdfV3ExtractionError {
    fn from(value: DocumentHandleError) -> Self {
        Self::Open(value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageSetExtraction {
    pub contract_version: u32,
    pub source_fingerprint: String,
    pub source_page_count: u32,
    pub requested_pages: PageSet,
    pub pages: Vec<PageGraph>,
    pub page_timings_ms: Vec<PageExtractionTiming>,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageExtractionTiming {
    pub page_number: u32,
    pub elapsed_ms: u64,
    pub character_count: usize,
    pub style_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfiumTextObjectSnapshot {
    pub source_object_id: String,
    pub page_object_index: usize,
    pub text_object_index: usize,
    pub font_name: String,
    pub text: String,
    pub mapped_atom_count: Option<usize>,
    pub mapped_unicode_atom_count: Option<usize>,
    pub first_atom_order: Option<usize>,
    pub last_atom_order: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfiumPageSnapshot {
    pub page_graph: PageGraph,
    pub page_object_count: usize,
    pub form_object_count: usize,
    pub text_objects: Vec<PdfiumTextObjectSnapshot>,
    pub timing: PageExtractionTiming,
}

pub(crate) fn extract_page_set(
    pdfium: &Pdfium,
    source_path: &Path,
    page_set: &PageSet,
) -> Result<PageSetExtraction, PdfV3ExtractionError> {
    let handle = DocumentHandle::open(pdfium, source_path)?;
    extract_page_set_from_handle(&handle, page_set)
}

pub(crate) fn extract_page_set_from_handle(
    handle: &DocumentHandle<'_>,
    page_set: &PageSet,
) -> Result<PageSetExtraction, PdfV3ExtractionError> {
    let started = Instant::now();
    let source_fingerprint = handle.source_fingerprint().to_string();
    let source_page_count = handle.page_count();
    validate_page_set(page_set, source_page_count)?;

    let mut pages = Vec::with_capacity(page_set.pages().len());
    let mut page_timings_ms = Vec::with_capacity(page_set.pages().len());
    for page_number in page_set.pages().iter().copied() {
        let snapshot = extract_pdfium_page_snapshot(handle, page_number)?;
        page_timings_ms.push(snapshot.timing);
        pages.push(snapshot.page_graph);
    }

    Ok(PageSetExtraction {
        contract_version: PDF_V3_CONTRACT_VERSION,
        source_fingerprint,
        source_page_count,
        requested_pages: page_set.clone(),
        pages,
        page_timings_ms,
        total_ms: started.elapsed().as_millis() as u64,
    })
}

pub(crate) fn extract_pdfium_page_snapshot(
    handle: &DocumentHandle<'_>,
    page_number: u32,
) -> Result<PdfiumPageSnapshot, PdfV3ExtractionError> {
    let page_started = Instant::now();
    let source_page_count = handle.page_count();
    if page_number == 0 || page_number > source_page_count {
        return Err(PdfV3ExtractionError::PageOutOfBounds {
            page: page_number,
            page_count: source_page_count,
        });
    }
    let page = handle
        .pdfium_document()
        .pages()
        .get(page_number as i32 - 1)
        .map_err(|error| PdfV3ExtractionError::PageRead {
            page: page_number,
            message: error.to_string(),
        })?;
    let page_text = page
        .text()
        .map_err(|error| PdfV3ExtractionError::TextRead {
            page: page_number,
            message: error.to_string(),
        })?;

    let mut atoms = Vec::with_capacity(page_text.len().max(0) as usize);
    let mut styles = BTreeMap::<String, PageStyle>::new();
    let mut warnings = BTreeSet::<String>::new();
    let mut object_ids_by_character = BTreeMap::<usize, String>::new();
    let page_objects = page.objects();
    let page_object_count = page_objects.len();
    let mut form_object_count = 0usize;
    let mut text_objects = Vec::new();
    for (page_object_index, object) in page_objects.iter().enumerate() {
        let source_object_id = format!("page-{page_number:04}-object-{page_object_index:06}");
        collect_pdfium_object_snapshot(
            &object,
            &page_text,
            source_object_id,
            page_object_index,
            &mut form_object_count,
            &mut text_objects,
            &mut object_ids_by_character,
            &mut warnings,
        );
    }

    for character in page_text.chars().iter() {
        let generated = character.is_generated().unwrap_or(false);
        let hyphen = character.is_hyphen().unwrap_or(false);
        let unicode = character.unicode_char().unwrap_or('\u{fffd}');
        if unicode == '\u{fffd}' {
            warnings.insert("invalid-unicode-mapping".to_string());
        }

        let style = style_for_character(&character);
        let style_id = style.style_id.clone();
        styles.entry(style_id.clone()).or_insert(style);

        let bounds = character
            .tight_bounds()
            .map(rect_to_array)
            .or_else(|_| character.loose_bounds().map(rect_to_array))
            .unwrap_or_else(|_| {
                warnings.insert("missing-character-bounds".to_string());
                [0.0; 4]
            });
        let loose_bounds = character.loose_bounds().ok().map(rect_to_array);
        let origin = character
            .origin()
            .ok()
            .map(|(horizontal, vertical)| [horizontal.value, vertical.value]);
        let text_matrix = character.matrix().ok().map(|matrix| {
            [
                matrix.a(),
                matrix.b(),
                matrix.c(),
                matrix.d(),
                matrix.e(),
                matrix.f(),
            ]
        });
        if text_matrix.is_none() {
            warnings.insert("missing-character-matrix".to_string());
        }

        atoms.push(PageAtom {
            atom_id: format!("page-{page_number:04}-char-{:06}", character.index()),
            source_text: unicode.to_string(),
            source_object_id: object_ids_by_character.get(&character.index()).cloned(),
            kind: if unicode == '\u{fffd}' {
                PageAtomKind::Unknown
            } else {
                PageAtomKind::Body
            },
            style_id: Some(style_id),
            bounds,
            loose_bounds,
            origin,
            text_matrix,
            angle_degrees: character.angle_degrees().ok(),
            order: character.index() as u32,
            generated,
            hyphen,
            requires_translation: !generated && unicode != '\u{fffd}' && !unicode.is_whitespace(),
            source_kind: PageAtomSourceKind::PdfiumUnverified,
            source_provenance: None,
        });
    }

    let styles = styles.into_values().collect::<Vec<_>>();
    if atoms.iter().any(|atom| atom.source_object_id.is_none()) {
        warnings.insert("pdfium-character-object-unmapped".to_string());
    }
    let timing = PageExtractionTiming {
        page_number,
        elapsed_ms: page_started.elapsed().as_millis() as u64,
        character_count: atoms.len(),
        style_count: styles.len(),
    };
    let atom_count = atoms.len();
    let page_graph = PageGraph {
        schema_version: PAGE_GRAPH_SCHEMA_VERSION,
        page_number,
        source_page_hash: page_source_hash(handle.source_fingerprint(), page_number),
        page_width: page.width().value,
        page_height: page.height().value,
        rotation_degrees: page
            .rotation()
            .map(|rotation| rotation.as_degrees() as i32)
            .unwrap_or_default(),
        atoms,
        styles,
        groups: Vec::new(),
        protected_spans: Vec::new(),
        reconciliation: PageReconciliationSummary::unreconciled(atom_count),
        warnings: warnings.into_iter().collect(),
    };

    Ok(PdfiumPageSnapshot {
        page_graph,
        page_object_count,
        form_object_count,
        text_objects,
        timing,
    })
}

fn collect_pdfium_object_snapshot<'a>(
    object: &PdfPageObject<'a>,
    page_text: &'a PdfPageText<'a>,
    source_object_id: String,
    root_page_object_index: usize,
    form_object_count: &mut usize,
    text_objects: &mut Vec<PdfiumTextObjectSnapshot>,
    object_ids_by_character: &mut BTreeMap<usize, String>,
    warnings: &mut BTreeSet<String>,
) {
    if let Some(form_object) = object.as_x_object_form_object() {
        *form_object_count += 1;
        for child_index in form_object.as_range() {
            match form_object.get(child_index) {
                Ok(child) => collect_pdfium_object_snapshot(
                    &child,
                    page_text,
                    format!("{source_object_id}-form-{child_index:06}"),
                    root_page_object_index,
                    form_object_count,
                    text_objects,
                    object_ids_by_character,
                    warnings,
                ),
                Err(_) => {
                    warnings.insert("pdfium-form-child-read-failed".to_string());
                }
            }
        }
        return;
    }

    let Some(text_object) = object.as_text_object() else {
        return;
    };
    let object_api_text = text_object.text();
    let (mapped_atom_count, mapped_unicode_atom_count, first_atom_order, last_atom_order, text) =
        match text_object.chars(page_text) {
            Ok(characters) => {
                for character in characters.iter() {
                    object_ids_by_character
                        .entry(character.index())
                        .or_insert_with(|| source_object_id.clone());
                }
                let atom_text = characters
                    .iter()
                    .map(|character| character.unicode_char())
                    .collect::<Option<String>>();
                let mapped_unicode_atom_count = atom_text.as_ref().map(|text| text.chars().count());
                (
                    Some(characters.len()),
                    mapped_unicode_atom_count,
                    characters.first_char_index(),
                    characters.last_char_index(),
                    atom_text.unwrap_or(object_api_text),
                )
            }
            Err(_) => {
                warnings.insert("pdfium-text-object-character-map-failed".to_string());
                (None, None, None, None, object_api_text)
            }
        };
    text_objects.push(PdfiumTextObjectSnapshot {
        source_object_id,
        page_object_index: root_page_object_index,
        text_object_index: text_objects.len(),
        font_name: text_object.font().name(),
        text,
        mapped_atom_count,
        mapped_unicode_atom_count,
        first_atom_order,
        last_atom_order,
    });
}

fn validate_page_set(page_set: &PageSet, page_count: u32) -> Result<(), PdfV3ExtractionError> {
    if let Some(page) = page_set
        .pages()
        .iter()
        .copied()
        .find(|page| *page > page_count)
    {
        return Err(PdfV3ExtractionError::PageOutOfBounds { page, page_count });
    }
    Ok(())
}

fn style_for_character(character: &PdfPageTextChar<'_>) -> PageStyle {
    let font_resource = Some(character.font_name()).filter(|name| !name.is_empty());
    let font_size = character.unscaled_font_size().value;
    let scaled_font_size = character.scaled_font_size().value;
    let font_weight = character.font_weight().map(font_weight_value);
    let italic = character.font_is_italic();
    let serif = character.font_is_serif();
    let fill_color = character.fill_color().ok().map(color_to_array);
    let stroke_color = character.stroke_color().ok().map(color_to_array);
    let opacity = fill_color.map(|color| color[3]);
    let render_mode = character.render_mode().ok().map(|mode| format!("{mode:?}"));
    let style_key = format!(
        "{:?}|{font_size:.4}|{scaled_font_size:.4}|{font_weight:?}|{italic}|{serif}|{fill_color:?}|{stroke_color:?}|{render_mode:?}",
        font_resource
    );

    PageStyle {
        style_id: format!("style-{}", short_hash(style_key.as_bytes())),
        font_resource,
        font_size,
        scaled_font_size,
        font_weight,
        italic,
        serif,
        fill_color,
        stroke_color,
        opacity,
        render_mode,
    }
}

fn font_weight_value(weight: PdfFontWeight) -> u16 {
    match weight {
        PdfFontWeight::Weight100 => 100,
        PdfFontWeight::Weight200 => 200,
        PdfFontWeight::Weight300 => 300,
        PdfFontWeight::Weight400Normal => 400,
        PdfFontWeight::Weight500 => 500,
        PdfFontWeight::Weight600 => 600,
        PdfFontWeight::Weight700Bold => 700,
        PdfFontWeight::Weight800 => 800,
        PdfFontWeight::Weight900 => 900,
        PdfFontWeight::Custom(value) => value.min(u16::MAX as u32) as u16,
    }
}

fn color_to_array(color: PdfColor) -> [f32; 4] {
    [
        f32::from(color.red()) / 255.0,
        f32::from(color.green()) / 255.0,
        f32::from(color.blue()) / 255.0,
        f32::from(color.alpha()) / 255.0,
    ]
}

fn rect_to_array(rect: PdfRect) -> [f32; 4] {
    [
        rect.left().value,
        rect.bottom().value,
        rect.right().value,
        rect.top().value,
    ]
}

fn page_source_hash(source_fingerprint: &str, page_number: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_fingerprint.as_bytes());
    hasher.update(page_number.to_le_bytes());
    format!("sha256:{}", hex_digest(hasher.finalize().as_slice()))
}

fn short_hash(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hex_digest(&hasher.finalize()[..8])
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_page_set, extract_pdfium_page_snapshot, PdfV3ExtractionError};
    use crate::{
        pdf_v3::{document::DocumentHandle, page_set::PageSet},
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn page_snapshot_object_coverage_matches_page_graph_atoms() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let snapshot = extract_pdfium_page_snapshot(&handle, 1).expect("page snapshot");

        assert!(snapshot.page_object_count > 0);
        assert_eq!(snapshot.form_object_count, 27);
        assert!(snapshot.text_objects.len() > snapshot.page_object_count);
        for object in &snapshot.text_objects {
            let graph_atom_count = snapshot
                .page_graph
                .atoms
                .iter()
                .filter(|atom| atom.source_object_id.as_deref() == Some(&object.source_object_id))
                .count();
            assert_eq!(object.mapped_atom_count, Some(graph_atom_count));
            assert_eq!(
                object.first_atom_order,
                snapshot
                    .page_graph
                    .atoms
                    .iter()
                    .find(|atom| atom.source_object_id.as_deref() == Some(&object.source_object_id))
                    .map(|atom| atom.order as usize)
            );
            assert_eq!(
                object.last_atom_order,
                snapshot
                    .page_graph
                    .atoms
                    .iter()
                    .rev()
                    .find(|atom| atom.source_object_id.as_deref() == Some(&object.source_object_id))
                    .map(|atom| atom.order as usize)
            );
        }
    }

    #[test]
    fn extracts_only_requested_pages_with_character_styles() {
        let _guard = pdfium_test_lock();
        let page_set = PageSet::from_pages([1, 3]).expect("page set");
        let extraction = extract_page_set(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            &page_set,
        )
        .expect("extract selected pages");

        assert_eq!(extraction.pages.len(), 2);
        assert_eq!(extraction.pages[0].page_number, 1);
        assert_eq!(extraction.pages[1].page_number, 3);
        assert!(extraction.pages.iter().all(|page| !page.atoms.is_empty()));
        assert!(extraction.pages.iter().all(|page| !page.styles.is_empty()));
        assert!(extraction
            .pages
            .iter()
            .flat_map(|page| &page.atoms)
            .any(|atom| {
                atom.style_id.is_some()
                    && atom.source_object_id.is_some()
                    && atom.bounds[2] >= atom.bounds[0]
                    && atom.bounds[3] >= atom.bounds[1]
            }));
    }

    #[test]
    fn repeated_extraction_keeps_object_ids_stable() {
        let _guard = pdfium_test_lock();
        let page_set = PageSet::from_pages([1]).expect("page set");
        let source = fixture_path("2305.13048v2.pdf");
        let first =
            extract_page_set(shared_pdfium(), &source, &page_set).expect("first extraction");
        let second =
            extract_page_set(shared_pdfium(), &source, &page_set).expect("second extraction");
        let first_ids = first.pages[0]
            .atoms
            .iter()
            .map(|atom| (&atom.atom_id, &atom.source_object_id))
            .collect::<Vec<_>>();
        let second_ids = second.pages[0]
            .atoms
            .iter()
            .map(|atom| (&atom.atom_id, &atom.source_object_id))
            .collect::<Vec<_>>();

        assert_eq!(first_ids, second_ids);
    }

    #[test]
    fn rejects_requested_pages_outside_document() {
        let _guard = pdfium_test_lock();
        let page_set = PageSet::from_pages([99]).expect("page set");
        let error = extract_page_set(
            shared_pdfium(),
            &fixture_path("simple-one-page.pdf"),
            &page_set,
        )
        .expect_err("out of bounds page must fail");

        assert_eq!(
            error,
            PdfV3ExtractionError::PageOutOfBounds {
                page: 99,
                page_count: 1
            }
        );
    }

    #[test]
    #[ignore = "manual Windows PDFium sparse-page timing probe"]
    fn manual_windows_sparse_page_probe() {
        let _guard = pdfium_test_lock();
        let page_set = PageSet::from_pages([1, 5, 10]).expect("page set");
        let extraction = extract_page_set(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            &page_set,
        )
        .expect("extract sparse pages");

        println!(
            "pdf-v3 sparse probe source_pages={} total={}ms pages={:?}",
            extraction.source_page_count, extraction.total_ms, extraction.page_timings_ms
        );
    }

    #[test]
    #[ignore = "manual Windows PDFium ten-page timing probe"]
    fn manual_windows_ten_page_probe() {
        let _guard = pdfium_test_lock();
        let page_set = PageSet::all(10).expect("page set");
        let extraction = extract_page_set(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            &page_set,
        )
        .expect("extract first ten pages");
        let character_count = extraction
            .page_timings_ms
            .iter()
            .map(|timing| timing.character_count)
            .sum::<usize>();
        let style_count = extraction
            .page_timings_ms
            .iter()
            .map(|timing| timing.style_count)
            .sum::<usize>();

        println!(
            "pdf-v3 ten-page probe source_pages={} total={}ms characters={} page_styles={}",
            extraction.source_page_count, extraction.total_ms, character_count, style_count
        );
    }
}
