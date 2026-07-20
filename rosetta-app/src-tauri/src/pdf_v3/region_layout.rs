use std::{collections::BTreeMap, fmt};

use super::{
    font::{PreparedTranslationFont, TranslationFontWeight},
    region_translation_patch::{
        validate_region_translation_patch, RegionTranslationPatch, RegionTranslationPatchError,
        RegionTranslationPatchParagraph, RegionTranslationPatchRendererDecision,
    },
    types::{PageAtom, PageGraph, PageGroupKind, PageStyle},
};

const DEFAULT_MINIMUM_FIT_SCALE_MILLI: u16 = 600;
const DEFAULT_FIT_STEP_MILLI: u16 = 25;
const DEFAULT_LINE_HEIGHT_MILLI: u16 = 1_220;
const DEFAULT_PARAGRAPH_GAP_MILLI: u16 = 350;
const DEFAULT_HORIZONTAL_PADDING: f32 = 0.5;
const DEFAULT_VERTICAL_PADDING: f32 = 0.5;
const MIN_FONT_SIZE: f32 = 4.0;
const MAX_FONT_SIZE: f32 = 96.0;
const DECORATIVE_CONTAINER_MAX_CHARACTERS: usize = 96;
const DECORATIVE_CONTAINER_MIN_FONT_RATIO: f32 = 2.0;
const DECORATIVE_CONTAINER_MIN_COLOR_DISTANCE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionLayoutPolicy {
    pub minimum_fit_scale_milli: u16,
    pub fit_step_milli: u16,
    pub line_height_milli: u16,
    pub paragraph_gap_milli: u16,
    pub horizontal_padding: f32,
    pub vertical_padding: f32,
}

