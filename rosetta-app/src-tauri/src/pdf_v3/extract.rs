use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    time::{Duration, Instant},
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
    pub setup_ms: u64,
    pub object_snapshot_ms: u64,
    pub object_text_us: u64,
    pub object_identity_us: u64,
    pub character_geometry_ms: u64,
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

struct PdfiumTextObjectSnapshotBuilder {
    snapshot: PdfiumTextObjectSnapshot,
    object_api_text: String,
    mapped_text: String,
    unicode_complete: bool,
    style_id: Option<String>,
}

impl PdfiumTextObjectSnapshotBuilder {
    fn record_character(&mut self, character_index: usize, unicode: Option<char>) {
        let mapped_atom_count = self.snapshot.mapped_atom_count.get_or_insert(0);
        *mapped_atom_count += 1;
        self.snapshot
            .first_atom_order
            .get_or_insert(character_index);
        self.snapshot.last_atom_order = Some(character_index);
        if let Some(unicode) = unicode {
            self.mapped_text.push(unicode);
        } else {
            self.unicode_complete = false;
        }
    }

    fn finish(mut self) -> PdfiumTextObjectSnapshot {
        if self.unicode_complete {
            self.snapshot.mapped_unicode_atom_count = Some(self.mapped_text.chars().count());
            self.snapshot.text = self.mapped_text;
        } else {
            self.snapshot.text = self.object_api_text;
        }
        self.snapshot
    }
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
    let setup_ms = elapsed_ms(page_started);

    let mut atoms = Vec::with_capacity(page_text.len().max(0) as usize);
    let mut styles = BTreeMap::<String, PageStyle>::new();
    let mut warnings = BTreeSet::<String>::new();
    let page_objects = page.objects();
    let page_object_count = page_objects.len();
    let mut form_object_count = 0usize;
    let mut text_object_builders = Vec::new();
    let mut text_object_indices_by_identity = BTreeMap::<usize, Vec<usize>>::new();
    let mut object_text_elapsed = Duration::ZERO;
    let object_snapshot_started = Instant::now();
    for (page_object_index, object) in page_objects.iter().enumerate() {
        let source_object_id = format!("page-{page_number:04}-object-{page_object_index:06}");
        collect_pdfium_object_snapshot(
            &object,
            &page_text,
            source_object_id,
            page_object_index,
            &mut form_object_count,
            &mut text_object_builders,
            &mut text_object_indices_by_identity,
            &mut object_text_elapsed,
            &mut warnings,
        );
    }
    let object_snapshot_ms = elapsed_ms(object_snapshot_started);

