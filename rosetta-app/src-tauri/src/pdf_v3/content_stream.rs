use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt,
    path::Path,
    time::{Duration, Instant},
};

use lopdf::{
    content::{Content, Operation},
    Dictionary, Document, Object, ObjectId, Stream, StringFormat,
};
use pdfium_render::prelude::Pdfium;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::identity::{
    compare_images, first_text_difference_index, render_page, text_hash, IdentityProbeError,
};

const MAX_FORM_XOBJECT_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ContentStreamProbeMode {
    SaveOnly,
    RewriteTextOperands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PdfStringEncoding {
    Literal,
    Hexadecimal,
}

impl From<StringFormat> for PdfStringEncoding {
    fn from(value: StringFormat) -> Self {
        match value {
            StringFormat::Literal => Self::Literal,
            StringFormat::Hexadecimal => Self::Hexadecimal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextOperandProvenance {
    pub operand_id: String,
    pub text_show_id: String,
    pub text_show_index: usize,
    pub page_number: u32,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub operator: String,
    pub operand_index: usize,
    pub array_index: Option<usize>,
    pub font_resource: Option<String>,
    pub inside_text_object: bool,
    pub string_encoding: PdfStringEncoding,
    pub encoded_byte_count: usize,
    pub encoded_byte_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContentStreamInspection {
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub referencing_pages: Vec<u32>,
    pub shared_across_pages: bool,
    pub is_form_xobject: bool,
    pub form_invocation_count: usize,
    pub shared_across_form_invocations: bool,
    pub form_invocation_paths: Vec<String>,
    pub source_decompressed_bytes: usize,
    pub canonical_bytes: usize,
    pub source_stream_hash: String,
    pub canonical_stream_hash: String,
    pub operation_count: usize,
    pub text_show_operator_count: usize,
    pub malformed_text_show_operator_count: usize,
    pub text_operand_count: usize,
    pub rewritten_text_operand_count: usize,
    pub text_operands: Vec<TextOperandProvenance>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContentStreamProbeResult {
    pub mode: ContentStreamProbeMode,
    pub page_number: u32,
    pub source_page_count: u32,
    pub output_page_count: u32,
    pub page_count_exact_match: bool,
    pub source_text_chars: usize,
    pub output_text_chars: usize,
    pub source_text_hash: String,
    pub output_text_hash: String,
    pub first_text_difference_index: Option<usize>,
    pub text_exact_match: bool,
    pub content_stream_count: usize,
    pub shared_content_stream_count: usize,
    pub form_xobject_invocation_count: usize,
    pub unique_form_stream_count: usize,
    pub shared_form_stream_count: usize,
    pub operation_count: usize,
    pub text_show_operator_count: usize,
    pub malformed_text_show_operator_count: usize,
    pub text_operand_count: usize,
    pub rewritten_text_operand_count: usize,
    pub output_bytes: usize,
    pub output_size_ratio: f64,
    pub changed_pixel_count: u64,
    pub changed_pixel_ratio: f64,
    pub mean_absolute_channel_difference: f64,
    pub max_channel_difference: u8,
    pub parse_and_rewrite_ms: u64,
    pub total_ms: u64,
    pub fallback_reasons: Vec<String>,
    pub streams: Vec<ContentStreamInspection>,
}

#[derive(Debug)]
pub(crate) enum ContentStreamProbeError {
    Load(String),
    EncryptedDocument,
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    MissingContentStream {
        object_number: u32,
        generation: u16,
    },
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
    ContentEncode {
        object_number: u32,
        generation: u16,
        message: String,
    },
    StreamWrite {
        object_number: u32,
        generation: u16,
        message: String,
    },
    Save(String),
    Identity(IdentityProbeError),
}

impl fmt::Display for ContentStreamProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(message) | Self::Save(message) => formatter.write_str(message),
            Self::EncryptedDocument => {
                formatter.write_str("encrypted PDFs are not supported by the operator probe")
            }
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::MissingContentStream {
                object_number,
                generation,
            } => write!(
                formatter,
                "PDF content stream {object_number} {generation} was not found"
            ),
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
            Self::ContentEncode {
                object_number,
                generation,
                message,
            } => write!(
                formatter,
                "failed to encode PDF content stream {object_number} {generation}: {message}"
            ),
            Self::StreamWrite {
                object_number,
                generation,
                message,
            } => write!(
                formatter,
                "failed to rewrite PDF content stream {object_number} {generation}: {message}"
            ),
            Self::Identity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ContentStreamProbeError {}

impl From<IdentityProbeError> for ContentStreamProbeError {
    fn from(value: IdentityProbeError) -> Self {
        Self::Identity(value)
    }
}

#[derive(Debug, Default)]
struct OperatorState {
    inside_text_object: bool,
    font_resource: Option<String>,
    saved_fonts: Vec<Option<String>>,
}

#[derive(Clone)]
struct ResourceContext<'a> {
    dictionaries: Vec<&'a Dictionary>,
}

#[derive(Debug, Clone)]
struct FormInvocationStep {
    parent_stream_id: ObjectId,
    operation_index: usize,
    form_stream_id: ObjectId,
}

#[derive(Default)]
pub(crate) struct DiscoveredStream {
    pub(crate) is_form_xobject: bool,
    pub(crate) form_invocation_paths: BTreeSet<String>,
}

#[derive(Default)]
pub(crate) struct StreamDiscovery {
    pub(crate) order: Vec<ObjectId>,
    pub(crate) streams: BTreeMap<ObjectId, DiscoveredStream>,
    pub(crate) form_xobject_invocation_count: usize,
    pub(crate) fallback_reasons: BTreeSet<String>,
}

struct PageStreamInspection {
    streams: Vec<ContentStreamInspection>,
    form_xobject_invocation_count: usize,
    unique_form_stream_count: usize,
    shared_form_stream_count: usize,
    fallback_reasons: Vec<String>,
}

pub(crate) fn probe_content_stream_save_only(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
) -> Result<ContentStreamProbeResult, ContentStreamProbeError> {
    probe_content_stream_identity(
        pdfium,
        source_path,
        page_number,
        target_width,
        ContentStreamProbeMode::SaveOnly,
    )
}

pub(crate) fn probe_content_stream_text_operand_identity(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
) -> Result<ContentStreamProbeResult, ContentStreamProbeError> {
    probe_content_stream_identity(
        pdfium,
        source_path,
        page_number,
        target_width,
        ContentStreamProbeMode::RewriteTextOperands,
    )
}

fn probe_content_stream_identity(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
    target_width: u32,
    mode: ContentStreamProbeMode,
) -> Result<ContentStreamProbeResult, ContentStreamProbeError> {
    let started = Instant::now();
    let source_bytes = std::fs::read(source_path)
        .map_err(|error| ContentStreamProbeError::Load(format!("failed to read PDF: {error}")))?;
    let mut document = Document::load_mem(&source_bytes)
        .map_err(|error| ContentStreamProbeError::Load(format!("failed to load PDF: {error}")))?;
    if document.is_encrypted() {
        return Err(ContentStreamProbeError::EncryptedDocument);
    }

    let pages = document.get_pages();
    let source_page_count = pages.len() as u32;
    let page_id =
        pages
            .get(&page_number)
            .copied()
            .ok_or(ContentStreamProbeError::PageOutOfBounds {
                page: page_number,
                page_count: source_page_count,
            })?;
    let page_references = content_stream_page_references(&document);
    let rewrite_started = Instant::now();
    let inspection = inspect_page_streams(
        &mut document,
        page_id,
        page_number,
        &page_references,
        mode == ContentStreamProbeMode::RewriteTextOperands,
    )?;
    let streams = inspection.streams;
    let parse_and_rewrite_ms = elapsed_millis(rewrite_started.elapsed());

    let mut output_pdf = Vec::new();
    document.save_to(&mut output_pdf).map_err(|error| {
        ContentStreamProbeError::Save(format!("failed to save operator probe PDF: {error}"))
    })?;

    let source_document = pdfium
        .load_pdf_from_byte_slice(&source_bytes, None)
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to load source in PDFium: {error}"))
        })?;
    let source_page = source_document
        .pages()
        .get(page_number as i32 - 1)
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to read source page: {error}"))
        })?;
    let source_text = source_page
        .text()
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to read source text: {error}"))
        })?
        .all();
    let source_image = render_page(&source_page, page_number, target_width)?;

    let output_document = pdfium
        .load_pdf_from_byte_slice(&output_pdf, None)
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to load output in PDFium: {error}"))
        })?;
    let output_page_count = output_document.pages().len() as u32;
    let output_page = output_document
        .pages()
        .get(page_number as i32 - 1)
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to read output page: {error}"))
        })?;
    let output_text = output_page
        .text()
        .map_err(|error| {
            ContentStreamProbeError::Load(format!("failed to read output text: {error}"))
        })?
        .all();
    let output_image = render_page(&output_page, page_number, target_width)?;
    let difference = compare_images(&source_image, &output_image)?;

    Ok(ContentStreamProbeResult {
        mode,
        page_number,
        source_page_count,
        output_page_count,
        page_count_exact_match: source_page_count == output_page_count,
        source_text_chars: source_text.chars().count(),
        output_text_chars: output_text.chars().count(),
        source_text_hash: text_hash(&source_text),
        output_text_hash: text_hash(&output_text),
        first_text_difference_index: first_text_difference_index(&source_text, &output_text),
        text_exact_match: source_text == output_text,
        content_stream_count: streams.len(),
        shared_content_stream_count: streams
            .iter()
            .filter(|stream| stream.shared_across_pages || stream.shared_across_form_invocations)
            .count(),
        form_xobject_invocation_count: inspection.form_xobject_invocation_count,
        unique_form_stream_count: inspection.unique_form_stream_count,
        shared_form_stream_count: inspection.shared_form_stream_count,
        operation_count: streams.iter().map(|stream| stream.operation_count).sum(),
        text_show_operator_count: streams
            .iter()
            .map(|stream| stream.text_show_operator_count)
            .sum(),
        malformed_text_show_operator_count: streams
            .iter()
            .map(|stream| stream.malformed_text_show_operator_count)
            .sum(),
        text_operand_count: streams.iter().map(|stream| stream.text_operand_count).sum(),
        rewritten_text_operand_count: streams
            .iter()
            .map(|stream| stream.rewritten_text_operand_count)
            .sum(),
        output_bytes: output_pdf.len(),
        output_size_ratio: if source_bytes.is_empty() {
            0.0
        } else {
            output_pdf.len() as f64 / source_bytes.len() as f64
        },
        changed_pixel_count: difference.changed_pixel_count,
        changed_pixel_ratio: difference.changed_pixel_ratio,
        mean_absolute_channel_difference: difference.mean_absolute_channel_difference,
        max_channel_difference: difference.max_channel_difference,
        parse_and_rewrite_ms,
        total_ms: elapsed_millis(started.elapsed()),
        fallback_reasons: inspection.fallback_reasons,
        streams,
    })
}

