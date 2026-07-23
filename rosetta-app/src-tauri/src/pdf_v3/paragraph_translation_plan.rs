use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use sha2::{Digest, Sha256};

use super::{
    translation_patch::TranslationPatchProtectedSpanPlacement,
    translation_plan::{TranslationUnitProtectedSpan, TranslationUnitResult},
    types::{
        PageAtom, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind, ProtectedSpan,
        PAGE_GRAPH_SCHEMA_VERSION,
    },
};

pub(crate) const VISUAL_PARAGRAPH_PLAN_SCHEMA_VERSION: u32 = 1;

const MIN_GROUP_CONFIDENCE: f32 = 0.80;
const MAX_PARAGRAPH_UNITS_PER_PAGE: usize = 25_000;
const MAX_PARAGRAPH_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_PAGE_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisualParagraphPagePlan {
    pub schema_version: u32,
    pub page_number: u32,
    pub source_page_hash: String,
    pub units: Vec<VisualParagraphUnitPlan>,
    pub preserved_containers: Vec<VisualParagraphPreservedContainer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisualParagraphUnitPlan {
    pub unit_id: String,
    pub paragraph_group_id: String,
    pub flow_container_group_id: String,
    pub order_on_page: u32,
    pub atom_ids: Vec<String>,
    pub source_text: String,
    pub provider_text: String,
    pub protected_spans: Vec<TranslationUnitProtectedSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisualParagraphPreservedContainer {
    pub flow_container_group_id: String,
    pub atom_ids: Vec<String>,
    pub reason_code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedVisualParagraphTranslation {
    pub unit_id: String,
    pub paragraph_group_id: String,
    pub flow_container_group_id: String,
    pub atom_ids: Vec<String>,
    pub translated_text: String,
    pub protected_spans: Vec<TranslationPatchProtectedSpanPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedVisualParagraphPage {
    pub translations: Vec<ResolvedVisualParagraphTranslation>,
    pub preserved_containers: BTreeMap<String, &'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VisualParagraphPlanError {
    UnsupportedPageGraphSchema {
        expected: u32,
        actual: u32,
    },
    InvalidPageGraph(&'static str),
    TooManyUnits {
        count: usize,
        maximum: usize,
    },
    ParagraphSourceTooLarge {
        bytes: usize,
        maximum: usize,
    },
    PageSourceTooLarge {
        bytes: usize,
        maximum: usize,
    },
    ResultCountMismatch {
        expected: usize,
        actual: usize,
    },
    UnknownResult(String),
    DuplicateResult(String),
    MissingResult(String),
    EmptyTranslation(String),
    TranslationControlCharacter(String),
    MissingProtectedToken {
        unit_id: String,
        token: String,
    },
    DuplicateProtectedToken {
        unit_id: String,
        token: String,
    },
    ReorderedProtectedToken {
        unit_id: String,
        token: String,
    },
    UnknownProtectedToken {
        unit_id: String,
        token: String,
    },
    SuspiciousOutput {
        unit_id: String,
        reason_code: &'static str,
    },
}

impl fmt::Display for VisualParagraphPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPageGraphSchema { expected, actual } => write!(
                formatter,
                "PageGraph schema mismatch: expected {expected}, found {actual}"
            ),
            Self::InvalidPageGraph(reason) => {
                write!(
                    formatter,
                    "PageGraph cannot produce a paragraph plan: {reason}"
                )
            }
            Self::TooManyUnits { count, maximum } => write!(
                formatter,
                "paragraph plan has {count} units, above maximum {maximum}"
            ),
            Self::ParagraphSourceTooLarge { bytes, maximum } => write!(
                formatter,
                "paragraph unit has {bytes} source bytes, above maximum {maximum}"
            ),
            Self::PageSourceTooLarge { bytes, maximum } => write!(
                formatter,
                "paragraph plan has {bytes} source bytes, above maximum {maximum}"
            ),
            Self::ResultCountMismatch { expected, actual } => write!(
                formatter,
                "paragraph result count mismatch: expected {expected}, found {actual}"
            ),
            Self::UnknownResult(unit_id) => {
                write!(
                    formatter,
                    "paragraph result references unknown unit {unit_id}"
                )
            }
            Self::DuplicateResult(unit_id) => {
                write!(formatter, "paragraph result repeats unit {unit_id}")
            }
            Self::MissingResult(unit_id) => {
                write!(formatter, "paragraph result is missing unit {unit_id}")
            }
            Self::EmptyTranslation(unit_id) => {
                write!(formatter, "paragraph result for unit {unit_id} is empty")
            }
            Self::TranslationControlCharacter(unit_id) => write!(
                formatter,
                "paragraph result for unit {unit_id} contains a control character"
            ),
            Self::MissingProtectedToken { unit_id, token } => write!(
                formatter,
                "paragraph result for unit {unit_id} is missing protected token {token}"
            ),
            Self::DuplicateProtectedToken { unit_id, token } => write!(
                formatter,
                "paragraph result for unit {unit_id} repeats protected token {token}"
            ),
            Self::ReorderedProtectedToken { unit_id, token } => write!(
                formatter,
                "paragraph result for unit {unit_id} reorders protected token {token}"
            ),
            Self::UnknownProtectedToken { unit_id, token } => write!(
                formatter,
                "paragraph result for unit {unit_id} contains unknown protected token {token}"
            ),
            Self::SuspiciousOutput {
                unit_id,
                reason_code,
            } => write!(
                formatter,
                "paragraph result for unit {unit_id} failed quality validation: {reason_code}"
            ),
        }
    }
}

impl std::error::Error for VisualParagraphPlanError {}

pub(crate) fn build_visual_paragraph_page_plan(
    page: &PageGraph,
) -> Result<VisualParagraphPagePlan, VisualParagraphPlanError> {
    validate_page(page)?;
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let spans_by_first_atom = protected_spans_by_first_atom(page, &atoms_by_id)?;
    let protected_atom_ids = page
        .protected_spans
        .iter()
        .flat_map(|span| span.atom_ids.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    let lines = groups(page, PageGroupKind::Line);
    let paragraphs = groups(page, PageGroupKind::Paragraph);
    let containers = groups(page, PageGroupKind::FlowContainer);
    if paragraphs.is_empty() || containers.is_empty() {
        return Err(VisualParagraphPlanError::InvalidPageGraph(
            "visual paragraph or flow-container groups are missing",
        ));
    }

    let paragraph_sets = paragraphs
        .iter()
        .map(|paragraph| {
            (
                paragraph.group_id.as_str(),
                paragraph
                    .atom_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for paragraph in &paragraphs {
        let paragraph_atoms = paragraph_sets
            .get(paragraph.group_id.as_str())
            .expect("indexed paragraph");
        let owner_count = containers
            .iter()
            .filter(|container| {
                let container_atoms = container
                    .atom_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>();
                !paragraph_atoms.is_empty() && paragraph_atoms.is_subset(&container_atoms)
            })
            .count();
        if owner_count != 1 {
            return Err(VisualParagraphPlanError::InvalidPageGraph(
                "paragraph flow-container ownership is ambiguous",
            ));
        }
    }
    let mut units = Vec::new();
    let mut preserved_containers = Vec::new();
    let mut total_source_bytes = 0usize;

    for container in containers {
        let container_atoms = container
            .atom_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let container_paragraphs = paragraphs
            .iter()
            .copied()
            .filter(|paragraph| {
                paragraph_sets
                    .get(paragraph.group_id.as_str())
                    .is_some_and(|atoms| !atoms.is_empty() && atoms.is_subset(&container_atoms))
            })
            .collect::<Vec<_>>();
        let preserve = |reason_code| VisualParagraphPreservedContainer {
            flow_container_group_id: container.group_id.clone(),
            atom_ids: container.atom_ids.clone(),
            reason_code,
        };

        if !valid_group(container)
            || container_paragraphs.is_empty()
            || !container_atoms
                .iter()
                .all(|atom_id| atoms_by_id.contains_key(atom_id))
        {
            preserved_containers.push(preserve("container-geometry-unsupported"));
            continue;
        }

        let covered_atoms = container_paragraphs
            .iter()
            .flat_map(|paragraph| paragraph.atom_ids.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let uncovered_translatable = container_atoms.iter().any(|atom_id| {
            !covered_atoms.contains(atom_id)
                && atoms_by_id
                    .get(atom_id)
                    .is_some_and(|atom| atom.requires_translation)
        });
        if uncovered_translatable {
            preserved_containers.push(preserve("container-paragraph-coverage-incomplete"));
            continue;
        }

        let mut container_units = Vec::new();
        let mut container_failure = None;
        for paragraph in container_paragraphs {
            match build_paragraph_unit(
                page,
                paragraph,
                container,
                &lines,
                &atoms_by_id,
                &spans_by_first_atom,
                &protected_atom_ids,
            ) {
                Ok(Some(unit)) => container_units.push(unit),
                Ok(None) => {}
                Err(reason_code) => {
                    container_failure = Some(reason_code);
                    break;
                }
            }
        }
        if let Some(reason_code) = container_failure {
            preserved_containers.push(preserve(reason_code));
            continue;
        }

        for unit in container_units {
            total_source_bytes = total_source_bytes
                .checked_add(unit.source_text.len())
                .ok_or(VisualParagraphPlanError::PageSourceTooLarge {
                    bytes: usize::MAX,
                    maximum: MAX_PAGE_SOURCE_BYTES,
                })?;
            if total_source_bytes > MAX_PAGE_SOURCE_BYTES {
                return Err(VisualParagraphPlanError::PageSourceTooLarge {
                    bytes: total_source_bytes,
                    maximum: MAX_PAGE_SOURCE_BYTES,
                });
            }
            units.push(unit);
            if units.len() > MAX_PARAGRAPH_UNITS_PER_PAGE {
                return Err(VisualParagraphPlanError::TooManyUnits {
                    count: units.len(),
                    maximum: MAX_PARAGRAPH_UNITS_PER_PAGE,
                });
            }
        }
    }

    Ok(VisualParagraphPagePlan {
        schema_version: VISUAL_PARAGRAPH_PLAN_SCHEMA_VERSION,
        page_number: page.page_number,
        source_page_hash: page.source_page_hash.clone(),
        units,
        preserved_containers,
    })
}

pub(crate) fn resolve_visual_paragraph_results(
    plan: &VisualParagraphPagePlan,
    results: Vec<TranslationUnitResult>,
    target_language: &str,
) -> Result<Vec<ResolvedVisualParagraphTranslation>, VisualParagraphPlanError> {
    if results.len() != plan.units.len() {
        return Err(VisualParagraphPlanError::ResultCountMismatch {
            expected: plan.units.len(),
            actual: results.len(),
        });
    }
    let units_by_id = plan
        .units
        .iter()
        .map(|unit| (unit.unit_id.as_str(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut results_by_id = BTreeMap::new();
    for result in results {
        if !units_by_id.contains_key(result.unit_id.as_str()) {
            return Err(VisualParagraphPlanError::UnknownResult(result.unit_id));
        }
        let result_id = result.unit_id.clone();
        if results_by_id.insert(result_id.clone(), result).is_some() {
            return Err(VisualParagraphPlanError::DuplicateResult(result_id));
        }
    }

    let mut resolved = Vec::with_capacity(plan.units.len());
    for unit in &plan.units {
        let result = results_by_id
            .remove(unit.unit_id.as_str())
            .ok_or_else(|| VisualParagraphPlanError::MissingResult(unit.unit_id.clone()))?;
        validate_output_shape(unit, &result.translated_text, target_language)?;
        let (translated_text, protected_spans) =
            restore_protected_spans(unit, &result.translated_text)?;
        resolved.push(ResolvedVisualParagraphTranslation {
            unit_id: unit.unit_id.clone(),
            paragraph_group_id: unit.paragraph_group_id.clone(),
            flow_container_group_id: unit.flow_container_group_id.clone(),
            atom_ids: unit.atom_ids.clone(),
            translated_text,
            protected_spans,
        });
    }
    Ok(resolved)
}

pub(crate) fn resolve_visual_paragraph_results_preserving_invalid_containers(
    plan: &VisualParagraphPagePlan,
    results: Vec<TranslationUnitResult>,
    target_language: &str,
) -> Result<ResolvedVisualParagraphPage, VisualParagraphPlanError> {
    if results.len() != plan.units.len() {
        return Err(VisualParagraphPlanError::ResultCountMismatch {
            expected: plan.units.len(),
            actual: results.len(),
        });
    }
    let units_by_id = plan
        .units
        .iter()
        .map(|unit| (unit.unit_id.as_str(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut results_by_id = BTreeMap::new();
    for result in results {
        if !units_by_id.contains_key(result.unit_id.as_str()) {
            return Err(VisualParagraphPlanError::UnknownResult(result.unit_id));
        }
        let result_id = result.unit_id.clone();
        if results_by_id.insert(result_id.clone(), result).is_some() {
            return Err(VisualParagraphPlanError::DuplicateResult(result_id));
        }
    }

    let mut translations = Vec::with_capacity(plan.units.len());
    let mut preserved_containers = BTreeMap::new();
    for unit in &plan.units {
        let result = results_by_id
            .remove(unit.unit_id.as_str())
            .ok_or_else(|| VisualParagraphPlanError::MissingResult(unit.unit_id.clone()))?;
        let resolved = validate_output_shape(unit, &result.translated_text, target_language)
            .and_then(|_| {
                restore_protected_spans(unit, &result.translated_text).map(
                    |(translated_text, protected_spans)| ResolvedVisualParagraphTranslation {
                        unit_id: unit.unit_id.clone(),
                        paragraph_group_id: unit.paragraph_group_id.clone(),
                        flow_container_group_id: unit.flow_container_group_id.clone(),
                        atom_ids: unit.atom_ids.clone(),
                        translated_text,
                        protected_spans,
                    },
                )
            });
        match resolved {
            Ok(translation) => translations.push(translation),
            Err(error) if is_unit_result_error(&error) => {
                preserved_containers
                    .entry(unit.flow_container_group_id.clone())
                    .or_insert(unit_result_preservation_reason(&error));
            }
            Err(error) => return Err(error),
        }
    }
    translations.retain(|translation| {
        !preserved_containers.contains_key(&translation.flow_container_group_id)
    });
    Ok(ResolvedVisualParagraphPage {
        translations,
        preserved_containers,
    })
}

fn is_unit_result_error(error: &VisualParagraphPlanError) -> bool {
    matches!(
        error,
        VisualParagraphPlanError::EmptyTranslation(_)
            | VisualParagraphPlanError::TranslationControlCharacter(_)
            | VisualParagraphPlanError::MissingProtectedToken { .. }
            | VisualParagraphPlanError::DuplicateProtectedToken { .. }
            | VisualParagraphPlanError::ReorderedProtectedToken { .. }
            | VisualParagraphPlanError::UnknownProtectedToken { .. }
            | VisualParagraphPlanError::SuspiciousOutput { .. }
    )
}

fn unit_result_preservation_reason(error: &VisualParagraphPlanError) -> &'static str {
    match error {
        VisualParagraphPlanError::SuspiciousOutput { reason_code, .. } => reason_code,
        VisualParagraphPlanError::MissingProtectedToken { .. }
        | VisualParagraphPlanError::DuplicateProtectedToken { .. }
        | VisualParagraphPlanError::ReorderedProtectedToken { .. }
        | VisualParagraphPlanError::UnknownProtectedToken { .. } => {
            "container-protected-token-invalid"
        }
        VisualParagraphPlanError::EmptyTranslation(_)
        | VisualParagraphPlanError::TranslationControlCharacter(_) => {
            "container-provider-output-invalid"
        }
        _ => "container-provider-output-invalid",
    }
}

fn build_paragraph_unit(
    page: &PageGraph,
    paragraph: &PageGroup,
    container: &PageGroup,
    lines: &[&PageGroup],
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
    spans_by_first_atom: &BTreeMap<&str, &ProtectedSpan>,
    protected_atom_ids: &BTreeSet<&str>,
) -> Result<Option<VisualParagraphUnitPlan>, &'static str> {
    if !valid_group(paragraph) || paragraph.atom_ids.is_empty() {
        return Err("paragraph-geometry-unsupported");
    }
    let paragraph_atom_set = paragraph
        .atom_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let atoms = paragraph
        .atom_ids
        .iter()
        .map(|atom_id| atoms_by_id.get(atom_id.as_str()).copied())
        .collect::<Option<Vec<_>>>()
        .ok_or("paragraph-atom-missing")?;
    if !atoms.iter().any(|atom| atom.requires_translation) {
        return Ok(None);
    }
    if atoms.iter().any(|atom| {
        !matches!(
            atom.source_kind,
            PageAtomSourceKind::PdfiumVerified
                | PageAtomSourceKind::ToUnicodeCorrected
                | PageAtomSourceKind::PdfiumSyntheticWhitespace
        ) || atom.source_text.chars().any(char::is_control)
    }) {
        return Err("paragraph-source-mapping-unsupported");
    }
    if atoms
        .iter()
        .any(|atom| atom.source_text.contains("{v") || atom.source_text.contains("}v"))
    {
        return Err("paragraph-placeholder-collision");
    }
    if page.protected_spans.iter().any(|span| {
        let touches = span
            .atom_ids
            .iter()
            .any(|atom_id| paragraph_atom_set.contains(atom_id.as_str()));
        touches
            && !span
                .atom_ids
                .iter()
                .all(|atom_id| paragraph_atom_set.contains(atom_id.as_str()))
    }) {
        return Err("protected-span-crosses-paragraph");
    }

    let paragraph_lines = lines
        .iter()
        .copied()
        .filter(|line| {
            let line_set = line
                .atom_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            !line_set.is_empty() && line_set.is_subset(&paragraph_atom_set)
        })
        .collect::<Vec<_>>();
    if paragraph_lines.is_empty() {
        return Err("paragraph-line-coverage-missing");
    }
    let covered_line_atoms = paragraph_lines
        .iter()
        .flat_map(|line| line.atom_ids.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    if atoms.iter().any(|atom| {
        atom.requires_translation && !covered_line_atoms.contains(atom.atom_id.as_str())
    }) {
        return Err("paragraph-line-coverage-incomplete");
    }

    let paragraph_spans = page
        .protected_spans
        .iter()
        .filter(|span| {
            span.atom_ids
                .iter()
                .all(|atom_id| paragraph_atom_set.contains(atom_id.as_str()))
        })
        .collect::<Vec<_>>();
    let tokens = paragraph_spans
        .iter()
        .enumerate()
        .map(|(index, span)| (span.span_id.as_str(), format!("{{v{index}}}")))
        .collect::<BTreeMap<_, _>>();
    let protected_spans = paragraph_spans
        .iter()
        .map(|span| TranslationUnitProtectedSpan {
            span_id: span.span_id.clone(),
            kind: span.kind,
            token: tokens
                .get(span.span_id.as_str())
                .cloned()
                .expect("paragraph token"),
            exact_text: span.exact_text.clone(),
        })
        .collect::<Vec<_>>();

    let mut source_lines = Vec::with_capacity(paragraph_lines.len());
    let mut provider_lines = Vec::with_capacity(paragraph_lines.len());
    let mut hyphenated_lines = Vec::with_capacity(paragraph_lines.len());
    for line in paragraph_lines {
        let (source_line, provider_line) = build_line_text(
            line,
            atoms_by_id,
            spans_by_first_atom,
            protected_atom_ids,
            &tokens,
        )?;
        if source_line.is_empty() || provider_line.is_empty() {
            return Err("paragraph-line-empty");
        }
        let hyphenated = line
            .atom_ids
            .iter()
            .rev()
            .filter_map(|atom_id| atoms_by_id.get(atom_id.as_str()).copied())
            .find(|atom| !atom.source_text.chars().all(char::is_whitespace))
            .is_some_and(|atom| atom.hyphen);
        source_lines.push(source_line);
        provider_lines.push(provider_line);
        hyphenated_lines.push(hyphenated);
    }

    let source_text = join_visual_lines(&source_lines, &hyphenated_lines);
    let provider_text = join_visual_lines(&provider_lines, &hyphenated_lines);
    if source_text.is_empty() || provider_text.is_empty() {
        return Err("paragraph-text-empty");
    }
    if source_text.len() > MAX_PARAGRAPH_SOURCE_BYTES {
        return Err("paragraph-source-too-large");
    }
    let order_on_page = atoms
        .iter()
        .map(|atom| atom.order)
        .min()
        .unwrap_or(u32::MAX);
    Ok(Some(VisualParagraphUnitPlan {
        unit_id: paragraph_unit_id(page, paragraph, container),
        paragraph_group_id: paragraph.group_id.clone(),
        flow_container_group_id: container.group_id.clone(),
        order_on_page,
        atom_ids: paragraph.atom_ids.clone(),
        source_text,
        provider_text,
        protected_spans,
    }))
}

fn build_line_text(
    line: &PageGroup,
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
    spans_by_first_atom: &BTreeMap<&str, &ProtectedSpan>,
    protected_atom_ids: &BTreeSet<&str>,
    tokens: &BTreeMap<&str, String>,
) -> Result<(String, String), &'static str> {
    let mut source = String::new();
    let mut provider = String::new();
    for atom_id in &line.atom_ids {
        let atom = atoms_by_id
            .get(atom_id.as_str())
            .copied()
            .ok_or("line-atom-missing")?;
        if let Some(span) = spans_by_first_atom.get(atom_id.as_str()).copied() {
            source.push_str(&span.exact_text);
            provider.push_str(
                tokens
                    .get(span.span_id.as_str())
                    .ok_or("protected-span-token-missing")?,
            );
        } else if protected_atom_ids.contains(atom_id.as_str()) {
            continue;
        } else {
            source.push_str(&atom.source_text);
            provider.push_str(&atom.source_text);
        }
    }
    Ok((
        normalize_visual_text(&source),
        normalize_visual_text(&provider),
    ))
}

fn normalize_visual_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space
            && !is_closing_punctuation(character)
            && output
                .chars()
                .last()
                .is_some_and(|last| !is_opening_punctuation(last))
        {
            output.push(' ');
        }
        output.push(character);
        pending_space = false;
    }
    output.trim().to_string()
}

fn join_visual_lines(lines: &[String], hyphenated_lines: &[bool]) -> String {
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if output.is_empty() {
            output.push_str(line);
            continue;
        }
        let dehyphenate = hyphenated_lines.get(index - 1).copied().unwrap_or(false)
            && output.ends_with(['-', '\u{2010}', '\u{2011}']);
        if dehyphenate {
            output.pop();
        } else if should_insert_line_space(&output, line) {
            output.push(' ');
        }
        output.push_str(line);
    }
    output
}

fn should_insert_line_space(left: &str, right: &str) -> bool {
    let Some(previous) = left.chars().last() else {
        return false;
    };
    let Some(next) = right.chars().next() else {
        return false;
    };
    !is_opening_punctuation(previous)
        && !is_closing_punctuation(next)
        && !(is_cjk(previous) && is_cjk(next))
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

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn validate_output_shape(
    unit: &VisualParagraphUnitPlan,
    translated_text: &str,
    target_language: &str,
) -> Result<(), VisualParagraphPlanError> {
    let trimmed = translated_text.trim();
    if trimmed.is_empty() {
        return Err(VisualParagraphPlanError::EmptyTranslation(
            unit.unit_id.clone(),
        ));
    }
    if translated_text.chars().any(char::is_control) {
        return Err(VisualParagraphPlanError::TranslationControlCharacter(
            unit.unit_id.clone(),
        ));
    }
    let source_chars = unit.source_text.chars().count();
    let translated_chars = translated_text.chars().count();
    if source_chars >= 24 && translated_chars.saturating_mul(8) < source_chars {
        return Err(VisualParagraphPlanError::SuspiciousOutput {
            unit_id: unit.unit_id.clone(),
            reason_code: "translation-too-short",
        });
    }
    if source_chars >= 8 && translated_chars > source_chars.saturating_mul(5) {
        return Err(VisualParagraphPlanError::SuspiciousOutput {
            unit_id: unit.unit_id.clone(),
            reason_code: "translation-too-long",
        });
    }
    if is_chinese_target(target_language) {
        let source_latin = unit
            .source_text
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .count();
        let cjk_count = translated_text
            .chars()
            .filter(|character| is_cjk(*character))
            .count();
        let translated_latin = translated_text
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .count();
        if source_latin >= 24 && cjk_count == 0 && translated_latin >= 12 {
            return Err(VisualParagraphPlanError::SuspiciousOutput {
                unit_id: unit.unit_id.clone(),
                reason_code: "translation-appears-untranslated",
            });
        }
        if source_latin >= 30
            && cjk_count >= 8
            && cjk_adjacent_single_latin_run_count(translated_text) >= 3
        {
            return Err(VisualParagraphPlanError::SuspiciousOutput {
                unit_id: unit.unit_id.clone(),
                reason_code: "translation-fragmented-mixed-language",
            });
        }
    }
    Ok(())
}

fn cjk_adjacent_single_latin_run_count(text: &str) -> usize {
    let characters = text.chars().collect::<Vec<_>>();
    characters
        .iter()
        .enumerate()
        .filter(|(index, character)| {
            if !character.is_ascii_alphabetic() {
                return false;
            }
            let previous = index.checked_sub(1).and_then(|index| characters.get(index));
            let next = characters.get(index + 1);
            let isolated = previous.is_none_or(|character| !character.is_ascii_alphabetic())
                && next.is_none_or(|character| !character.is_ascii_alphabetic());
            isolated
                && (previous.is_some_and(|character| is_cjk(*character))
                    || next.is_some_and(|character| is_cjk(*character)))
        })
        .count()
}

fn is_chinese_target(target_language: &str) -> bool {
    let normalized = target_language.trim().to_ascii_lowercase();
    normalized == "zh" || normalized.starts_with("zh-") || normalized.starts_with("zh_")
}

fn restore_protected_spans(
    unit: &VisualParagraphUnitPlan,
    provider_text: &str,
) -> Result<(String, Vec<TranslationPatchProtectedSpanPlacement>), VisualParagraphPlanError> {
    let expected_tokens = unit
        .protected_spans
        .iter()
        .map(|span| span.token.as_str())
        .collect::<Vec<_>>();
    let actual_tokens = find_placeholder_tokens(provider_text);
    for token in &actual_tokens {
        if !expected_tokens.contains(&token.as_str()) {
            return Err(VisualParagraphPlanError::UnknownProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: token.clone(),
            });
        }
    }
    for token in &expected_tokens {
        let count = actual_tokens
            .iter()
            .filter(|actual| actual.as_str() == *token)
            .count();
        if count == 0 {
            return Err(VisualParagraphPlanError::MissingProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: (*token).to_string(),
            });
        }
        if count > 1 {
            return Err(VisualParagraphPlanError::DuplicateProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: (*token).to_string(),
            });
        }
    }
    for (index, expected) in expected_tokens.iter().enumerate() {
        if actual_tokens.get(index).map(String::as_str) != Some(*expected) {
            return Err(VisualParagraphPlanError::ReorderedProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: (*expected).to_string(),
            });
        }
    }

    let mut translated_text = String::with_capacity(provider_text.len());
    let mut placements = Vec::with_capacity(unit.protected_spans.len());
    let mut cursor = 0usize;
    for span in &unit.protected_spans {
        let relative = provider_text[cursor..].find(&span.token).ok_or_else(|| {
            VisualParagraphPlanError::MissingProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: span.token.clone(),
            }
        })?;
        let start = cursor + relative;
        translated_text.push_str(&provider_text[cursor..start]);
        let translated_start = u32::try_from(translated_text.len()).map_err(|_| {
            VisualParagraphPlanError::PageSourceTooLarge {
                bytes: translated_text.len(),
                maximum: u32::MAX as usize,
            }
        })?;
        translated_text.push_str(&span.exact_text);
        placements.push(TranslationPatchProtectedSpanPlacement {
            span_id: span.span_id.clone(),
            translated_start,
        });
        cursor = start + span.token.len();
    }
    translated_text.push_str(&provider_text[cursor..]);
    Ok((translated_text, placements))
}

fn find_placeholder_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0usize;
    while cursor + 3 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'v' {
            let mut end = cursor + 2;
            let first_digit = end;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > first_digit && end < bytes.len() && bytes[end] == b'}' {
                tokens.push(text[cursor..=end].to_string());
                cursor = end + 1;
                continue;
            }
        }
        cursor += 1;
    }
    tokens
}

fn groups(page: &PageGraph, kind: PageGroupKind) -> Vec<&PageGroup> {
    page.groups
        .iter()
        .filter(|group| group.kind == kind)
        .collect()
}

fn valid_group(group: &PageGroup) -> bool {
    !group.group_id.is_empty()
        && group.confidence.is_finite()
        && group.confidence >= MIN_GROUP_CONFIDENCE
        && group.bounds.iter().all(|value| value.is_finite())
        && group.bounds[2] > group.bounds[0]
        && group.bounds[3] > group.bounds[1]
}

fn validate_page(page: &PageGraph) -> Result<(), VisualParagraphPlanError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(VisualParagraphPlanError::UnsupportedPageGraphSchema {
            expected: PAGE_GRAPH_SCHEMA_VERSION,
            actual: page.schema_version,
        });
    }
    if page.page_number == 0 || page.source_page_hash.is_empty() {
        return Err(VisualParagraphPlanError::InvalidPageGraph(
            "page identity is invalid",
        ));
    }
    let mut atom_ids = BTreeSet::new();
    let mut group_ids = BTreeSet::new();
    if page.atoms.iter().any(|atom| {
        atom.atom_id.is_empty()
            || atom.source_text.is_empty()
            || !atom_ids.insert(atom.atom_id.as_str())
    }) || page
        .groups
        .iter()
        .any(|group| group.group_id.is_empty() || !group_ids.insert(group.group_id.as_str()))
    {
        return Err(VisualParagraphPlanError::InvalidPageGraph(
            "atom or group identity is invalid",
        ));
    }
    Ok(())
}

