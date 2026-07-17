use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    time::Instant,
};

use lopdf::{content::Content, Document, Object, ObjectId, StringFormat};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::font::{
    stage_translation_fonts_page_dictionary, PreparedTranslationFont, TranslationFontError,
    TranslationFontWeight,
};
use super::{
    layout::{derive_text_show_fit_bounds, TextShowFitBoundsError, TextShowGeometryKey},
    patch::{
        stage_invocation_local_copy_on_write_batch, ContentPatchError,
        InvocationLocalCopyOnWriteTarget, ResourceReferenceBinding,
    },
    style::{plan_text_show_style, TextShowStyleError, TextShowStylePlan},
    types::PageGraph,
};

#[derive(Debug, Clone)]
pub(crate) struct TextShowReplacementRequest {
    pub geometry: TextShowGeometryKey,
    pub expected_operator: String,
    pub expected_operand_hash: String,
    pub translated_text: String,
    pub minimum_fit_scale: f32,
}

impl TextShowReplacementRequest {
    fn stream_id(&self) -> ObjectId {
        (
            self.geometry.stream_object_number,
            self.geometry.stream_generation,
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextShowReplacementTargetRequest {
    pub replacements: Vec<TextShowReplacementRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextShowReplacementResult {
    pub schema: &'static str,
    pub page_number: u32,
    pub stream_id: String,
    pub operation_index: usize,
    pub form_invocation_depth: usize,
    pub fit_scale: f32,
    pub max_advance: f32,
    pub page_advance: f32,
    pub baseline_scale: f32,
    pub natural_advance: f32,
    pub fitted_advance: f32,
    pub geometry_atom_count: usize,
    pub style_id: String,
    pub translation_font_weight: TranslationFontWeight,
    pub source_font_weight: u16,
    pub source_fill_color: [f32; 4],
    pub source_opacity: f32,
    pub source_render_mode: String,
    pub staged_font_object_count: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextShowReplacementTransactionResult {
    pub schema: &'static str,
    pub page_number: u32,
    pub stream_id: String,
    pub replacement_count: usize,
    pub form_invocation_depth: usize,
    pub translation_font_weights: Vec<TranslationFontWeight>,
    pub staged_font_object_count: usize,
    pub cloned_stream_count: usize,
    pub page_content_rewired: bool,
    pub elapsed_ms: u64,
    pub replacements: Vec<TextShowReplacementResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextShowReplacementBatchTargetResult {
    pub schema: &'static str,
    pub stream_id: String,
    pub replacement_count: usize,
    pub form_invocation_depth: usize,
    pub translation_font_weights: Vec<TranslationFontWeight>,
    pub replacements: Vec<TextShowReplacementResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextShowReplacementBatchResult {
    pub schema: &'static str,
    pub page_number: u32,
    pub target_count: usize,
    pub replacement_count: usize,
    pub translation_font_weights: Vec<TranslationFontWeight>,
    pub staged_font_object_count: usize,
    pub cloned_stream_count: usize,
    pub page_content_rewired: bool,
    pub elapsed_ms: u64,
    pub targets: Vec<TextShowReplacementBatchTargetResult>,
}

#[derive(Debug)]
pub(crate) enum TextShowReplacementError {
    Font(TranslationFontError),
    FitBounds(TextShowFitBoundsError),
    Patch(ContentPatchError),
    Style(TextShowStyleError),
    MissingTranslationFontFace(TranslationFontWeight),
    DuplicateTranslationFontFace(TranslationFontWeight),
    PageOutOfBounds {
        page: u32,
        page_count: u32,
    },
    StreamOutsidePage,
    RepeatedPageStream,
    StreamRead(String),
    ContentDecode(String),
    OperationMissing,
    UnsupportedOperator(String),
    SourceIdentityMismatch(String),
    MissingSourceFontState,
    SourcePaintStateUnsupported,
    SourceStyleMismatch,
    LaterTextShowInTextObject,
    InvalidTextPositionReset,
    EmptyBatch,
    BatchPageMismatch,
    DuplicateBatchTarget,
    EmptyTransaction,
    TransactionTargetMismatch,
    DuplicateTransactionOperation,
    CrossTextObjectTransaction,
    EmptyTranslation,
    InvalidFitBounds,
    Overflow {
        required_scale: f32,
        minimum_scale: f32,
    },
    ContentEncode(String),
    StreamWrite(String),
}

impl fmt::Display for TextShowReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(error) => error.fmt(formatter),
            Self::FitBounds(error) => error.fmt(formatter),
            Self::Patch(error) => error.fmt(formatter),
            Self::Style(error) => error.fmt(formatter),
            Self::MissingTranslationFontFace(weight) => {
                write!(
                    formatter,
                    "translation font face {weight:?} is not prepared"
                )
            }
            Self::DuplicateTranslationFontFace(weight) => {
                write!(formatter, "translation font face {weight:?} is duplicated")
            }
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::StreamOutsidePage => {
                formatter.write_str("replacement stream is outside the selected page")
            }
            Self::RepeatedPageStream => formatter
                .write_str("single-object replacement requires one selected-page stream reference"),
            Self::StreamRead(message)
            | Self::ContentDecode(message)
            | Self::ContentEncode(message)
            | Self::StreamWrite(message)
            | Self::SourceIdentityMismatch(message) => formatter.write_str(message),
            Self::OperationMissing => formatter.write_str("replacement operation is missing"),
            Self::UnsupportedOperator(operator) => {
                write!(formatter, "text-show operator {operator} is unsupported")
            }
            Self::MissingSourceFontState => {
                formatter.write_str("source text-show font state is incomplete")
            }
            Self::SourcePaintStateUnsupported => {
                formatter.write_str("source text paint state cannot be validated safely")
            }
            Self::SourceStyleMismatch => {
                formatter.write_str("source text paint state no longer matches PageGraph style")
            }
            Self::LaterTextShowInTextObject => formatter.write_str(
                "replacement target is followed by an unanchored show in the same text object",
            ),
            Self::InvalidTextPositionReset => formatter
                .write_str("text-position reset before the next show is malformed or unsupported"),
            Self::EmptyBatch => formatter.write_str("text-show replacement batch is empty"),
            Self::BatchPageMismatch => {
                formatter.write_str("text-show replacement batch must target one page")
            }
            Self::DuplicateBatchTarget => formatter
                .write_str("text-show replacement batch contains a duplicate stream/path target"),
            Self::EmptyTransaction => {
                formatter.write_str("text-show replacement transaction is empty")
            }
            Self::TransactionTargetMismatch => formatter
                .write_str("text-show replacement transaction must target one page content stream"),
            Self::DuplicateTransactionOperation => formatter
                .write_str("text-show replacement transaction contains a duplicate operation"),
            Self::CrossTextObjectTransaction => formatter.write_str(
                "text-show replacement transaction must stay inside one BT/ET text object",
            ),
            Self::EmptyTranslation => formatter.write_str("replacement text is empty"),
            Self::InvalidFitBounds => formatter.write_str("replacement fit bounds are invalid"),
            Self::Overflow {
                required_scale,
                minimum_scale,
            } => write!(
                formatter,
                "replacement requires scale {required_scale:.4}, below minimum {minimum_scale:.4}"
            ),
        }
    }
}

impl std::error::Error for TextShowReplacementError {}

impl From<TranslationFontError> for TextShowReplacementError {
    fn from(value: TranslationFontError) -> Self {
        Self::Font(value)
    }
}

impl From<TextShowFitBoundsError> for TextShowReplacementError {
    fn from(value: TextShowFitBoundsError) -> Self {
        Self::FitBounds(value)
    }
}

impl From<ContentPatchError> for TextShowReplacementError {
    fn from(value: ContentPatchError) -> Self {
        Self::Patch(value)
    }
}

impl From<TextShowStyleError> for TextShowReplacementError {
    fn from(value: TextShowStyleError) -> Self {
        Self::Style(value)
    }
}

#[derive(Clone)]
struct SavedTextState {
    font_resource: Option<Vec<u8>>,
    font_size: Option<f32>,
    horizontal_scaling: f32,
    fill_color: [f32; 4],
    stroke_color: [f32; 4],
    render_mode: i32,
    paint_supported: bool,
}

#[derive(Clone)]
struct TextState {
    font_resource: Option<Vec<u8>>,
    font_size: Option<f32>,
    horizontal_scaling: f32,
    fill_color: [f32; 4],
    stroke_color: [f32; 4],
    render_mode: i32,
    paint_supported: bool,
    inside_text_object: bool,
    saved: Vec<SavedTextState>,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font_resource: None,
            font_size: None,
            horizontal_scaling: 100.0,
            fill_color: [0.0, 0.0, 0.0, 1.0],
            stroke_color: [0.0, 0.0, 0.0, 1.0],
            render_mode: 0,
            paint_supported: true,
            inside_text_object: false,
            saved: Vec::new(),
        }
    }
}

pub(crate) fn apply_single_text_show_replacement(
    document: &mut Document,
    page_graph: &PageGraph,
    request: &TextShowReplacementRequest,
    font: &PreparedTranslationFont,
) -> Result<TextShowReplacementResult, TextShowReplacementError> {
    let transaction = apply_text_show_replacement_transaction(
        document,
        page_graph,
        std::slice::from_ref(request),
        &[font],
    )?;
    transaction
        .replacements
        .into_iter()
        .next()
        .ok_or(TextShowReplacementError::EmptyTransaction)
}

pub(crate) fn apply_text_show_replacement_transaction(
    document: &mut Document,
    page_graph: &PageGraph,
    requests: &[TextShowReplacementRequest],
    fonts: &[&PreparedTranslationFont],
) -> Result<TextShowReplacementTransactionResult, TextShowReplacementError> {
    let target_request = TextShowReplacementTargetRequest {
        replacements: requests.to_vec(),
    };
    let mut batch = apply_text_show_replacement_batch(
        document,
        page_graph,
        std::slice::from_ref(&target_request),
        fonts,
    )?;
    let target = batch
        .targets
        .pop()
        .ok_or(TextShowReplacementError::EmptyTransaction)?;
    Ok(TextShowReplacementTransactionResult {
        schema: "rosetta-pdf-v3-text-show-replacement-transaction/3",
        page_number: batch.page_number,
        stream_id: target.stream_id,
        replacement_count: target.replacement_count,
        form_invocation_depth: target.form_invocation_depth,
        translation_font_weights: target.translation_font_weights,
        staged_font_object_count: batch.staged_font_object_count,
        cloned_stream_count: batch.cloned_stream_count,
        page_content_rewired: batch.page_content_rewired,
        elapsed_ms: batch.elapsed_ms,
        replacements: target.replacements,
    })
}

pub(crate) fn apply_text_show_replacement_batch(
    document: &mut Document,
    page_graph: &PageGraph,
    target_requests: &[TextShowReplacementTargetRequest],
    fonts: &[&PreparedTranslationFont],
) -> Result<TextShowReplacementBatchResult, TextShowReplacementError> {
    let started = Instant::now();
    let first_request = target_requests
        .first()
        .and_then(|target| target.replacements.first())
        .ok_or(TextShowReplacementError::EmptyBatch)?;
    let page_number = first_request.geometry.page_number;
    let mut fonts_by_weight = BTreeMap::new();
    for font in fonts {
        if fonts_by_weight.insert(font.weight(), *font).is_some() {
            return Err(TextShowReplacementError::DuplicateTranslationFontFace(
                font.weight(),
            ));
        }
    }

    let pages = document.get_pages();
    let page_count = pages.len() as u32;
    let page_id =
        pages
            .get(&page_number)
            .copied()
            .ok_or(TextShowReplacementError::PageOutOfBounds {
                page: page_number,
                page_count,
            })?;
    let mut target_keys = BTreeSet::new();
    let mut planned_targets = Vec::with_capacity(target_requests.len());
    for target_request in target_requests {
        let target = plan_replacement_target(
            document,
            page_id,
            page_graph,
            &target_request.replacements,
            &fonts_by_weight,
        )?;
        if target.page_number != page_number {
            return Err(TextShowReplacementError::BatchPageMismatch);
        }
        if !target_keys.insert(target.key.clone()) {
            return Err(TextShowReplacementError::DuplicateBatchTarget);
        }
        planned_targets.push(target);
    }
    planned_targets.sort_by(|left, right| left.key.cmp(&right.key));

    let translation_font_weights = planned_targets
        .iter()
        .flat_map(|target| {
            target
                .planned
                .iter()
                .map(|replacement| replacement.translation_font_weight)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut reserved_through = document.max_id;
    let mut staged_fonts = Vec::with_capacity(translation_font_weights.len());
    for weight in &translation_font_weights {
        let font = fonts_by_weight.get(weight).copied().ok_or(
            TextShowReplacementError::MissingTranslationFontFace(*weight),
        )?;
        let staged = font.stage_after(
            document,
            translation_font_resource(*weight).to_vec(),
            reserved_through,
        )?;
        reserved_through = staged.next_object_number;
        staged_fonts.push(staged);
    }
    let staged_font_object_count = staged_fonts.iter().map(|font| font.objects.len()).sum();
    let resource_bindings = staged_fonts
        .iter()
        .map(|font| ResourceReferenceBinding {
            category: b"Font".to_vec(),
            name: font.resource_name.clone(),
            object_id: font.type0_font_id,
        })
        .collect::<Vec<_>>();
    let requires_copy_on_write = planned_targets
        .iter()
        .any(|target| target.requires_copy_on_write);
    let (staged_streams, staged_page, cloned_stream_count, page_content_rewired) =
        if requires_copy_on_write {
            let targets = planned_targets
                .iter_mut()
                .map(|target| {
                    Ok(InvocationLocalCopyOnWriteTarget {
                        target_stream_id: target.key.stream_id,
                        form_invocation_path: target.key.form_invocation_path.clone(),
                        patched_stream: target.staged_stream.take().ok_or(
                            TextShowReplacementError::SourceIdentityMismatch(
                                "replacement target stream was already consumed".to_string(),
                            ),
                        )?,
                        resource_bindings: resource_bindings.clone(),
                    })
                })
                .collect::<Result<Vec<_>, TextShowReplacementError>>()?;
            let stage = stage_invocation_local_copy_on_write_batch(
                document,
                page_id,
                page_number,
                targets,
                reserved_through,
            )?;
            reserved_through = stage.next_object_number;
            let cloned_stream_count = stage.streams.len();
            (stage.streams, stage.page, cloned_stream_count, true)
        } else {
            let mut staged_streams = BTreeMap::new();
            for target in &mut planned_targets {
                let staged_stream = target.staged_stream.take().ok_or(
                    TextShowReplacementError::SourceIdentityMismatch(
                        "replacement target stream was already consumed".to_string(),
                    ),
                )?;
                staged_streams.insert(target.key.stream_id, staged_stream);
            }
            let staged_page = stage_translation_fonts_page_dictionary(
                document,
                page_id,
                staged_fonts
                    .iter()
                    .map(|font| (font.resource_name.as_slice(), font.type0_font_id)),
            )?;
            (staged_streams, staged_page, 0, false)
        };

    for staged_font in staged_fonts {
        for (object_id, object) in staged_font.objects {
            document.objects.insert(object_id, object);
        }
    }
    for (staged_stream_id, stream) in staged_streams {
        document
            .objects
            .insert(staged_stream_id, Object::Stream(stream));
    }
    document
        .objects
        .insert(page_id, Object::Dictionary(staged_page));
    document.max_id = document.max_id.max(reserved_through);

    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let mut replacement_count = 0usize;
    let targets = planned_targets
        .into_iter()
        .map(|target| {
            let target_weights = target
                .planned
                .iter()
                .map(|replacement| replacement.translation_font_weight)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let stream_id = format!("{}-{}", target.key.stream_id.0, target.key.stream_id.1);
            let form_invocation_depth = target.key.form_invocation_path.len();
            let replacements = target
                .planned
                .into_iter()
                .map(|replacement| TextShowReplacementResult {
                    schema: "rosetta-pdf-v3-text-show-replacement/6",
                    page_number,
                    stream_id: stream_id.clone(),
                    operation_index: replacement.operation_index,
                    form_invocation_depth,
                    fit_scale: replacement.fit_scale,
                    max_advance: replacement.max_advance,
                    page_advance: replacement.page_advance,
                    baseline_scale: replacement.baseline_scale,
                    natural_advance: replacement.natural_advance,
                    fitted_advance: replacement.fitted_advance,
                    geometry_atom_count: replacement.geometry_atom_count,
                    style_id: replacement.style_id,
                    translation_font_weight: replacement.translation_font_weight,
                    source_font_weight: replacement.source_font_weight,
                    source_fill_color: replacement.source_fill_color,
                    source_opacity: replacement.source_opacity,
                    source_render_mode: replacement.source_render_mode,
                    staged_font_object_count,
                    elapsed_ms,
                })
                .collect::<Vec<_>>();
            replacement_count += replacements.len();
            TextShowReplacementBatchTargetResult {
                schema: "rosetta-pdf-v3-text-show-replacement-batch-target/1",
                stream_id,
                replacement_count: replacements.len(),
                form_invocation_depth,
                translation_font_weights: target_weights,
                replacements,
            }
        })
        .collect::<Vec<_>>();

    Ok(TextShowReplacementBatchResult {
        schema: "rosetta-pdf-v3-text-show-replacement-batch/1",
        page_number,
        target_count: targets.len(),
        replacement_count,
        translation_font_weights,
        staged_font_object_count,
        cloned_stream_count,
        page_content_rewired,
        elapsed_ms,
        targets,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReplacementTargetKey {
    stream_id: ObjectId,
    form_invocation_path: Vec<super::types::FormInvocationStep>,
}

struct PlannedReplacementTarget {
    key: ReplacementTargetKey,
    page_number: u32,
    requires_copy_on_write: bool,
    staged_stream: Option<lopdf::Stream>,
    planned: Vec<PlannedTextShowReplacement>,
}

fn plan_replacement_target(
    document: &Document,
    page_id: ObjectId,
    page_graph: &PageGraph,
    requests: &[TextShowReplacementRequest],
    fonts_by_weight: &BTreeMap<TranslationFontWeight, &PreparedTranslationFont>,
) -> Result<PlannedReplacementTarget, TextShowReplacementError> {
    let first_request = requests
        .first()
        .ok_or(TextShowReplacementError::EmptyTransaction)?;
    if requests.iter().any(|request| {
        request.geometry.page_number != first_request.geometry.page_number
            || request.stream_id() != first_request.stream_id()
            || request.geometry.form_invocation_path != first_request.geometry.form_invocation_path
    }) {
        return Err(TextShowReplacementError::TransactionTargetMismatch);
    }
    let mut operation_indices = BTreeSet::new();
    for request in requests {
        if !operation_indices.insert(request.geometry.operation_index) {
            return Err(TextShowReplacementError::DuplicateTransactionOperation);
        }
        if request.translated_text.is_empty() {
            return Err(TextShowReplacementError::EmptyTranslation);
        }
        if !request.minimum_fit_scale.is_finite()
            || !(0.0..=1.0).contains(&request.minimum_fit_scale)
        {
            return Err(TextShowReplacementError::InvalidFitBounds);
        }
    }

    let stream_id = first_request.stream_id();
    let form_invocation_path = &first_request.geometry.form_invocation_path;
    let is_form_target = !form_invocation_path.is_empty();
    let top_level_requires_copy_on_write = if is_form_target {
        false
    } else {
        let selected_references = document
            .get_page_contents(page_id)
            .into_iter()
            .filter(|candidate| *candidate == stream_id)
            .count();
        if selected_references == 0 {
            return Err(TextShowReplacementError::StreamOutsidePage);
        }
        if selected_references != 1 {
            return Err(TextShowReplacementError::RepeatedPageStream);
        }
        content_stream_referencing_pages(document, stream_id).len() != 1
    };

    let source_stream = document
        .get_object(stream_id)
        .and_then(Object::as_stream)
        .map_err(|error| TextShowReplacementError::StreamRead(error.to_string()))?;
    let source_content = source_stream
        .get_plain_content()
        .map_err(|error| TextShowReplacementError::StreamRead(error.to_string()))?;
    let mut content = Content::decode(&source_content)
        .map_err(|error| TextShowReplacementError::ContentDecode(error.to_string()))?;
    let mut text_object_bounds = None;
    let mut planned = Vec::with_capacity(requests.len());
    for request in requests {
        let bounds = find_text_object_bounds(&content, request.geometry.operation_index)?;
        match text_object_bounds {
            Some(expected) if expected != bounds => {
                return Err(TextShowReplacementError::CrossTextObjectTransaction);
            }
            None => text_object_bounds = Some(bounds),
            _ => {}
        }
        planned.push(plan_text_show_replacement(
            &content,
            page_graph,
            request,
            fonts_by_weight,
        )?);
    }
    planned.sort_by_key(|replacement| replacement.operation_index);
    for replacement in planned.iter().rev() {
        content.operations.splice(
            replacement.operation_index..=replacement.operation_index,
            replacement.operations.clone(),
        );
    }
    let rewritten_content = content
        .encode()
        .map_err(|error| TextShowReplacementError::ContentEncode(error.to_string()))?;
    let mut staged_stream = source_stream.clone();
    staged_stream.set_plain_content(rewritten_content);
    staged_stream
        .compress()
        .map_err(|error| TextShowReplacementError::StreamWrite(error.to_string()))?;

    Ok(PlannedReplacementTarget {
        key: ReplacementTargetKey {
            stream_id,
            form_invocation_path: form_invocation_path.clone(),
        },
        page_number: first_request.geometry.page_number,
        requires_copy_on_write: is_form_target || top_level_requires_copy_on_write,
        staged_stream: Some(staged_stream),
        planned,
    })
}

#[derive(Clone)]
struct PlannedTextShowReplacement {
    operation_index: usize,
    operations: Vec<lopdf::content::Operation>,
    fit_scale: f32,
    max_advance: f32,
    page_advance: f32,
    baseline_scale: f32,
    natural_advance: f32,
    fitted_advance: f32,
    geometry_atom_count: usize,
    style_id: String,
    translation_font_weight: TranslationFontWeight,
    source_font_weight: u16,
    source_fill_color: [f32; 4],
    source_opacity: f32,
    source_render_mode: String,
}

fn plan_text_show_replacement(
    content: &Content,
    page_graph: &PageGraph,
    request: &TextShowReplacementRequest,
    fonts_by_weight: &BTreeMap<TranslationFontWeight, &PreparedTranslationFont>,
) -> Result<PlannedTextShowReplacement, TextShowReplacementError> {
    let source_state = state_before_operation(content, request.geometry.operation_index)?;
    validate_source_state(request, &source_state)?;
    validate_text_position_boundary(content, request.geometry.operation_index)?;
    let operation = content
        .operations
        .get(request.geometry.operation_index)
        .ok_or(TextShowReplacementError::OperationMissing)?;
    if operation.operator != request.expected_operator {
        return Err(TextShowReplacementError::SourceIdentityMismatch(format!(
            "operator mismatch: expected {}, found {}",
            request.expected_operator, operation.operator
        )));
    }
    if !is_text_show_operator(&operation.operator) {
        return Err(TextShowReplacementError::UnsupportedOperator(
            operation.operator.clone(),
        ));
    }
    let actual_hash = text_operand_hash(operation);
    if actual_hash != request.expected_operand_hash {
        return Err(TextShowReplacementError::SourceIdentityMismatch(format!(
            "operand hash mismatch: expected {}, found {}",
            request.expected_operand_hash, actual_hash
        )));
    }

    let fit_bounds = derive_text_show_fit_bounds(page_graph, &request.geometry)?;
    let style_plan = plan_text_show_style(page_graph, &fit_bounds.style_id)?;
    validate_source_style(&style_plan, &source_state)?;
    let font = fonts_by_weight
        .get(&style_plan.translation_font_weight)
        .copied()
        .ok_or(TextShowReplacementError::MissingTranslationFontFace(
            style_plan.translation_font_weight,
        ))?;
    let staged_font_resource = translation_font_resource(style_plan.translation_font_weight);
    let natural_advance = font.text_advance_1000(&request.translated_text)? as f32 / 1000.0
        * request.geometry.source_font_size
        * (request.geometry.source_horizontal_scaling / 100.0);
    if !natural_advance.is_finite() || natural_advance <= 0.0 {
        return Err(TextShowReplacementError::InvalidFitBounds);
    }
    let fit_scale = (fit_bounds.max_advance / natural_advance).min(1.0);
    if fit_scale < request.minimum_fit_scale {
        return Err(TextShowReplacementError::Overflow {
            required_scale: fit_scale,
            minimum_scale: request.minimum_fit_scale,
        });
    }
    let encoded = font.encode_text(&request.translated_text)?;
    let replacement_operation = replacement_operation(operation, encoded)?;
    let translated_scaling = request.geometry.source_horizontal_scaling * fit_scale;
    let operations = vec![
        lopdf::content::Operation::new(
            "Tf",
            vec![
                Object::Name(staged_font_resource.to_vec()),
                Object::Real(request.geometry.source_font_size),
            ],
        ),
        lopdf::content::Operation::new("Tz", vec![Object::Real(translated_scaling)]),
        replacement_operation,
        lopdf::content::Operation::new(
            "Tf",
            vec![
                Object::Name(request.geometry.source_font_resource.as_bytes().to_vec()),
                Object::Real(request.geometry.source_font_size),
            ],
        ),
        lopdf::content::Operation::new(
            "Tz",
            vec![Object::Real(request.geometry.source_horizontal_scaling)],
        ),
    ];

    Ok(PlannedTextShowReplacement {
        operation_index: request.geometry.operation_index,
        operations,
        fit_scale,
        max_advance: fit_bounds.max_advance,
        page_advance: fit_bounds.page_advance,
        baseline_scale: fit_bounds.baseline_scale,
        natural_advance,
        fitted_advance: natural_advance * fit_scale,
        geometry_atom_count: fit_bounds.atom_count,
        style_id: style_plan.style_id,
        translation_font_weight: style_plan.translation_font_weight,
        source_font_weight: style_plan.source_font_weight,
        source_fill_color: style_plan.fill_color,
        source_opacity: style_plan.opacity,
        source_render_mode: style_plan.render_mode,
    })
}

fn translation_font_resource(weight: TranslationFontWeight) -> &'static [u8] {
    match weight {
        TranslationFontWeight::Regular => b"RosettaTranslationRegular",
        TranslationFontWeight::Bold => b"RosettaTranslationBold",
    }
}

fn state_before_operation(
    content: &Content,
    operation_index: usize,
) -> Result<TextState, TextShowReplacementError> {
    if operation_index >= content.operations.len() {
        return Err(TextShowReplacementError::OperationMissing);
    }
    let mut state = TextState::default();
    for operation in &content.operations[..operation_index] {
        match operation.operator.as_str() {
            "q" => state.saved.push(SavedTextState {
                font_resource: state.font_resource.clone(),
                font_size: state.font_size,
                horizontal_scaling: state.horizontal_scaling,
                fill_color: state.fill_color,
                stroke_color: state.stroke_color,
                render_mode: state.render_mode,
                paint_supported: state.paint_supported,
            }),
            "Q" => {
                if let Some(saved) = state.saved.pop() {
                    state.font_resource = saved.font_resource;
                    state.font_size = saved.font_size;
                    state.horizontal_scaling = saved.horizontal_scaling;
                    state.fill_color = saved.fill_color;
                    state.stroke_color = saved.stroke_color;
                    state.render_mode = saved.render_mode;
                    state.paint_supported = saved.paint_supported;
                }
            }
            "BT" => state.inside_text_object = true,
            "ET" => state.inside_text_object = false,
            "Tf" => {
                state.font_resource = operation
                    .operands
                    .first()
                    .and_then(|operand| operand.as_name().ok())
                    .map(ToOwned::to_owned);
                state.font_size = operation.operands.get(1).and_then(numeric_operand_f32);
            }
            "Tz" => {
                if let Some(value) = operation.operands.first().and_then(numeric_operand_f32) {
                    state.horizontal_scaling = value;
                }
            }
            "Tr" => {
                if let Some(value) = operation.operands.first().and_then(numeric_operand_i32) {
                    state.render_mode = value;
                } else {
                    state.paint_supported = false;
                }
            }
            "g" => set_device_gray(operation, &mut state.fill_color, &mut state.paint_supported),
            "G" => set_device_gray(
                operation,
                &mut state.stroke_color,
                &mut state.paint_supported,
            ),
            "rg" => set_device_rgb(operation, &mut state.fill_color, &mut state.paint_supported),
            "RG" => set_device_rgb(
                operation,
                &mut state.stroke_color,
                &mut state.paint_supported,
            ),
            "k" => set_device_cmyk(operation, &mut state.fill_color, &mut state.paint_supported),
            "K" => set_device_cmyk(
                operation,
                &mut state.stroke_color,
                &mut state.paint_supported,
            ),
            "cs" | "CS" | "sc" | "SC" | "scn" | "SCN" | "gs" => {
                state.paint_supported = false;
            }
            _ => {}
        }
    }
    Ok(state)
}

fn validate_source_style(
    plan: &TextShowStylePlan,
    state: &TextState,
) -> Result<(), TextShowReplacementError> {
    if !state.paint_supported {
        return Err(TextShowReplacementError::SourcePaintStateUnsupported);
    }
    if state.render_mode != 0
        || !colors_match(state.fill_color, plan.fill_color)
        || !approximately_equal(state.fill_color[3], plan.opacity)
    {
        return Err(TextShowReplacementError::SourceStyleMismatch);
    }
    Ok(())
}

fn set_device_gray(
    operation: &lopdf::content::Operation,
    target: &mut [f32; 4],
    supported: &mut bool,
) {
    let Some(gray) = operation.operands.first().and_then(numeric_operand_f32) else {
        *supported = false;
        return;
    };
    if !(0.0..=1.0).contains(&gray) {
        *supported = false;
        return;
    }
    *target = [gray, gray, gray, target[3]];
}

fn set_device_rgb(
    operation: &lopdf::content::Operation,
    target: &mut [f32; 4],
    supported: &mut bool,
) {
    let Some(components) = numeric_components(operation, 3) else {
        *supported = false;
        return;
    };
    *target = [components[0], components[1], components[2], target[3]];
}

fn set_device_cmyk(
    operation: &lopdf::content::Operation,
    target: &mut [f32; 4],
    supported: &mut bool,
) {
    let Some(components) = numeric_components(operation, 4) else {
        *supported = false;
        return;
    };
    let [cyan, magenta, yellow, black] = components.as_slice() else {
        *supported = false;
        return;
    };
    *target = [
        1.0 - (cyan + black).min(1.0),
        1.0 - (magenta + black).min(1.0),
        1.0 - (yellow + black).min(1.0),
        target[3],
    ];
}

fn numeric_components(operation: &lopdf::content::Operation, count: usize) -> Option<Vec<f32>> {
    if operation.operands.len() != count {
        return None;
    }
    operation
        .operands
        .iter()
        .map(numeric_operand_f32)
        .collect::<Option<Vec<_>>>()
        .filter(|components| {
            components
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        })
}

fn validate_source_state(
    request: &TextShowReplacementRequest,
    state: &TextState,
) -> Result<(), TextShowReplacementError> {
    let Some(font_resource) = state.font_resource.as_deref() else {
        return Err(TextShowReplacementError::MissingSourceFontState);
    };
    let Some(font_size) = state.font_size else {
        return Err(TextShowReplacementError::MissingSourceFontState);
    };
    if !state.inside_text_object
        || font_resource != request.geometry.source_font_resource.as_bytes()
        || !approximately_equal(font_size, request.geometry.source_font_size)
        || !approximately_equal(
            state.horizontal_scaling,
            request.geometry.source_horizontal_scaling,
        )
    {
        return Err(TextShowReplacementError::SourceIdentityMismatch(
            "source text state no longer matches provenance".to_string(),
        ));
    }
    Ok(())
}

fn find_text_object_bounds(
    content: &Content,
    operation_index: usize,
) -> Result<(usize, usize), TextShowReplacementError> {
    if operation_index >= content.operations.len() {
        return Err(TextShowReplacementError::OperationMissing);
    }
    let mut start = None;
    for (index, operation) in content.operations[..operation_index]
        .iter()
        .enumerate()
        .rev()
    {
        match operation.operator.as_str() {
            "BT" => {
                start = Some(index);
                break;
            }
            "ET" => return Err(TextShowReplacementError::CrossTextObjectTransaction),
            _ => {}
        }
    }
    let start = start.ok_or(TextShowReplacementError::CrossTextObjectTransaction)?;
    for (end, operation) in content
        .operations
        .iter()
        .enumerate()
        .skip(operation_index + 1)
    {
        if operation.operator == "ET" {
            return Ok((start, end));
        }
        if operation.operator == "BT" {
            return Err(TextShowReplacementError::CrossTextObjectTransaction);
        }
    }
    Err(TextShowReplacementError::CrossTextObjectTransaction)
}

fn validate_text_position_boundary(
    content: &Content,
    operation_index: usize,
) -> Result<(), TextShowReplacementError> {
    let mut position_reset = false;
    for operation in content.operations.iter().skip(operation_index + 1) {
        match operation.operator.as_str() {
            "ET" => return Ok(()),
            "BT" => return Err(TextShowReplacementError::CrossTextObjectTransaction),
            "Tm" | "Td" | "TD" | "T*" => {
                if !valid_text_position_reset(operation) {
                    return Err(TextShowReplacementError::InvalidTextPositionReset);
                }
                position_reset = true;
            }
            "'" | "\"" => {
                return if valid_anchored_show(operation) {
                    Ok(())
                } else {
                    Err(TextShowReplacementError::InvalidTextPositionReset)
                };
            }
            "Tj" | "TJ" if position_reset => return Ok(()),
            "Tj" | "TJ" => return Err(TextShowReplacementError::LaterTextShowInTextObject),
            _ => {}
        }
    }
    Err(TextShowReplacementError::CrossTextObjectTransaction)
}

fn valid_text_position_reset(operation: &lopdf::content::Operation) -> bool {
    let expected_operands = match operation.operator.as_str() {
        "Tm" => 6,
        "Td" | "TD" => 2,
        "T*" => 0,
        _ => return false,
    };
    operation.operands.len() == expected_operands
        && operation
            .operands
            .iter()
            .all(|operand| numeric_operand_f32(operand).is_some_and(f32::is_finite))
}

fn valid_anchored_show(operation: &lopdf::content::Operation) -> bool {
    match operation.operator.as_str() {
        "'" => operation.operands.len() == 1 && operation.operands[0].as_str().is_ok(),
        "\"" => {
            operation.operands.len() == 3
                && operation.operands[..2]
                    .iter()
                    .all(|operand| numeric_operand_f32(operand).is_some_and(f32::is_finite))
                && operation.operands[2].as_str().is_ok()
        }
        _ => false,
    }
}

fn replacement_operation(
    source: &lopdf::content::Operation,
    encoded: Vec<u8>,
) -> Result<lopdf::content::Operation, TextShowReplacementError> {
    let string = Object::String(encoded, StringFormat::Hexadecimal);
    match source.operator.as_str() {
        "Tj" | "TJ" => Ok(lopdf::content::Operation::new("Tj", vec![string])),
        "'" => Ok(lopdf::content::Operation::new("'", vec![string])),
        "\"" if source.operands.len() >= 2 => Ok(lopdf::content::Operation::new(
            "\"",
            vec![
                source.operands[0].clone(),
                source.operands[1].clone(),
                string,
            ],
        )),
        operator => Err(TextShowReplacementError::UnsupportedOperator(
            operator.to_string(),
        )),
    }
}

fn text_operand_hash(operation: &lopdf::content::Operation) -> String {
    let operands = match operation.operator.as_str() {
        "Tj" | "'" => operation
            .operands
            .first()
            .and_then(|operand| operand.as_str().ok())
            .into_iter()
            .collect::<Vec<_>>(),
        "\"" => operation
            .operands
            .get(2)
            .and_then(|operand| operand.as_str().ok())
            .into_iter()
            .collect::<Vec<_>>(),
        "TJ" => operation
            .operands
            .first()
            .and_then(|operand| operand.as_array().ok())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut hasher = Sha256::new();
    for operand in operands {
        hasher.update((operand.len() as u64).to_le_bytes());
        hasher.update(operand);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn content_stream_referencing_pages(document: &Document, target: ObjectId) -> BTreeSet<u32> {
    document
        .get_pages()
        .into_iter()
        .filter_map(|(page_number, page_id)| {
            document
                .get_page_contents(page_id)
                .contains(&target)
                .then_some(page_number)
        })
        .collect()
}

fn numeric_operand_f32(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(value) => Some(*value as f32),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn numeric_operand_i32(object: &Object) -> Option<i32> {
    match object {
        Object::Integer(value) => i32::try_from(*value).ok(),
        Object::Real(value) if value.is_finite() && value.fract().abs() <= 0.0001 => {
            Some(*value as i32)
        }
        _ => None,
    }
}

fn colors_match(left: [f32; 4], right: [f32; 4]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= (1.0 / 255.0 + 0.0001))
}

fn approximately_equal(left: f32, right: f32) -> bool {
    (left - right).abs() <= 0.0001
}

fn is_text_show_operator(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use lopdf::{
        content::Content, content::Operation, Dictionary, Document, Object, ObjectId, Stream,
        StringFormat,
    };

    use super::{
        apply_single_text_show_replacement, apply_text_show_replacement_batch,
        apply_text_show_replacement_transaction, find_text_object_bounds, state_before_operation,
        validate_source_style, validate_text_position_boundary, ContentPatchError,
        TextShowReplacementError, TextShowReplacementRequest, TextShowReplacementTargetRequest,
    };
    use crate::{
        pdf_v3::{
            font::{TranslationFontAsset, TranslationFontWeight, UnifiedTranslationFontPlan},
            layout::{derive_text_show_fit_bounds, TextShowGeometryKey},
            mapping::map_page_atoms_to_content_operands,
            reconcile::build_reconciled_page_graph,
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn later_show_requires_a_valid_text_position_anchor() {
        let anchored = Content {
            operations: vec![
                Operation::new("BT", Vec::new()),
                text_show(b"first"),
                Operation::new("Td", vec![Object::Integer(0), Object::Integer(-12)]),
                text_show(b"second"),
                Operation::new("ET", Vec::new()),
            ],
        };
        let consecutive = Content {
            operations: vec![
                Operation::new("BT", Vec::new()),
                text_show(b"first"),
                text_show(b"second"),
                Operation::new("ET", Vec::new()),
            ],
        };
        let malformed = Content {
            operations: vec![
                Operation::new("BT", Vec::new()),
                text_show(b"first"),
                Operation::new("Td", vec![Object::Integer(0)]),
                text_show(b"second"),
                Operation::new("ET", Vec::new()),
            ],
        };

        assert!(validate_text_position_boundary(&anchored, 1).is_ok());
        assert!(matches!(
            validate_text_position_boundary(&consecutive, 1),
            Err(TextShowReplacementError::LaterTextShowInTextObject)
        ));
        assert!(matches!(
            validate_text_position_boundary(&malformed, 1),
            Err(TextShowReplacementError::InvalidTextPositionReset)
        ));
        assert_eq!(find_text_object_bounds(&anchored, 1).unwrap(), (0, 4));
        assert_eq!(find_text_object_bounds(&anchored, 3).unwrap(), (0, 4));
    }

    fn text_show(text: &[u8]) -> Operation {
        Operation::new(
            "Tj",
            vec![Object::String(text.to_vec(), StringFormat::Literal)],
        )
    }

    struct TemporaryPdf(PathBuf);

    impl TemporaryPdf {
        fn write(bytes: &[u8]) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "rosetta-pdf-v3-form-replacement-{}-{nonce}.pdf",
                std::process::id()
            ));
            fs::write(&path, bytes).expect("write temporary PDF");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TemporaryPdf {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn repeated_decodable_form_document() -> (Document, Vec<u8>, ObjectId, ObjectId, ObjectId) {
        let source =
            fs::read(fixture_path("002-trivial-libre-office-writer.pdf")).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let page_id = document.get_pages()[&1];
        let page_resources = match document
            .get_dictionary(page_id)
            .and_then(|page| page.get(b"Resources"))
            .expect("page resources")
        {
            Object::Dictionary(resources) => resources.clone(),
            Object::Reference(resources_id) => document
                .get_dictionary(*resources_id)
                .cloned()
                .expect("indirect page resources"),
            _ => panic!("page resources must be a dictionary"),
        };
        let mut page_operations = Vec::new();
        for stream_id in document.get_page_contents(page_id) {
            let stream = document
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .expect("page content stream");
            page_operations.extend(
                Content::decode(&stream.get_plain_content().expect("page content"))
                    .expect("decoded page content")
                    .operations,
            );
        }
        let page_content = Content {
            operations: page_operations,
        };
        let text_show_index = page_content
            .operations
            .iter()
            .position(|operation| super::is_text_show_operator(&operation.operator))
            .expect("source text show");
        let (text_object_start, text_object_end) =
            find_text_object_bounds(&page_content, text_show_index).expect("source text object");
        let form_operations = page_content.operations[text_object_start..=text_object_end].to_vec();
        let form_content = Content {
            operations: form_operations,
        }
        .encode()
        .expect("encoded Form content");
        let mut form_dictionary = Dictionary::new();
        form_dictionary.set("Type", Object::Name(b"XObject".to_vec()));
        form_dictionary.set("Subtype", Object::Name(b"Form".to_vec()));
        form_dictionary.set("FormType", Object::Integer(1));
        form_dictionary.set(
            "BBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(1000),
                Object::Integer(1000),
            ]),
        );
        form_dictionary.set("Resources", Object::Dictionary(page_resources.clone()));
        let mut form_stream = Stream::new(form_dictionary, form_content);
        form_stream.compress().expect("compressed Form stream");
        let form_stream_id = document.add_object(form_stream);

        let mut root_resources = page_resources;
        let mut xobjects = root_resources
            .get(b"XObject")
            .ok()
            .and_then(|object| object.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        xobjects.set("SharedForm", Object::Reference(form_stream_id));
        root_resources.set("XObject", Object::Dictionary(xobjects));
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Resources", Object::Dictionary(root_resources));

        let root_content = Content {
            operations: vec![
                Operation::new("q", Vec::new()),
                Operation::new("Do", vec![Object::Name(b"SharedForm".to_vec())]),
                Operation::new("Q", Vec::new()),
                Operation::new("q", Vec::new()),
                Operation::new(
                    "cm",
                    vec![
                        Object::Integer(1),
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Integer(1),
                        Object::Integer(0),
                        Object::Integer(-40),
                    ],
                ),
                Operation::new("Do", vec![Object::Name(b"SharedForm".to_vec())]),
                Operation::new("Q", Vec::new()),
            ],
        }
        .encode()
        .expect("encoded root content");
        let mut root_stream = Stream::new(Dictionary::new(), root_content);
        root_stream.compress().expect("compressed root stream");
        let root_stream_id = document.add_object(root_stream);
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set("Contents", Object::Reference(root_stream_id));

        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save repeated Form baseline");
        (document, baseline, page_id, root_stream_id, form_stream_id)
    }

    fn mixed_form_and_top_level_document(
    ) -> (Document, Vec<u8>, ObjectId, ObjectId, ObjectId, ObjectId) {
        let (mut document, _, page_id, root_stream_id, form_stream_id) =
            repeated_decodable_form_document();
        let form_stream = document
            .get_object(form_stream_id)
            .and_then(Object::as_stream)
            .expect("source Form stream");
        let form_content = Content::decode(&form_stream.get_plain_content().expect("Form content"))
            .expect("decoded Form content");
        let mut top_level_operations = vec![
            Operation::new("q", Vec::new()),
            Operation::new(
                "cm",
                vec![
                    Object::Integer(1),
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(1),
                    Object::Integer(0),
                    Object::Integer(-80),
                ],
            ),
        ];
        top_level_operations.extend(form_content.operations);
        top_level_operations.push(Operation::new("Q", Vec::new()));
        let top_level_content = Content {
            operations: top_level_operations,
        }
        .encode()
        .expect("encoded top-level content");
        let mut top_level_stream = Stream::new(Dictionary::new(), top_level_content);
        top_level_stream
            .compress()
            .expect("compressed top-level stream");
        let top_level_stream_id = document.add_object(top_level_stream);
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dictionary")
            .set(
                "Contents",
                Object::Array(vec![
                    Object::Reference(root_stream_id),
                    Object::Reference(top_level_stream_id),
                ]),
            );

        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save mixed Form/top-level baseline");
        (
            document,
            baseline,
            page_id,
            root_stream_id,
            form_stream_id,
            top_level_stream_id,
        )
    }

    fn cross_page_shared_content_document() -> (Document, Vec<u8>, ObjectId, ObjectId) {
        let source =
            fs::read(fixture_path("002-trivial-libre-office-writer.pdf")).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let first_page_id = document.get_pages()[&1];
        let mut second_page = document
            .get_dictionary(first_page_id)
            .cloned()
            .expect("first page dictionary");
        let parent_id = second_page
            .get(b"Parent")
            .and_then(Object::as_reference)
            .expect("page tree parent");
        second_page.set("Parent", Object::Reference(parent_id));
        let second_page_id = document.add_object(Object::Dictionary(second_page));
        let parent = document
            .get_object_mut(parent_id)
            .and_then(Object::as_dict_mut)
            .expect("page tree dictionary");
        parent
            .get_mut(b"Kids")
            .and_then(Object::as_array_mut)
            .expect("page tree kids")
            .push(Object::Reference(second_page_id));
        let page_count = parent
            .get(b"Count")
            .and_then(Object::as_i64)
            .expect("page tree count");
        parent.set("Count", Object::Integer(page_count + 1));

        let mut baseline_document = document.clone();
        let mut baseline = Vec::new();
        baseline_document
            .save_to(&mut baseline)
            .expect("save shared page baseline");
        (document, baseline, first_page_id, second_page_id)
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unique_text_show_replacement_is_searchable_and_atomic() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let replacement = "Unified replacement";
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.9,
        };
        let result =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect("single text-show replacement");
        assert_eq!(result.staged_font_object_count, 6);
        assert_eq!(result.fit_scale, 1.0);
        assert!(result.max_advance > 450.0);

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save replacement");
        let pdfium = shared_pdfium();
        let output_document = pdfium
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_text = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(output_text.contains(replacement));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn anchored_multi_show_transaction_is_atomic_and_searchable() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let mut selected_text_object = None;
        let mut selected = Vec::new();

        for target in mapping.mappings.iter().filter(|mapping| {
            mapping.form_invocation_path.is_empty()
                && mapping.source_font_resource.is_some()
                && mapping.source_font_size.is_some()
        }) {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                continue;
            };
            let Ok(style_plan) =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
            else {
                continue;
            };
            if style_plan.translation_font_weight != TranslationFontWeight::Bold {
                continue;
            }
            let stream = document
                .get_object((target.stream_object_number, target.stream_generation))
                .and_then(Object::as_stream)
                .expect("source stream");
            let content = Content::decode(&stream.get_plain_content().expect("plain content"))
                .expect("decoded content");
            if validate_text_position_boundary(&content, target.operation_index).is_err() {
                continue;
            }
            let Ok(source_state) = state_before_operation(&content, target.operation_index) else {
                continue;
            };
            if validate_source_style(&style_plan, &source_state).is_err() {
                continue;
            }
            let Ok(text_object_bounds) = find_text_object_bounds(&content, target.operation_index)
            else {
                continue;
            };
            let text_object_key = (
                target.stream_object_number,
                target.stream_generation,
                text_object_bounds,
            );
            match selected_text_object {
                Some(expected) if expected != text_object_key => continue,
                None => selected_text_object = Some(text_object_key),
                _ => {}
            }
            let text_show = mapping
                .text_shows
                .iter()
                .find(|show| show.text_show_id == target.text_show_id)
                .expect("target text show");
            selected.push((
                geometry,
                text_show.operator.clone(),
                text_show.encoded_byte_hash.clone(),
            ));
            if selected.len() == 2 {
                break;
            }
        }
        assert_eq!(
            selected.len(),
            2,
            "two anchored Bold shows in one text object"
        );

        let translated = ["VXA", "VXB"];
        let requests = selected
            .into_iter()
            .zip(translated)
            .map(
                |((geometry, expected_operator, expected_operand_hash), translated_text)| {
                    TextShowReplacementRequest {
                        geometry,
                        expected_operator,
                        expected_operand_hash,
                        translated_text: translated_text.to_string(),
                        minimum_fit_scale: 0.5,
                    }
                },
            )
            .collect::<Vec<_>>();
        let asset = TranslationFontAsset::open_weighted(
            "ArialBold",
            TranslationFontWeight::Bold,
            PathBuf::from(r"C:\Windows\Fonts\arialbd.ttf").as_path(),
            0,
        )
        .expect("Arial Bold font");
        let mut font_plan = UnifiedTranslationFontPlan::default();
        for translated_text in translated {
            font_plan.add_text(translated_text);
        }
        let prepared = asset.prepare(&font_plan).expect("prepared Bold font");

        let before = document.clone();
        let mut stale_requests = requests.clone();
        stale_requests[1].expected_operand_hash = "sha256:stale".to_string();
        let error = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            &stale_requests,
            &[&prepared],
        )
        .expect_err("stale second request must reject the complete transaction");
        assert!(matches!(
            error,
            TextShowReplacementError::SourceIdentityMismatch(_)
        ));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);

        let result = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            &requests,
            &[&prepared],
        )
        .expect("anchored multi-show transaction");
        assert_eq!(result.replacement_count, 2);
        assert_eq!(result.staged_font_object_count, 6);
        assert!(result.replacements[0].operation_index < result.replacements[1].operation_index);

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save transaction output");
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_text = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(output_text.contains(translated[0]));
        assert!(output_text.contains(translated[1]));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn anchored_mixed_face_transaction_stages_distinct_fonts_atomically() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let mut candidates = std::collections::BTreeMap::<
            (u32, u16, (usize, usize)),
            std::collections::BTreeMap<
                TranslationFontWeight,
                (TextShowGeometryKey, String, String),
            >,
        >::new();

        for target in mapping.mappings.iter().filter(|mapping| {
            mapping.form_invocation_path.is_empty()
                && mapping.source_font_resource.is_some()
                && mapping.source_font_size.is_some()
                && mapping.object_text_chars >= 2
        }) {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                continue;
            };
            let Ok(style_plan) =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
            else {
                continue;
            };
            let stream = document
                .get_object((target.stream_object_number, target.stream_generation))
                .and_then(Object::as_stream)
                .expect("source stream");
            let content = Content::decode(&stream.get_plain_content().expect("plain content"))
                .expect("decoded content");
            if validate_text_position_boundary(&content, target.operation_index).is_err() {
                continue;
            }
            let Ok(source_state) = state_before_operation(&content, target.operation_index) else {
                continue;
            };
            if validate_source_style(&style_plan, &source_state).is_err() {
                continue;
            }
            let Ok(text_object_bounds) = find_text_object_bounds(&content, target.operation_index)
            else {
                continue;
            };
            let text_show = mapping
                .text_shows
                .iter()
                .find(|show| show.text_show_id == target.text_show_id)
                .expect("target text show");
            let candidate = (
                geometry,
                text_show.operator.clone(),
                text_show.encoded_byte_hash.clone(),
            );
            candidates
                .entry((
                    target.stream_object_number,
                    target.stream_generation,
                    text_object_bounds,
                ))
                .or_insert_with(std::collections::BTreeMap::new)
                .entry(style_plan.translation_font_weight)
                .and_modify(|existing| {
                    if candidate.0.source_font_size > existing.0.source_font_size {
                        *existing = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }

        let mixed = candidates
            .values()
            .find(|group| {
                group.contains_key(&TranslationFontWeight::Regular)
                    && group.contains_key(&TranslationFontWeight::Bold)
            })
            .expect("one anchored text object with Regular and Bold shows");
        let production_font_probe = env::var_os("ROSETTA_PDF_V3_MIXED_SOURCE_HAN").is_some();
        let translated = if production_font_probe {
            [
                (TranslationFontWeight::Regular, "\u{7532}"),
                (TranslationFontWeight::Bold, "\u{4e59}"),
            ]
        } else {
            [
                (TranslationFontWeight::Regular, "\u{03a9}R"),
                (TranslationFontWeight::Bold, "\u{03a8}B"),
            ]
        };
        let requests = translated
            .iter()
            .map(|(weight, translated_text)| {
                let (geometry, expected_operator, expected_operand_hash) = &mixed[weight];
                TextShowReplacementRequest {
                    geometry: geometry.clone(),
                    expected_operator: expected_operator.clone(),
                    expected_operand_hash: expected_operand_hash.clone(),
                    translated_text: (*translated_text).to_string(),
                    minimum_fit_scale: 0.2,
                }
            })
            .collect::<Vec<_>>();

        let mut regular_plan = UnifiedTranslationFontPlan::default();
        regular_plan.add_text(translated[0].1);
        let regular_path = if production_font_probe {
            env::var("ROSETTA_PDF_V3_FONT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"))
        } else {
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf")
        };
        let regular = TranslationFontAsset::open_weighted(
            if production_font_probe {
                "SourceHanSansCNRegular"
            } else {
                "ArialRegular"
            },
            TranslationFontWeight::Regular,
            &regular_path,
            0,
        )
        .expect("Arial Regular font")
        .prepare(&regular_plan)
        .expect("prepared Regular font");
        let mut bold_plan = UnifiedTranslationFontPlan::default();
        bold_plan.add_text(translated[1].1);
        let bold_path = if production_font_probe {
            env::var("ROSETTA_PDF_V3_BOLD_FONT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Bold.ttf"))
        } else {
            PathBuf::from(r"C:\Windows\Fonts\arialbd.ttf")
        };
        let bold = TranslationFontAsset::open_weighted(
            if production_font_probe {
                "SourceHanSansCNBold"
            } else {
                "ArialBold"
            },
            TranslationFontWeight::Bold,
            &bold_path,
            0,
        )
        .expect("Arial Bold font")
        .prepare(&bold_plan)
        .expect("prepared Bold font");
        let before = document.clone();
        let missing_bold_error = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            &requests,
            &[&regular],
        )
        .expect_err("missing Bold face must reject the complete transaction");
        assert!(matches!(
            missing_bold_error,
            TextShowReplacementError::MissingTranslationFontFace(TranslationFontWeight::Bold)
        ));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
        let before_max_id = document.max_id;

        let result = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            &requests,
            &[&regular, &bold],
        )
        .expect("mixed-face transaction");

        assert_eq!(result.replacement_count, 2);
        assert_eq!(result.staged_font_object_count, 12);
        assert_eq!(document.max_id, before_max_id + 12);
        assert_eq!(
            result.translation_font_weights,
            vec![TranslationFontWeight::Regular, TranslationFontWeight::Bold]
        );
        let page_id = document.get_pages()[&1];
        let page = document.get_dictionary(page_id).expect("rewritten page");
        let resources = page
            .get(b"Resources")
            .and_then(Object::as_dict)
            .expect("materialized page resources");
        let fonts = resources
            .get(b"Font")
            .and_then(Object::as_dict)
            .expect("materialized page fonts");
        assert_ne!(
            fonts
                .get(b"RosettaTranslationRegular")
                .and_then(Object::as_reference)
                .expect("Regular font resource"),
            fonts
                .get(b"RosettaTranslationBold")
                .and_then(Object::as_reference)
                .expect("Bold font resource")
        );

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save mixed output");
        if let Ok(path) = env::var("ROSETTA_PDF_V3_MIXED_OUTPUT") {
            fs::write(path, &output).expect("write mixed output");
        }
        if production_font_probe {
            println!(
                "pdf-v3 mixed-face Source Han replacement count={} fits={:?} elapsed={}ms output={}",
                result.replacement_count,
                result
                    .replacements
                    .iter()
                    .map(|replacement| replacement.fit_scale)
                    .collect::<Vec<_>>(),
                result.elapsed_ms,
                output.len()
            );
        }
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_page = output_document.pages().get(0).expect("output page");
        let output_text = output_page.text().expect("output text");
        let output_text_all = output_text.all();
        assert!(translated
            .iter()
            .all(|(_, translated_text)| output_text_all.contains(translated_text)));
        let expected_fonts = if production_font_probe {
            ["sourcehansanscnregular", "sourcehansanscnbold"]
        } else {
            ["arialregular", "arialbold"]
        };
        let output_characters = output_text.chars();
        for ((_, translated_text), expected_font) in translated.iter().zip(expected_fonts) {
            let character = translated_text
                .chars()
                .next()
                .expect("translated character");
            let output_character = output_characters
                .iter()
                .find(|candidate| candidate.unicode_char() == Some(character))
                .expect("translated character");
            assert!(output_character
                .font_name()
                .to_ascii_lowercase()
                .contains(expected_font));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn shared_form_replacement_clones_only_the_selected_invocation() {
        let _guard = pdfium_test_lock();
        let (mut document, baseline, page_id, root_stream_id, form_stream_id) =
            repeated_decodable_form_document();
        let baseline_pdf = TemporaryPdf::write(&baseline);
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared Form mapping");
        let page_graph = build_reconciled_page_graph(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared Form PageGraph");
        let mut selected = None;
        for target in mapping.mappings.iter().filter(|mapping| {
            mapping.stream_object_number == form_stream_id.0
                && mapping.stream_generation == form_stream_id.1
                && mapping.form_invocation_path.len() == 1
                && mapping.form_invocation_path[0].operation_index == 5
                && mapping.source_font_resource.is_some()
                && mapping.source_font_size.is_some()
        }) {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                continue;
            };
            let Ok(style_plan) =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
            else {
                continue;
            };
            if style_plan.translation_font_weight != TranslationFontWeight::Regular {
                continue;
            }
            let form_stream = document
                .get_object(form_stream_id)
                .and_then(Object::as_stream)
                .expect("source Form stream");
            let content = Content::decode(&form_stream.get_plain_content().expect("Form content"))
                .expect("decoded Form content");
            if validate_text_position_boundary(&content, target.operation_index).is_err() {
                continue;
            }
            let Ok(source_state) = state_before_operation(&content, target.operation_index) else {
                continue;
            };
            if validate_source_style(&style_plan, &source_state).is_err() {
                continue;
            }
            let original_text = target
                .decoded_units
                .iter()
                .map(|unit| unit.text.as_str())
                .collect::<String>();
            selected = Some((geometry, original_text));
            break;
        }
        let (geometry, original_text) = selected.expect("replaceable shared Form show");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == geometry.text_show_id)
            .expect("target text show");
        let production_font_probe = env::var_os("ROSETTA_PDF_V3_FORM_SOURCE_HAN").is_some();
        let replacement = if production_font_probe {
            "\u{7532}FORM"
        } else {
            "\u{03a9}FORM"
        };
        let mut font_plan = UnifiedTranslationFontPlan::default();
        font_plan.add_text(replacement);
        let font_path = if production_font_probe {
            env::var("ROSETTA_PDF_V3_FONT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"))
        } else {
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf")
        };
        let prepared = TranslationFontAsset::open_weighted(
            if production_font_probe {
                "SourceHanSansCNRegular"
            } else {
                "ArialRegular"
            },
            TranslationFontWeight::Regular,
            &font_path,
            0,
        )
        .expect("Arial Regular font")
        .prepare(&font_plan)
        .expect("prepared Regular font");
        let request = TextShowReplacementRequest {
            geometry,
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.5,
        };

        let mut invalid_document = document.clone();
        let invalid_root = invalid_document
            .get_object(root_stream_id)
            .and_then(Object::as_stream)
            .expect("root stream");
        let mut invalid_content = Content::decode(
            &invalid_root
                .get_plain_content()
                .expect("plain root content"),
        )
        .expect("decoded root content");
        invalid_content.operations[5] = Operation::new("q", Vec::new());
        let mut invalid_root = invalid_root.clone();
        invalid_root.set_plain_content(invalid_content.encode().expect("invalid root content"));
        invalid_root.compress().expect("compressed invalid root");
        invalid_document
            .objects
            .insert(root_stream_id, Object::Stream(invalid_root));
        let before_invalid = invalid_document.clone();
        let error = apply_single_text_show_replacement(
            &mut invalid_document,
            &page_graph,
            &request,
            &prepared,
        )
        .expect_err("invalid invocation path must reject replacement");
        assert!(matches!(
            error,
            TextShowReplacementError::Patch(ContentPatchError::FormInvocationPathInvalid(_))
        ));
        assert_eq!(invalid_document.max_id, before_invalid.max_id);
        assert_eq!(invalid_document.objects, before_invalid.objects);

        let before_max_id = document.max_id;
        let source_form = document
            .get_object(form_stream_id)
            .cloned()
            .expect("source Form object");
        let result = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            std::slice::from_ref(&request),
            &[&prepared],
        )
        .expect("shared Form replacement");

        assert_eq!(result.form_invocation_depth, 1);
        assert_eq!(result.cloned_stream_count, 2);
        assert!(result.page_content_rewired);
        assert_eq!(result.staged_font_object_count, 6);
        assert_eq!(document.max_id, before_max_id + 8);
        assert_eq!(
            document.get_object(form_stream_id).expect("source Form"),
            &source_form
        );
        assert_ne!(document.get_page_contents(page_id), vec![root_stream_id]);

        let cloned_forms = document
            .objects
            .iter()
            .filter_map(|(object_id, object)| {
                (object_id.0 > before_max_id)
                    .then(|| object.as_stream().ok().map(|stream| (*object_id, stream)))
                    .flatten()
                    .filter(|(_, stream)| {
                        stream
                            .dict
                            .get(b"Subtype")
                            .and_then(Object::as_name)
                            .is_ok_and(|subtype| subtype == b"Form")
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(cloned_forms.len(), 1);
        let cloned_form_resources = cloned_forms[0]
            .1
            .dict
            .get(b"Resources")
            .and_then(Object::as_dict)
            .expect("cloned Form resources");
        let cloned_form_fonts = cloned_form_resources
            .get(b"Font")
            .and_then(Object::as_dict)
            .expect("cloned Form fonts");
        assert!(cloned_form_fonts.has(b"RosettaTranslationRegular"));
        let source_form_resources = source_form
            .as_stream()
            .and_then(|stream| stream.dict.get(b"Resources"))
            .and_then(Object::as_dict)
            .expect("source Form resources");
        assert!(!source_form_resources
            .get(b"Font")
            .and_then(Object::as_dict)
            .is_ok_and(|fonts| fonts.has(b"RosettaTranslationRegular")));

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save Form output");
        if let Ok(path) = env::var("ROSETTA_PDF_V3_FORM_BASELINE_OUTPUT") {
            fs::write(path, &baseline).expect("write Form baseline");
        }
        if let Ok(path) = env::var("ROSETTA_PDF_V3_FORM_OUTPUT") {
            fs::write(path, &output).expect("write Form output");
        }
        if production_font_probe {
            println!(
                "pdf-v3 Form COW replacement fit={:.4} clones={} elapsed={}ms output={}",
                result.replacements[0].fit_scale,
                result.cloned_stream_count,
                result.elapsed_ms,
                output.len()
            );
        }
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium Form output");
        let output_page = output_document.pages().get(0).expect("output page");
        let output_text = output_page.text().expect("output text");
        let output_text_all = output_text.all();
        assert!(output_text_all.contains(replacement));
        assert!(!original_text.is_empty());
        assert!(output_text_all.contains(&original_text));
        let output_characters = output_text.chars();
        let translated_codepoint = replacement.chars().next().expect("translated codepoint");
        let translated_character = output_characters
            .iter()
            .find(|character| character.unicode_char() == Some(translated_codepoint))
            .expect("translated Form character");
        assert!(translated_character
            .font_name()
            .to_ascii_lowercase()
            .contains(if production_font_probe {
                "sourcehansanscnregular"
            } else {
                "arialregular"
            }));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn multi_target_form_replacement_merges_clones_and_reuses_one_font() {
        let _guard = pdfium_test_lock();
        let (mut document, baseline, page_id, root_stream_id, form_stream_id) =
            repeated_decodable_form_document();
        let baseline_pdf = TemporaryPdf::write(&baseline);
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared Form mapping");
        let page_graph = build_reconciled_page_graph(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared Form PageGraph");
        let production_font_probe = env::var_os("ROSETTA_PDF_V3_MULTI_FORM_SOURCE_HAN").is_some();
        let translated = if production_font_probe {
            ["\u{7532}BATCH", "\u{4e59}BATCH"]
        } else {
            ["ALPHA", "BETA"]
        };
        let mut target_requests = Vec::new();
        for (root_operation_index, replacement) in [1usize, 5].into_iter().zip(translated) {
            let mut selected = None;
            for target in mapping.mappings.iter().filter(|mapping| {
                mapping.stream_object_number == form_stream_id.0
                    && mapping.stream_generation == form_stream_id.1
                    && mapping.form_invocation_path.len() == 1
                    && mapping.form_invocation_path[0].operation_index == root_operation_index
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            }) {
                let geometry = TextShowGeometryKey {
                    page_number: 1,
                    text_show_id: target.text_show_id.clone(),
                    form_invocation_path: target.form_invocation_path.clone(),
                    stream_object_number: target.stream_object_number,
                    stream_generation: target.stream_generation,
                    operation_index: target.operation_index,
                    source_font_resource: target
                        .source_font_resource
                        .clone()
                        .expect("source font resource"),
                    source_font_size: target.source_font_size.expect("source font size"),
                    source_horizontal_scaling: target.source_horizontal_scaling,
                };
                let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                    continue;
                };
                let Ok(style_plan) =
                    crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
                else {
                    continue;
                };
                if style_plan.translation_font_weight != TranslationFontWeight::Regular {
                    continue;
                }
                let form_stream = document
                    .get_object(form_stream_id)
                    .and_then(Object::as_stream)
                    .expect("source Form stream");
                let content =
                    Content::decode(&form_stream.get_plain_content().expect("Form content"))
                        .expect("decoded Form content");
                if validate_text_position_boundary(&content, target.operation_index).is_err() {
                    continue;
                }
                let Ok(source_state) = state_before_operation(&content, target.operation_index)
                else {
                    continue;
                };
                if validate_source_style(&style_plan, &source_state).is_err() {
                    continue;
                }
                let text_show = mapping
                    .text_shows
                    .iter()
                    .find(|show| show.text_show_id == target.text_show_id)
                    .expect("target text show");
                selected = Some(TextShowReplacementRequest {
                    geometry,
                    expected_operator: text_show.operator.clone(),
                    expected_operand_hash: text_show.encoded_byte_hash.clone(),
                    translated_text: replacement.to_string(),
                    minimum_fit_scale: 0.5,
                });
                break;
            }
            target_requests.push(TextShowReplacementTargetRequest {
                replacements: vec![selected.expect("replaceable Form target")],
            });
        }

        let mut font_plan = UnifiedTranslationFontPlan::default();
        for replacement in translated {
            font_plan.add_text(replacement);
        }
        let font_path = if production_font_probe {
            env::var("ROSETTA_PDF_V3_FONT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"))
        } else {
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf")
        };
        let prepared = TranslationFontAsset::open_weighted(
            if production_font_probe {
                "SourceHanSansCNRegular"
            } else {
                "ArialRegular"
            },
            TranslationFontWeight::Regular,
            &font_path,
            0,
        )
        .expect("Regular font")
        .prepare(&font_plan)
        .expect("prepared Regular font");

        let mut invalid_document = document.clone();
        let invalid_root = invalid_document
            .get_object(root_stream_id)
            .and_then(Object::as_stream)
            .expect("root stream");
        let mut invalid_content = Content::decode(
            &invalid_root
                .get_plain_content()
                .expect("plain root content"),
        )
        .expect("decoded root content");
        invalid_content.operations[5] = Operation::new("q", Vec::new());
        let mut invalid_root = invalid_root.clone();
        invalid_root.set_plain_content(invalid_content.encode().expect("invalid root content"));
        invalid_root.compress().expect("compressed invalid root");
        invalid_document
            .objects
            .insert(root_stream_id, Object::Stream(invalid_root));
        let before_invalid = invalid_document.clone();
        let error = apply_text_show_replacement_batch(
            &mut invalid_document,
            &page_graph,
            &target_requests,
            &[&prepared],
        )
        .expect_err("invalid second invocation must reject the complete batch");
        assert!(matches!(
            error,
            TextShowReplacementError::Patch(ContentPatchError::FormInvocationPathInvalid(_))
        ));
        assert_eq!(invalid_document.max_id, before_invalid.max_id);
        assert_eq!(invalid_document.objects, before_invalid.objects);

        let before_max_id = document.max_id;
        let source_root = document
            .get_object(root_stream_id)
            .cloned()
            .expect("source root object");
        let source_form = document
            .get_object(form_stream_id)
            .cloned()
            .expect("source Form object");
        let result = apply_text_show_replacement_batch(
            &mut document,
            &page_graph,
            &target_requests,
            &[&prepared],
        )
        .expect("multi-target Form replacement");

        assert_eq!(
            result.schema,
            "rosetta-pdf-v3-text-show-replacement-batch/1"
        );
        assert_eq!(result.target_count, 2);
        assert_eq!(result.replacement_count, 2);
        assert_eq!(
            result.translation_font_weights,
            [TranslationFontWeight::Regular]
        );
        assert_eq!(result.staged_font_object_count, 6);
        assert_eq!(result.cloned_stream_count, 3);
        assert!(result.page_content_rewired);
        assert_eq!(document.max_id, before_max_id + 9);
        assert_eq!(
            document.get_object(root_stream_id).expect("source root"),
            &source_root
        );
        assert_eq!(
            document.get_object(form_stream_id).expect("source Form"),
            &source_form
        );
        assert_ne!(document.get_page_contents(page_id), vec![root_stream_id]);
        assert!(result.targets.iter().all(|target| {
            target.schema == "rosetta-pdf-v3-text-show-replacement-batch-target/1"
                && target.form_invocation_depth == 1
                && target.replacement_count == 1
        }));

        let cloned_form_count = document
            .objects
            .iter()
            .filter(|(object_id, object)| {
                object_id.0 > before_max_id
                    && object.as_stream().is_ok_and(|stream| {
                        stream
                            .dict
                            .get(b"Subtype")
                            .and_then(Object::as_name)
                            .is_ok_and(|subtype| subtype == b"Form")
                    })
            })
            .count();
        assert_eq!(cloned_form_count, 2);

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save multi-target Form output");
        if let Ok(path) = env::var("ROSETTA_PDF_V3_MULTI_FORM_BASELINE_OUTPUT") {
            fs::write(path, &baseline).expect("write multi-Form baseline");
        }
        if let Ok(path) = env::var("ROSETTA_PDF_V3_MULTI_FORM_OUTPUT") {
            fs::write(path, &output).expect("write multi-Form output");
        }
        if production_font_probe {
            println!(
                "pdf-v3 multi-Form replacement targets={} clones={} elapsed={}ms output={}",
                result.target_count,
                result.cloned_stream_count,
                result.elapsed_ms,
                output.len()
            );
        }
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium multi-Form output");
        let output_text = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        for replacement in translated {
            assert!(output_text.contains(replacement));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn mixed_form_and_top_level_batch_rewires_both_roots_atomically() {
        let _guard = pdfium_test_lock();
        let (mut document, baseline, page_id, root_stream_id, form_stream_id, top_level_stream_id) =
            mixed_form_and_top_level_document();
        let baseline_pdf = TemporaryPdf::write(&baseline);
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("mixed Form/top-level mapping");
        let page_graph = build_reconciled_page_graph(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("mixed Form/top-level PageGraph");

        let request_for = |target: &crate::pdf_v3::mapping::TextObjectOperandMapping,
                           replacement: &str|
         -> Option<TextShowReplacementRequest> {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target.source_font_resource.clone()?,
                source_font_size: target.source_font_size?,
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let fit_bounds = derive_text_show_fit_bounds(&page_graph, &geometry).ok()?;
            let style_plan =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
                    .ok()?;
            if style_plan.translation_font_weight != TranslationFontWeight::Regular {
                return None;
            }
            let stream = document
                .get_object((target.stream_object_number, target.stream_generation))
                .and_then(Object::as_stream)
                .ok()?;
            let content = Content::decode(&stream.get_plain_content().ok()?).ok()?;
            validate_text_position_boundary(&content, target.operation_index).ok()?;
            let source_state = state_before_operation(&content, target.operation_index).ok()?;
            validate_source_style(&style_plan, &source_state).ok()?;
            let text_show = mapping
                .text_shows
                .iter()
                .find(|show| show.text_show_id == target.text_show_id)?;
            Some(TextShowReplacementRequest {
                geometry,
                expected_operator: text_show.operator.clone(),
                expected_operand_hash: text_show.encoded_byte_hash.clone(),
                translated_text: replacement.to_string(),
                minimum_fit_scale: 0.5,
            })
        };

        let (form_request, preserved_form_text) = mapping
            .mappings
            .iter()
            .filter(|target| {
                target.stream_object_number == form_stream_id.0
                    && target.stream_generation == form_stream_id.1
                    && target.form_invocation_path.len() == 1
                    && target.form_invocation_path[0].operation_index == 1
            })
            .find_map(|target| {
                request_for(target, "FORMX").map(|request| {
                    let source_text = target
                        .decoded_units
                        .iter()
                        .map(|unit| unit.text.as_str())
                        .collect::<String>();
                    (request, source_text)
                })
            })
            .expect("replaceable Form target");
        let top_level_request = mapping
            .mappings
            .iter()
            .filter(|target| {
                target.stream_object_number == top_level_stream_id.0
                    && target.stream_generation == top_level_stream_id.1
                    && target.form_invocation_path.is_empty()
            })
            .find_map(|target| request_for(target, "TOPX"))
            .expect("replaceable top-level target");
        assert!(!preserved_form_text.is_empty());

        let target_requests = vec![
            TextShowReplacementTargetRequest {
                replacements: vec![form_request],
            },
            TextShowReplacementTargetRequest {
                replacements: vec![top_level_request],
            },
        ];
        let mut font_plan = UnifiedTranslationFontPlan::default();
        font_plan.add_text("FORMX");
        font_plan.add_text("TOPX");
        let prepared = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            &PathBuf::from(r"C:\Windows\Fonts\arial.ttf"),
            0,
        )
        .expect("Arial Regular font")
        .prepare(&font_plan)
        .expect("prepared Regular font");

        let mut invalid_document = document.clone();
        let before_invalid = invalid_document.clone();
        let mut invalid_targets = target_requests.clone();
        invalid_targets[1].replacements[0].expected_operand_hash = "invalid".to_string();
        apply_text_show_replacement_batch(
            &mut invalid_document,
            &page_graph,
            &invalid_targets,
            &[&prepared],
        )
        .expect_err("invalid top-level target must reject the complete batch");
        assert_eq!(invalid_document.max_id, before_invalid.max_id);
        assert_eq!(invalid_document.objects, before_invalid.objects);

        let before_max_id = document.max_id;
        let source_root = document
            .get_object(root_stream_id)
            .cloned()
            .expect("source root object");
        let source_form = document
            .get_object(form_stream_id)
            .cloned()
            .expect("source Form object");
        let source_top_level = document
            .get_object(top_level_stream_id)
            .cloned()
            .expect("source top-level object");
        let result = apply_text_show_replacement_batch(
            &mut document,
            &page_graph,
            &target_requests,
            &[&prepared],
        )
        .expect("mixed Form/top-level replacement");

        assert_eq!(result.target_count, 2);
        assert_eq!(result.replacement_count, 2);
        assert_eq!(result.staged_font_object_count, 6);
        assert_eq!(result.cloned_stream_count, 3);
        assert!(result.page_content_rewired);
        assert_eq!(document.max_id, before_max_id + 9);
        assert_eq!(
            document.get_object(root_stream_id).expect("source root"),
            &source_root
        );
        assert_eq!(
            document.get_object(form_stream_id).expect("source Form"),
            &source_form
        );
        assert_eq!(
            document
                .get_object(top_level_stream_id)
                .expect("source top-level"),
            &source_top_level
        );
        let page_contents = document.get_page_contents(page_id);
        assert_eq!(page_contents.len(), 2);
        assert!(!page_contents.contains(&root_stream_id));
        assert!(!page_contents.contains(&top_level_stream_id));
        assert_eq!(
            result
                .targets
                .iter()
                .map(|target| target.form_invocation_depth)
                .collect::<std::collections::BTreeSet<_>>(),
            [0, 1].into_iter().collect()
        );

        let cloned_form = document
            .objects
            .iter()
            .filter_map(|(object_id, object)| {
                (object_id.0 > before_max_id)
                    .then(|| object.as_stream().ok())
                    .flatten()
                    .filter(|stream| {
                        stream
                            .dict
                            .get(b"Subtype")
                            .and_then(Object::as_name)
                            .is_ok_and(|subtype| subtype == b"Form")
                    })
            })
            .next()
            .expect("cloned Form stream");
        assert!(cloned_form
            .dict
            .get(b"Resources")
            .and_then(Object::as_dict)
            .and_then(|resources| resources.get(b"Font"))
            .and_then(Object::as_dict)
            .is_ok_and(|fonts| fonts.has(b"RosettaTranslationRegular")));
        assert!(document
            .get_dictionary(page_id)
            .and_then(|page| page.get(b"Resources"))
            .and_then(Object::as_dict)
            .and_then(|resources| resources.get(b"Font"))
            .and_then(Object::as_dict)
            .is_ok_and(|fonts| fonts.has(b"RosettaTranslationRegular")));

        let diagnostics = serde_json::to_string(&result).expect("serialized diagnostics");
        assert!(!diagnostics.contains("FORMX"));
        assert!(!diagnostics.contains("TOPX"));
        assert!(!diagnostics.contains(&preserved_form_text));

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save mixed Form/top-level output");
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium mixed Form/top-level output");
        let output_text = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(output_text.contains("FORMX"));
        assert!(output_text.contains("TOPX"));
        assert!(output_text.contains(&preserved_form_text));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cross_page_shared_stream_replacement_rewires_only_the_selected_page() {
        let _guard = pdfium_test_lock();
        let (mut document, baseline, first_page_id, second_page_id) =
            cross_page_shared_content_document();
        let baseline_pdf = TemporaryPdf::write(&baseline);
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared page-stream mapping");
        let page_graph = build_reconciled_page_graph(shared_pdfium(), baseline_pdf.path(), 1)
            .expect("shared page-stream PageGraph");
        let mut selected = None;
        for target in mapping.mappings.iter().filter(|mapping| {
            mapping.form_invocation_path.is_empty()
                && mapping.source_font_resource.is_some()
                && mapping.source_font_size.is_some()
        }) {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: Vec::new(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                continue;
            };
            let Ok(style_plan) =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
            else {
                continue;
            };
            if style_plan.translation_font_weight != TranslationFontWeight::Regular {
                continue;
            }
            let stream_id = (target.stream_object_number, target.stream_generation);
            let stream = document
                .get_object(stream_id)
                .and_then(Object::as_stream)
                .expect("shared page stream");
            let content = Content::decode(&stream.get_plain_content().expect("page content"))
                .expect("decoded page content");
            if validate_text_position_boundary(&content, target.operation_index).is_err() {
                continue;
            }
            let Ok(source_state) = state_before_operation(&content, target.operation_index) else {
                continue;
            };
            if validate_source_style(&style_plan, &source_state).is_err() {
                continue;
            }
            let original_text = target
                .decoded_units
                .iter()
                .map(|unit| unit.text.as_str())
                .collect::<String>();
            selected = Some((geometry, original_text));
            break;
        }
        let (geometry, original_text) = selected.expect("replaceable shared page-stream show");
        let source_stream_id = (geometry.stream_object_number, geometry.stream_generation);
        assert!(document
            .get_page_contents(first_page_id)
            .contains(&source_stream_id));
        assert!(document
            .get_page_contents(second_page_id)
            .contains(&source_stream_id));
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == geometry.text_show_id)
            .expect("target text show");
        let replacement = "\u{03a9}PAGE";
        let mut font_plan = UnifiedTranslationFontPlan::default();
        font_plan.add_text(replacement);
        let prepared = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial Regular font")
        .prepare(&font_plan)
        .expect("prepared Regular font");
        let request = TextShowReplacementRequest {
            geometry,
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.5,
        };
        let before_max_id = document.max_id;
        let source_stream = document
            .get_object(source_stream_id)
            .cloned()
            .expect("source shared stream");

        let result = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            std::slice::from_ref(&request),
            &[&prepared],
        )
        .expect("shared page-stream replacement");

        assert_eq!(result.form_invocation_depth, 0);
        assert_eq!(result.cloned_stream_count, 1);
        assert!(result.page_content_rewired);
        assert_eq!(result.staged_font_object_count, 6);
        assert_eq!(document.max_id, before_max_id + 7);
        assert_eq!(
            document
                .get_object(source_stream_id)
                .expect("source shared stream"),
            &source_stream
        );
        assert!(!document
            .get_page_contents(first_page_id)
            .contains(&source_stream_id));
        assert!(document
            .get_page_contents(second_page_id)
            .contains(&source_stream_id));

        let mut output = Vec::new();
        document
            .save_to(&mut output)
            .expect("save shared page-stream output");
        if let Ok(path) = env::var("ROSETTA_PDF_V3_SHARED_PAGE_BASELINE_OUTPUT") {
            fs::write(path, &baseline).expect("write shared page baseline");
        }
        if let Ok(path) = env::var("ROSETTA_PDF_V3_SHARED_PAGE_OUTPUT") {
            fs::write(path, &output).expect("write shared page output");
        }
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium shared page-stream output");
        let first_output_page = output_document.pages().get(0).expect("first output page");
        let first_output_text = first_output_page.text().expect("first output text").all();
        let second_output_page = output_document.pages().get(1).expect("second output page");
        let second_output_text = second_output_page.text().expect("second output text").all();
        assert!(first_output_text.contains(replacement));
        assert!(!original_text.is_empty());
        assert!(second_output_text.contains(&original_text));
        assert!(!second_output_text.contains(replacement));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn overflow_rejection_leaves_document_unchanged() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let replacement =
            "This replacement is deliberately much too wide for the allowed region ".repeat(10);
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(&replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let before = document.clone();
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement,
            minimum_fit_scale: 0.9,
        };
        let error =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect_err("overflow must preserve source");
        assert!(matches!(error, TextShowReplacementError::Overflow { .. }));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stale_page_graph_rejection_leaves_document_unchanged() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let mut page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        page_graph.schema_version += 1;
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let replacement = "Unified replacement";
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let before = document.clone();
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.9,
        };

        let error =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect_err("stale PageGraph must preserve source");

        assert!(matches!(error, TextShowReplacementError::FitBounds(_)));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wrong_translation_font_weight_leaves_document_unchanged() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let mut page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let style_id = page_graph
            .atoms
            .iter()
            .find(|atom| {
                atom.source_provenance
                    .as_ref()
                    .is_some_and(|provenance| provenance.text_show_id == target.text_show_id)
            })
            .and_then(|atom| atom.style_id.clone())
            .expect("target style");
        page_graph
            .styles
            .iter_mut()
            .find(|style| style.style_id == style_id)
            .expect("target PageStyle")
            .font_weight = Some(700);
        let replacement = "Unified replacement";
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let before = document.clone();
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.9,
        };

        let error =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect_err("regular font must not replace bold source text");

        assert!(matches!(
            error,
            TextShowReplacementError::MissingTranslationFontFace(TranslationFontWeight::Bold)
        ));
        assert_eq!(document.max_id, before.max_id);
        assert_eq!(document.objects, before.objects);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn device_rgb_fill_is_validated_and_inherited_by_replacement() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let mut page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let target_object_id = target.source_object_id.clone();
        let source_style_id = page_graph
            .atoms
            .iter()
            .find(|atom| atom.source_object_id.as_deref() == Some(&target_object_id))
            .and_then(|atom| atom.style_id.clone())
            .expect("source style");
        let mut colored_style = page_graph
            .styles
            .iter()
            .find(|style| style.style_id == source_style_id)
            .cloned()
            .expect("source PageStyle");
        colored_style.style_id = "style-device-rgb-probe".to_string();
        colored_style.fill_color = Some([0.8, 0.1, 0.2, 1.0]);
        colored_style.opacity = Some(1.0);
        page_graph.styles.push(colored_style);
        for atom in page_graph
            .atoms
            .iter_mut()
            .filter(|atom| atom.source_object_id.as_deref() == Some(&target_object_id))
        {
            atom.style_id = Some("style-device-rgb-probe".to_string());
            if let Some(provenance) = atom.source_provenance.as_mut() {
                provenance.operation_index += 1;
            }
        }

        let replacement = "ColorProbe";
        let asset = TranslationFontAsset::open(
            "ArialRegular",
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let stream_id = (target.stream_object_number, target.stream_generation);
        let source_stream = document
            .get_object(stream_id)
            .and_then(Object::as_stream)
            .expect("source stream");
        let mut content =
            Content::decode(&source_stream.get_plain_content().expect("plain content"))
                .expect("decoded content");
        content.operations.insert(
            target.operation_index,
            Operation::new(
                "rg",
                vec![Object::Real(0.8), Object::Real(0.1), Object::Real(0.2)],
            ),
        );
        let mut colored_stream = source_stream.clone();
        colored_stream.set_plain_content(content.encode().expect("encoded colored content"));
        colored_stream
            .compress()
            .expect("compressed colored stream");
        document
            .objects
            .insert(stream_id, Object::Stream(colored_stream));
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index + 1,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.9,
        };

        let result =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect("colored replacement");

        assert_eq!(result.source_fill_color, [0.8, 0.1, 0.2, 1.0]);
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save colored output");
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_page = output_document.pages().get(0).expect("output page");
        let output_text = output_page.text().expect("output text");
        let output_characters = output_text.chars();
        let translated_character = output_characters
            .iter()
            .find(|character| character.unicode_char() == Some('C'))
            .expect("translated character");
        let color = translated_character
            .fill_color()
            .expect("translated fill color");
        assert!((i16::from(color.red()) - 204).abs() <= 1);
        assert!((i16::from(color.green()) - 26).abs() <= 1);
        assert!((i16::from(color.blue()) - 51).abs() <= 1);
        assert_eq!(color.alpha(), 255);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows Source Han single-object replacement probe"]
    fn manual_windows_source_han_single_object_replacement_probe() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let target = mapping
            .mappings
            .iter()
            .find(|mapping| {
                mapping.form_invocation_path.is_empty()
                    && mapping.source_font_resource.is_some()
                    && mapping.source_font_size.is_some()
            })
            .expect("replaceable mapping");
        let text_show = mapping
            .text_shows
            .iter()
            .find(|show| show.text_show_id == target.text_show_id)
            .expect("target text show");
        let replacement = "统一字体安全回填";
        let font_path = env::var("ROSETTA_PDF_V3_FONT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"));
        let asset = TranslationFontAsset::open("SourceHanSansCNRegular", &font_path, 0)
            .expect("Source Han font");
        let mut plan = UnifiedTranslationFontPlan::default();
        plan.add_text(replacement);
        let prepared = asset.prepare(&plan).expect("prepared font");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let request = TextShowReplacementRequest {
            geometry: TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            },
            expected_operator: text_show.operator.clone(),
            expected_operand_hash: text_show.encoded_byte_hash.clone(),
            translated_text: replacement.to_string(),
            minimum_fit_scale: 0.9,
        };
        let result =
            apply_single_text_show_replacement(&mut document, &page_graph, &request, &prepared)
                .expect("CJK replacement");
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save output");
        let pdfium = shared_pdfium();
        let output_document = pdfium
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_text = output_document
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(output_text.contains(replacement));
        println!(
            "pdf-v3 single replacement fit={:.4} max={:.2} page={:.2} matrix_scale={:.4} natural={:.2} fitted={:.2} elapsed={}ms output={}",
            result.fit_scale,
            result.max_advance,
            result.page_advance,
            result.baseline_scale,
            result.natural_advance,
            result.fitted_advance,
            result.elapsed_ms,
            output.len()
        );
        if let Ok(path) = env::var("ROSETTA_PDF_V3_REPLACEMENT_OUTPUT") {
            fs::write(path, &output).expect("write replacement output");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows Source Han multi-show Bold replacement probe"]
    fn manual_windows_source_han_multi_show_bold_probe() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("2305.13048v2.pdf");
        let mapping = map_page_atoms_to_content_operands(shared_pdfium(), &source_path, 1)
            .expect("source mapping");
        let page_graph =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let translated = ["甲", "乙"];
        let mut selected_text_object = None;
        let mut requests = Vec::new();

        for target in mapping.mappings.iter().filter(|mapping| {
            mapping.form_invocation_path.is_empty()
                && mapping.source_font_resource.is_some()
                && mapping.source_font_size.is_some()
        }) {
            let geometry = TextShowGeometryKey {
                page_number: 1,
                text_show_id: target.text_show_id.clone(),
                form_invocation_path: target.form_invocation_path.clone(),
                stream_object_number: target.stream_object_number,
                stream_generation: target.stream_generation,
                operation_index: target.operation_index,
                source_font_resource: target
                    .source_font_resource
                    .clone()
                    .expect("source font resource"),
                source_font_size: target.source_font_size.expect("source font size"),
                source_horizontal_scaling: target.source_horizontal_scaling,
            };
            let Ok(fit_bounds) = derive_text_show_fit_bounds(&page_graph, &geometry) else {
                continue;
            };
            let Ok(style_plan) =
                crate::pdf_v3::style::plan_text_show_style(&page_graph, &fit_bounds.style_id)
            else {
                continue;
            };
            if style_plan.translation_font_weight != TranslationFontWeight::Bold {
                continue;
            }
            let stream = document
                .get_object((target.stream_object_number, target.stream_generation))
                .and_then(Object::as_stream)
                .expect("source stream");
            let content = Content::decode(&stream.get_plain_content().expect("plain content"))
                .expect("decoded content");
            if validate_text_position_boundary(&content, target.operation_index).is_err() {
                continue;
            }
            let Ok(source_state) = state_before_operation(&content, target.operation_index) else {
                continue;
            };
            if validate_source_style(&style_plan, &source_state).is_err() {
                continue;
            }
            let Ok(text_object_bounds) = find_text_object_bounds(&content, target.operation_index)
            else {
                continue;
            };
            let text_object_key = (
                target.stream_object_number,
                target.stream_generation,
                text_object_bounds,
            );
            match selected_text_object {
                Some(expected) if expected != text_object_key => continue,
                None => selected_text_object = Some(text_object_key),
                _ => {}
            }
            let text_show = mapping
                .text_shows
                .iter()
                .find(|show| show.text_show_id == target.text_show_id)
                .expect("target text show");
            requests.push(TextShowReplacementRequest {
                geometry,
                expected_operator: text_show.operator.clone(),
                expected_operand_hash: text_show.encoded_byte_hash.clone(),
                translated_text: translated[requests.len()].to_string(),
                minimum_fit_scale: 0.5,
            });
            if requests.len() == translated.len() {
                break;
            }
        }
        assert_eq!(requests.len(), translated.len());

        let font_path = env::var("ROSETTA_PDF_V3_BOLD_FONT_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Bold.ttf"));
        let asset = TranslationFontAsset::open_weighted(
            "SourceHanSansCNBold",
            TranslationFontWeight::Bold,
            &font_path,
            0,
        )
        .expect("Source Han bold font");
        let mut plan = UnifiedTranslationFontPlan::default();
        for translated_text in translated {
            plan.add_text(translated_text);
        }
        let prepared = asset.prepare(&plan).expect("prepared bold font");
        let result = apply_text_show_replacement_transaction(
            &mut document,
            &page_graph,
            &requests,
            &[&prepared],
        )
        .expect("multi-show Bold replacement");
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save bold output");
        let output_document = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output");
        let output_page = output_document.pages().get(0).expect("output page");
        let output_text = output_page.text().expect("output text");
        let output_text_all = output_text.all();
        assert!(translated
            .iter()
            .all(|translated_text| output_text_all.contains(translated_text)));
        let output_characters = output_text.chars();
        for (translated_text, replacement_result) in translated.iter().zip(&result.replacements) {
            let translated_character = output_characters
                .iter()
                .find(|character| character.unicode_char() == translated_text.chars().next())
                .expect("translated bold character");
            assert!(translated_character
                .font_name()
                .to_ascii_lowercase()
                .contains("sourcehansanscnbold"));
            let color = translated_character
                .fill_color()
                .expect("translated fill color");
            let actual_color = [
                f32::from(color.red()) / 255.0,
                f32::from(color.green()) / 255.0,
                f32::from(color.blue()) / 255.0,
                f32::from(color.alpha()) / 255.0,
            ];
            assert!(actual_color
                .into_iter()
                .zip(replacement_result.source_fill_color)
                .all(|(actual, expected)| (actual - expected).abs() <= 1.0 / 255.0 + 0.0001));
        }
        assert_eq!(
            result.translation_font_weights,
            vec![TranslationFontWeight::Bold]
        );
        println!(
            "pdf-v3 multi-show Bold replacement count={} fits={:?} colors={:?} elapsed={}ms output={}",
            result.replacement_count,
            result
                .replacements
                .iter()
                .map(|replacement| replacement.fit_scale)
                .collect::<Vec<_>>(),
            result
                .replacements
                .iter()
                .map(|replacement| replacement.source_fill_color)
                .collect::<Vec<_>>(),
            result.elapsed_ms,
            output.len()
        );
        if let Ok(path) = env::var("ROSETTA_PDF_V3_BOLD_REPLACEMENT_OUTPUT") {
            fs::write(path, &output).expect("write bold replacement output");
        }
    }
}