fn content_stream_page_references(document: &Document) -> BTreeMap<ObjectId, Vec<u32>> {
    let mut references = BTreeMap::<ObjectId, Vec<u32>>::new();
    for (page_number, page_id) in document.get_pages() {
        for stream_id in document.get_page_contents(page_id) {
            references.entry(stream_id).or_default().push(page_number);
        }
    }
    references
}

fn inspect_page_streams(
    document: &mut Document,
    page_id: ObjectId,
    page_number: u32,
    page_references: &BTreeMap<ObjectId, Vec<u32>>,
    rewrite: bool,
) -> Result<PageStreamInspection, ContentStreamProbeError> {
    let discovery = discover_page_streams(document, page_id, page_number)?;
    let mut top_level_state = OperatorState::default();
    let mut next_text_show_index = 0usize;
    let mut inspections = Vec::with_capacity(discovery.order.len());
    for stream_id in &discovery.order {
        let stream_id = *stream_id;
        let discovered = discovery.streams.get(&stream_id).ok_or(
            ContentStreamProbeError::MissingContentStream {
                object_number: stream_id.0,
                generation: stream_id.1,
            },
        )?;
        let source_content = document
            .get_object(stream_id)
            .map_err(|_| ContentStreamProbeError::MissingContentStream {
                object_number: stream_id.0,
                generation: stream_id.1,
            })?
            .as_stream()
            .map_err(|error| ContentStreamProbeError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?
            .get_plain_content()
            .map_err(|error| ContentStreamProbeError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
        let mut content = Content::decode(&source_content).map_err(|error| {
            ContentStreamProbeError::ContentDecode {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            }
        })?;
        let mut form_state = OperatorState::default();
        let state = if discovered.is_form_xobject {
            &mut form_state
        } else {
            &mut top_level_state
        };
        let referencing_pages = if discovered.is_form_xobject {
            vec![page_number]
        } else {
            page_references.get(&stream_id).cloned().unwrap_or_default()
        };
        let inspection = inspect_operations(
            &mut content.operations,
            stream_id,
            page_number,
            referencing_pages,
            discovered.is_form_xobject,
            discovered.form_invocation_paths.iter().cloned().collect(),
            source_content,
            state,
            &mut next_text_show_index,
            rewrite,
        )?;
        if rewrite {
            let rewritten =
                content
                    .encode()
                    .map_err(|error| ContentStreamProbeError::ContentEncode {
                        object_number: stream_id.0,
                        generation: stream_id.1,
                        message: error.to_string(),
                    })?;
            let stream = document
                .get_object_mut(stream_id)
                .and_then(Object::as_stream_mut)
                .map_err(|error| ContentStreamProbeError::StreamWrite {
                    object_number: stream_id.0,
                    generation: stream_id.1,
                    message: error.to_string(),
                })?;
            stream.set_plain_content(rewritten);
            stream
                .compress()
                .map_err(|error| ContentStreamProbeError::StreamWrite {
                    object_number: stream_id.0,
                    generation: stream_id.1,
                    message: error.to_string(),
                })?;
        }
        inspections.push(inspection);
    }
    let unique_form_stream_count = discovery
        .streams
        .values()
        .filter(|stream| stream.is_form_xobject)
        .count();
    let shared_form_stream_count = discovery
        .streams
        .values()
        .filter(|stream| stream.is_form_xobject && stream.form_invocation_paths.len() > 1)
        .count();
    Ok(PageStreamInspection {
        streams: inspections,
        form_xobject_invocation_count: discovery.form_xobject_invocation_count,
        unique_form_stream_count,
        shared_form_stream_count,
        fallback_reasons: discovery.fallback_reasons.into_iter().collect(),
    })
}

pub(crate) fn discover_page_streams(
    document: &Document,
    page_id: ObjectId,
    page_number: u32,
) -> Result<StreamDiscovery, ContentStreamProbeError> {
    let resources = page_resource_context(document, page_id)?;
    let mut discovery = StreamDiscovery::default();
    let mut active_form_streams = HashSet::new();
    for stream_id in document.get_page_contents(page_id) {
        record_discovered_stream(&mut discovery, stream_id, false, None);
        discover_form_streams(
            document,
            stream_id,
            &resources,
            page_number,
            &[],
            &mut active_form_streams,
            &mut discovery,
        )?;
    }
    Ok(discovery)
}

#[allow(clippy::too_many_arguments)]
fn discover_form_streams<'a>(
    document: &'a Document,
    stream_id: ObjectId,
    resources: &ResourceContext<'a>,
    page_number: u32,
    invocation_path: &[FormInvocationStep],
    active_form_streams: &mut HashSet<ObjectId>,
    discovery: &mut StreamDiscovery,
) -> Result<(), ContentStreamProbeError> {
    let stream = document
        .get_object(stream_id)
        .and_then(Object::as_stream)
        .map_err(|error| ContentStreamProbeError::StreamRead {
            object_number: stream_id.0,
            generation: stream_id.1,
            message: error.to_string(),
        })?;
    let source_content =
        stream
            .get_plain_content()
            .map_err(|error| ContentStreamProbeError::StreamRead {
                object_number: stream_id.0,
                generation: stream_id.1,
                message: error.to_string(),
            })?;
    let content = Content::decode(&source_content).map_err(|error| {
        ContentStreamProbeError::ContentDecode {
            object_number: stream_id.0,
            generation: stream_id.1,
            message: error.to_string(),
        }
    })?;
    for (operation_index, operation) in content.operations.iter().enumerate() {
        if operation.operator != "Do" {
            continue;
        }
        let Some(resource_name) = operation
            .operands
            .first()
            .and_then(|operand| operand.as_name().ok())
        else {
            discovery
                .fallback_reasons
                .insert("form-xobject-name-missing".to_string());
            continue;
        };
        let Some((form_stream_id, form_stream)) =
            resolve_xobject(document, resources, resource_name)
        else {
            continue;
        };
        if !form_stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|subtype| subtype == b"Form")
        {
            continue;
        }
        discovery.form_xobject_invocation_count += 1;
        let Some(form_stream_id) = form_stream_id else {
            discovery
                .fallback_reasons
                .insert("direct-form-xobject-stream-unsupported".to_string());
            continue;
        };
        let mut child_path = invocation_path.to_vec();
        child_path.push(FormInvocationStep {
            parent_stream_id: stream_id,
            operation_index,
            form_stream_id,
        });
        record_discovered_stream(
            discovery,
            form_stream_id,
            true,
            Some(format_form_invocation_path(page_number, &child_path)),
        );
        if invocation_path.len() >= MAX_FORM_XOBJECT_DEPTH {
            discovery
                .fallback_reasons
                .insert("form-xobject-depth-limit".to_string());
            continue;
        }
        if !active_form_streams.insert(form_stream_id) {
            discovery
                .fallback_reasons
                .insert("form-xobject-reference-cycle".to_string());
            continue;
        }
        let child_resources = form_resource_context(document, form_stream, resources);
        let result = discover_form_streams(
            document,
            form_stream_id,
            &child_resources,
            page_number,
            &child_path,
            active_form_streams,
            discovery,
        );
        active_form_streams.remove(&form_stream_id);
        result?;
    }
    Ok(())
}