fn protected_spans_by_first_atom<'a>(
    page: &'a PageGraph,
    atoms_by_id: &BTreeMap<&str, &PageAtom>,
) -> Result<BTreeMap<&'a str, &'a ProtectedSpan>, VisualParagraphPlanError> {
    let mut result = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for span in &page.protected_spans {
        if span.span_id.is_empty() || span.exact_text.is_empty() || span.atom_ids.is_empty() {
            return Err(VisualParagraphPlanError::InvalidPageGraph(
                "protected span identity is invalid",
            ));
        }
        let mut exact_text = String::new();
        for atom_id in &span.atom_ids {
            let atom = atoms_by_id.get(atom_id.as_str()).copied().ok_or(
                VisualParagraphPlanError::InvalidPageGraph("protected span atom is missing"),
            )?;
            if !claimed.insert(atom_id.as_str()) {
                return Err(VisualParagraphPlanError::InvalidPageGraph(
                    "protected spans overlap",
                ));
            }
            exact_text.push_str(&atom.source_text);
        }
        if exact_text != span.exact_text {
            return Err(VisualParagraphPlanError::InvalidPageGraph(
                "protected span text does not match its atoms",
            ));
        }
        result.insert(span.atom_ids[0].as_str(), span);
    }
    Ok(result)
}

