use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use sha2::{Digest, Sha256};

use super::{
    translation_patch::{
        build_translation_patch, TranslationPatchDraft, TranslationPatchEntryDraft,
        TranslationPatchError, TranslationPatchProtectedSpanPlacement,
    },
    types::{
        PageAtom, PageAtomSourceKind, PageAtomSourceProvenance, PageGraph, ProtectedSpan,
        ProtectedSpanKind, TranslationPatch, PAGE_GRAPH_SCHEMA_VERSION,
    },
};

pub(crate) const TRANSLATION_PLAN_SCHEMA_VERSION: u32 = 1;

const MAX_PLAN_UNITS: usize = 100_000;
const MAX_PLAN_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_UNIT_SOURCE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPagePlan {
    pub schema_version: u32,
    pub page_number: u32,
    pub source_page_hash: String,
    pub units: Vec<TranslationUnitPlan>,
    pub preserved_regions: Vec<TranslationPlanPreservedRegion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationUnitPlan {
    pub unit_id: String,
    pub order_on_page: u32,
    pub atom_ids: Vec<String>,
    pub source_text: String,
    pub provider_text: String,
    pub protected_spans: Vec<TranslationUnitProtectedSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationUnitProtectedSpan {
    pub span_id: String,
    pub kind: ProtectedSpanKind,
    pub token: String,
    pub exact_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPlanPreservedRegion {
    pub atom_ids: Vec<String>,
    pub reason_code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationUnitResult {
    pub unit_id: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPatchDraftMetadata {
    pub target_language: String,
    pub translation_revision: u64,
    pub provider_id: String,
    pub model_id: String,
    pub renderer_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranslationPlanError {
    UnsupportedPageGraphSchema { expected: u32, actual: u32 },
    InvalidPageGraph(&'static str),
    TooManyUnits { count: usize, maximum: usize },
    UnitSourceTooLarge { bytes: usize, maximum: usize },
    PlanSourceTooLarge { bytes: usize, maximum: usize },
    PlanPageMismatch,
    PlanSourceMismatch,
    PlanContentMismatch,
    NoTranslatableUnits,
    ResultCountMismatch { expected: usize, actual: usize },
    UnknownResult(String),
    DuplicateResult(String),
    MissingResult(String),
    EmptyTranslation(String),
    TranslationControlCharacter(String),
    MissingProtectedToken { unit_id: String, token: String },
    DuplicateProtectedToken { unit_id: String, token: String },
    ReorderedProtectedToken { unit_id: String, token: String },
    UnknownProtectedToken { unit_id: String, token: String },
    TranslationPatch(TranslationPatchError),
}

impl fmt::Display for TranslationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPageGraphSchema { expected, actual } => write!(
                formatter,
                "PageGraph schema mismatch: expected {expected}, found {actual}"
            ),
            Self::InvalidPageGraph(reason) => {
                write!(
                    formatter,
                    "PageGraph cannot produce a translation plan: {reason}"
                )
            }
            Self::TooManyUnits { count, maximum } => write!(
                formatter,
                "translation plan has {count} units, above maximum {maximum}"
            ),
            Self::UnitSourceTooLarge { bytes, maximum } => write!(
                formatter,
                "translation unit has {bytes} source bytes, above maximum {maximum}"
            ),
            Self::PlanSourceTooLarge { bytes, maximum } => write!(
                formatter,
                "translation plan has {bytes} source bytes, above maximum {maximum}"
            ),
            Self::PlanPageMismatch => formatter.write_str("translation plan page does not match"),
            Self::PlanSourceMismatch => {
                formatter.write_str("translation plan source authority does not match")
            }
            Self::PlanContentMismatch => {
                formatter.write_str("translation plan content is stale or non-canonical")
            }
            Self::NoTranslatableUnits => formatter.write_str(
                "translation plan has no safe units; the page must be explicitly preserved",
            ),
            Self::ResultCountMismatch { expected, actual } => write!(
                formatter,
                "translation result count mismatch: expected {expected}, found {actual}"
            ),
            Self::UnknownResult(unit_id) => {
                write!(
                    formatter,
                    "translation result references unknown unit {unit_id}"
                )
            }
            Self::DuplicateResult(unit_id) => {
                write!(formatter, "translation result repeats unit {unit_id}")
            }
            Self::MissingResult(unit_id) => {
                write!(formatter, "translation result is missing unit {unit_id}")
            }
            Self::EmptyTranslation(unit_id) => {
                write!(formatter, "translation result for unit {unit_id} is empty")
            }
            Self::TranslationControlCharacter(unit_id) => write!(
                formatter,
                "translation result for unit {unit_id} contains a control character"
            ),
            Self::MissingProtectedToken { unit_id, token } => write!(
                formatter,
                "translation result for unit {unit_id} is missing protected token {token}"
            ),
            Self::DuplicateProtectedToken { unit_id, token } => write!(
                formatter,
                "translation result for unit {unit_id} repeats protected token {token}"
            ),
            Self::ReorderedProtectedToken { unit_id, token } => write!(
                formatter,
                "translation result for unit {unit_id} reorders protected token {token}"
            ),
            Self::UnknownProtectedToken { unit_id, token } => write!(
                formatter,
                "translation result for unit {unit_id} contains unknown protected token {token}"
            ),
            Self::TranslationPatch(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranslationPlanError {}

impl From<TranslationPatchError> for TranslationPlanError {
    fn from(value: TranslationPatchError) -> Self {
        Self::TranslationPatch(value)
    }
}

pub(crate) fn build_translation_page_plan(
    page: &PageGraph,
) -> Result<TranslationPagePlan, TranslationPlanError> {
    validate_page_graph(page)?;
    let spans_by_atom = index_protected_spans(page)?;
    let mut objects = BTreeMap::<String, Vec<&PageAtom>>::new();
    let mut preserved_regions = Vec::new();

    for atom in &page.atoms {
        if let Some(source_object_id) = atom.source_object_id.as_ref() {
            objects
                .entry(source_object_id.clone())
                .or_default()
                .push(atom);
        } else if atom.requires_translation {
            preserved_regions.push(TranslationPlanPreservedRegion {
                atom_ids: vec![atom.atom_id.clone()],
                reason_code: "source-object-missing",
            });
        }
    }

    let mut ordered_objects = objects.into_values().collect::<Vec<_>>();
    for atoms in &mut ordered_objects {
        atoms.sort_by_key(|atom| atom.order);
    }
    ordered_objects.sort_by_key(|atoms| atoms.first().map(|atom| atom.order).unwrap_or(u32::MAX));

    let mut units = Vec::new();
    let mut total_source_bytes = 0usize;
    for object_atoms in ordered_objects {
        let mapped_atoms = object_atoms
            .iter()
            .copied()
            .filter(|atom| atom.source_provenance.is_some())
            .collect::<Vec<_>>();
        if !object_atoms.iter().any(|atom| atom.requires_translation) {
            continue;
        }
        let atom_ids = mapped_atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let preserved_atom_ids = object_atoms
            .iter()
            .filter(|atom| atom.requires_translation)
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let preserve = |reason_code| TranslationPlanPreservedRegion {
            atom_ids: if atom_ids.is_empty() {
                preserved_atom_ids.clone()
            } else {
                atom_ids.clone()
            },
            reason_code,
        };
        if mapped_atoms.is_empty()
            || mapped_atoms.iter().any(|atom| {
                !matches!(
                    atom.source_kind,
                    PageAtomSourceKind::PdfiumVerified | PageAtomSourceKind::ToUnicodeCorrected
                )
            })
        {
            preserved_regions.push(preserve("source-mapping-unsupported"));
            continue;
        }
        let style_ids = mapped_atoms
            .iter()
            .filter_map(|atom| atom.style_id.as_deref())
            .collect::<BTreeSet<_>>();
        if style_ids.len() != 1 || mapped_atoms.iter().any(|atom| atom.style_id.is_none()) {
            preserved_regions.push(preserve("source-style-mixed"));
            continue;
        }
        let Some(provenance) = mapped_atoms[0].source_provenance.as_ref() else {
            preserved_regions.push(preserve("source-provenance-missing"));
            continue;
        };
        if mapped_atoms.iter().any(|atom| {
            atom.source_provenance
                .as_ref()
                .is_none_or(|candidate| !same_text_show(provenance, candidate))
        }) {
            preserved_regions.push(preserve("source-text-show-mixed"));
            continue;
        }
        if mapped_atoms
            .iter()
            .any(|atom| atom.source_text.chars().any(char::is_control))
        {
            preserved_regions.push(preserve("source-control-character"));
            continue;
        }

        let mut covered_spans = BTreeMap::<String, &ProtectedSpan>::new();
        let atom_id_set = atom_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let mut partial_span = false;
        for atom_id in &atom_ids {
            if let Some(spans) = spans_by_atom.get(atom_id.as_str()) {
                for span in spans {
                    if span
                        .atom_ids
                        .iter()
                        .all(|candidate| atom_id_set.contains(candidate.as_str()))
                    {
                        covered_spans.insert(span.span_id.clone(), span);
                    } else {
                        partial_span = true;
                    }
                }
            }
        }
        if partial_span {
            preserved_regions.push(preserve("protected-span-crosses-source-object"));
            continue;
        }

        let source_text = mapped_atoms
            .iter()
            .map(|atom| atom.source_text.as_str())
            .collect::<String>();
        if !find_placeholder_tokens(&source_text).is_empty() {
            preserved_regions.push(preserve("source-placeholder-collision"));
            continue;
        }
        if source_text.len() > MAX_UNIT_SOURCE_BYTES {
            return Err(TranslationPlanError::UnitSourceTooLarge {
                bytes: source_text.len(),
                maximum: MAX_UNIT_SOURCE_BYTES,
            });
        }
        total_source_bytes = total_source_bytes.checked_add(source_text.len()).ok_or(
            TranslationPlanError::PlanSourceTooLarge {
                bytes: usize::MAX,
                maximum: MAX_PLAN_SOURCE_BYTES,
            },
        )?;
        if total_source_bytes > MAX_PLAN_SOURCE_BYTES {
            return Err(TranslationPlanError::PlanSourceTooLarge {
                bytes: total_source_bytes,
                maximum: MAX_PLAN_SOURCE_BYTES,
            });
        }

        let protected_ranges = match protected_ranges(&mapped_atoms, covered_spans.into_values()) {
            Ok(ranges) => ranges,
            Err(reason_code) => {
                preserved_regions.push(preserve(reason_code));
                continue;
            }
        };
        let protected_atom_ids = protected_ranges
            .iter()
            .flat_map(|range| range.span.atom_ids.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        if !mapped_atoms.iter().any(|atom| {
            atom.requires_translation && !protected_atom_ids.contains(atom.atom_id.as_str())
        }) {
            continue;
        }

        let (provider_text, protected_spans) = build_provider_text(&source_text, &protected_ranges);
        let first_order = mapped_atoms[0].order;
        units.push(TranslationUnitPlan {
            unit_id: unit_id(page, &mapped_atoms),
            order_on_page: first_order,
            atom_ids,
            source_text,
            provider_text,
            protected_spans,
        });
        if units.len() > MAX_PLAN_UNITS {
            return Err(TranslationPlanError::TooManyUnits {
                count: units.len(),
                maximum: MAX_PLAN_UNITS,
            });
        }
    }

    Ok(TranslationPagePlan {
        schema_version: TRANSLATION_PLAN_SCHEMA_VERSION,
        page_number: page.page_number,
        source_page_hash: page.source_page_hash.clone(),
        units,
        preserved_regions,
    })
}

pub(crate) fn reassemble_translation_patch_draft(
    page: &PageGraph,
    plan: &TranslationPagePlan,
    results: Vec<TranslationUnitResult>,
    metadata: TranslationPatchDraftMetadata,
) -> Result<TranslationPatchDraft, TranslationPlanError> {
    validate_plan_binding(page, plan)?;
    if results.len() != plan.units.len() {
        return Err(TranslationPlanError::ResultCountMismatch {
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
            return Err(TranslationPlanError::UnknownResult(result.unit_id));
        }
        let result_id = result.unit_id.clone();
        if results_by_id.insert(result_id.clone(), result).is_some() {
            return Err(TranslationPlanError::DuplicateResult(result_id));
        }
    }

    let mut entries = Vec::with_capacity(plan.units.len());
    for unit in &plan.units {
        let result = results_by_id
            .remove(unit.unit_id.as_str())
            .ok_or_else(|| TranslationPlanError::MissingResult(unit.unit_id.clone()))?;
        let (translated_text, protected_spans) = reassemble_unit(unit, &result.translated_text)?;
        entries.push(TranslationPatchEntryDraft {
            atom_ids: unit.atom_ids.clone(),
            translated_text,
            protected_spans,
        });
    }

    Ok(TranslationPatchDraft {
        target_language: metadata.target_language,
        translation_revision: metadata.translation_revision,
        provider_id: metadata.provider_id,
        model_id: metadata.model_id,
        renderer_version: metadata.renderer_version,
        entries,
    })
}

pub(crate) fn build_translation_patch_from_plan(
    page: &PageGraph,
    plan: &TranslationPagePlan,
    results: Vec<TranslationUnitResult>,
    metadata: TranslationPatchDraftMetadata,
) -> Result<TranslationPatch, TranslationPlanError> {
    if plan.units.is_empty() {
        return Err(TranslationPlanError::NoTranslatableUnits);
    }
    let draft = reassemble_translation_patch_draft(page, plan, results, metadata)?;
    Ok(build_translation_patch(page, draft)?)
}

fn validate_page_graph(page: &PageGraph) -> Result<(), TranslationPlanError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(TranslationPlanError::UnsupportedPageGraphSchema {
            expected: PAGE_GRAPH_SCHEMA_VERSION,
            actual: page.schema_version,
        });
    }
    if page.page_number == 0 || page.source_page_hash.is_empty() {
        return Err(TranslationPlanError::InvalidPageGraph(
            "page identity is invalid",
        ));
    }
    let mut atom_ids = BTreeSet::new();
    let mut atom_orders = BTreeSet::new();
    for atom in &page.atoms {
        if atom.atom_id.is_empty()
            || atom.source_text.is_empty()
            || !atom_ids.insert(atom.atom_id.as_str())
            || !atom_orders.insert(atom.order)
        {
            return Err(TranslationPlanError::InvalidPageGraph(
                "atom identity or order is invalid",
            ));
        }
    }
    Ok(())
}

fn index_protected_spans(
    page: &PageGraph,
) -> Result<BTreeMap<&str, Vec<&ProtectedSpan>>, TranslationPlanError> {
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let mut span_ids = BTreeSet::new();
    let mut claimed_atoms = BTreeSet::new();
    let mut spans_by_atom = BTreeMap::<&str, Vec<&ProtectedSpan>>::new();
    for span in &page.protected_spans {
        if span.span_id.is_empty()
            || span.exact_text.is_empty()
            || span.atom_ids.is_empty()
            || !span_ids.insert(span.span_id.as_str())
        {
            return Err(TranslationPlanError::InvalidPageGraph(
                "protected span identity is invalid",
            ));
        }
        let mut source_text = String::new();
        let mut previous_order = None;
        for atom_id in &span.atom_ids {
            let atom = atoms_by_id.get(atom_id.as_str()).copied().ok_or(
                TranslationPlanError::InvalidPageGraph("protected span atom is missing"),
            )?;
            if previous_order.is_some_and(|order| atom.order <= order)
                || !claimed_atoms.insert(atom_id.as_str())
            {
                return Err(TranslationPlanError::InvalidPageGraph(
                    "protected spans overlap or are out of order",
                ));
            }
            previous_order = Some(atom.order);
            source_text.push_str(&atom.source_text);
            spans_by_atom.entry(atom_id).or_default().push(span);
        }
        if source_text != span.exact_text {
            return Err(TranslationPlanError::InvalidPageGraph(
                "protected span text does not match its atoms",
            ));
        }
    }
    Ok(spans_by_atom)
}

fn validate_plan_binding(
    page: &PageGraph,
    plan: &TranslationPagePlan,
) -> Result<(), TranslationPlanError> {
    if plan.schema_version != TRANSLATION_PLAN_SCHEMA_VERSION
        || plan.page_number != page.page_number
    {
        return Err(TranslationPlanError::PlanPageMismatch);
    }
    if plan.source_page_hash != page.source_page_hash {
        return Err(TranslationPlanError::PlanSourceMismatch);
    }
    if build_translation_page_plan(page)? != *plan {
        return Err(TranslationPlanError::PlanContentMismatch);
    }
    Ok(())
}

struct ProtectedRange<'a> {
    start: usize,
    end: usize,
    span: &'a ProtectedSpan,
}

fn protected_ranges<'a>(
    atoms: &[&PageAtom],
    spans: impl IntoIterator<Item = &'a ProtectedSpan>,
) -> Result<Vec<ProtectedRange<'a>>, &'static str> {
    let mut offsets = BTreeMap::<&str, (usize, usize, usize)>::new();
    let mut cursor = 0usize;
    for (index, atom) in atoms.iter().enumerate() {
        let end = cursor.saturating_add(atom.source_text.len());
        offsets.insert(atom.atom_id.as_str(), (index, cursor, end));
        cursor = end;
    }
    let mut ranges = Vec::new();
    for span in spans {
        let covered = span
            .atom_ids
            .iter()
            .map(|atom_id| offsets.get(atom_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
            .ok_or("protected-span-crosses-source-object")?;
        let Some(first) = covered.first().copied() else {
            return Err("protected-span-empty");
        };
        let Some(last) = covered.last().copied() else {
            return Err("protected-span-empty");
        };
        if covered
            .iter()
            .enumerate()
            .any(|(offset, (index, _, _))| *index != first.0 + offset)
        {
            return Err("protected-span-noncontiguous");
        }
        ranges.push(ProtectedRange {
            start: first.1,
            end: last.2,
            span,
        });
    }
    ranges.sort_by_key(|range| range.start);
    if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err("protected-span-overlap");
    }
    Ok(ranges)
}

fn build_provider_text(
    source_text: &str,
    ranges: &[ProtectedRange<'_>],
) -> (String, Vec<TranslationUnitProtectedSpan>) {
    let mut provider_text = String::with_capacity(source_text.len());
    let mut protected_spans = Vec::with_capacity(ranges.len());
    let mut cursor = 0usize;
    let mut token_index = 0u64;
    for range in ranges {
        provider_text.push_str(&source_text[cursor..range.start]);
        let token = loop {
            let candidate = format!("{{v{token_index}}}");
            token_index = token_index.saturating_add(1);
            if !source_text.contains(&candidate)
                && protected_spans
                    .iter()
                    .all(|span: &TranslationUnitProtectedSpan| span.token != candidate)
            {
                break candidate;
            }
        };
        provider_text.push_str(&token);
        protected_spans.push(TranslationUnitProtectedSpan {
            span_id: range.span.span_id.clone(),
            kind: range.span.kind,
            token,
            exact_text: range.span.exact_text.clone(),
        });
        cursor = range.end;
    }
    provider_text.push_str(&source_text[cursor..]);
    (provider_text, protected_spans)
}

fn reassemble_unit(
    unit: &TranslationUnitPlan,
    provider_text: &str,
) -> Result<(String, Vec<TranslationPatchProtectedSpanPlacement>), TranslationPlanError> {
    if provider_text.trim().is_empty() {
        return Err(TranslationPlanError::EmptyTranslation(unit.unit_id.clone()));
    }
    if provider_text.chars().any(char::is_control) {
        return Err(TranslationPlanError::TranslationControlCharacter(
            unit.unit_id.clone(),
        ));
    }
    let expected_tokens = unit
        .protected_spans
        .iter()
        .map(|span| span.token.as_str())
        .collect::<Vec<_>>();
    let actual_tokens = find_placeholder_tokens(provider_text);
    for token in &actual_tokens {
        if !expected_tokens.contains(&token.as_str()) {
            return Err(TranslationPlanError::UnknownProtectedToken {
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
            return Err(TranslationPlanError::MissingProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: (*token).to_string(),
            });
        }
        if count > 1 {
            return Err(TranslationPlanError::DuplicateProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: (*token).to_string(),
            });
        }
    }
    for (index, expected) in expected_tokens.iter().enumerate() {
        if actual_tokens.get(index).map(String::as_str) != Some(*expected) {
            return Err(TranslationPlanError::ReorderedProtectedToken {
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
            TranslationPlanError::MissingProtectedToken {
                unit_id: unit.unit_id.clone(),
                token: span.token.clone(),
            }
        })?;
        let start = cursor + relative;
        translated_text.push_str(&provider_text[cursor..start]);
        let translated_start = u32::try_from(translated_text.len()).map_err(|_| {
            TranslationPlanError::PlanSourceTooLarge {
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

fn same_text_show(expected: &PageAtomSourceProvenance, actual: &PageAtomSourceProvenance) -> bool {
    expected.text_show_id == actual.text_show_id
        && expected.form_invocation_path == actual.form_invocation_path
        && expected.stream_object_number == actual.stream_object_number
        && expected.stream_generation == actual.stream_generation
        && expected.operation_index == actual.operation_index
        && expected.text_show_operator == actual.text_show_operator
        && expected.text_show_operand_hash == actual.text_show_operand_hash
        && expected.source_font_resource == actual.source_font_resource
        && expected.source_font_size == actual.source_font_size
        && expected.source_horizontal_scaling == actual.source_horizontal_scaling
}

fn unit_id(page: &PageGraph, atoms: &[&PageAtom]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rosetta-pdf-v3-translation-unit/1\0");
    hasher.update(page.source_page_hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(page.page_number.to_le_bytes());
    for atom in atoms {
        hasher.update(b"\0");
        hasher.update(atom.atom_id.as_bytes());
    }
    format!("unit-{}", hex_digest(hasher.finalize()))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        build_translation_page_plan, build_translation_patch_from_plan,
        reassemble_translation_patch_draft, TranslationPatchDraftMetadata, TranslationPlanError,
        TranslationUnitResult, MAX_UNIT_SOURCE_BYTES,
    };
    use crate::pdf_v3::{
        patch_renderer::TRANSLATION_PATCH_RENDERER_VERSION,
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageAtomSourceProvenance, PageGraph,
            PageReconciliationSummary, PageStyle, ProtectedSpan, ProtectedSpanKind,
            PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    #[test]
    fn plan_reassembles_protected_text_by_atom_identity() {
        let page = protected_page();
        let plan = build_translation_page_plan(&page).expect("translation plan");

        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].source_text, "See [1] now");
        assert_eq!(plan.units[0].provider_text, "See {v0} now");
        let patch = build_translation_patch_from_plan(
            &page,
            &plan,
            vec![result(&plan.units[0].unit_id, "参见{v0}。")],
            metadata(),
        )
        .expect("translation patch");

        assert_eq!(patch.entries.len(), 1);
        assert_eq!(patch.entries[0].translated_text, "参见[1]。");
        assert_eq!(patch.entries[0].protected_spans.len(), 1);
        assert_eq!(patch.entries[0].protected_spans[0].exact_text, "[1]");
        assert_eq!(patch.entries[0].protected_spans[0].translated_start, 6);
    }

    #[test]
    fn duplicate_source_text_keeps_distinct_stable_unit_identity() {
        let mut page = page_from_objects(&[("object-a", "Same"), ("object-b", "Same")]);
        let first = build_translation_page_plan(&page).expect("first plan");
        let second = build_translation_page_plan(&page).expect("second plan");

        assert_eq!(first, second);
        assert_eq!(first.units.len(), 2);
        assert_ne!(first.units[0].unit_id, first.units[1].unit_id);

        page.atoms[0].atom_id = "changed-atom".to_string();
        let changed = build_translation_page_plan(&page).expect("changed plan");
        assert_ne!(first.units[0].unit_id, changed.units[0].unit_id);
    }

    #[test]
    fn result_order_does_not_change_patch_order() {
        let page = page_from_objects(&[("object-a", "First"), ("object-b", "Second")]);
        let plan = build_translation_page_plan(&page).expect("translation plan");
        let draft = reassemble_translation_patch_draft(
            &page,
            &plan,
            vec![
                result(&plan.units[1].unit_id, "第二"),
                result(&plan.units[0].unit_id, "第一"),
            ],
            metadata(),
        )
        .expect("patch draft");

        assert_eq!(draft.entries[0].translated_text, "第一");
        assert_eq!(draft.entries[1].translated_text, "第二");
    }

    #[test]
    fn missing_duplicate_and_unknown_results_are_rejected() {
        let page = page_from_objects(&[("object-a", "First"), ("object-b", "Second")]);
        let plan = build_translation_page_plan(&page).expect("translation plan");
        let missing = reassemble_translation_patch_draft(
            &page,
            &plan,
            vec![result(&plan.units[0].unit_id, "第一")],
            metadata(),
        )
        .expect_err("missing result");
        assert!(matches!(
            missing,
            TranslationPlanError::ResultCountMismatch {
                expected: 2,
                actual: 1
            }
        ));

        let duplicate = reassemble_translation_patch_draft(
            &page,
            &plan,
            vec![
                result(&plan.units[0].unit_id, "第一"),
                result(&plan.units[0].unit_id, "重复"),
            ],
            metadata(),
        )
        .expect_err("duplicate result");
        assert!(matches!(
            duplicate,
            TranslationPlanError::DuplicateResult(_)
        ));

        let unknown = reassemble_translation_patch_draft(
            &page,
            &plan,
            vec![
                result(&plan.units[0].unit_id, "第一"),
                result("unit-unknown", "未知"),
            ],
            metadata(),
        )
        .expect_err("unknown result");
        assert!(matches!(unknown, TranslationPlanError::UnknownResult(_)));
    }

    #[test]
    fn protected_tokens_must_be_exact_unique_and_ordered() {
        let mut page = protected_page();
        page.protected_spans.push(ProtectedSpan {
            span_id: "span-now".to_string(),
            kind: ProtectedSpanKind::Style,
            atom_ids: vec![
                "atom-8".to_string(),
                "atom-9".to_string(),
                "atom-10".to_string(),
            ],
            exact_text: "now".to_string(),
        });
        let plan = build_translation_page_plan(&page).expect("translation plan");
        assert_eq!(plan.units[0].provider_text, "See {v0} {v1}");

        for (text, expected) in [
            ("参见{v1}{v0}", "reordered"),
            ("参见{v0}", "missing"),
            ("参见{v0}{v1}{v1}", "duplicate"),
            ("参见{v0}{v1}{v9}", "unknown"),
        ] {
            let error = reassemble_translation_patch_draft(
                &page,
                &plan,
                vec![result(&plan.units[0].unit_id, text)],
                metadata(),
            )
            .expect_err(expected);
            match expected {
                "reordered" => assert!(matches!(
                    error,
                    TranslationPlanError::ReorderedProtectedToken { .. }
                )),
                "missing" => assert!(matches!(
                    error,
                    TranslationPlanError::MissingProtectedToken { .. }
                )),
                "duplicate" => assert!(matches!(
                    error,
                    TranslationPlanError::DuplicateProtectedToken { .. }
                )),
                "unknown" => assert!(matches!(
                    error,
                    TranslationPlanError::UnknownProtectedToken { .. }
                )),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn mixed_style_and_cross_object_protection_preserve_source() {
        let mut mixed = page_from_objects(&[("object-a", "Mixed")]);
        mixed.atoms[1].style_id = Some("style-2".to_string());
        let mixed_plan = build_translation_page_plan(&mixed).expect("mixed plan");
        assert!(mixed_plan.units.is_empty());
        assert_eq!(
            mixed_plan.preserved_regions[0].reason_code,
            "source-style-mixed"
        );

        let mut crossed = page_from_objects(&[("object-a", "A"), ("object-b", "B")]);
        crossed.protected_spans.push(ProtectedSpan {
            span_id: "span-crossed".to_string(),
            kind: ProtectedSpanKind::Citation,
            atom_ids: vec!["atom-0".to_string(), "atom-1".to_string()],
            exact_text: "AB".to_string(),
        });
        let crossed_plan = build_translation_page_plan(&crossed).expect("crossed plan");
        assert!(crossed_plan.units.is_empty());
        assert_eq!(crossed_plan.preserved_regions.len(), 2);
        assert!(crossed_plan
            .preserved_regions
            .iter()
            .all(|region| { region.reason_code == "protected-span-crosses-source-object" }));
        let error =
            build_translation_patch_from_plan(&crossed, &crossed_plan, Vec::new(), metadata())
                .expect_err("empty safe plan must preserve the page explicitly");
        assert_eq!(error, TranslationPlanError::NoTranslatableUnits);
    }

    #[test]
    fn stale_or_tampered_plan_is_rejected_before_reassembly() {
        let page = page_from_objects(&[("object-a", "Text")]);
        let mut plan = build_translation_page_plan(&page).expect("translation plan");
        plan.units[0].atom_ids.reverse();
        let error = reassemble_translation_patch_draft(
            &page,
            &plan,
            vec![result(&plan.units[0].unit_id, "译文")],
            metadata(),
        )
        .expect_err("tampered plan");

        assert_eq!(error, TranslationPlanError::PlanContentMismatch);
    }

    #[test]
    fn plan_enforces_unit_byte_bound_and_preserves_source_placeholder_collisions() {
        let mut oversized = page_from_objects(&[("object-a", "A")]);
        oversized.atoms[0].source_text = "x".repeat(MAX_UNIT_SOURCE_BYTES + 1);
        let error = build_translation_page_plan(&oversized).expect_err("oversized unit");
        assert!(matches!(
            error,
            TranslationPlanError::UnitSourceTooLarge { .. }
        ));

        let collision = page_from_objects(&[("object-a", "Keep {v0}")]);
        let plan = build_translation_page_plan(&collision).expect("collision plan");
        assert!(plan.units.is_empty());
        assert_eq!(
            plan.preserved_regions[0].reason_code,
            "source-placeholder-collision"
        );
    }

    fn metadata() -> TranslationPatchDraftMetadata {
        TranslationPatchDraftMetadata {
            target_language: "zh-CN".to_string(),
            translation_revision: 1,
            provider_id: "provider-test".to_string(),
            model_id: "model-test".to_string(),
            renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
        }
    }

    fn result(unit_id: &str, translated_text: &str) -> TranslationUnitResult {
        TranslationUnitResult {
            unit_id: unit_id.to_string(),
            translated_text: translated_text.to_string(),
        }
    }

    fn protected_page() -> PageGraph {
        let mut page = page_from_objects(&[("object-a", "See [1] now")]);
        page.protected_spans.push(ProtectedSpan {
            span_id: "span-citation".to_string(),
            kind: ProtectedSpanKind::Citation,
            atom_ids: vec![
                "atom-4".to_string(),
                "atom-5".to_string(),
                "atom-6".to_string(),
            ],
            exact_text: "[1]".to_string(),
        });
        page
    }

    fn page_from_objects(objects: &[(&str, &str)]) -> PageGraph {
        let mut atoms = Vec::new();
        let mut order = 0u32;
        for (object_index, (object_id, text)) in objects.iter().enumerate() {
            for character in text.chars() {
                atoms.push(atom(
                    order,
                    object_id,
                    &format!("show-{object_index}"),
                    character,
                ));
                order += 1;
            }
        }
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:test-page".to_string(),
            page_width: 100.0,
            page_height: 100.0,
            rotation_degrees: 0,
            reconciliation: PageReconciliationSummary::unreconciled(atoms.len()),
            atoms,
            styles: vec![style("style-1"), style("style-2")],
            groups: Vec::new(),
            protected_spans: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn atom(order: u32, object_id: &str, text_show_id: &str, character: char) -> PageAtom {
        PageAtom {
            atom_id: format!("atom-{order}"),
            source_text: character.to_string(),
            source_object_id: Some(object_id.to_string()),
            kind: PageAtomKind::Body,
            style_id: Some("style-1".to_string()),
            bounds: [order as f32, 0.0, order as f32 + 1.0, 1.0],
            loose_bounds: None,
            origin: Some([order as f32, 0.0]),
            text_matrix: Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            angle_degrees: Some(0.0),
            order,
            generated: false,
            hyphen: false,
            requires_translation: !character.is_whitespace(),
            source_kind: PageAtomSourceKind::PdfiumVerified,
            source_provenance: Some(PageAtomSourceProvenance {
                mapping_id: format!("mapping-{text_show_id}"),
                text_show_id: text_show_id.to_string(),
                text_show_index: 0,
                operand_id: format!("operand-{text_show_id}"),
                operand_index: 0,
                array_index: None,
                encoded_start: order as usize,
                encoded_len: 1,
                source_unit_char_index: 0,
                source_unit_char_count: 1,
                form_invocation_path: Vec::new(),
                stream_object_number: 4,
                stream_generation: 0,
                operation_index: 7,
                text_show_operator: "Tj".to_string(),
                text_show_operand_hash:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                source_font_resource: Some("F1".to_string()),
                source_font_size: Some(10.0),
                source_horizontal_scaling: 100.0,
            }),
        }
    }

    fn style(style_id: &str) -> PageStyle {
        PageStyle {
            style_id: style_id.to_string(),
            font_resource: Some("F1".to_string()),
            font_size: 10.0,
            scaled_font_size: 10.0,
            font_weight: Some(400),
            italic: false,
            serif: false,
            fill_color: Some([0.0, 0.0, 0.0, 1.0]),
            stroke_color: None,
            opacity: Some(1.0),
            render_mode: Some("fill".to_string()),
        }
    }
}
