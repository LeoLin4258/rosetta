use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use lopdf::{content::Content, Dictionary, Document, Encoding, Object, ObjectId};
use pdfium_render::prelude::Pdfium;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    content_stream::{operand_id, text_show_id},
    document::{DocumentHandle, DocumentHandleError},
    extract::{extract_pdfium_page_snapshot, PdfV3ExtractionError, PdfiumPageSnapshot},
    identity::text_hash,
    page_context::{PdfPageContextError, PdfPageObjectContext, PdfResourceContext},
    page_index::{PdfPageIndex, PdfPageIndexError},
    source_cmap::{ToUnicodeDecodedUnit, ToUnicodeMap},
    source_object::{PdfObjectView, PdfSourceObjectError},
    types::FormInvocationStep,
};

const MAX_FORM_XOBJECT_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FontEncodingKind {
    OneByte,
    IdentityCMap,
    NamedCMap,
    DifferencesDictionary,
    ToUnicodeOnly,
    Implicit,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TextShowDecodeStatus {
    Decoded,
    FontResourceMissing,
    FontDecoderUnavailable,
    DecodeFailed,
    NoStringOperands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OperandMappingStatus {
    Exact,
    WhitespaceEquivalent,
    OrdinalCountMismatch,
    OutsideTextObject,
    DecodeUnavailable,
    DecodedTextMismatch,
    FontMismatch,
    AtomCoverageMismatch,
    SharedContentStream,
}

impl OperandMappingStatus {
    fn fallback_reason(self) -> Option<&'static str> {
        match self {
            Self::Exact => None,
            Self::WhitespaceEquivalent => Some("text-match-requires-whitespace-remap"),
            Self::OrdinalCountMismatch => Some("text-object-show-count-mismatch"),
            Self::OutsideTextObject => Some("text-show-outside-bt-et"),
            Self::DecodeUnavailable => Some("text-show-decode-unavailable"),
            Self::DecodedTextMismatch => Some("decoded-text-object-mismatch"),
            Self::FontMismatch => Some("font-resource-object-mismatch"),
            Self::AtomCoverageMismatch => Some("pdfium-object-atom-coverage-mismatch"),
            Self::SharedContentStream => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FontResourceInspection {
    pub resource_name: String,
    pub base_font: Option<String>,
    pub normalized_base_font: Option<String>,
    pub subtype: Option<String>,
    pub encoding_kind: FontEncodingKind,
    pub encoding_name: Option<String>,
    pub has_to_unicode: bool,
    pub embedded: bool,
    pub source_decode_available: bool,
    pub source_decode_error: Option<String>,
    pub source_reencode_candidate: bool,
    pub translated_text_reuse_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextShowInspection {
    pub text_show_id: String,
    pub text_show_index: usize,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub operator: String,
    pub font_resource: Option<String>,
    pub font_base_name: Option<String>,
    pub inside_text_object: bool,
    pub operand_count: usize,
    pub encoded_byte_count: usize,
    pub encoded_byte_hash: String,
    pub decode_status: TextShowDecodeStatus,
    pub decoded_char_count: Option<usize>,
    pub decoded_text_hash: Option<String>,
    pub source_round_trip_exact: Option<bool>,
    pub source_font_resource: Option<String>,
    pub source_font_size: Option<f32>,
    pub source_horizontal_scaling: f32,
    pub form_invocation_path: Vec<FormInvocationStep>,
    pub shared_content_stream: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfiumTextObjectInspection {
    pub source_object_id: String,
    pub page_object_index: usize,
    pub text_object_index: usize,
    pub font_name: String,
    pub normalized_font_name: String,
    pub text_char_count: usize,
    pub text_hash: String,
    pub mapped_atom_count: Option<usize>,
    pub mapped_unicode_atom_count: Option<usize>,
    pub first_atom_order: Option<usize>,
    pub last_atom_order: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextObjectOperandMapping {
    pub mapping_id: String,
    pub source_object_id: String,
    pub text_show_id: String,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub text_show_operator: String,
    pub text_show_operand_hash: String,
    pub text_object_index: usize,
    pub text_show_index: usize,
    pub object_text_hash: String,
    pub decoded_text_hash: Option<String>,
    pub object_text_chars: usize,
    pub decoded_text_chars: Option<usize>,
    pub object_whitespace_chars: usize,
    pub decoded_whitespace_chars: Option<usize>,
    pub text_match_ignoring_whitespace: bool,
    pub first_non_whitespace_object_codepoint: Option<u32>,
    pub first_non_whitespace_decoded_codepoint: Option<u32>,
    pub mapped_atom_count: Option<usize>,
    pub font_resource: Option<String>,
    pub pdfium_font_name: String,
    pub font_name_match: bool,
    pub atom_coverage_match: bool,
    pub source_round_trip_exact: Option<bool>,
    pub source_font_resource: Option<String>,
    pub source_font_size: Option<f32>,
    pub source_horizontal_scaling: f32,
    pub form_invocation_path: Vec<FormInvocationStep>,
    pub status: OperandMappingStatus,
    #[serde(skip_serializing)]
    pub decoded_units: Vec<DecodedTextUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedTextUnit {
    pub operand_id: String,
    pub operand_index: usize,
    pub array_index: Option<usize>,
    pub encoded_start: usize,
    pub encoded_len: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageOperandMappingResult {
    pub schema: &'static str,
    pub page_number: u32,
    pub source_page_count: u32,
    pub pdfium_page_object_count: usize,
    pub pdfium_text_object_count: usize,
    pub pdfium_form_object_count: usize,
    pub content_stream_count: usize,
    pub form_xobject_invocation_count: usize,
    pub unique_form_stream_count: usize,
    pub shared_form_stream_count: usize,
    pub text_show_count: usize,
    pub ordinal_alignment_valid: bool,
    pub paired_mapping_count: usize,
    pub exact_mapping_count: usize,
    pub whitespace_equivalent_mapping_count: usize,
    pub unmatched_text_object_count: usize,
    pub unmatched_text_show_count: usize,
    pub font_resources: Vec<FontResourceInspection>,
    pub text_objects: Vec<PdfiumTextObjectInspection>,
    pub text_shows: Vec<TextShowInspection>,
    pub mappings: Vec<TextObjectOperandMapping>,
    pub fallback_reasons: Vec<String>,
    pub elapsed_ms: u64,
    #[serde(skip_serializing)]
    pub timing: PageOperandMappingTiming,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PageOperandMappingTiming {
    pub page_lookup_us: u64,
    pub collect_text_shows_us: u64,
    pub stream_decode_us: u64,
    pub font_inspection_us: u64,
    pub text_show_decode_us: u64,
    pub object_prepare_us: u64,
    pub pair_mappings_us: u64,
    pub stream_decode_cache_hits: usize,
}

#[derive(Debug)]
pub(crate) enum PageOperandMappingError {
    Open(DocumentHandleError),
    Inspection(PdfV3ExtractionError),
    PageIndex(PdfPageIndexError),
    PageContext(PdfPageContextError),
    SourceObject(PdfSourceObjectError),
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    FontRead(String),
    StreamRead {
        object_number: u32,
        generation: u16,
        message: String,
    },
    ContentDecode {
        object_number: u32,
        generation: u16,
        message: String,
    },
}

impl fmt::Display for PageOperandMappingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => error.fmt(formatter),
            Self::Inspection(error) => error.fmt(formatter),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::PageContext(error) => error.fmt(formatter),
            Self::SourceObject(error) => error.fmt(formatter),
            Self::FontRead(message) => formatter.write_str(message),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::StreamRead {
                object_number,
                generation,
                message,
            } => write!(
                formatter,
                "failed to read PDF content stream {object_number} {generation}: {message}"
            ),
            Self::ContentDecode {
                object_number,
                generation,
                message,
            } => write!(
                formatter,
                "failed to decode PDF content stream {object_number} {generation}: {message}"
            ),
        }
    }
}

impl std::error::Error for PageOperandMappingError {}

impl From<DocumentHandleError> for PageOperandMappingError {
    fn from(value: DocumentHandleError) -> Self {
        Self::Open(value)
    }
}

impl From<PdfV3ExtractionError> for PageOperandMappingError {
    fn from(value: PdfV3ExtractionError) -> Self {
        Self::Inspection(value)
    }
}

impl From<PdfPageIndexError> for PageOperandMappingError {
    fn from(value: PdfPageIndexError) -> Self {
        Self::PageIndex(value)
    }
}

impl From<PdfPageContextError> for PageOperandMappingError {
    fn from(value: PdfPageContextError) -> Self {
        Self::PageContext(value)
    }
}

impl From<PdfSourceObjectError> for PageOperandMappingError {
    fn from(value: PdfSourceObjectError) -> Self {
        Self::SourceObject(value)
    }
}

struct FontDecoder {
    resource_name: Vec<u8>,
    decoder: SourceDecoder,
    source_reencode_candidate: bool,
}

enum SourceDecoder {
    Lopdf {
        font: Dictionary,
        document: Document,
    },
    ToUnicode(ToUnicodeMap),
}

impl SourceDecoder {
    fn decode(&self, encoded: &[u8]) -> Result<String, String> {
        match self {
            Self::Lopdf { font, document } => {
                let encoding = font
                    .get_font_encoding(document)
                    .map_err(|error| error.to_string())?;
                Document::decode_text(&encoding, encoded).map_err(|error| error.to_string())
            }
            Self::ToUnicode(map) => map.decode(encoded).map_err(|error| error.to_string()),
        }
    }

    fn decode_units(&self, encoded: &[u8]) -> Result<Vec<SourceDecodedUnit>, String> {
        match self {
            Self::ToUnicode(map) => map
                .decode_units(encoded)
                .map_err(|error| error.to_string())
                .map(|units| units.into_iter().map(SourceDecodedUnit::from).collect()),
            Self::Lopdf { font, document } => match font.get_font_encoding(document) {
                Ok(Encoding::OneByteEncoding(_)) => encoded
                    .iter()
                    .enumerate()
                    .map(|(encoded_start, byte)| {
                        self.decode(std::slice::from_ref(byte))
                            .map(|text| SourceDecodedUnit {
                                text,
                                encoded_start,
                                encoded_len: 1,
                            })
                    })
                    .collect(),
                Ok(_) => self.decode(encoded).map(|text| {
                    vec![SourceDecodedUnit {
                        text,
                        encoded_start: 0,
                        encoded_len: encoded.len(),
                    }]
                }),
                Err(error) => Err(error.to_string()),
            },
        }
    }

    fn encode(&self, text: &str) -> Option<Vec<u8>> {
        match self {
            Self::Lopdf { font, document } => font
                .get_font_encoding(document)
                .ok()
                .filter(encoding_can_reencode)
                .map(|encoding| Document::encode_text(&encoding, text)),
            _ => None,
        }
    }
}

struct SourceDecodedUnit {
    text: String,
    encoded_start: usize,
    encoded_len: usize,
}

impl From<ToUnicodeDecodedUnit> for SourceDecodedUnit {
    fn from(value: ToUnicodeDecodedUnit) -> Self {
        Self {
            text: value.text,
            encoded_start: value.encoded_start,
            encoded_len: value.encoded_len,
        }
    }
}

struct RawTextOperand<'a> {
    operand_id: String,
    operand_index: usize,
    array_index: Option<usize>,
    encoded: &'a [u8],
}

struct RawTextShow {
    inspection: TextShowInspection,
    decoded_text: Option<String>,
    decoded_units: Vec<DecodedTextUnit>,
}

struct RawPdfiumTextObject {
    inspection: PdfiumTextObjectInspection,
    text: String,
}

struct CollectedTextShows {
    text_shows: Vec<RawTextShow>,
    font_resources: Vec<FontResourceInspection>,
    content_stream_count: usize,
    form_xobject_invocation_count: usize,
    unique_form_stream_count: usize,
    shared_form_stream_count: usize,
    fallback_reasons: BTreeSet<String>,
    stream_decode_elapsed: Duration,
    font_inspection_elapsed: Duration,
    text_show_decode_elapsed: Duration,
    stream_decode_cache_hits: usize,
}

#[derive(Default)]
struct TextShowCollection {
    text_shows: Vec<RawTextShow>,
    font_resources: BTreeMap<String, FontResourceInspection>,
    visited_streams: HashSet<ObjectId>,
    form_invocation_counts: HashMap<ObjectId, usize>,
    active_form_streams: HashSet<ObjectId>,
    fallback_reasons: BTreeSet<String>,
    stream_decode_elapsed: Duration,
    font_inspection_elapsed: Duration,
    text_show_decode_elapsed: Duration,
    decoded_streams: HashMap<ObjectId, Arc<Content>>,
    stream_decode_cache_hits: usize,
}

#[derive(Clone)]
struct TextOperatorState {
    current_font: Option<Vec<u8>>,
    current_font_size: Option<f32>,
    horizontal_scaling: f32,
    inside_text_object: bool,
    saved_states: Vec<(Option<Vec<u8>>, Option<f32>, f32)>,
}

impl Default for TextOperatorState {
    fn default() -> Self {
        Self {
            current_font: None,
            current_font_size: None,
            horizontal_scaling: 100.0,
            inside_text_object: false,
            saved_states: Vec::new(),
        }
    }
}

pub(crate) fn map_page_atoms_to_content_operands(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
) -> Result<PageOperandMappingResult, PageOperandMappingError> {
    let handle = DocumentHandle::open(pdfium, source_path)?;
    map_page_atoms_to_content_operands_from_handle(&handle, page_number)
}

pub(crate) fn map_page_atoms_to_content_operands_from_handle(
    handle: &DocumentHandle<'_>,
    page_number: u32,
) -> Result<PageOperandMappingResult, PageOperandMappingError> {
    let snapshot = extract_pdfium_page_snapshot(handle, page_number)?;
    map_page_atoms_to_content_operands_from_snapshot(handle, &snapshot)
}

pub(crate) fn map_page_atoms_to_content_operands_from_snapshot(
    handle: &DocumentHandle<'_>,
    snapshot: &PdfiumPageSnapshot,
) -> Result<PageOperandMappingResult, PageOperandMappingError> {
    let started = Instant::now();
    let source_objects = handle.source_objects();
    let page_number = snapshot.page_graph.page_number;

    let page_lookup_started = Instant::now();
    let source_page_count = handle.page_count();
    let page_index = PdfPageIndex::resolve_page(source_objects, page_number)?;
    let indexed_page = page_index.page(page_number)?;
    let page_context = PdfPageObjectContext::resolve(source_objects, indexed_page)?;
    let page_lookup_us = elapsed_us(page_lookup_started.elapsed());
    let collect_text_shows_started = Instant::now();
    let collected_text_shows = collect_text_shows(
        source_objects,
        indexed_page.content_stream_ids(),
        page_context.resource_context(),
        page_number,
    )?;
    let collect_text_shows_us = elapsed_us(collect_text_shows_started.elapsed());
    let content_stream_count = collected_text_shows.content_stream_count;
    let form_xobject_invocation_count = collected_text_shows.form_xobject_invocation_count;
    let unique_form_stream_count = collected_text_shows.unique_form_stream_count;
    let shared_form_stream_count = collected_text_shows.shared_form_stream_count;
    let font_resources = collected_text_shows.font_resources;
    let raw_text_shows = collected_text_shows.text_shows;
    let stream_decode_us = elapsed_us(collected_text_shows.stream_decode_elapsed);
    let font_inspection_us = elapsed_us(collected_text_shows.font_inspection_elapsed);
    let text_show_decode_us = elapsed_us(collected_text_shows.text_show_decode_elapsed);
    let pdfium_page_object_count = snapshot.page_object_count;
    let pdfium_form_object_count = snapshot.form_object_count;
    let object_prepare_started = Instant::now();
    let raw_text_objects = snapshot
        .text_objects
        .iter()
        .map(|object| {
            let font_name = object.font_name.clone();
            let text = object.text.clone();
            RawPdfiumTextObject {
                inspection: PdfiumTextObjectInspection {
                    source_object_id: object.source_object_id.clone(),
                    page_object_index: object.page_object_index,
                    text_object_index: object.text_object_index,
                    normalized_font_name: normalize_font_name(&font_name),
                    font_name,
                    text_char_count: text.chars().count(),
                    text_hash: text_hash(&text),
                    mapped_atom_count: object.mapped_atom_count,
                    mapped_unicode_atom_count: object.mapped_unicode_atom_count,
                    first_atom_order: object.first_atom_order,
                    last_atom_order: object.last_atom_order,
                },
                text,
            }
        })
        .collect::<Vec<_>>();
    let object_prepare_us = elapsed_us(object_prepare_started.elapsed());

    let ordinal_alignment_valid = raw_text_objects.len() == raw_text_shows.len();
    let paired_mapping_count = raw_text_objects.len().min(raw_text_shows.len());
    let mut fallback_reasons = BTreeSet::<String>::new();
    fallback_reasons.extend(collected_text_shows.fallback_reasons);
    if !ordinal_alignment_valid {
        fallback_reasons.insert("text-object-show-count-mismatch".to_string());
    }
    if pdfium_form_object_count != form_xobject_invocation_count {
        fallback_reasons.insert("form-xobject-invocation-count-mismatch".to_string());
    }

    let pair_mappings_started = Instant::now();
    let mut mappings = Vec::with_capacity(paired_mapping_count);
    for (text_object, text_show) in raw_text_objects.iter().zip(raw_text_shows.iter()) {
        let font_name_match = text_show
            .inspection
            .font_base_name
            .as_deref()
            .map(normalize_font_name)
            .is_some_and(|font| font == text_object.inspection.normalized_font_name);
        let decoded_text_matches = text_show
            .decoded_text
            .as_ref()
            .is_some_and(|decoded| decoded == &text_object.text);
        let object_whitespace_chars = whitespace_char_count(&text_object.text);
        let decoded_whitespace_chars = text_show.decoded_text.as_deref().map(whitespace_char_count);
        let text_match_ignoring_whitespace =
            text_show.decoded_text.as_deref().is_some_and(|decoded| {
                text_without_whitespace(decoded) == text_without_whitespace(&text_object.text)
            });
        let (first_non_whitespace_object_codepoint, first_non_whitespace_decoded_codepoint) =
            text_show
                .decoded_text
                .as_deref()
                .map(|decoded| first_non_whitespace_difference(&text_object.text, decoded))
                .unwrap_or((None, None));
        let atom_coverage_matches = text_object
            .inspection
            .mapped_atom_count
            .is_some_and(|count| count == text_object.inspection.text_char_count)
            && text_object.inspection.mapped_unicode_atom_count
                == text_object.inspection.mapped_atom_count;
        let status = if !ordinal_alignment_valid {
            OperandMappingStatus::OrdinalCountMismatch
        } else if !text_show.inspection.inside_text_object {
            OperandMappingStatus::OutsideTextObject
        } else if text_show.decoded_text.is_none() {
            OperandMappingStatus::DecodeUnavailable
        } else if !font_name_match {
            OperandMappingStatus::FontMismatch
        } else if !atom_coverage_matches {
            OperandMappingStatus::AtomCoverageMismatch
        } else if text_show.inspection.shared_content_stream {
            OperandMappingStatus::SharedContentStream
        } else if !decoded_text_matches && text_match_ignoring_whitespace {
            OperandMappingStatus::WhitespaceEquivalent
        } else if !decoded_text_matches {
            OperandMappingStatus::DecodedTextMismatch
        } else {
            OperandMappingStatus::Exact
        };
        if let Some(reason) = status.fallback_reason() {
            fallback_reasons.insert(reason.to_string());
        }
        if !font_name_match {
            fallback_reasons.insert("font-resource-object-mismatch".to_string());
        }
        if !atom_coverage_matches {
            fallback_reasons.insert("pdfium-object-atom-coverage-mismatch".to_string());
        }
        mappings.push(TextObjectOperandMapping {
            mapping_id: format!(
                "page-{page_number:04}-map-{:06}",
                text_object.inspection.text_object_index
            ),
            source_object_id: text_object.inspection.source_object_id.clone(),
            text_show_id: text_show.inspection.text_show_id.clone(),
            stream_object_number: text_show.inspection.stream_object_number,
            stream_generation: text_show.inspection.stream_generation,
            operation_index: text_show.inspection.operation_index,
            text_show_operator: text_show.inspection.operator.clone(),
            text_show_operand_hash: text_show.inspection.encoded_byte_hash.clone(),
            text_object_index: text_object.inspection.text_object_index,
            text_show_index: text_show.inspection.text_show_index,
            object_text_hash: text_object.inspection.text_hash.clone(),
            decoded_text_hash: text_show.inspection.decoded_text_hash.clone(),
            object_text_chars: text_object.inspection.text_char_count,
            decoded_text_chars: text_show.inspection.decoded_char_count,
            object_whitespace_chars,
            decoded_whitespace_chars,
            text_match_ignoring_whitespace,
            first_non_whitespace_object_codepoint,
            first_non_whitespace_decoded_codepoint,
            mapped_atom_count: text_object.inspection.mapped_atom_count,
            font_resource: text_show.inspection.font_resource.clone(),
            pdfium_font_name: text_object.inspection.font_name.clone(),
            font_name_match,
            atom_coverage_match: atom_coverage_matches,
            source_round_trip_exact: text_show.inspection.source_round_trip_exact,
            source_font_resource: text_show.inspection.source_font_resource.clone(),
            source_font_size: text_show.inspection.source_font_size,
            source_horizontal_scaling: text_show.inspection.source_horizontal_scaling,
            form_invocation_path: text_show.inspection.form_invocation_path.clone(),
            status,
            decoded_units: text_show.decoded_units.clone(),
        });
    }
    let pair_mappings_us = elapsed_us(pair_mappings_started.elapsed());

    for show in &raw_text_shows {
        if show.inspection.decode_status != TextShowDecodeStatus::Decoded {
            fallback_reasons.insert("text-show-decode-unavailable".to_string());
        }
    }
    let exact_mapping_count = mappings
        .iter()
        .filter(|mapping| mapping.status == OperandMappingStatus::Exact)
        .count();
    let whitespace_equivalent_mapping_count = mappings
        .iter()
        .filter(|mapping| mapping.status == OperandMappingStatus::WhitespaceEquivalent)
        .count();

    Ok(PageOperandMappingResult {
        schema: "rosetta-pdf-v3-page-operand-mapping/2",
        page_number,
        source_page_count,
        pdfium_page_object_count,
        pdfium_text_object_count: raw_text_objects.len(),
        pdfium_form_object_count,
        content_stream_count,
        form_xobject_invocation_count,
        unique_form_stream_count,
        shared_form_stream_count,
        text_show_count: raw_text_shows.len(),
        ordinal_alignment_valid,
        paired_mapping_count,
        exact_mapping_count,
        whitespace_equivalent_mapping_count,
        unmatched_text_object_count: raw_text_objects.len().saturating_sub(paired_mapping_count),
        unmatched_text_show_count: raw_text_shows.len().saturating_sub(paired_mapping_count),
        font_resources,
        text_objects: raw_text_objects
            .into_iter()
            .map(|object| object.inspection)
            .collect(),
        text_shows: raw_text_shows
            .into_iter()
            .map(|show| show.inspection)
            .collect(),
        mappings,
        fallback_reasons: fallback_reasons.into_iter().collect(),
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        timing: PageOperandMappingTiming {
            page_lookup_us,
            collect_text_shows_us,
            stream_decode_us,
            font_inspection_us,
            text_show_decode_us,
            object_prepare_us,
            pair_mappings_us,
            stream_decode_cache_hits: collected_text_shows.stream_decode_cache_hits,
        },
    })
}

fn inspect_font_resources(
    objects: &dyn PdfObjectView,
    resources: &PdfResourceContext,
    scope_id: &str,
) -> Result<(Vec<FontResourceInspection>, Vec<FontDecoder>), PageOperandMappingError> {
    let fonts = fonts_for_context(objects, resources)?;
    let mut inspections = Vec::with_capacity(fonts.len());
    let mut decoders = Vec::with_capacity(fonts.len());
    for (resource_name, font) in fonts {
        let (encoding_kind, encoding_name) = classify_font_encoding(&font);
        let has_to_unicode = font.get(b"ToUnicode").is_ok();
        let embedded = font_is_embedded(&font, objects);
        let (decoder, source_decode_error) = source_decoder(&font, objects, encoding_kind);
        let source_decode_available = decoder.is_some();
        let source_reencode_candidate = decoder
            .as_ref()
            .is_some_and(|decoder| decoder.encode("").is_some());
        if let Some(decoder) = decoder {
            decoders.push(FontDecoder {
                resource_name: resource_name.clone(),
                decoder,
                source_reencode_candidate,
            });
        }
        let base_font = font
            .get(b"BaseFont")
            .and_then(Object::as_name)
            .ok()
            .map(|name| String::from_utf8_lossy(name).into_owned());
        inspections.push(FontResourceInspection {
            resource_name: format!("{scope_id}/{}", String::from_utf8_lossy(&resource_name)),
            normalized_base_font: base_font.as_deref().map(normalize_font_name),
            base_font,
            subtype: font
                .get(b"Subtype")
                .and_then(Object::as_name)
                .ok()
                .map(|name| String::from_utf8_lossy(name).into_owned()),
            encoding_kind,
            encoding_name,
            has_to_unicode,
            embedded,
            source_decode_available,
            source_decode_error,
            source_reencode_candidate,
            translated_text_reuse_allowed: false,
        });
    }
    Ok((inspections, decoders))
}

fn fonts_for_context(
    objects: &dyn PdfObjectView,
    resources: &PdfResourceContext,
) -> Result<BTreeMap<Vec<u8>, Dictionary>, PageOperandMappingError> {
    let mut fonts = BTreeMap::new();
    let Ok(value) = resources.dictionary().get(b"Font") else {
        return Ok(fonts);
    };
    let Some(font_dictionary) = dereference_dictionary(value, objects)? else {
        return Ok(fonts);
    };
    for (name, object) in font_dictionary.iter() {
        if let Some(font) = dereference_dictionary(object, objects)? {
            fonts.insert(name.clone(), font);
        }
    }
    Ok(fonts)
}

fn source_decoder(
    font: &Dictionary,
    objects: &dyn PdfObjectView,
    encoding_kind: FontEncodingKind,
) -> (Option<SourceDecoder>, Option<String>) {
    let to_unicode = font
        .get(b"ToUnicode")
        .ok()
        .and_then(|object| resolve_object(object, objects).ok().flatten())
        .and_then(|object| object.as_stream().ok().cloned())
        .map(|stream| {
            stream
                .get_plain_content()
                .map_err(|error| format!("failed to decompress ToUnicode CMap: {error}"))
                .and_then(|content| {
                    ToUnicodeMap::parse(&content)
                        .map_err(|error| format!("failed to parse ToUnicode CMap: {error}"))
                })
        });

    if let Some(Ok(map)) = to_unicode.as_ref() {
        return (Some(SourceDecoder::ToUnicode(map.clone())), None);
    }

    let mut fallback_font = font.clone();
    if let Ok(to_unicode) = font.get(b"ToUnicode") {
        if let Ok(Some(object)) = resolve_object(to_unicode, objects) {
            fallback_font.set("ToUnicode", object);
        }
    }
    let fallback_document = Document::new();
    match fallback_font.get_font_encoding(&fallback_document) {
        Ok(encoding)
            if lopdf_fallback_allowed(font, encoding_kind) && encoding_can_decode(&encoding) =>
        {
            return (
                Some(SourceDecoder::Lopdf {
                    font: fallback_font,
                    document: fallback_document,
                }),
                None,
            );
        }
        lopdf_result => {
            let mut errors = Vec::new();
            if let Some(Err(error)) = to_unicode {
                errors.push(error);
            }
            match lopdf_result {
                Err(error) => errors.push(format!("lopdf decoder unavailable: {error}")),
                Ok(_) => errors.push("font encoding is not supported for source decoding".into()),
            }
            (None, Some(errors.join("; ")))
        }
    }
}

fn lopdf_fallback_allowed(font: &Dictionary, encoding_kind: FontEncodingKind) -> bool {
    match encoding_kind {
        FontEncodingKind::OneByte
        | FontEncodingKind::IdentityCMap
        | FontEncodingKind::NamedCMap => true,
        FontEncodingKind::Implicit => font
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Type1"),
        FontEncodingKind::DifferencesDictionary
        | FontEncodingKind::ToUnicodeOnly
        | FontEncodingKind::Unsupported => false,
    }
}

fn classify_font_encoding(font: &Dictionary) -> (FontEncodingKind, Option<String>) {
    match font.get(b"Encoding") {
        Ok(Object::Name(name)) => {
            let name = String::from_utf8_lossy(name).into_owned();
            let kind = match name.as_str() {
                "StandardEncoding" | "MacRomanEncoding" | "MacExpertEncoding"
                | "WinAnsiEncoding" | "PDFDocEncoding" => FontEncodingKind::OneByte,
                "Identity-H" | "Identity-V" => FontEncodingKind::IdentityCMap,
                _ => FontEncodingKind::NamedCMap,
            };
            (kind, Some(name))
        }
        Ok(Object::Dictionary(_)) | Ok(Object::Reference(_)) => {
            (FontEncodingKind::DifferencesDictionary, None)
        }
        Err(_) if font.get(b"ToUnicode").is_ok() => (FontEncodingKind::ToUnicodeOnly, None),
        Err(_) => (FontEncodingKind::Implicit, None),
        _ => (FontEncodingKind::Unsupported, None),
    }
}

fn encoding_can_decode(encoding: &Encoding<'_>) -> bool {
    match encoding {
        Encoding::OneByteEncoding(_) | Encoding::UnicodeMapEncoding(_) => true,
        Encoding::SimpleEncoding(name) => {
            matches!(*name, "UniGB-UCS2-H" | "UniGB-UTF16-H" | "UniGB−UTF16−H")
        }
    }
}

fn encoding_can_reencode(encoding: &Encoding<'_>) -> bool {
    matches!(encoding, Encoding::OneByteEncoding(_))
}

fn font_is_embedded(font: &Dictionary, objects: &dyn PdfObjectView) -> bool {
    font_descriptor(font, objects).is_some_and(|descriptor| {
        [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
            .iter()
            .any(|key| descriptor.get(key).is_ok())
    })
}

fn font_descriptor(font: &Dictionary, objects: &dyn PdfObjectView) -> Option<Dictionary> {
    if let Some(descriptor) = dictionary_entry(font, b"FontDescriptor", objects) {
        return Some(descriptor);
    }
    let descendants = font
        .get(b"DescendantFonts")
        .and_then(Object::as_array)
        .ok()?;
    let descendant = descendants.first()?;
    let descendant = dereference_dictionary(descendant, objects).ok()??;
    dictionary_entry(&descendant, b"FontDescriptor", objects)
}

fn dictionary_entry(
    dictionary: &Dictionary,
    key: &[u8],
    objects: &dyn PdfObjectView,
) -> Option<Dictionary> {
    dictionary
        .get(key)
        .ok()
        .and_then(|object| dereference_dictionary(object, objects).ok().flatten())
}

fn dereference_dictionary(
    object: &Object,
    objects: &dyn PdfObjectView,
) -> Result<Option<Dictionary>, PdfSourceObjectError> {
    Ok(resolve_object(object, objects)?.and_then(|object| object.as_dict().ok().cloned()))
}

fn resolve_object(
    object: &Object,
    objects: &dyn PdfObjectView,
) -> Result<Option<Object>, PdfSourceObjectError> {
    let mut current = object.clone();
    let mut visited = BTreeSet::new();
    for _ in 0..64 {
        match current {
            Object::Reference(id) => {
                if !visited.insert(id) {
                    return Ok(None);
                }
                current = objects.object(id)?;
            }
            object => return Ok(Some(object)),
        }
    }
    Ok(None)
}

fn collect_text_shows(
    objects: &dyn PdfObjectView,
    content_stream_ids: &[ObjectId],
    resources: &PdfResourceContext,
    page_number: u32,
) -> Result<CollectedTextShows, PageOperandMappingError> {
    let mut collection = TextShowCollection::default();
    let mut state = TextOperatorState::default();
    let mut active_content_entries = HashSet::new();
    for stream_id in content_stream_ids.iter().copied() {
        collect_page_content_entry(
            objects,
            stream_id,
            resources,
            page_number,
            &mut state,
            &mut collection,
            &mut active_content_entries,
            0,
        )?;
    }

    let shared_form_streams = collection
        .form_invocation_counts
        .iter()
        .filter_map(|(stream_id, count)| (*count > 1).then_some(*stream_id))
        .collect::<HashSet<_>>();
    for text_show in &mut collection.text_shows {
        let stream_id = (
            text_show.inspection.stream_object_number,
            text_show.inspection.stream_generation,
        );
        text_show.inspection.shared_content_stream =
            !text_show.inspection.form_invocation_path.is_empty()
                && shared_form_streams.contains(&stream_id);
    }

    Ok(CollectedTextShows {
        text_shows: collection.text_shows,
        font_resources: collection.font_resources.into_values().collect(),
        content_stream_count: collection.visited_streams.len(),
        form_xobject_invocation_count: collection.form_invocation_counts.values().sum(),
        unique_form_stream_count: collection.form_invocation_counts.len(),
        shared_form_stream_count: shared_form_streams.len(),
        fallback_reasons: collection.fallback_reasons,
        stream_decode_elapsed: collection.stream_decode_elapsed,
        font_inspection_elapsed: collection.font_inspection_elapsed,
        text_show_decode_elapsed: collection.text_show_decode_elapsed,
        stream_decode_cache_hits: collection.stream_decode_cache_hits,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_page_content_entry(
    objects: &dyn PdfObjectView,
    entry_id: ObjectId,
    resources: &PdfResourceContext,
    page_number: u32,
    state: &mut TextOperatorState,
    collection: &mut TextShowCollection,
    active_entries: &mut HashSet<ObjectId>,
    depth: usize,
) -> Result<(), PageOperandMappingError> {
    if depth >= 64 || !active_entries.insert(entry_id) {
        collection
            .fallback_reasons
            .insert("page-content-reference-cycle-or-depth-limit".to_string());
        return Ok(());
    }
    let object = objects
        .object(entry_id)
        .map_err(|error| PageOperandMappingError::StreamRead {
            object_number: entry_id.0,
            generation: entry_id.1,
            message: error.to_string(),
        })?;
    let result = match object {
        Object::Stream(_) => collect_stream_text_shows(
            objects,
            entry_id,
            resources,
            page_number,
            &[],
            state,
            collection,
        ),
        Object::Reference(next) => collect_page_content_entry(
            objects,
            next,
            resources,
            page_number,
            state,
            collection,
            active_entries,
            depth + 1,
        ),
        Object::Array(entries) => {
            for entry in entries {
                let Ok(next) = entry.as_reference() else {
                    collection
                        .fallback_reasons
                        .insert("direct-page-content-stream-unsupported".to_string());
                    continue;
                };
                collect_page_content_entry(
                    objects,
                    next,
                    resources,
                    page_number,
                    state,
                    collection,
                    active_entries,
                    depth + 1,
                )?;
            }
            Ok(())
        }
        _ => Err(PageOperandMappingError::StreamRead {
            object_number: entry_id.0,
            generation: entry_id.1,
            message: "page content entry is neither a stream nor an array".to_string(),
        }),
    };
    active_entries.remove(&entry_id);
    result
}

#[allow(clippy::too_many_arguments)]
fn collect_stream_text_shows(
    objects: &dyn PdfObjectView,
    stream_id: ObjectId,
    resources: &PdfResourceContext,
    page_number: u32,
    invocation_path: &[FormInvocationStep],
    state: &mut TextOperatorState,
    collection: &mut TextShowCollection,
) -> Result<(), PageOperandMappingError> {
    collection.visited_streams.insert(stream_id);
    let stream_decode_started = Instant::now();
    let stream_object =
        objects
            .object(stream_id)
            .map_err(|error| PageOperandMappingError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
    let stream =
        stream_object
            .as_stream()
            .map_err(|error| PageOperandMappingError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
    let content = if let Some(content) = collection.decoded_streams.get(&stream_id) {
        collection.stream_decode_cache_hits += 1;
        Arc::clone(content)
    } else {
        let source_content =
            stream
                .get_plain_content()
                .map_err(|error| PageOperandMappingError::StreamRead {
                    object_number: stream_id.0,
                    generation: stream_id.1,
                    message: error.to_string(),
                })?;
        let content = Arc::new(Content::decode(&source_content).map_err(|error| {
            PageOperandMappingError::ContentDecode {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            }
        })?);
        collection
            .decoded_streams
            .insert(stream_id, Arc::clone(&content));
        collection.stream_decode_elapsed += stream_decode_started.elapsed();
        content
    };
    let scope_id = if invocation_path.is_empty() {
        "page".to_string()
    } else {
        format!("form-{}-{}", stream_id.0, stream_id.1)
    };
    let mut local_fonts = None;

    for (operation_index, operation) in content.operations.iter().enumerate() {
        match operation.operator.as_str() {
            "q" => state.saved_states.push((
                state.current_font.clone(),
                state.current_font_size,
                state.horizontal_scaling,
            )),
            "Q" => {
                if let Some((font, font_size, horizontal_scaling)) = state.saved_states.pop() {
                    state.current_font = font;
                    state.current_font_size = font_size;
                    state.horizontal_scaling = horizontal_scaling;
                }
            }
            "BT" => state.inside_text_object = true,
            "ET" => state.inside_text_object = false,
            "Tf" => {
                state.current_font = operation
                    .operands
                    .first()
                    .and_then(|operand| operand.as_name().ok())
                    .map(ToOwned::to_owned);
                state.current_font_size = operation.operands.get(1).and_then(numeric_operand_f32);
            }
            "Tz" => {
                if let Some(horizontal_scaling) =
                    operation.operands.first().and_then(numeric_operand_f32)
                {
                    state.horizontal_scaling = horizontal_scaling;
                }
            }
            _ => {}
        }

        if matches!(operation.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
            if local_fonts.is_none() {
                let font_inspection_started = Instant::now();
                let inspected = inspect_font_resources(objects, resources, &scope_id)?;
                collection.font_inspection_elapsed += font_inspection_started.elapsed();
                for font in &inspected.0 {
                    collection
                        .font_resources
                        .entry(font.resource_name.clone())
                        .or_insert_with(|| font.clone());
                }
                local_fonts = Some(inspected);
            }
            let (fonts, decoders) = local_fonts.as_ref().ok_or_else(|| {
                PageOperandMappingError::FontRead(
                    "font inspection was not initialized for a text-show operation".to_string(),
                )
            })?;
            let text_show_decode_started = Instant::now();
            let operands = text_operands(operation, page_number, stream_id, operation_index);
            let font_decoder = state.current_font.as_deref().and_then(|font| {
                decoders
                    .iter()
                    .find(|decoder| decoder.resource_name == font)
            });
            let unqualified_font_resource = state
                .current_font
                .as_deref()
                .map(|font| String::from_utf8_lossy(font).into_owned());
            let font_resource = unqualified_font_resource
                .as_deref()
                .map(|font| format!("{scope_id}/{font}"));
            let font_base_name = font_resource.as_deref().and_then(|resource| {
                fonts
                    .iter()
                    .find(|font| font.resource_name == resource)
                    .and_then(|font| font.base_font.clone())
            });
            let (decode_status, decoded_text, source_round_trip_exact, decoded_units) =
                decode_text_operands(&operands, state.current_font.is_some(), font_decoder);
            let encoded_byte_count = operands.iter().map(|operand| operand.encoded.len()).sum();
            let encoded_byte_hash = operand_hash(&operands);
            let decoded_char_count = decoded_text.as_ref().map(|text| text.chars().count());
            let decoded_text_hash = decoded_text.as_ref().map(|text| text_hash(text));
            collection.text_show_decode_elapsed += text_show_decode_started.elapsed();
            collection.text_shows.push(RawTextShow {
                inspection: TextShowInspection {
                    text_show_id: invoked_text_show_id(
                        page_number,
                        stream_id,
                        operation_index,
                        invocation_path,
                    ),
                    text_show_index: collection.text_shows.len(),
                    stream_object_number: stream_id.0,
                    stream_generation: stream_id.1,
                    operation_index,
                    operator: operation.operator.clone(),
                    font_resource,
                    font_base_name,
                    inside_text_object: state.inside_text_object,
                    operand_count: operands.len(),
                    encoded_byte_count,
                    encoded_byte_hash,
                    decode_status,
                    decoded_char_count,
                    decoded_text_hash,
                    source_round_trip_exact,
                    source_font_resource: unqualified_font_resource,
                    source_font_size: state.current_font_size,
                    source_horizontal_scaling: state.horizontal_scaling,
                    form_invocation_path: invocation_path.to_vec(),
                    shared_content_stream: false,
                },
                decoded_text,
                decoded_units,
            });
        }

        if operation.operator != "Do" {
            continue;
        }
        let Some(resource_name) = operation
            .operands
            .first()
            .and_then(|operand| operand.as_name().ok())
        else {
            collection
                .fallback_reasons
                .insert("form-xobject-name-missing".to_string());
            continue;
        };
        let Some(resolved_form) = resources.resolve_xobject(objects, resource_name)? else {
            continue;
        };
        let form_stream_id = resolved_form.object_id();
        let form_stream = resolved_form.stream();
        if !form_stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Form")
        {
            continue;
        }
        let Some(form_stream_id) = form_stream_id else {
            collection
                .fallback_reasons
                .insert("direct-form-xobject-stream-unsupported".to_string());
            continue;
        };
        *collection
            .form_invocation_counts
            .entry(form_stream_id)
            .or_default() += 1;
        if !collection.active_form_streams.insert(form_stream_id) {
            collection
                .fallback_reasons
                .insert("form-xobject-reference-cycle".to_string());
            continue;
        }
        if invocation_path.len() >= MAX_FORM_XOBJECT_DEPTH {
            collection.active_form_streams.remove(&form_stream_id);
            collection
                .fallback_reasons
                .insert("form-xobject-depth-limit".to_string());
            continue;
        }
        let mut child_path = invocation_path.to_vec();
        child_path.push(FormInvocationStep {
            parent_stream_object_number: stream_id.0,
            parent_stream_generation: stream_id.1,
            operation_index,
            form_stream_object_number: form_stream_id.0,
            form_stream_generation: form_stream_id.1,
        });
        let child_resources = resources.invoked_form(objects, form_stream_id, form_stream)?;
        let mut child_state = TextOperatorState {
            current_font: state.current_font.clone(),
            current_font_size: state.current_font_size,
            horizontal_scaling: state.horizontal_scaling,
            ..TextOperatorState::default()
        };
        let result = collect_stream_text_shows(
            objects,
            form_stream_id,
            &child_resources,
            page_number,
            &child_path,
            &mut child_state,
            collection,
        );
        collection.active_form_streams.remove(&form_stream_id);
        result?;
    }
    Ok(())
}

fn invoked_text_show_id(
    page_number: u32,
    stream_id: ObjectId,
    operation_index: usize,
    invocation_path: &[FormInvocationStep],
) -> String {
    let base = text_show_id(page_number, stream_id, operation_index);
    if invocation_path.is_empty() {
        return base;
    }
    let mut hasher = Sha256::new();
    for step in invocation_path {
        hasher.update(step.parent_stream_object_number.to_le_bytes());
        hasher.update(step.parent_stream_generation.to_le_bytes());
        hasher.update((step.operation_index as u64).to_le_bytes());
        hasher.update(step.form_stream_object_number.to_le_bytes());
        hasher.update(step.form_stream_generation.to_le_bytes());
    }
    let invocation_hash = hasher.finalize();
    let invocation_hash = hex_digest(&invocation_hash[..8]);
    format!("{base}-inv-{invocation_hash}")
}

fn text_operands<'a>(
    operation: &'a lopdf::content::Operation,
    page_number: u32,
    stream_id: ObjectId,
    operation_index: usize,
) -> Vec<RawTextOperand<'a>> {
    let make_operand = |operand_index, array_index, encoded| RawTextOperand {
        operand_id: operand_id(
            page_number,
            stream_id,
            operation_index,
            operand_index,
            array_index,
        ),
        operand_index,
        array_index,
        encoded,
    };
    match operation.operator.as_str() {
        "Tj" | "'" => operation
            .operands
            .first()
            .and_then(|operand| operand.as_str().ok())
            .map(|bytes| vec![make_operand(0, None, bytes)])
            .unwrap_or_default(),
        "\"" => operation
            .operands
            .get(2)
            .and_then(|operand| operand.as_str().ok())
            .map(|bytes| vec![make_operand(2, None, bytes)])
            .unwrap_or_default(),
        "TJ" => operation
            .operands
            .first()
            .and_then(|operand| operand.as_array().ok())
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(array_index, item)| {
                        item.as_str()
                            .ok()
                            .map(|bytes| make_operand(0, Some(array_index), bytes))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn numeric_operand_f32(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(value) => Some(*value as f32),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn decode_text_operands(
    operands: &[RawTextOperand<'_>],
    has_font_resource: bool,
    decoder: Option<&FontDecoder>,
) -> (
    TextShowDecodeStatus,
    Option<String>,
    Option<bool>,
    Vec<DecodedTextUnit>,
) {
    if operands.is_empty() {
        return (
            TextShowDecodeStatus::NoStringOperands,
            None,
            None,
            Vec::new(),
        );
    }
    let Some(decoder) = decoder else {
        return (
            if has_font_resource {
                TextShowDecodeStatus::FontDecoderUnavailable
            } else {
                TextShowDecodeStatus::FontResourceMissing
            },
            None,
            None,
            Vec::new(),
        );
    };
    let mut decoded = String::new();
    let mut decoded_units = Vec::new();
    let mut round_trip_exact = decoder.source_reencode_candidate.then_some(true);
    for operand in operands {
        let Ok(units) = decoder.decoder.decode_units(operand.encoded) else {
            return (TextShowDecodeStatus::DecodeFailed, None, None, Vec::new());
        };
        let value = units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<String>();
        if let Some(exact) = round_trip_exact.as_mut() {
            *exact &= decoder.decoder.encode(&value).as_deref() == Some(operand.encoded);
        }
        decoded.push_str(&value);
        decoded_units.extend(units.into_iter().map(|unit| DecodedTextUnit {
            operand_id: operand.operand_id.clone(),
            operand_index: operand.operand_index,
            array_index: operand.array_index,
            encoded_start: unit.encoded_start,
            encoded_len: unit.encoded_len,
            text: unit.text,
        }));
    }
    (
        TextShowDecodeStatus::Decoded,
        Some(decoded),
        round_trip_exact,
        decoded_units,
    )
}

fn normalize_font_name(value: &str) -> String {
    let value = value.trim_start_matches('/');
    let value = value
        .split_once('+')
        .filter(|(prefix, _)| {
            prefix.len() == 6 && prefix.bytes().all(|byte| byte.is_ascii_uppercase())
        })
        .map(|(_, name)| name)
        .unwrap_or(value);
    value.to_ascii_lowercase()
}

fn whitespace_char_count(value: &str) -> usize {
    value
        .chars()
        .filter(|character| character.is_whitespace())
        .count()
}

fn text_without_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn first_non_whitespace_difference(left: &str, right: &str) -> (Option<u32>, Option<u32>) {
    let mut left = left.chars().filter(|character| !character.is_whitespace());
    let mut right = right.chars().filter(|character| !character.is_whitespace());
    loop {
        let left_character = left.next();
        let right_character = right.next();
        if left_character != right_character {
            return (
                left_character.map(u32::from),
                right_character.map(u32::from),
            );
        }
        if left_character.is_none() {
            return (None, None);
        }
    }
}

fn elapsed_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn operand_hash(operands: &[RawTextOperand<'_>]) -> String {
    let mut hasher = Sha256::new();
    for operand in operands {
        hasher.update((operand.encoded.len() as u64).to_le_bytes());
        hasher.update(operand.encoded);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{map_page_atoms_to_content_operands, OperandMappingStatus};
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    #[test]
    fn simple_page_maps_every_pdfium_text_object_exactly() {
        let _guard = pdfium_test_lock();
        let result = map_page_atoms_to_content_operands(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
        )
        .expect("simple operand mapping");

        assert!(result.ordinal_alignment_valid);
        assert!(result.pdfium_text_object_count > 0);
        assert_eq!(result.pdfium_text_object_count, result.text_show_count);
        assert_eq!(
            result.exact_mapping_count, result.paired_mapping_count,
            "fonts={:#?}\nmappings={:#?}",
            result.font_resources, result.mappings
        );
        assert!(result
            .mappings
            .iter()
            .all(|mapping| mapping.status == OperandMappingStatus::Exact));
        assert!(result
            .font_resources
            .iter()
            .all(|font| !font.translated_text_reuse_allowed));
    }

    #[test]
    fn repeated_mapping_keeps_source_and_show_ids_stable() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let first =
            map_page_atoms_to_content_operands(shared_pdfium(), &source, 1).expect("first mapping");
        let second = map_page_atoms_to_content_operands(shared_pdfium(), &source, 1)
            .expect("second mapping");
        let first_ids = first
            .mappings
            .iter()
            .map(|mapping| {
                (
                    &mapping.source_object_id,
                    &mapping.text_show_id,
                    mapping.status,
                )
            })
            .collect::<Vec<_>>();
        let second_ids = second
            .mappings
            .iter()
            .map(|mapping| {
                (
                    &mapping.source_object_id,
                    &mapping.text_show_id,
                    mapping.status,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(first_ids, second_ids);
    }

    #[test]
    fn real_page_recurses_form_invocations_with_stable_provenance() {
        let _guard = pdfium_test_lock();
        let result = map_page_atoms_to_content_operands(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            1,
        )
        .expect("recursive form mapping");

        assert!(result.ordinal_alignment_valid);
        assert_eq!(result.pdfium_text_object_count, 258);
        assert_eq!(result.text_show_count, 258);
        assert_eq!(result.pdfium_form_object_count, 27);
        assert_eq!(result.form_xobject_invocation_count, 27);
        assert_eq!(result.unique_form_stream_count, 5);
        assert_eq!(result.shared_form_stream_count, 4);
        assert_eq!(result.content_stream_count, 7);
        assert!(result.timing.stream_decode_cache_hits > 0);
        let form_text_shows = result
            .text_shows
            .iter()
            .filter(|show| !show.form_invocation_path.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(form_text_shows.len(), 16);
        assert!(form_text_shows
            .iter()
            .all(|show| !show.shared_content_stream));
        assert_eq!(
            form_text_shows
                .iter()
                .map(|show| &show.text_show_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            form_text_shows.len()
        );
        assert_eq!(
            result
                .mappings
                .iter()
                .filter(|mapping| mapping.status == OperandMappingStatus::DecodeUnavailable)
                .count(),
            16
        );
        assert!(!result
            .fallback_reasons
            .iter()
            .any(|reason| reason == "form-xobject-requires-recursive-mapping"));
    }

    #[test]
    fn serialized_mapping_contains_no_text_payloads() {
        let _guard = pdfium_test_lock();
        let result = map_page_atoms_to_content_operands(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
        )
        .expect("mapping");
        let json = serde_json::to_string(&result).expect("serialize mapping");

        assert!(!json.contains("Lorem ipsum"));
        assert!(!json.contains("decodedText\""));
        assert!(!json.contains("objectText\""));
        assert!(json.contains("decodedTextHash"));
        assert!(json.contains("objectTextHash"));
        assert!(!json.contains("\"timing\""));
        assert!(!json.contains("streamDecodeCacheHits"));
    }

    #[test]
    fn fixture_corpus_mapping_is_conservative_and_nonempty() {
        let _guard = pdfium_test_lock();
        for fixture in [
            "simple-one-page.pdf",
            "pdflatex-image.pdf",
            "multicolumn.pdf",
            "google-doc-document.pdf",
            "GeoTopo.pdf",
        ] {
            let result =
                map_page_atoms_to_content_operands(shared_pdfium(), &fixture_path(fixture), 1)
                    .unwrap_or_else(|error| panic!("mapping failed for {fixture}: {error}"));

            assert!(result.exact_mapping_count <= result.paired_mapping_count);
            assert!(result.paired_mapping_count <= result.pdfium_text_object_count);
            assert!(result.paired_mapping_count <= result.text_show_count);
            assert!(result
                .font_resources
                .iter()
                .all(|font| !font.translated_text_reuse_allowed));
            println!(
                "pdf-v3 fixture mapping fixture={fixture} objects={} shows={} paired={} exact={} whitespace_equivalent={} fallbacks={:?}",
                result.pdfium_text_object_count,
                result.text_show_count,
                result.paired_mapping_count,
                result.exact_mapping_count,
                result.whitespace_equivalent_mapping_count,
                result.fallback_reasons
            );
        }
    }

    #[test]
    #[ignore = "manual Windows real-page PageGraph-to-operand mapping probe"]
    fn manual_windows_real_page_mapping_probe() {
        let _guard = pdfium_test_lock();
        let result = map_page_atoms_to_content_operands(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            1,
        )
        .expect("real-page mapping");
        let mut status_counts = std::collections::BTreeMap::new();
        for mapping in &result.mappings {
            *status_counts.entry(mapping.status).or_insert(0usize) += 1;
        }
        let mut codepoint_differences = std::collections::BTreeMap::new();
        for mapping in result.mappings.iter().filter(|mapping| {
            mapping.status == OperandMappingStatus::DecodedTextMismatch
                && !mapping.text_match_ignoring_whitespace
        }) {
            *codepoint_differences
                .entry((
                    mapping.first_non_whitespace_object_codepoint,
                    mapping.first_non_whitespace_decoded_codepoint,
                ))
                .or_insert(0usize) += 1;
        }

        println!(
            "pdf-v3 mapping page={} objects={} text_objects={} forms={} streams={} shows={} ordinal={} paired={} exact={} whitespace_equivalent={} unmatched_objects={} unmatched_shows={} fonts={} fallbacks={:?} statuses={:?} elapsed={}ms",
            result.page_number,
            result.pdfium_page_object_count,
            result.pdfium_text_object_count,
            result.pdfium_form_object_count,
            result.content_stream_count,
            result.text_show_count,
            result.ordinal_alignment_valid,
            result.paired_mapping_count,
            result.exact_mapping_count,
            result.whitespace_equivalent_mapping_count,
            result.unmatched_text_object_count,
            result.unmatched_text_show_count,
            result.font_resources.len(),
            result.fallback_reasons,
            status_counts,
            result.elapsed_ms
        );
        println!("pdf-v3 mapping fonts={:?}", result.font_resources);
        println!("pdf-v3 mapping codepoint_differences={codepoint_differences:?}");
    }
}