fn paragraph_unit_id(page: &PageGraph, paragraph: &PageGroup, container: &PageGroup) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rosetta-pdf-v3-visual-paragraph-unit/1\0");
    hasher.update(page.source_page_hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(page.page_number.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(container.group_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(paragraph.group_id.as_bytes());
    for atom_id in &paragraph.atom_ids {
        hasher.update(b"\0");
        hasher.update(atom_id.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("paragraph-unit-{hex}")
}

#[cfg(test)]
mod tests {
    use super::{
        build_visual_paragraph_page_plan, resolve_visual_paragraph_results,
        resolve_visual_paragraph_results_preserving_invalid_containers, VisualParagraphPlanError,
    };
    use crate::pdf_v3::{
        translation_plan::TranslationUnitResult,
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind,
            PageReconciliationSummary, ProtectedSpan, ProtectedSpanKind, PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    #[test]
    fn visual_paragraph_plan_reconstructs_clean_cross_object_text() {
        let page = page();
        let plan = build_visual_paragraph_page_plan(&page).expect("paragraph plan");

        assert_eq!(plan.units.len(), 1);
        assert!(plan.preserved_containers.is_empty());
        assert_eq!(
            plan.units[0].source_text,
            "Welcome to our first newsletter of 2017. It has been a while."
        );
        assert_eq!(
            plan.units[0].provider_text,
            "Welcome to our first newsletter of {v0}. It has been a while."
        );
        assert_eq!(plan.units[0].protected_spans.len(), 1);
    }

    #[test]
    fn paragraph_results_restore_protected_text_and_reject_fragmented_mixing() {
        let plan = build_visual_paragraph_page_plan(&page()).expect("paragraph plan");
        let unit = &plan.units[0];
        let resolved = resolve_visual_paragraph_results(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: "欢迎阅读我们{v0}年的第一期新闻简报。已经有一段时间了。"
                    .to_string(),
            }],
            "zh-CN",
        )
        .expect("resolved paragraph");
        assert!(resolved[0].translated_text.contains("2017"));

        let error = resolve_visual_paragraph_results(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: "W欢迎 e来到 v我们的{v0}新闻简报，这是一段完全破碎的译文。"
                    .to_string(),
            }],
            "zh-CN",
        )
        .expect_err("fragmented mixed-language output");
        assert!(matches!(
            error,
            VisualParagraphPlanError::SuspiciousOutput {
                reason_code: "translation-fragmented-mixed-language",
                ..
            }
        ));

        let resolved = resolve_visual_paragraph_results(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text:
                    "团队遍布 NY、SF、LA 和 LV，并与 AWS、L.A. 与 PGA 的合作伙伴共同交付。{v0}"
                        .to_string(),
            }],
            "zh-CN",
        )
        .expect("legitimate names and acronyms");
        assert!(resolved[0].translated_text.contains("NY"));
    }

    #[test]
    fn invalid_provider_result_preserves_only_its_flow_container() {
        let plan = build_visual_paragraph_page_plan(&page()).expect("paragraph plan");
        let unit = &plan.units[0];
        let resolved = resolve_visual_paragraph_results_preserving_invalid_containers(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: "W欢迎 e来到 v我们的{v0}新闻简报，这是一段完全破碎的译文。"
                    .to_string(),
            }],
            "zh-CN",
        )
        .expect("container preservation");

        assert!(resolved.translations.is_empty());
        assert_eq!(
            resolved
                .preserved_containers
                .get(&unit.flow_container_group_id),
            Some(&"translation-fragmented-mixed-language")
        );
    }

    #[test]
    fn unsafe_paragraph_preserves_the_complete_flow_container() {
        let mut page = page();
        page.atoms[0].source_kind = PageAtomSourceKind::PreservedUnmapped;
        let plan = build_visual_paragraph_page_plan(&page).expect("conservative plan");

        assert!(plan.units.is_empty());
        assert_eq!(plan.preserved_containers.len(), 1);
        assert_eq!(
            plan.preserved_containers[0].reason_code,
            "paragraph-source-mapping-unsupported"
        );
        assert_eq!(
            plan.preserved_containers[0].atom_ids,
            page.groups
                .iter()
                .find(|group| group.kind == PageGroupKind::FlowContainer)
                .expect("container")
                .atom_ids
        );
    }

    fn page() -> PageGraph {
        let lines = [
            "Welcome  to our first newsletter of 2017.",
            "It has been a while.",
        ];
        let mut atoms = Vec::new();
        let mut line_atom_ids = Vec::new();
        let mut order = 0u32;
        for (line_index, line) in lines.iter().enumerate() {
            let mut ids = Vec::new();
            for character in line.chars() {
                let atom_id = format!("atom-{order}");
                ids.push(atom_id.clone());
                atoms.push(PageAtom {
                    atom_id,
                    source_text: character.to_string(),
                    source_object_id: Some(format!("object-{}", order / 7)),
                    kind: PageAtomKind::Body,
                    style_id: Some(if order % 9 == 0 { "bold" } else { "regular" }.to_string()),
                    bounds: [
                        order as f32,
                        80.0 - line_index as f32 * 12.0,
                        order as f32 + 1.0,
                        90.0 - line_index as f32 * 12.0,
                    ],
                    loose_bounds: None,
                    origin: Some([order as f32, 80.0 - line_index as f32 * 12.0]),
                    text_matrix: Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                    angle_degrees: Some(0.0),
                    order,
                    generated: false,
                    hyphen: false,
                    requires_translation: character.is_ascii_alphabetic(),
                    source_kind: if character.is_whitespace() {
                        PageAtomSourceKind::PdfiumSyntheticWhitespace
                    } else {
                        PageAtomSourceKind::PdfiumVerified
                    },
                    source_provenance: None,
                });
                order += 1;
            }
            line_atom_ids.push(ids);
        }
        let paragraph_atom_ids = line_atom_ids.iter().flatten().cloned().collect::<Vec<_>>();
        let number_atom_ids = atoms
            .iter()
            .filter(|atom| matches!(atom.source_text.as_str(), "2" | "0" | "1" | "7"))
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let mut groups = line_atom_ids
            .iter()
            .enumerate()
            .map(|(index, atom_ids)| PageGroup {
                group_id: format!("line-{index}"),
                kind: PageGroupKind::Line,
                atom_ids: atom_ids.clone(),
                bounds: [
                    0.0,
                    60.0 + index as f32 * 10.0,
                    100.0,
                    70.0 + index as f32 * 10.0,
                ],
                confidence: 0.99,
            })
            .collect::<Vec<_>>();
        groups.push(PageGroup {
            group_id: "paragraph-1".to_string(),
            kind: PageGroupKind::Paragraph,
            atom_ids: paragraph_atom_ids.clone(),
            bounds: [0.0, 60.0, 100.0, 90.0],
            confidence: 0.98,
        });
        groups.push(PageGroup {
            group_id: "container-1".to_string(),
            kind: PageGroupKind::FlowContainer,
            atom_ids: paragraph_atom_ids,
            bounds: [0.0, 60.0, 100.0, 90.0],
            confidence: 0.97,
        });
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:paragraph-test".to_string(),
            page_width: 100.0,
            page_height: 100.0,
            rotation_degrees: 0,
            atoms,
            styles: Vec::new(),
            groups,
            protected_spans: vec![ProtectedSpan {
                span_id: "number-2017".to_string(),
                kind: ProtectedSpanKind::Number,
                atom_ids: number_atom_ids,
                exact_text: "2017".to_string(),
            }],
            reconciliation: PageReconciliationSummary::unreconciled(order as usize),
            warnings: Vec::new(),
        }
    }
}