    let mut object_identity_elapsed = Duration::ZERO;
    let character_geometry_started = Instant::now();
    for character in page_text.chars().iter() {
        let generated = character.is_generated().unwrap_or(false);
        let hyphen = character.is_hyphen().unwrap_or(false);
        let mapped_unicode = character.unicode_char();
        let unicode = mapped_unicode.unwrap_or('\u{fffd}');
        if unicode == '\u{fffd}' {
            warnings.insert("invalid-unicode-mapping".to_string());
        }

        let object_identity_started = Instant::now();
        let object_identity = character.text_object_identity().ok();
        object_identity_elapsed += object_identity_started.elapsed();
        let object_indices =
            object_identity.and_then(|identity| text_object_indices_by_identity.get(&identity));
        let source_object_id = object_indices.and_then(|indices| {
            indices.first().map(|index| {
                text_object_builders[*index]
                    .snapshot
                    .source_object_id
                    .clone()
            })
        });
        let style_id = if let Some(object_indices) = object_indices {
            let primary_index = object_indices[0];
            let style_id =
                if let Some(style_id) = text_object_builders[primary_index].style_id.as_ref() {
                    style_id.clone()
                } else {
                    let style = style_for_character(&character);
                    let style_id = style.style_id.clone();
                    styles.entry(style_id.clone()).or_insert(style);
                    for object_index in object_indices {
                        text_object_builders[*object_index].style_id = Some(style_id.clone());
                    }
                    style_id
                };
            for object_index in object_indices {
                text_object_builders[*object_index]
                    .record_character(character.index(), mapped_unicode);
            }
            style_id
        } else {
            let style = style_for_character(&character);
            let style_id = style.style_id.clone();
            styles.entry(style_id.clone()).or_insert(style);
            style_id
        };

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
            source_object_id,
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
    let character_geometry_ms = elapsed_ms(character_geometry_started);

    let text_objects = text_object_builders
        .into_iter()
        .map(PdfiumTextObjectSnapshotBuilder::finish)
        .collect();
    let styles = styles.into_values().collect::<Vec<_>>();
    if atoms.iter().any(|atom| atom.source_object_id.is_none()) {
        warnings.insert("pdfium-character-object-unmapped".to_string());
    }
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
    let timing = PageExtractionTiming {
        page_number,
        elapsed_ms: elapsed_ms(page_started),
        setup_ms,
        object_snapshot_ms,
        object_text_us: elapsed_us(object_text_elapsed),
        object_identity_us: elapsed_us(object_identity_elapsed),
        character_geometry_ms,
        character_count: page_graph.atoms.len(),
        style_count: page_graph.styles.len(),
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
    text_objects: &mut Vec<PdfiumTextObjectSnapshotBuilder>,
    text_object_indices_by_identity: &mut BTreeMap<usize, Vec<usize>>,
    object_text_elapsed: &mut Duration,
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
                    text_object_indices_by_identity,
                    object_text_elapsed,
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
    let object_text_started = Instant::now();
    let object_api_text = page_text.for_object(text_object);
    *object_text_elapsed += object_text_started.elapsed();
    let object_identity = text_object.object_identity();
    let text_object_index = text_objects.len();
    text_object_indices_by_identity
        .entry(object_identity)
        .or_default()
        .push(text_object_index);
    text_objects.push(PdfiumTextObjectSnapshotBuilder {
        snapshot: PdfiumTextObjectSnapshot {
            source_object_id,
            page_object_index: root_page_object_index,
            text_object_index,
            font_name: text_object.font().name(),
            text: String::new(),
            mapped_atom_count: Some(0),
            mapped_unicode_atom_count: None,
            first_atom_order: None,
            last_atom_order: None,
        },
        object_api_text,
        mapped_text: String::new(),
        unicode_complete: true,
        style_id: None,
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

pub(crate) fn page_source_hash(source_fingerprint: &str, page_number: u32) -> String {
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

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn elapsed_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use pdfium_render::prelude::{PdfPageObject, PdfPageObjectsCommon, PdfPageText};

    use super::{
        extract_page_set, extract_pdfium_page_snapshot, style_for_character, PdfV3ExtractionError,
        PdfiumPageSnapshot,
    };
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
    fn text_object_style_cache_matches_direct_character_styles() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let snapshot = extract_pdfium_page_snapshot(&handle, 1).expect("page snapshot");
        let page = handle.pdfium_document().pages().get(0).expect("first page");
        let page_text = page.text().expect("page text");

        for character in page_text.chars().iter() {
            let atom = &snapshot.page_graph.atoms[character.index()];
            let actual = snapshot
                .page_graph
                .styles
                .iter()
                .find(|style| Some(&style.style_id) == atom.style_id.as_ref())
                .expect("atom style");
            assert_eq!(actual, &style_for_character(&character));
        }
    }

    #[test]
    fn single_pass_object_identity_matches_pdfium_object_scans() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let snapshot = extract_pdfium_page_snapshot(&handle, 1).expect("page snapshot");
        let page = handle.pdfium_document().pages().get(0).expect("first page");
        let page_text = page.text().expect("page text");

        for (page_object_index, object) in page.objects().iter().enumerate() {
            assert_object_mapping_matches_pdfium_scan(
                &object,
                &page_text,
                format!("page-0001-object-{page_object_index:06}"),
                &snapshot,
            );
        }
    }

    fn assert_object_mapping_matches_pdfium_scan<'a>(
        object: &PdfPageObject<'a>,
        page_text: &'a PdfPageText<'a>,
        source_object_id: String,
        snapshot: &PdfiumPageSnapshot,
    ) {
        if let Some(form_object) = object.as_x_object_form_object() {
            for child_index in form_object.as_range() {
                let child = form_object.get(child_index).expect("form child");
                assert_object_mapping_matches_pdfium_scan(
                    &child,
                    page_text,
                    format!("{source_object_id}-form-{child_index:06}"),
                    snapshot,
                );
            }
            return;
        }

        let Some(text_object) = object.as_text_object() else {
            return;
        };
        let expected_characters = text_object.chars(page_text).expect("object characters");
        let expected_text = expected_characters
            .iter()
            .map(|character| character.unicode_char())
            .collect::<Option<String>>()
            .unwrap_or_else(|| page_text.for_object(text_object));
        let actual = snapshot
            .text_objects
            .iter()
            .find(|object| object.source_object_id == source_object_id)
            .expect("text object snapshot");

        assert_eq!(actual.text, expected_text);
        assert_eq!(actual.mapped_atom_count, Some(expected_characters.len()));
        assert_eq!(
            actual.mapped_unicode_atom_count,
            expected_characters
                .iter()
                .map(|character| character.unicode_char())
                .collect::<Option<String>>()
                .map(|text| text.chars().count())
        );
        assert_eq!(
            actual.first_atom_order,
            expected_characters.first_char_index()
        );
        assert_eq!(
            actual.last_atom_order,
            expected_characters.last_char_index()
        );
        for character in expected_characters.iter() {
            assert_eq!(
                snapshot.page_graph.atoms[character.index()]
                    .source_object_id
                    .as_deref(),
                Some(source_object_id.as_str())
            );
        }
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