impl Default for RegionLayoutPolicy {
    fn default() -> Self {
        Self {
            minimum_fit_scale_milli: DEFAULT_MINIMUM_FIT_SCALE_MILLI,
            fit_step_milli: DEFAULT_FIT_STEP_MILLI,
            line_height_milli: DEFAULT_LINE_HEIGHT_MILLI,
            paragraph_gap_milli: DEFAULT_PARAGRAPH_GAP_MILLI,
            horizontal_padding: DEFAULT_HORIZONTAL_PADDING,
            vertical_padding: DEFAULT_VERTICAL_PADDING,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RegionLayoutBatch {
    pub layouts: Vec<FlowContainerLayout>,
    pub decisions: BTreeMap<String, RegionTranslationPatchRendererDecision>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FlowContainerLayout {
    pub container_id: String,
    pub flow_container_group_id: String,
    pub bounds: [f32; 4],
    pub fit_scale: f32,
    pub lines: Vec<FlowLayoutLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FlowLayoutLine {
    pub paragraph_group_id: String,
    pub text: String,
    pub x: f32,
    pub baseline_y: f32,
    pub font_size: f32,
    pub font_weight: TranslationFontWeight,
    pub fill_color: [f32; 4],
    pub opacity: f32,
}

pub(crate) trait RegionFontMetrics {
    fn supports(&self, weight: TranslationFontWeight) -> bool;

    fn text_advance_1000(&self, weight: TranslationFontWeight, text: &str) -> Result<i64, String>;
}

pub(crate) struct PreparedRegionFontMetrics<'a> {
    regular: &'a PreparedTranslationFont,
    bold: Option<&'a PreparedTranslationFont>,
}

impl<'a> PreparedRegionFontMetrics<'a> {
    pub(crate) fn new(
        regular: &'a PreparedTranslationFont,
        bold: Option<&'a PreparedTranslationFont>,
    ) -> Self {
        Self { regular, bold }
    }

    fn font(&self, weight: TranslationFontWeight) -> &PreparedTranslationFont {
        match weight {
            TranslationFontWeight::Regular => self.regular,
            TranslationFontWeight::Bold => self.bold.unwrap_or(self.regular),
        }
    }
}

impl RegionFontMetrics for PreparedRegionFontMetrics<'_> {
    fn supports(&self, weight: TranslationFontWeight) -> bool {
        match weight {
            TranslationFontWeight::Regular => true,
            TranslationFontWeight::Bold => self.bold.is_some(),
        }
    }

    fn text_advance_1000(&self, weight: TranslationFontWeight, text: &str) -> Result<i64, String> {
        self.font(weight)
            .text_advance_1000(text)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegionLayoutError {
    InvalidPolicy,
    Patch(RegionTranslationPatchError),
    MissingGroup(String),
    MissingAtom(String),
    MissingStyle(String),
    InvalidGeometry(String),
    Font(String),
    PersistedDecisionMismatch(String),
}

impl fmt::Display for RegionLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("region layout policy is invalid"),
            Self::Patch(error) => error.fmt(formatter),
            Self::MissingGroup(group_id) => write!(formatter, "missing PageGraph group {group_id}"),
            Self::MissingAtom(atom_id) => write!(formatter, "missing PageGraph atom {atom_id}"),
            Self::MissingStyle(style_id) => write!(formatter, "missing PageGraph style {style_id}"),
            Self::InvalidGeometry(group_id) => {
                write!(formatter, "PageGraph group {group_id} has invalid geometry")
            }
            Self::Font(message) => formatter.write_str(message),
            Self::PersistedDecisionMismatch(container_id) => write!(
                formatter,
                "persisted region renderer decision changed for {container_id}"
            ),
        }
    }
}

impl std::error::Error for RegionLayoutError {}

impl From<RegionTranslationPatchError> for RegionLayoutError {
    fn from(value: RegionTranslationPatchError) -> Self {
        Self::Patch(value)
    }
}

pub(crate) fn layout_region_translation_patch(
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    metrics: &dyn RegionFontMetrics,
    policy: RegionLayoutPolicy,
) -> Result<RegionLayoutBatch, RegionLayoutError> {
    validate_policy(policy)?;
    validate_region_translation_patch(page, patch)?;
    let groups_by_id = page
        .groups
        .iter()
        .map(|group| (group.group_id.as_str(), group))
        .collect::<BTreeMap<_, _>>();
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let styles_by_id = page
        .styles
        .iter()
        .map(|style| (style.style_id.as_str(), style))
        .collect::<BTreeMap<_, _>>();
    let mut layouts = Vec::new();
    let mut decisions = BTreeMap::new();

    for container in &patch.containers {
        if matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { .. }
        ) {
            continue;
        }
        let group = groups_by_id
            .get(container.flow_container_group_id.as_str())
            .copied()
            .filter(|group| group.kind == PageGroupKind::FlowContainer)
            .ok_or_else(|| {
                RegionLayoutError::MissingGroup(container.flow_container_group_id.clone())
            })?;
        let preserve_decorative = should_preserve_decorative_container(
            &container.paragraphs,
            &atoms_by_id,
            &styles_by_id,
        )?;
        let (layout, preservation_reason) = if preserve_decorative {
            (None, "region-decorative-mixed-scale")
        } else {
            match layout_container(
                container.container_id.as_str(),
                group.bounds,
                &container.paragraphs,
                &groups_by_id,
                &atoms_by_id,
                &styles_by_id,
                metrics,
                policy,
            ) {
                Ok(layout) => (layout, "region-layout-overflow"),
                Err(RegionLayoutError::Font(_)) => (None, "region-font-glyph-unavailable"),
                Err(error) => return Err(error),
            }
        };
        let next_decision = match &layout {
            Some(layout) => RegionTranslationPatchRendererDecision::Reflowed {
                line_count: u32::try_from(layout.lines.len()).unwrap_or(u32::MAX),
                fit_scale: layout.fit_scale,
            },
            None => RegionTranslationPatchRendererDecision::Preserved {
                reason_code: preservation_reason.to_string(),
            },
        };
        match &container.renderer_decision {
            RegionTranslationPatchRendererDecision::Pending => {
                decisions.insert(container.container_id.clone(), next_decision);
            }
            persisted @ RegionTranslationPatchRendererDecision::Reflowed { .. } => {
                if persisted != &next_decision {
                    return Err(RegionLayoutError::PersistedDecisionMismatch(
                        container.container_id.clone(),
                    ));
                }
            }
            RegionTranslationPatchRendererDecision::Preserved { .. } => unreachable!(),
        }
        if let Some(layout) = layout {
            layouts.push(FlowContainerLayout {
                container_id: container.container_id.clone(),
                flow_container_group_id: container.flow_container_group_id.clone(),
                ..layout
            });
        }
    }

    Ok(RegionLayoutBatch { layouts, decisions })
}

#[allow(clippy::too_many_arguments)]
fn layout_container(
    container_id: &str,
    bounds: [f32; 4],
    paragraphs: &[RegionTranslationPatchParagraph],
    groups_by_id: &BTreeMap<&str, &super::types::PageGroup>,
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
    styles_by_id: &BTreeMap<&str, &PageStyle>,
    metrics: &dyn RegionFontMetrics,
    policy: RegionLayoutPolicy,
) -> Result<Option<FlowContainerLayout>, RegionLayoutError> {
    let usable_bounds = [
        bounds[0] + policy.horizontal_padding,
        bounds[1] + policy.vertical_padding,
        bounds[2] - policy.horizontal_padding,
        bounds[3] - policy.vertical_padding,
    ];
    if usable_bounds.iter().any(|value| !value.is_finite())
        || usable_bounds[2] <= usable_bounds[0]
        || usable_bounds[3] <= usable_bounds[1]
    {
        return Err(RegionLayoutError::InvalidGeometry(container_id.to_string()));
    }
    let paragraph_inputs = paragraphs
        .iter()
        .map(|paragraph| {
            paragraph_input(paragraph, groups_by_id, atoms_by_id, styles_by_id, metrics)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if paragraph_inputs.is_empty() {
        return Ok(None);
    }

    let mut scale_milli = 1_000u16;
    loop {
        let fit_scale = f32::from(scale_milli) / 1_000.0;
        if let Some(lines) =
            try_layout_at_scale(&paragraph_inputs, usable_bounds, fit_scale, metrics, policy)?
        {
            return Ok(Some(FlowContainerLayout {
                container_id: String::new(),
                flow_container_group_id: String::new(),
                bounds,
                fit_scale,
                lines,
            }));
        }
        if scale_milli <= policy.minimum_fit_scale_milli {
            return Ok(None);
        }
        scale_milli = scale_milli
            .saturating_sub(policy.fit_step_milli)
            .max(policy.minimum_fit_scale_milli);
    }
}

#[derive(Debug, Clone)]
struct ParagraphLayoutInput {
    paragraph_group_id: String,
    text: String,
    source_font_size: f32,
    font_weight: TranslationFontWeight,
    fill_color: [f32; 4],
    opacity: f32,
}

fn should_preserve_decorative_container(
    paragraphs: &[RegionTranslationPatchParagraph],
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
    styles_by_id: &BTreeMap<&str, &PageStyle>,
) -> Result<bool, RegionLayoutError> {
    if paragraphs.len() < 2 {
        return Ok(false);
    }
    let character_count = paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.atoms.iter())
        .map(|atom_ref| {
            atoms_by_id
                .get(atom_ref.atom_id.as_str())
                .copied()
                .ok_or_else(|| RegionLayoutError::MissingAtom(atom_ref.atom_id.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|atom| {
            atom.source_text
                .chars()
                .filter(|character| !character.is_whitespace())
                .count()
        })
        .sum::<usize>();
    if character_count > DECORATIVE_CONTAINER_MAX_CHARACTERS {
        return Ok(false);
    }

    let styles = paragraphs
        .iter()
        .map(|paragraph| dominant_paragraph_style(paragraph, atoms_by_id, styles_by_id))
        .collect::<Result<Vec<_>, _>>()?;
    let minimum_size = styles
        .iter()
        .map(|style| style.scaled_font_size.abs())
        .fold(f32::INFINITY, f32::min);
    let maximum_size = styles
        .iter()
        .map(|style| style.scaled_font_size.abs())
        .fold(0.0f32, f32::max);
    if !minimum_size.is_finite()
        || !maximum_size.is_finite()
        || minimum_size <= 0.0
        || maximum_size / minimum_size < DECORATIVE_CONTAINER_MIN_FONT_RATIO
    {
        return Ok(false);
    }

    let colors = styles
        .iter()
        .map(|style| style.fill_color.unwrap_or([0.0, 0.0, 0.0, 1.0]))
        .collect::<Vec<_>>();
    Ok(colors.iter().enumerate().any(|(index, left)| {
        colors.iter().skip(index + 1).any(|right| {
            left[..3]
                .iter()
                .zip(&right[..3])
                .map(|(left, right)| (left - right).abs())
                .sum::<f32>()
                >= DECORATIVE_CONTAINER_MIN_COLOR_DISTANCE
        })
    }))
}

fn dominant_paragraph_style<'a>(
    paragraph: &RegionTranslationPatchParagraph,
    atoms_by_id: &BTreeMap<&str, &'a PageAtom>,
    styles_by_id: &BTreeMap<&str, &'a PageStyle>,
) -> Result<&'a PageStyle, RegionLayoutError> {
    let mut style_weights = BTreeMap::<&str, usize>::new();
    for atom_ref in &paragraph.atoms {
        let atom = atoms_by_id
            .get(atom_ref.atom_id.as_str())
            .copied()
            .ok_or_else(|| RegionLayoutError::MissingAtom(atom_ref.atom_id.clone()))?;
        let Some(style_id) = atom.style_id.as_deref() else {
            continue;
        };
        let weight = atom
            .source_text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count()
            .max(1);
        *style_weights.entry(style_id).or_default() += weight;
    }
    let style_id = style_weights
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(style_id, _)| style_id)
        .ok_or_else(|| RegionLayoutError::MissingStyle(paragraph.paragraph_group_id.clone()))?;
    styles_by_id
        .get(style_id)
        .copied()
        .ok_or_else(|| RegionLayoutError::MissingStyle(style_id.to_string()))
}

fn paragraph_input(
    paragraph: &RegionTranslationPatchParagraph,
    groups_by_id: &BTreeMap<&str, &super::types::PageGroup>,
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
    styles_by_id: &BTreeMap<&str, &PageStyle>,
    metrics: &dyn RegionFontMetrics,
) -> Result<ParagraphLayoutInput, RegionLayoutError> {
    let group = groups_by_id
        .get(paragraph.paragraph_group_id.as_str())
        .copied()
        .filter(|group| group.kind == PageGroupKind::Paragraph)
        .ok_or_else(|| RegionLayoutError::MissingGroup(paragraph.paragraph_group_id.clone()))?;
    if group.bounds.iter().any(|value| !value.is_finite()) {
        return Err(RegionLayoutError::InvalidGeometry(group.group_id.clone()));
    }
    let style = dominant_paragraph_style(paragraph, atoms_by_id, styles_by_id)?;
    let requested_weight = if style.font_weight.unwrap_or(400) >= 600 {
        TranslationFontWeight::Bold
    } else {
        TranslationFontWeight::Regular
    };
    let font_weight = if metrics.supports(requested_weight) {
        requested_weight
    } else {
        TranslationFontWeight::Regular
    };
    let source_font_size = style
        .scaled_font_size
        .abs()
        .clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    Ok(ParagraphLayoutInput {
        paragraph_group_id: paragraph.paragraph_group_id.clone(),
        text: paragraph.translated_text.trim().to_string(),
        source_font_size,
        font_weight,
        fill_color: style.fill_color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
        opacity: style.opacity.unwrap_or(1.0).clamp(0.0, 1.0),
    })
}

fn try_layout_at_scale(
    paragraphs: &[ParagraphLayoutInput],
    bounds: [f32; 4],
    fit_scale: f32,
    metrics: &dyn RegionFontMetrics,
    policy: RegionLayoutPolicy,
) -> Result<Option<Vec<FlowLayoutLine>>, RegionLayoutError> {
    let width = bounds[2] - bounds[0];
    let mut baseline_y = bounds[3];
    let mut lines = Vec::new();
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let font_size = paragraph.source_font_size * fit_scale;
        let line_height = font_size * f32::from(policy.line_height_milli) / 1_000.0;
        let paragraph_gap = font_size * f32::from(policy.paragraph_gap_milli) / 1_000.0;
        if paragraph_index > 0 {
            baseline_y -= paragraph_gap;
        }
        baseline_y -= font_size;
        let wrapped = wrap_text(
            &paragraph.text,
            width,
            font_size,
            paragraph.font_weight,
            metrics,
        )?;
        if wrapped.is_empty() {
            return Ok(None);
        }
        for (line_index, text) in wrapped.into_iter().enumerate() {
            if line_index > 0 {
                baseline_y -= line_height;
            }
            let descent_allowance = font_size * 0.25;
            if baseline_y - descent_allowance < bounds[1] {
                return Ok(None);
            }
            lines.push(FlowLayoutLine {
                paragraph_group_id: paragraph.paragraph_group_id.clone(),
                text,
                x: bounds[0],
                baseline_y,
                font_size,
                font_weight: paragraph.font_weight,
                fill_color: paragraph.fill_color,
                opacity: paragraph.opacity,
            });
        }
    }
    Ok(Some(lines))
}

#[derive(Debug, Clone)]
struct LayoutToken {
    text: String,
    advance_1000: i64,
    whitespace: bool,
    opening: bool,
    closing: bool,
}

fn wrap_text(
    text: &str,
    max_width: f32,
    font_size: f32,
    font_weight: TranslationFontWeight,
    metrics: &dyn RegionFontMetrics,
) -> Result<Vec<String>, RegionLayoutError> {
    let tokens = tokenize(text)
        .into_iter()
        .map(|text| {
            let advance_1000 = metrics
                .text_advance_1000(font_weight, &text)
                .map_err(RegionLayoutError::Font)?;
            let whitespace = text.chars().all(char::is_whitespace);
            let opening = text.chars().last().is_some_and(is_opening_punctuation);
            let closing = text.chars().next().is_some_and(is_closing_punctuation);
            Ok(LayoutToken {
                text,
                advance_1000,
                whitespace,
                opening,
                closing,
            })
        })
        .collect::<Result<Vec<_>, RegionLayoutError>>()?;
    let max_advance_1000 = f64::from(max_width) * 1_000.0 / f64::from(font_size);
    let mut expanded = Vec::new();
    for token in tokens {
        if token.advance_1000 as f64 <= max_advance_1000 || token.whitespace {
            expanded.push(token);
            continue;
        }
        for character in token.text.chars() {
            let text = character.to_string();
            expanded.push(LayoutToken {
                advance_1000: metrics
                    .text_advance_1000(font_weight, &text)
                    .map_err(RegionLayoutError::Font)?,
                whitespace: character.is_whitespace(),
                opening: is_opening_punctuation(character),
                closing: is_closing_punctuation(character),
                text,
            });
        }
    }

    let mut lines = Vec::new();
    let mut current = Vec::<LayoutToken>::new();
    let mut current_advance = 0i64;
    for token in expanded {
        if token.whitespace && current.is_empty() {
            continue;
        }
        let candidate_advance = current_advance.saturating_add(token.advance_1000);
        let closing_hang_limit = max_advance_1000 * 1.04;
        let fits = candidate_advance as f64 <= max_advance_1000
            || (token.closing && candidate_advance as f64 <= closing_hang_limit);
        if !current.is_empty() && !fits {
            if current.last().is_some_and(|previous| previous.opening) {
                let opening = current.pop().expect("opening token");
                push_line(&mut lines, &mut current);
                current.push(opening);
                current_advance = current.iter().map(|item| item.advance_1000).sum::<i64>();
            } else {
                push_line(&mut lines, &mut current);
                current_advance = 0;
            }
        }
        if token.whitespace && current.is_empty() {
            continue;
        }
        current_advance = current_advance.saturating_add(token.advance_1000);
        current.push(token);
    }
    push_line(&mut lines, &mut current);
    Ok(lines)
}

fn push_line(lines: &mut Vec<String>, current: &mut Vec<LayoutToken>) {
    while current.last().is_some_and(|token| token.whitespace) {
        current.pop();
    }
    let text = current
        .drain(..)
        .map(|token| token.text)
        .collect::<String>();
    if !text.is_empty() {
        lines.push(text);
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_kind = None::<TokenKind>;
    for character in text.chars() {
        let kind = token_kind(character);
        let join =
            matches!(kind, TokenKind::Latin | TokenKind::Whitespace) && current_kind == Some(kind);
        if !join && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(character);
        current_kind = Some(kind);
        if matches!(kind, TokenKind::Cjk | TokenKind::Punctuation) {
            tokens.push(std::mem::take(&mut current));
            current_kind = None;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Latin,
    Cjk,
    Whitespace,
    Punctuation,
}

fn token_kind(character: char) -> TokenKind {
    if character.is_whitespace() {
        TokenKind::Whitespace
    } else if is_cjk(character) {
        TokenKind::Cjk
    } else if character.is_alphanumeric()
        || matches!(character, '\'' | '\u{2019}' | '-' | '_' | '/' | '\\')
    {
        TokenKind::Latin
    } else {
        TokenKind::Punctuation
    }
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn is_opening_punctuation(character: char) -> bool {
    matches!(
        character,
        '(' | '['
            | '{'
            | '\u{2018}'
            | '\u{201c}'
            | '\u{3008}'
            | '\u{300a}'
            | '\u{300c}'
            | '\u{300e}'
            | '\u{3010}'
    )
}

fn is_closing_punctuation(character: char) -> bool {
    matches!(
        character,
        ',' | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '%'
            | ')'
            | ']'
            | '}'
            | '\u{2019}'
            | '\u{201d}'
            | '\u{3001}'
            | '\u{3002}'
            | '\u{3009}'
            | '\u{300b}'
            | '\u{300d}'
            | '\u{300f}'
            | '\u{3011}'
            | '\u{ff0c}'
            | '\u{ff01}'
            | '\u{ff1a}'
            | '\u{ff1b}'
            | '\u{ff1f}'
    )
}

fn validate_policy(policy: RegionLayoutPolicy) -> Result<(), RegionLayoutError> {
    if policy.minimum_fit_scale_milli == 0
        || policy.minimum_fit_scale_milli > 1_000
        || policy.fit_step_milli == 0
        || policy.fit_step_milli > 1_000
        || policy.line_height_milli < 1_000
        || policy.line_height_milli > 3_000
        || policy.paragraph_gap_milli > 2_000
        || !policy.horizontal_padding.is_finite()
        || policy.horizontal_padding < 0.0
        || !policy.vertical_padding.is_finite()
        || policy.vertical_padding < 0.0
    {
        return Err(RegionLayoutError::InvalidPolicy);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use std::path::PathBuf;

    use super::{
        layout_region_translation_patch, tokenize, RegionFontMetrics, RegionLayoutPolicy, TokenKind,
    };
    use crate::pdf_v3::{
        paragraph_translation_plan::{
            build_visual_paragraph_page_plan, resolve_visual_paragraph_results,
        },
        patch_renderer::TRANSLATION_PATCH_RENDERER_VERSION,
        region_translation_patch::{
            build_region_translation_patch, RegionTranslationPatchDraft,
            RegionTranslationPatchRendererDecision,
        },
        translation_plan::{TranslationPatchDraftMetadata, TranslationUnitResult},
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind,
            PageReconciliationSummary, PageStyle, PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    struct FixedMetrics;

    impl RegionFontMetrics for FixedMetrics {
        fn supports(&self, _weight: crate::pdf_v3::font::TranslationFontWeight) -> bool {
            true
        }

        fn text_advance_1000(
            &self,
            _weight: crate::pdf_v3::font::TranslationFontWeight,
            text: &str,
        ) -> Result<i64, String> {
            Ok(text
                .chars()
                .map(|character| if character.is_ascii() { 500 } else { 1_000 })
                .sum())
        }
    }

    #[test]
    fn tokenizer_keeps_latin_words_and_splits_cjk_without_spaces() {
        let tokens = tokenize("Rosetta PDF中文换行");
        assert_eq!(tokens, vec!["Rosetta", " ", "PDF", "中", "文", "换", "行"]);
        assert_eq!(super::token_kind('R'), TokenKind::Latin);
        assert_eq!(super::token_kind('中'), TokenKind::Cjk);
    }

    #[test]
    fn region_layout_wraps_complete_paragraphs_and_reuses_persisted_decision() {
        let page = page([0.0, 0.0, 72.0, 54.0]);
        let patch = patch(
            &page,
            "这是一个完整中文段落，用于验证整块自动换行。第二句继续保持可读性。",
        );
        let first = layout_region_translation_patch(
            &page,
            &patch,
            &FixedMetrics,
            RegionLayoutPolicy::default(),
        )
        .expect("layout");
        assert_eq!(first.layouts.len(), 1);
        assert!(first.layouts[0].lines.len() >= 3);
        assert!(first.layouts[0]
            .lines
            .iter()
            .all(|line| !line.text.contains(' ')));
        let decision = first
            .decisions
            .get(&patch.containers[0].container_id)
            .cloned()
            .expect("decision");
        assert!(matches!(
            decision,
            RegionTranslationPatchRendererDecision::Reflowed { .. }
        ));
        let resolved =
            crate::pdf_v3::region_translation_patch::resolve_region_translation_patch_decisions(
                &page,
                &patch,
                &first.decisions,
            )
            .expect("resolved patch");
        let second = layout_region_translation_patch(
            &page,
            &resolved,
            &FixedMetrics,
            RegionLayoutPolicy::default(),
        )
        .expect("recreated layout");
        assert_eq!(first.layouts, second.layouts);
        assert!(second.decisions.is_empty());
    }

    #[test]
    fn region_layout_preserves_container_when_minimum_scale_still_overflows() {
        let page = page([0.0, 0.0, 30.0, 12.0]);
        let patch = patch(&page, &"极长内容".repeat(30));
        let batch = layout_region_translation_patch(
            &page,
            &patch,
            &FixedMetrics,
            RegionLayoutPolicy::default(),
        )
        .expect("overflow decision");
        assert!(batch.layouts.is_empty());
        assert!(matches!(
            batch.decisions.get(&patch.containers[0].container_id),
            Some(RegionTranslationPatchRendererDecision::Preserved { reason_code })
                if reason_code == "region-layout-overflow"
        ));
    }

    #[test]
    fn region_layout_preserves_small_mixed_scale_decorative_container() {
        let mut page = page([0.0, 0.0, 72.0, 64.0]);
        let accent_atom_ids = page.atoms[..2]
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let label_atom_ids = page.atoms[2..]
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        for atom in &mut page.atoms[..2] {
            atom.style_id = Some("accent".to_string());
        }
        page.styles.push(PageStyle {
            style_id: "accent".to_string(),
            font_resource: Some("F2".to_string()),
            font_size: 30.0,
            scaled_font_size: 30.0,
            font_weight: Some(400),
            italic: false,
            serif: false,
            fill_color: Some([1.0, 0.65, 0.2, 1.0]),
            stroke_color: None,
            opacity: Some(1.0),
            render_mode: Some("fill".to_string()),
        });
        let all_atom_ids = page
            .atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        page.groups = vec![
            PageGroup {
                group_id: "line-accent".to_string(),
                kind: PageGroupKind::Line,
                atom_ids: accent_atom_ids.clone(),
                bounds: [0.0, 34.0, 30.0, 64.0],
                confidence: 0.99,
            },
            PageGroup {
                group_id: "paragraph-accent".to_string(),
                kind: PageGroupKind::Paragraph,
                atom_ids: accent_atom_ids,
                bounds: [0.0, 34.0, 30.0, 64.0],
                confidence: 0.98,
            },
            PageGroup {
                group_id: "line-label".to_string(),
                kind: PageGroupKind::Line,
                atom_ids: label_atom_ids.clone(),
                bounds: [0.0, 0.0, 72.0, 24.0],
                confidence: 0.99,
            },
            PageGroup {
                group_id: "paragraph-label".to_string(),
                kind: PageGroupKind::Paragraph,
                atom_ids: label_atom_ids,
                bounds: [0.0, 0.0, 72.0, 24.0],
                confidence: 0.98,
            },
            PageGroup {
                group_id: "container-1".to_string(),
                kind: PageGroupKind::FlowContainer,
                atom_ids: all_atom_ids,
                bounds: [0.0, 0.0, 72.0, 64.0],
                confidence: 0.97,
            },
        ];

        let plan = build_visual_paragraph_page_plan(&page).expect("decorative plan");
        assert_eq!(plan.units.len(), 2);
        let results = plan
            .units
            .iter()
            .enumerate()
            .map(|(index, unit)| TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: if index == 0 {
                    "34".to_string()
                } else {
                    "纽约、旧金山、洛杉矶".to_string()
                },
            })
            .collect::<Vec<_>>();
        let translations = resolve_visual_paragraph_results(&plan, results, "zh-CN")
            .expect("decorative translations");
        let patch = build_region_translation_patch(
            &page,
            RegionTranslationPatchDraft {
                plan,
                translations,
                metadata: TranslationPatchDraftMetadata {
                    target_language: "zh-CN".to_string(),
                    translation_revision: 1,
                    provider_id: "provider-test".to_string(),
                    model_id: "model-test".to_string(),
                    renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                },
            },
        )
        .expect("decorative patch");
        let batch = layout_region_translation_patch(
            &page,
            &patch,
            &FixedMetrics,
            RegionLayoutPolicy::default(),
        )
        .expect("decorative preservation");

        assert!(batch.layouts.is_empty());
        assert!(matches!(
            batch.decisions.get(&patch.containers[0].container_id),
            Some(RegionTranslationPatchRendererDecision::Preserved { reason_code })
                if reason_code == "region-decorative-mixed-scale"
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows external PDF region-layout probe"]
    fn manual_windows_external_region_layout_probe() {
        use crate::{
            pdf_v3::{
                document::DocumentHandle,
                font::{TranslationFontAsset, TranslationFontWeight, UnifiedTranslationFontPlan},
                reconcile::build_reconciled_page_graph_from_handle,
            },
            rosetta_jobs::formats::pdf::test_helpers::{pdfium_test_lock, shared_pdfium},
        };

        let _guard = pdfium_test_lock();
        let source = std::env::var_os("ROSETTA_PDF_V3_REGION_LAYOUT_PROBE")
            .map(PathBuf::from)
            .expect("ROSETTA_PDF_V3_REGION_LAYOUT_PROBE");
        let regular_path = std::env::var_os("ROSETTA_PDF_V3_REGION_REGULAR_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"));
        let bold_path = std::env::var_os("ROSETTA_PDF_V3_REGION_BOLD_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Bold.ttf"));
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");

        for page_number in 1..=handle.page_count() {
            let page = build_reconciled_page_graph_from_handle(&handle, page_number)
                .unwrap_or_else(|error| panic!("page {page_number}: {error}"));
            let plan = build_visual_paragraph_page_plan(&page).expect("paragraph plan");
            let results = plan
                .units
                .iter()
                .map(|unit| {
                    let count = (unit.source_text.chars().count() * 45 / 100).max(4);
                    TranslationUnitResult {
                        unit_id: unit.unit_id.clone(),
                        translated_text: format!("{}。", "译".repeat(count)),
                    }
                })
                .collect::<Vec<_>>();
            let translations =
                resolve_visual_paragraph_results(&plan, results, "zh-CN").expect("translations");
            let patch = build_region_translation_patch(
                &page,
                RegionTranslationPatchDraft {
                    plan,
                    translations,
                    metadata: TranslationPatchDraftMetadata {
                        target_language: "zh-CN".to_string(),
                        translation_revision: 1,
                        provider_id: "layout-probe".to_string(),
                        model_id: "synthetic-no-provider".to_string(),
                        renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                    },
                },
            )
            .expect("region patch");
            let mut font_plan = UnifiedTranslationFontPlan::default();
            for container in &patch.containers {
                for paragraph in &container.paragraphs {
                    font_plan.add_text(&paragraph.translated_text);
                }
            }
            let regular = TranslationFontAsset::open_weighted(
                "SourceHanSansCNRegular",
                TranslationFontWeight::Regular,
                &regular_path,
                0,
            )
            .and_then(|asset| asset.prepare(&font_plan))
            .expect("regular font");
            let bold = TranslationFontAsset::open_weighted(
                "SourceHanSansCNBold",
                TranslationFontWeight::Bold,
                &bold_path,
                0,
            )
            .and_then(|asset| asset.prepare(&font_plan))
            .expect("bold font");
            let metrics = super::PreparedRegionFontMetrics::new(&regular, Some(&bold));
            let batch = layout_region_translation_patch(
                &page,
                &patch,
                &metrics,
                RegionLayoutPolicy::default(),
            )
            .expect("layout batch");
            let preserved = batch
                .decisions
                .values()
                .filter(|decision| {
                    matches!(
                        decision,
                        RegionTranslationPatchRendererDecision::Preserved { .. }
                    )
                })
                .count();
            let minimum_scale = batch
                .layouts
                .iter()
                .map(|layout| layout.fit_scale)
                .fold(1.0f32, f32::min);
            let line_count = batch
                .layouts
                .iter()
                .map(|layout| layout.lines.len())
                .sum::<usize>();
            println!(
                "pdf-v3 region-layout-probe page={page_number} units={} containers={} layouts={} lines={line_count} preserved={preserved} minimumScale={minimum_scale:.3} regularGlyphs={} boldGlyphs={}",
                patch
                    .containers
                    .iter()
                    .map(|container| container.paragraphs.len())
                    .sum::<usize>(),
                patch.containers.len(),
                batch.layouts.len(),
                regular.glyph_count(),
                bold.glyph_count(),
            );
        }
    }

    fn patch(
        page: &PageGraph,
        translation: &str,
    ) -> crate::pdf_v3::region_translation_patch::RegionTranslationPatch {
        let plan = build_visual_paragraph_page_plan(page).expect("plan");
        let translations = resolve_visual_paragraph_results(
            &plan,
            vec![TranslationUnitResult {
                unit_id: plan.units[0].unit_id.clone(),
                translated_text: translation.to_string(),
            }],
            "zh-CN",
        )
        .expect("translation");
        build_region_translation_patch(
            page,
            RegionTranslationPatchDraft {
                plan,
                translations,
                metadata: TranslationPatchDraftMetadata {
                    target_language: "zh-CN".to_string(),
                    translation_revision: 1,
                    provider_id: "provider-test".to_string(),
                    model_id: "model-test".to_string(),
                    renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                },
            },
        )
        .expect("patch")
    }

    fn page(container_bounds: [f32; 4]) -> PageGraph {
        let source = "Complete source paragraph";
        let atoms = source
            .chars()
            .enumerate()
            .map(|(index, character)| PageAtom {
                atom_id: format!("atom-{index}"),
                source_text: character.to_string(),
                source_object_id: Some(format!("object-{}", index / 8)),
                kind: PageAtomKind::Body,
                style_id: Some("body".to_string()),
                bounds: [index as f32, 10.0, index as f32 + 1.0, 20.0],
                loose_bounds: None,
                origin: Some([index as f32, 10.0]),
                text_matrix: Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                angle_degrees: Some(0.0),
                order: index as u32,
                generated: false,
                hyphen: false,
                requires_translation: character.is_ascii_alphabetic(),
                source_kind: if character.is_whitespace() {
                    PageAtomSourceKind::PdfiumSyntheticWhitespace
                } else {
                    PageAtomSourceKind::PdfiumVerified
                },
                source_provenance: None,
            })
            .collect::<Vec<_>>();
        let atom_ids = atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:region-layout-page".to_string(),
            page_width: 200.0,
            page_height: 200.0,
            rotation_degrees: 0,
            atoms,
            styles: vec![PageStyle {
                style_id: "body".to_string(),
                font_resource: Some("F1".to_string()),
                font_size: 10.0,
                scaled_font_size: 10.0,
                font_weight: Some(400),
                italic: false,
                serif: false,
                fill_color: Some([0.1, 0.2, 0.3, 1.0]),
                stroke_color: None,
                opacity: Some(0.9),
                render_mode: Some("fill".to_string()),
            }],
            groups: vec![
                PageGroup {
                    group_id: "line-1".to_string(),
                    kind: PageGroupKind::Line,
                    atom_ids: atom_ids.clone(),
                    bounds: container_bounds,
                    confidence: 0.99,
                },
                PageGroup {
                    group_id: "paragraph-1".to_string(),
                    kind: PageGroupKind::Paragraph,
                    atom_ids: atom_ids.clone(),
                    bounds: container_bounds,
                    confidence: 0.98,
                },
                PageGroup {
                    group_id: "container-1".to_string(),
                    kind: PageGroupKind::FlowContainer,
                    atom_ids,
                    bounds: container_bounds,
                    confidence: 0.97,
                },
            ],
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(source.chars().count()),
            warnings: Vec::new(),
        }
    }
}