fn record_discovered_stream(
    discovery: &mut StreamDiscovery,
    stream_id: ObjectId,
    is_form_xobject: bool,
    invocation_path: Option<String>,
) {
    if !discovery.streams.contains_key(&stream_id) {
        discovery.order.push(stream_id);
    }
    let stream = discovery.streams.entry(stream_id).or_default();
    stream.is_form_xobject |= is_form_xobject;
    if let Some(invocation_path) = invocation_path {
        stream.form_invocation_paths.insert(invocation_path);
    }
}

fn page_resource_context<'a>(
    document: &'a Document,
    page_id: ObjectId,
) -> Result<ResourceContext<'a>, ContentStreamProbeError> {
    let (direct, resource_ids) = document.get_page_resources(page_id).map_err(|error| {
        ContentStreamProbeError::Load(format!("failed to inspect page resources: {error}"))
    })?;
    let mut dictionaries = Vec::new();
    if let Some(direct) = direct {
        dictionaries.push(direct);
    }
    for resource_id in resource_ids {
        if let Ok(dictionary) = document.get_dictionary(resource_id) {
            if !dictionaries
                .iter()
                .any(|existing| std::ptr::eq(*existing, dictionary))
            {
                dictionaries.push(dictionary);
            }
        }
    }
    Ok(ResourceContext { dictionaries })
}

fn form_resource_context<'a>(
    document: &'a Document,
    stream: &'a Stream,
    parent: &ResourceContext<'a>,
) -> ResourceContext<'a> {
    let mut dictionaries = Vec::new();
    if let Some(resources) = dictionary_entry(&stream.dict, b"Resources", document) {
        dictionaries.push(resources);
    }
    for parent in &parent.dictionaries {
        if !dictionaries
            .iter()
            .any(|existing| std::ptr::eq(*existing, *parent))
        {
            dictionaries.push(parent);
        }
    }
    ResourceContext { dictionaries }
}

fn resolve_xobject<'a>(
    document: &'a Document,
    resources: &ResourceContext<'a>,
    resource_name: &[u8],
) -> Option<(Option<ObjectId>, &'a Stream)> {
    for resources in &resources.dictionaries {
        let Some(xobjects) = dictionary_entry(resources, b"XObject", document) else {
            continue;
        };
        let Ok(object) = xobjects.get(resource_name) else {
            continue;
        };
        match object {
            Object::Reference(object_id) => {
                if let Ok(stream) = document.get_object(*object_id).and_then(Object::as_stream) {
                    return Some((Some(*object_id), stream));
                }
            }
            Object::Stream(stream) => return Some((None, stream)),
            _ => {}
        }
    }
    None
}

fn dictionary_entry<'a>(
    dictionary: &'a Dictionary,
    key: &[u8],
    document: &'a Document,
) -> Option<&'a Dictionary> {
    dictionary
        .get(key)
        .ok()
        .and_then(|object| dereference_dictionary(object, document))
}

fn dereference_dictionary<'a>(
    object: &'a Object,
    document: &'a Document,
) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Reference(id) => document.get_dictionary(*id).ok(),
        _ => None,
    }
}

fn format_form_invocation_path(page_number: u32, path: &[FormInvocationStep]) -> String {
    let steps = path
        .iter()
        .map(|step| {
            format!(
                "{}-{}:{}->{}-{}",
                step.parent_stream_id.0,
                step.parent_stream_id.1,
                step.operation_index,
                step.form_stream_id.0,
                step.form_stream_id.1
            )
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("page-{page_number:04}/{steps}")
}

fn inspect_operations(
    operations: &mut [Operation],
    stream_id: ObjectId,
    page_number: u32,
    referencing_pages: Vec<u32>,
    is_form_xobject: bool,
    form_invocation_paths: Vec<String>,
    source_content: Vec<u8>,
    state: &mut OperatorState,
    next_text_show_index: &mut usize,
    rewrite: bool,
) -> Result<ContentStreamInspection, ContentStreamProbeError> {
    let canonical_content = Content {
        operations: operations.to_vec(),
    }
    .encode()
    .map_err(|error| ContentStreamProbeError::ContentEncode {
        object_number: stream_id.0,
        generation: stream_id.1,
        message: error.to_string(),
    })?;
    let mut text_operands = Vec::new();
    let mut text_show_operator_count = 0usize;
    let mut malformed_text_show_operator_count = 0usize;
    let mut rewritten_text_operand_count = 0usize;

    for (operation_index, operation) in operations.iter_mut().enumerate() {
        update_state_before_operation(state, operation);
        let operator = operation.operator.clone();
        if is_text_show_operator(&operator) {
            text_show_operator_count += 1;
            let text_show_index = *next_text_show_index;
            *next_text_show_index += 1;
            let text_show_id = text_show_id(page_number, stream_id, operation_index);
            let before = text_operands.len();
            visit_text_operands_mut(operation, |operand_index, array_index, bytes, format| {
                let encoded_byte_hash = byte_hash(bytes);
                text_operands.push(TextOperandProvenance {
                    operand_id: operand_id(
                        page_number,
                        stream_id,
                        operation_index,
                        operand_index,
                        array_index,
                    ),
                    text_show_id: text_show_id.clone(),
                    text_show_index,
                    page_number,
                    stream_object_number: stream_id.0,
                    stream_generation: stream_id.1,
                    operation_index,
                    operator: operator.clone(),
                    operand_index,
                    array_index,
                    font_resource: state.font_resource.clone(),
                    inside_text_object: state.inside_text_object,
                    string_encoding: format.into(),
                    encoded_byte_count: bytes.len(),
                    encoded_byte_hash,
                });
                if rewrite {
                    *bytes = bytes.clone();
                    rewritten_text_operand_count += 1;
                }
            });
            if text_operands.len() == before {
                malformed_text_show_operator_count += 1;
            }
        }
        update_state_after_operation(state, operation);
    }

    Ok(ContentStreamInspection {
        stream_object_number: stream_id.0,
        stream_generation: stream_id.1,
        shared_across_pages: referencing_pages.len() > 1,
        is_form_xobject,
        form_invocation_count: form_invocation_paths.len(),
        shared_across_form_invocations: form_invocation_paths.len() > 1,
        form_invocation_paths,
        referencing_pages,
        source_decompressed_bytes: source_content.len(),
        canonical_bytes: canonical_content.len(),
        source_stream_hash: byte_hash(&source_content),
        canonical_stream_hash: byte_hash(&canonical_content),
        operation_count: operations.len(),
        text_show_operator_count,
        malformed_text_show_operator_count,
        text_operand_count: text_operands.len(),
        rewritten_text_operand_count,
        text_operands,
    })
}

fn update_state_before_operation(state: &mut OperatorState, operation: &Operation) {
    match operation.operator.as_str() {
        "q" => state.saved_fonts.push(state.font_resource.clone()),
        "Q" => state.font_resource = state.saved_fonts.pop().flatten(),
        "BT" => state.inside_text_object = true,
        "Tf" => {
            state.font_resource = operation.operands.first().and_then(|operand| {
                operand
                    .as_name()
                    .ok()
                    .map(|name| String::from_utf8_lossy(name).into_owned())
            });
        }
        _ => {}
    }
}

fn update_state_after_operation(state: &mut OperatorState, operation: &Operation) {
    if operation.operator == "ET" {
        state.inside_text_object = false;
    }
}

fn is_text_show_operator(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

fn visit_text_operands_mut(
    operation: &mut Operation,
    mut visitor: impl FnMut(usize, Option<usize>, &mut Vec<u8>, StringFormat),
) {
    match operation.operator.as_str() {
        "Tj" | "'" => visit_string_operand(&mut operation.operands, 0, &mut visitor),
        "\"" => visit_string_operand(&mut operation.operands, 2, &mut visitor),
        "TJ" => {
            let Some(Object::Array(items)) = operation.operands.first_mut() else {
                return;
            };
            for (array_index, item) in items.iter_mut().enumerate() {
                if let Object::String(bytes, format) = item {
                    visitor(0, Some(array_index), bytes, *format);
                }
            }
        }
        _ => {}
    }
}

fn visit_string_operand(
    operands: &mut [Object],
    operand_index: usize,
    visitor: &mut impl FnMut(usize, Option<usize>, &mut Vec<u8>, StringFormat),
) {
    let Some(Object::String(bytes, format)) = operands.get_mut(operand_index) else {
        return;
    };
    visitor(operand_index, None, bytes, *format);
}

pub(super) fn operand_id(
    page_number: u32,
    stream_id: ObjectId,
    operation_index: usize,
    operand_index: usize,
    array_index: Option<usize>,
) -> String {
    match array_index {
        Some(array_index) => format!(
            "page-{page_number:04}-stream-{:08}-{:05}-op-{operation_index:08}-arg-{operand_index:03}-item-{array_index:05}",
            stream_id.0, stream_id.1
        ),
        None => format!(
            "page-{page_number:04}-stream-{:08}-{:05}-op-{operation_index:08}-arg-{operand_index:03}",
            stream_id.0, stream_id.1
        ),
    }
}

pub(super) fn text_show_id(
    page_number: u32,
    stream_id: ObjectId,
    operation_index: usize,
) -> String {
    format!(
        "page-{page_number:04}-stream-{:08}-{:05}-op-{operation_index:08}",
        stream_id.0, stream_id.1
    )
}

fn byte_hash(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{
        probe_content_stream_save_only, probe_content_stream_text_operand_identity,
        ContentStreamProbeMode,
    };
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    #[test]
    fn simple_operator_identity_is_text_and_pixel_exact() {
        let _guard = pdfium_test_lock();
        let result = probe_content_stream_text_operand_identity(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
            900,
        )
        .expect("operator identity");

        assert_eq!(result.mode, ContentStreamProbeMode::RewriteTextOperands);
        assert!(result.text_operand_count > 0);
        assert_eq!(
            result.rewritten_text_operand_count,
            result.text_operand_count
        );
        assert_eq!(result.malformed_text_show_operator_count, 0);
        assert!(result.page_count_exact_match);
        assert!(result.text_exact_match);
        assert_eq!(result.changed_pixel_count, 0);
        assert_eq!(result.max_channel_difference, 0);
    }

    #[test]
    fn operator_provenance_is_stable_across_repeated_inspection() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let first = probe_content_stream_save_only(shared_pdfium(), &source, 1, 900)
            .expect("first inspection");
        let second = probe_content_stream_save_only(shared_pdfium(), &source, 1, 900)
            .expect("second inspection");
        let first_operands = first
            .streams
            .iter()
            .flat_map(|stream| &stream.text_operands)
            .collect::<Vec<_>>();
        let second_operands = second
            .streams
            .iter()
            .flat_map(|stream| &stream.text_operands)
            .collect::<Vec<_>>();

        assert_eq!(first_operands, second_operands);
    }

    #[test]
    fn real_page_rewrites_recursive_form_streams_once() {
        let _guard = pdfium_test_lock();
        let result = probe_content_stream_text_operand_identity(
            shared_pdfium(),
            &fixture_path("2305.13048v2.pdf"),
            1,
            900,
        )
        .expect("recursive form identity");

        assert_eq!(result.content_stream_count, 7);
        assert_eq!(result.form_xobject_invocation_count, 27);
        assert_eq!(result.unique_form_stream_count, 5);
        assert_eq!(result.shared_form_stream_count, 4);
        assert_eq!(result.shared_content_stream_count, 4);
        assert_eq!(result.text_show_operator_count, 258);
        assert_eq!(result.text_operand_count, 800);
        assert_eq!(result.rewritten_text_operand_count, 800);
        assert!(result.fallback_reasons.is_empty());
        assert_eq!(
            result
                .streams
                .iter()
                .filter(|stream| stream.is_form_xobject)
                .count(),
            5
        );
        assert_eq!(
            result
                .streams
                .iter()
                .filter(|stream| stream.is_form_xobject)
                .map(|stream| stream.form_invocation_count)
                .sum::<usize>(),
            27
        );
        assert!(result.text_exact_match);
        assert_eq!(result.changed_pixel_count, 0);
        assert_eq!(result.max_channel_difference, 0);
    }

    #[test]
    fn serialized_probe_contains_hashes_but_not_text_payloads() {
        let _guard = pdfium_test_lock();
        let result = probe_content_stream_save_only(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
            900,
        )
        .expect("save-only probe");
        let value = serde_json::to_value(result).expect("serialize probe");
        let object = value.as_object().expect("probe object");

        assert!(object.contains_key("sourceTextHash"));
        assert!(object.contains_key("outputTextHash"));
        assert!(!object.contains_key("sourceText"));
        assert!(!object.contains_key("outputText"));
    }

    #[test]
    fn first_page_fixture_corpus_is_text_and_pixel_exact() {
        let _guard = pdfium_test_lock();
        for fixture in [
            "simple-one-page.pdf",
            "pdflatex-image.pdf",
            "multicolumn.pdf",
            "google-doc-document.pdf",
            "GeoTopo.pdf",
        ] {
            let result = probe_content_stream_text_operand_identity(
                shared_pdfium(),
                &fixture_path(fixture),
                1,
                900,
            )
            .unwrap_or_else(|error| panic!("operator identity failed for {fixture}: {error}"));

            assert!(
                result.text_exact_match,
                "text identity failed for {fixture}"
            );
            assert!(
                result.page_count_exact_match,
                "page count identity failed for {fixture}"
            );
            assert_eq!(
                result.changed_pixel_count, 0,
                "pixel identity failed for {fixture}"
            );
            assert_eq!(
                result.max_channel_difference, 0,
                "channel identity failed for {fixture}"
            );
            assert_eq!(
                result.rewritten_text_operand_count, result.text_operand_count,
                "not every text operand was rewritten for {fixture}"
            );
        }
    }

    #[test]
    #[ignore = "manual Windows real-page content-stream identity probe"]
    fn manual_windows_real_page_operator_identity_probe() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let save_only = probe_content_stream_save_only(shared_pdfium(), &source, 1, 1200)
            .expect("save-only operator probe");
        let rewrite = probe_content_stream_text_operand_identity(shared_pdfium(), &source, 1, 1200)
            .expect("operator identity");

        println!(
            "pdf-v3 operator save-only text_match={} chars={}/{} pixels={} ratio={:.6} mean={:.6} max={} streams={} operations={} text_ops={} operands={} bytes={} size_ratio={:.6} parse={}ms total={}ms",
            save_only.text_exact_match,
            save_only.source_text_chars,
            save_only.output_text_chars,
            save_only.changed_pixel_count,
            save_only.changed_pixel_ratio,
            save_only.mean_absolute_channel_difference,
            save_only.max_channel_difference,
            save_only.content_stream_count,
            save_only.operation_count,
            save_only.text_show_operator_count,
            save_only.text_operand_count,
            save_only.output_bytes,
            save_only.output_size_ratio,
            save_only.parse_and_rewrite_ms,
            save_only.total_ms
        );
        println!(
            "pdf-v3 operator rewrite text_match={} chars={}/{} first_diff={:?} pixels={} ratio={:.6} mean={:.6} max={} streams={} shared={} form_invocations={} unique_forms={} shared_forms={} operations={} text_ops={} malformed={} operands={}/{} bytes={} size_ratio={:.6} parse={}ms total={}ms",
            rewrite.text_exact_match,
            rewrite.source_text_chars,
            rewrite.output_text_chars,
            rewrite.first_text_difference_index,
            rewrite.changed_pixel_count,
            rewrite.changed_pixel_ratio,
            rewrite.mean_absolute_channel_difference,
            rewrite.max_channel_difference,
            rewrite.content_stream_count,
            rewrite.shared_content_stream_count,
            rewrite.form_xobject_invocation_count,
            rewrite.unique_form_stream_count,
            rewrite.shared_form_stream_count,
            rewrite.operation_count,
            rewrite.text_show_operator_count,
            rewrite.malformed_text_show_operator_count,
            rewrite.rewritten_text_operand_count,
            rewrite.text_operand_count,
            rewrite.output_bytes,
            rewrite.output_size_ratio,
            rewrite.parse_and_rewrite_ms,
            rewrite.total_ms
        );

        if let Ok(path) = env::var("ROSETTA_PDF_V3_OPERATOR_OUTPUT") {
            let output = super::operator_identity_output(&source, 1).expect("operator output");
            fs::write(path, output).expect("write manual operator output");
        }
    }
}

#[cfg(test)]
fn operator_identity_output(
    source_path: &Path,
    page_number: u32,
) -> Result<Vec<u8>, ContentStreamProbeError> {
    let source_bytes = std::fs::read(source_path)
        .map_err(|error| ContentStreamProbeError::Load(format!("failed to read PDF: {error}")))?;
    let mut document = Document::load_mem(&source_bytes)
        .map_err(|error| ContentStreamProbeError::Load(format!("failed to load PDF: {error}")))?;
    let pages = document.get_pages();
    let page_id =
        pages
            .get(&page_number)
            .copied()
            .ok_or(ContentStreamProbeError::PageOutOfBounds {
                page: page_number,
                page_count: pages.len() as u32,
            })?;
    let references = content_stream_page_references(&document);
    inspect_page_streams(&mut document, page_id, page_number, &references, true)?;
    let mut output = Vec::new();
    document.save_to(&mut output).map_err(|error| {
        ContentStreamProbeError::Save(format!("failed to save operator output: {error}"))
    })?;
    Ok(output)
}
