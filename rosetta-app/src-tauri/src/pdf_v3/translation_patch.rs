use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use sha2::{Digest, Sha256};

use super::types::{
    PageAtom, PageAtomSourceKind, PageGraph, ProtectedSpan, TranslationPatch,
    TranslationPatchAtomRef, TranslationPatchEntry, TranslationPatchProtectedSpan,
    TranslationPatchProvider, TranslationPatchRendererDecision, PAGE_GRAPH_SCHEMA_VERSION,
    TRANSLATION_PATCH_SCHEMA_VERSION,
};

const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATCH_ENTRIES: usize = 100_000;
const MAX_TRANSLATED_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct TranslationPatchDraft {
    pub target_language: String,
    pub translation_revision: u64,
    pub provider_id: String,
    pub model_id: String,
    pub renderer_version: String,
    pub entries: Vec<TranslationPatchEntryDraft>,
}

#[derive(Debug, Clone)]
pub(crate) struct TranslationPatchEntryDraft {
    pub atom_ids: Vec<String>,
    pub translated_text: String,
    pub protected_spans: Vec<TranslationPatchProtectedSpanPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationPatchProtectedSpanPlacement {
    pub span_id: String,
    pub translated_start: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranslationPatchError {
    UnsupportedPageGraphSchema { expected: u32, actual: u32 },
    UnsupportedPatchSchema { expected: u32, actual: u32 },
    InvalidMetadata(&'static str),
    TooManyEntries { count: usize, maximum: usize },
    PatchTooLarge { bytes: usize, maximum: usize },
    EmptyEntry,
    TranslationTooLarge { bytes: usize, maximum: usize },
    UnknownAtom(String),
    DuplicateAtom(String),
    UnsupportedAtom(String),
    InconsistentStyle,
    UnknownProtectedSpan(String),
    PartialProtectedSpan(String),
    MissingProtectedSpan(String),
    DuplicateProtectedSpan(String),
    InvalidSourceProtectedSpan(String),
    InvalidProtectedSpanPlacement(String),
    PageMismatch { expected: u32, actual: u32 },
    SourcePageHashMismatch,
    EntryMismatch(String),
    PatchIdMismatch,
    InvalidRendererDecision(String),
    RendererDecisionPending(String),
    RendererDecisionAlreadyResolved(String),
    MissingRendererDecision(String),
    UnknownRendererDecision(String),
    Serialization(String),
}

impl fmt::Display for TranslationPatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPageGraphSchema { expected, actual } => write!(
                formatter,
                "PageGraph schema mismatch: expected {expected}, found {actual}"
            ),
            Self::UnsupportedPatchSchema { expected, actual } => write!(
                formatter,
                "TranslationPatch schema mismatch: expected {expected}, found {actual}"
            ),
            Self::InvalidMetadata(field) => {
                write!(
                    formatter,
                    "TranslationPatch metadata field {field} is invalid"
                )
            }
            Self::TooManyEntries { count, maximum } => write!(
                formatter,
                "TranslationPatch has {count} entries, above maximum {maximum}"
            ),
            Self::PatchTooLarge { bytes, maximum } => write!(
                formatter,
                "TranslationPatch has {bytes} bytes, above maximum {maximum}"
            ),
            Self::EmptyEntry => formatter.write_str("TranslationPatch entry is empty"),
            Self::TranslationTooLarge { bytes, maximum } => write!(
                formatter,
                "TranslationPatch entry has {bytes} translated bytes, above maximum {maximum}"
            ),
            Self::UnknownAtom(atom_id) => write!(formatter, "unknown PageGraph atom {atom_id}"),
            Self::DuplicateAtom(atom_id) => {
                write!(formatter, "PageGraph atom {atom_id} is used more than once")
            }
            Self::UnsupportedAtom(atom_id) => {
                write!(formatter, "PageGraph atom {atom_id} cannot be patched")
            }
            Self::InconsistentStyle => {
                formatter.write_str("TranslationPatch entry atoms must share one source style")
            }
            Self::UnknownProtectedSpan(span_id) => {
                write!(formatter, "unknown protected span {span_id}")
            }
            Self::PartialProtectedSpan(span_id) => write!(
                formatter,
                "protected span {span_id} is only partially covered by the entry"
            ),
            Self::MissingProtectedSpan(span_id) => write!(
                formatter,
                "protected span {span_id} has no translated placement"
            ),
            Self::DuplicateProtectedSpan(span_id) => write!(
                formatter,
                "protected span {span_id} has more than one translated placement"
            ),
            Self::InvalidSourceProtectedSpan(span_id) => {
                write!(formatter, "PageGraph protected span {span_id} is invalid")
            }
            Self::InvalidProtectedSpanPlacement(span_id) => write!(
                formatter,
                "protected span {span_id} does not match its translated byte range"
            ),
            Self::PageMismatch { expected, actual } => write!(
                formatter,
                "TranslationPatch page mismatch: expected {expected}, found {actual}"
            ),
            Self::SourcePageHashMismatch => {
                formatter.write_str("TranslationPatch source page hash is stale")
            }
            Self::EntryMismatch(entry_id) => {
                write!(
                    formatter,
                    "TranslationPatch entry {entry_id} is stale or non-canonical"
                )
            }
            Self::PatchIdMismatch => {
                formatter.write_str("TranslationPatch patchId does not match its content")
            }
            Self::InvalidRendererDecision(entry_id) => write!(
                formatter,
                "TranslationPatch entry {entry_id} has an invalid renderer decision"
            ),
            Self::RendererDecisionPending(entry_id) => write!(
                formatter,
                "TranslationPatch entry {entry_id} renderer decision is still pending"
            ),
            Self::RendererDecisionAlreadyResolved(entry_id) => write!(
                formatter,
                "TranslationPatch entry {entry_id} renderer decision is already resolved"
            ),
            Self::MissingRendererDecision(entry_id) => write!(
                formatter,
                "TranslationPatch entry {entry_id} is missing a renderer decision"
            ),
            Self::UnknownRendererDecision(entry_id) => write!(
                formatter,
                "TranslationPatch renderer decision references unknown entry {entry_id}"
            ),
            Self::Serialization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TranslationPatchError {}

pub(crate) fn build_translation_patch(
    page: &PageGraph,
    draft: TranslationPatchDraft,
) -> Result<TranslationPatch, TranslationPatchError> {
    validate_page_schema(page)?;
    validate_source_protected_spans(page)?;
    validate_metadata(&draft)?;
    if draft.entries.len() > MAX_PATCH_ENTRIES {
        return Err(TranslationPatchError::TooManyEntries {
            count: draft.entries.len(),
            maximum: MAX_PATCH_ENTRIES,
        });
    }

    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let spans_by_id = page
        .protected_spans
        .iter()
        .map(|span| (span.span_id.as_str(), span))
        .collect::<BTreeMap<_, _>>();
    let mut used_atoms = BTreeSet::new();
    let mut entries = Vec::with_capacity(draft.entries.len());
    let mut translated_bytes = 0usize;

    for entry in draft.entries {
        if entry.atom_ids.is_empty() || entry.translated_text.is_empty() {
            return Err(TranslationPatchError::EmptyEntry);
        }
        if entry.translated_text.len() > MAX_TRANSLATED_TEXT_BYTES {
            return Err(TranslationPatchError::TranslationTooLarge {
                bytes: entry.translated_text.len(),
                maximum: MAX_TRANSLATED_TEXT_BYTES,
            });
        }
        translated_bytes = translated_bytes
            .checked_add(entry.translated_text.len())
            .ok_or(TranslationPatchError::PatchTooLarge {
                bytes: usize::MAX,
                maximum: MAX_PATCH_BYTES,
            })?;
        if translated_bytes > MAX_PATCH_BYTES {
            return Err(TranslationPatchError::PatchTooLarge {
                bytes: translated_bytes,
                maximum: MAX_PATCH_BYTES,
            });
        }

        let mut atoms = entry
            .atom_ids
            .iter()
            .map(|atom_id| {
                atoms_by_id
                    .get(atom_id.as_str())
                    .copied()
                    .ok_or_else(|| TranslationPatchError::UnknownAtom(atom_id.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        atoms.sort_by_key(|atom| atom.order);
        let mut entry_atom_ids = BTreeSet::new();
        for atom in &atoms {
            if !entry_atom_ids.insert(atom.atom_id.as_str())
                || !used_atoms.insert(atom.atom_id.as_str())
            {
                return Err(TranslationPatchError::DuplicateAtom(atom.atom_id.clone()));
            }
            if atom.source_kind == PageAtomSourceKind::PreservedUnmapped {
                return Err(TranslationPatchError::UnsupportedAtom(atom.atom_id.clone()));
            }
        }
        let style_ids = atoms
            .iter()
            .filter_map(|atom| atom.style_id.as_deref())
            .collect::<BTreeSet<_>>();
        if style_ids.len() != 1 || atoms.iter().any(|atom| atom.style_id.is_none()) {
            return Err(TranslationPatchError::InconsistentStyle);
        }
        let style_id = style_ids
            .first()
            .copied()
            .ok_or(TranslationPatchError::InconsistentStyle)?
            .to_string();
        let atom_id_set = atoms
            .iter()
            .map(|atom| atom.atom_id.as_str())
            .collect::<BTreeSet<_>>();
        let covered_spans = page
            .protected_spans
            .iter()
            .filter(|span| {
                span.atom_ids
                    .iter()
                    .any(|atom_id| atom_id_set.contains(atom_id.as_str()))
            })
            .map(|span| {
                if span
                    .atom_ids
                    .iter()
                    .all(|atom_id| atom_id_set.contains(atom_id.as_str()))
                {
                    Ok(span)
                } else {
                    Err(TranslationPatchError::PartialProtectedSpan(
                        span.span_id.clone(),
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let protected_spans = build_protected_spans(
            &entry.translated_text,
            &entry.protected_spans,
            &covered_spans,
            &spans_by_id,
        )?;
        let atom_refs = atoms
            .iter()
            .map(|atom| {
                Ok(TranslationPatchAtomRef {
                    atom_id: atom.atom_id.clone(),
                    source_atom_hash: source_atom_hash(atom)?,
                })
            })
            .collect::<Result<Vec<_>, TranslationPatchError>>()?;
        let entry_id = entry_id(&page.source_page_hash, &atom_refs);
        let first_order = atoms
            .first()
            .map(|atom| atom.order)
            .ok_or(TranslationPatchError::EmptyEntry)?;
        entries.push((
            first_order,
            TranslationPatchEntry {
                entry_id,
                atoms: atom_refs,
                translated_text: entry.translated_text,
                protected_spans,
                style_id,
                renderer_decision: TranslationPatchRendererDecision::Pending,
            },
        ));
    }
    entries.sort_by_key(|(first_order, _)| *first_order);

    let mut patch = TranslationPatch {
        schema_version: TRANSLATION_PATCH_SCHEMA_VERSION,
        patch_id: String::new(),
        page_number: page.page_number,
        source_page_hash: page.source_page_hash.clone(),
        target_language: draft.target_language,
        translation_revision: draft.translation_revision,
        provider: TranslationPatchProvider {
            provider_id: draft.provider_id,
            model_id: draft.model_id,
        },
        entries: entries.into_iter().map(|(_, entry)| entry).collect(),
        renderer_version: draft.renderer_version,
    };
    patch.patch_id = patch_id(&patch)?;
    encode_translation_patch(&patch)?;
    Ok(patch)
}

pub(crate) fn encode_translation_patch(
    patch: &TranslationPatch,
) -> Result<Vec<u8>, TranslationPatchError> {
    let bytes = serde_json::to_vec(patch).map_err(|error| {
        TranslationPatchError::Serialization(format!("failed to encode TranslationPatch: {error}"))
    })?;
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(TranslationPatchError::PatchTooLarge {
            bytes: bytes.len(),
            maximum: MAX_PATCH_BYTES,
        });
    }
    Ok(bytes)
}

pub(crate) fn decode_and_validate_translation_patch(
    page: &PageGraph,
    bytes: &[u8],
) -> Result<TranslationPatch, TranslationPatchError> {
    let patch = decode_and_validate_translation_patch_identity(bytes)?;
    validate_translation_patch(page, &patch)?;
    Ok(patch)
}

pub(crate) fn decode_and_validate_translation_patch_identity(
    bytes: &[u8],
) -> Result<TranslationPatch, TranslationPatchError> {
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(TranslationPatchError::PatchTooLarge {
            bytes: bytes.len(),
            maximum: MAX_PATCH_BYTES,
        });
    }
    let patch = serde_json::from_slice::<TranslationPatch>(bytes).map_err(|error| {
        TranslationPatchError::Serialization(format!("failed to decode TranslationPatch: {error}"))
    })?;
    if patch.schema_version != TRANSLATION_PATCH_SCHEMA_VERSION {
        return Err(TranslationPatchError::UnsupportedPatchSchema {
            expected: TRANSLATION_PATCH_SCHEMA_VERSION,
            actual: patch.schema_version,
        });
    }
    if patch.page_number == 0 {
        return Err(TranslationPatchError::InvalidMetadata("pageNumber"));
    }
    if patch.source_page_hash.is_empty() {
        return Err(TranslationPatchError::InvalidMetadata("sourcePageHash"));
    }
    validate_patch_metadata(&patch)?;
    if patch.entries.len() > MAX_PATCH_ENTRIES {
        return Err(TranslationPatchError::TooManyEntries {
            count: patch.entries.len(),
            maximum: MAX_PATCH_ENTRIES,
        });
    }
    for entry in &patch.entries {
        if entry.translated_text.len() > MAX_TRANSLATED_TEXT_BYTES {
            return Err(TranslationPatchError::TranslationTooLarge {
                bytes: entry.translated_text.len(),
                maximum: MAX_TRANSLATED_TEXT_BYTES,
            });
        }
        validate_renderer_decision(&entry.entry_id, &entry.renderer_decision)?;
    }
    if patch_id(&patch)? != patch.patch_id {
        return Err(TranslationPatchError::PatchIdMismatch);
    }
    Ok(patch)
}

pub(crate) fn validate_translation_patch(
    page: &PageGraph,
    patch: &TranslationPatch,
) -> Result<(), TranslationPatchError> {
    validate_page_schema(page)?;
    if patch.schema_version != TRANSLATION_PATCH_SCHEMA_VERSION {
        return Err(TranslationPatchError::UnsupportedPatchSchema {
            expected: TRANSLATION_PATCH_SCHEMA_VERSION,
            actual: patch.schema_version,
        });
    }
    if patch.page_number != page.page_number {
        return Err(TranslationPatchError::PageMismatch {
            expected: page.page_number,
            actual: patch.page_number,
        });
    }
    if patch.source_page_hash != page.source_page_hash {
        return Err(TranslationPatchError::SourcePageHashMismatch);
    }
    let draft = TranslationPatchDraft {
        target_language: patch.target_language.clone(),
        translation_revision: patch.translation_revision,
        provider_id: patch.provider.provider_id.clone(),
        model_id: patch.provider.model_id.clone(),
        renderer_version: patch.renderer_version.clone(),
        entries: patch
            .entries
            .iter()
            .map(|entry| TranslationPatchEntryDraft {
                atom_ids: entry
                    .atoms
                    .iter()
                    .map(|atom| atom.atom_id.clone())
                    .collect(),
                translated_text: entry.translated_text.clone(),
                protected_spans: entry
                    .protected_spans
                    .iter()
                    .map(|span| TranslationPatchProtectedSpanPlacement {
                        span_id: span.span_id.clone(),
                        translated_start: span.translated_start,
                    })
                    .collect(),
            })
            .collect(),
    };
    let rebuilt = build_translation_patch(page, draft)?;
    if rebuilt.entries.len() != patch.entries.len() {
        return Err(TranslationPatchError::EntryMismatch(
            "entry-count".to_string(),
        ));
    }
    for (expected, actual) in rebuilt.entries.iter().zip(&patch.entries) {
        if expected.entry_id != actual.entry_id
            || expected.atoms != actual.atoms
            || expected.translated_text != actual.translated_text
            || expected.protected_spans != actual.protected_spans
            || expected.style_id != actual.style_id
        {
            return Err(TranslationPatchError::EntryMismatch(
                actual.entry_id.clone(),
            ));
        }
        validate_renderer_decision(&actual.entry_id, &actual.renderer_decision)?;
    }
    if patch_id(patch)? != patch.patch_id {
        return Err(TranslationPatchError::PatchIdMismatch);
    }
    Ok(())
}

pub(crate) fn resolve_translation_patch_renderer_decisions(
    page: &PageGraph,
    patch: &TranslationPatch,
    decisions: &BTreeMap<String, TranslationPatchRendererDecision>,
) -> Result<TranslationPatch, TranslationPatchError> {
    validate_translation_patch(page, patch)?;
    let entry_ids = patch
        .entries
        .iter()
        .map(|entry| entry.entry_id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(unknown) = decisions
        .keys()
        .find(|entry_id| !entry_ids.contains(entry_id.as_str()))
    {
        return Err(TranslationPatchError::UnknownRendererDecision(
            unknown.clone(),
        ));
    }

    let mut resolved = patch.clone();
    for entry in &mut resolved.entries {
        if !matches!(
            entry.renderer_decision,
            TranslationPatchRendererDecision::Pending
        ) {
            return Err(TranslationPatchError::RendererDecisionAlreadyResolved(
                entry.entry_id.clone(),
            ));
        }
        let decision = decisions.get(&entry.entry_id).cloned().ok_or_else(|| {
            TranslationPatchError::MissingRendererDecision(entry.entry_id.clone())
        })?;
        if matches!(decision, TranslationPatchRendererDecision::Pending) {
            return Err(TranslationPatchError::InvalidRendererDecision(
                entry.entry_id.clone(),
            ));
        }
        validate_renderer_decision(&entry.entry_id, &decision)?;
        entry.renderer_decision = decision;
    }
    resolved.patch_id = patch_id(&resolved)?;
    validate_translation_patch(page, &resolved)?;
    encode_translation_patch(&resolved)?;
    Ok(resolved)
}

pub(crate) fn ensure_translation_patch_renderer_resolved(
    patch: &TranslationPatch,
) -> Result<(), TranslationPatchError> {
    if let Some(entry) = patch.entries.iter().find(|entry| {
        matches!(
            entry.renderer_decision,
            TranslationPatchRendererDecision::Pending
        )
    }) {
        return Err(TranslationPatchError::RendererDecisionPending(
            entry.entry_id.clone(),
        ));
    }
    Ok(())
}

fn validate_page_schema(page: &PageGraph) -> Result<(), TranslationPatchError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(TranslationPatchError::UnsupportedPageGraphSchema {
            expected: PAGE_GRAPH_SCHEMA_VERSION,
            actual: page.schema_version,
        });
    }
    if page.page_number == 0 {
        return Err(TranslationPatchError::InvalidMetadata("pageNumber"));
    }
    if page.source_page_hash.is_empty() {
        return Err(TranslationPatchError::InvalidMetadata("sourcePageHash"));
    }
    Ok(())
}

fn validate_source_protected_spans(page: &PageGraph) -> Result<(), TranslationPatchError> {
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let mut span_ids = BTreeSet::new();

    for span in &page.protected_spans {
        if span.span_id.is_empty()
            || span.exact_text.is_empty()
            || span.atom_ids.is_empty()
            || !span_ids.insert(span.span_id.as_str())
        {
            return Err(TranslationPatchError::InvalidSourceProtectedSpan(
                span.span_id.clone(),
            ));
        }

        let mut seen_atoms = BTreeSet::new();
        let mut previous_order = None;
        let mut source_text = String::new();
        for atom_id in &span.atom_ids {
            let atom = atoms_by_id.get(atom_id.as_str()).copied().ok_or_else(|| {
                TranslationPatchError::InvalidSourceProtectedSpan(span.span_id.clone())
            })?;
            if !seen_atoms.insert(atom_id.as_str())
                || previous_order.is_some_and(|order| atom.order <= order)
            {
                return Err(TranslationPatchError::InvalidSourceProtectedSpan(
                    span.span_id.clone(),
                ));
            }
            previous_order = Some(atom.order);
            source_text.push_str(&atom.source_text);
        }
        if source_text != span.exact_text {
            return Err(TranslationPatchError::InvalidSourceProtectedSpan(
                span.span_id.clone(),
            ));
        }
    }

    Ok(())
}

fn validate_metadata(draft: &TranslationPatchDraft) -> Result<(), TranslationPatchError> {
    if draft.translation_revision == 0 {
        return Err(TranslationPatchError::InvalidMetadata(
            "translationRevision",
        ));
    }
    validate_identifier(&draft.target_language, "targetLanguage")?;
    validate_identifier(&draft.provider_id, "providerId")?;
    validate_identifier(&draft.model_id, "modelId")?;
    validate_identifier(&draft.renderer_version, "rendererVersion")?;
    Ok(())
}

fn validate_patch_metadata(patch: &TranslationPatch) -> Result<(), TranslationPatchError> {
    if patch.translation_revision == 0 {
        return Err(TranslationPatchError::InvalidMetadata(
            "translationRevision",
        ));
    }
    validate_identifier(&patch.target_language, "targetLanguage")?;
    validate_identifier(&patch.provider.provider_id, "providerId")?;
    validate_identifier(&patch.provider.model_id, "modelId")?;
    validate_identifier(&patch.renderer_version, "rendererVersion")?;
    Ok(())
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), TranslationPatchError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(TranslationPatchError::InvalidMetadata(field));
    }
    Ok(())
}

fn build_protected_spans(
    translated_text: &str,
    placements: &[TranslationPatchProtectedSpanPlacement],
    covered_spans: &[&ProtectedSpan],
    spans_by_id: &BTreeMap<&str, &ProtectedSpan>,
) -> Result<Vec<TranslationPatchProtectedSpan>, TranslationPatchError> {
    let covered_ids = covered_spans
        .iter()
        .map(|span| span.span_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(placements.len());
    for placement in placements {
        let span = spans_by_id
            .get(placement.span_id.as_str())
            .copied()
            .ok_or_else(|| {
                TranslationPatchError::UnknownProtectedSpan(placement.span_id.clone())
            })?;
        if !covered_ids.contains(span.span_id.as_str()) {
            return Err(TranslationPatchError::UnknownProtectedSpan(
                span.span_id.clone(),
            ));
        }
        if !seen.insert(span.span_id.as_str()) {
            return Err(TranslationPatchError::DuplicateProtectedSpan(
                span.span_id.clone(),
            ));
        }
        let translated_start = usize::try_from(placement.translated_start).map_err(|_| {
            TranslationPatchError::InvalidProtectedSpanPlacement(span.span_id.clone())
        })?;
        let end = translated_start
            .checked_add(span.exact_text.len())
            .ok_or_else(|| {
                TranslationPatchError::InvalidProtectedSpanPlacement(span.span_id.clone())
            })?;
        if translated_text.get(translated_start..end) != Some(span.exact_text.as_str()) {
            return Err(TranslationPatchError::InvalidProtectedSpanPlacement(
                span.span_id.clone(),
            ));
        }
        let translated_len = u32::try_from(span.exact_text.len()).map_err(|_| {
            TranslationPatchError::InvalidProtectedSpanPlacement(span.span_id.clone())
        })?;
        result.push(TranslationPatchProtectedSpan {
            span_id: span.span_id.clone(),
            kind: span.kind,
            exact_text: span.exact_text.clone(),
            translated_start: placement.translated_start,
            translated_len,
        });
    }
    for span in covered_spans {
        if !seen.contains(span.span_id.as_str()) {
            return Err(TranslationPatchError::MissingProtectedSpan(
                span.span_id.clone(),
            ));
        }
    }
    result.sort_by_key(|span| span.translated_start);
    for pair in result.windows(2) {
        let left_end = pair[0]
            .translated_start
            .checked_add(pair[0].translated_len)
            .ok_or_else(|| {
                TranslationPatchError::InvalidProtectedSpanPlacement(pair[0].span_id.clone())
            })?;
        if left_end > pair[1].translated_start {
            return Err(TranslationPatchError::InvalidProtectedSpanPlacement(
                pair[1].span_id.clone(),
            ));
        }
    }
    Ok(result)
}

fn validate_renderer_decision(
    entry_id: &str,
    decision: &TranslationPatchRendererDecision,
) -> Result<(), TranslationPatchError> {
    match decision {
        TranslationPatchRendererDecision::Pending => Ok(()),
        TranslationPatchRendererDecision::Fitted { fit_scale, .. }
            if fit_scale.is_finite() && (0.0..=1.0).contains(fit_scale) =>
        {
            Ok(())
        }
        TranslationPatchRendererDecision::Preserved { reason_code }
            if !reason_code.is_empty()
                && reason_code.len() <= 128
                && reason_code.trim() == reason_code
                && !reason_code.chars().any(char::is_control) =>
        {
            Ok(())
        }
        _ => Err(TranslationPatchError::InvalidRendererDecision(
            entry_id.to_string(),
        )),
    }
}

pub(crate) fn source_atom_hash(atom: &PageAtom) -> Result<String, TranslationPatchError> {
    let bytes = serde_json::to_vec(atom).map_err(|error| {
        TranslationPatchError::Serialization(format!("failed to hash PageGraph atom: {error}"))
    })?;
    Ok(sha256(&bytes))
}

fn entry_id(source_page_hash: &str, atoms: &[TranslationPatchAtomRef]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rosetta-pdf-v3-translation-patch-entry/1\0");
    hasher.update(source_page_hash.as_bytes());
    for atom in atoms {
        hasher.update(b"\0");
        hasher.update(atom.atom_id.as_bytes());
    }
    format!("entry-{}", hex_digest(hasher.finalize()))
}

fn patch_id(patch: &TranslationPatch) -> Result<String, TranslationPatchError> {
    let mut canonical = patch.clone();
    canonical.patch_id.clear();
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        TranslationPatchError::Serialization(format!(
            "failed to calculate TranslationPatch patchId: {error}"
        ))
    })?;
    Ok(format!("patch-{}", sha256(&bytes)))
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finalize())
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        build_translation_patch, decode_and_validate_translation_patch, encode_translation_patch,
        patch_id, resolve_translation_patch_renderer_decisions, validate_translation_patch,
        TranslationPatchDraft, TranslationPatchEntryDraft, TranslationPatchError,
        TranslationPatchProtectedSpanPlacement, MAX_PATCH_BYTES,
    };
    use crate::pdf_v3::types::{
        PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageReconciliationSummary,
        PageStyle, ProtectedSpan, ProtectedSpanKind, TranslationPatchFitStrategy,
        TranslationPatchRendererDecision, PAGE_GRAPH_SCHEMA_VERSION,
    };

    #[test]
    fn patch_is_deterministic_compact_and_contains_only_protected_source_text() {
        let page = page_graph();
        let draft = patch_draft(vec![
            "atom-citation-close",
            "atom-body",
            "atom-citation-open",
        ]);

        let patch = build_translation_patch(&page, draft).expect("translation patch");
        let bytes = encode_translation_patch(&patch).expect("encoded patch");
        let decoded = decode_and_validate_translation_patch(&page, &bytes).expect("valid patch");

        assert_eq!(decoded, patch);
        assert!(patch.patch_id.starts_with("patch-"));
        assert!(patch.entries[0].entry_id.starts_with("entry-"));
        assert_eq!(
            patch.entries[0]
                .atoms
                .iter()
                .map(|atom| atom.atom_id.as_str())
                .collect::<Vec<_>>(),
            ["atom-body", "atom-citation-open", "atom-citation-close"]
        );
        assert_eq!(patch.entries[0].protected_spans[0].exact_text, "[1]");
        let json = String::from_utf8(bytes).expect("UTF-8 patch");
        assert!(!json.contains("Confidential source sentence"));
        assert!(json.contains("[1]"));
        assert!(json.len() < 2_000);

        let repeated = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect("repeated translation patch");
        assert_eq!(repeated, patch);
    }

    #[test]
    fn stale_page_atom_and_patch_id_are_rejected() {
        let page = page_graph();
        let patch = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect("translation patch");

        let mut stale_page = page.clone();
        stale_page.atoms[0].source_text.push('!');
        let error = validate_translation_patch(&stale_page, &patch)
            .expect_err("stale source atom must reject the patch");
        assert!(matches!(error, TranslationPatchError::EntryMismatch(_)));

        let mut corrupt_patch = patch;
        corrupt_patch.entries[0].translated_text.push('!');
        let error = validate_translation_patch(&page, &corrupt_patch)
            .expect_err("corrupt patch id must be rejected");
        assert!(matches!(error, TranslationPatchError::PatchIdMismatch));
    }

    #[test]
    fn protected_span_requires_complete_atoms_and_exact_byte_placement() {
        let page = page_graph();
        let mut partial = patch_draft(vec!["atom-body", "atom-citation-open"]);
        partial.entries[0].protected_spans.clear();
        let error = build_translation_patch(&page, partial)
            .expect_err("partial protected span must reject the entry");
        assert!(matches!(
            error,
            TranslationPatchError::PartialProtectedSpan(_)
        ));

        let mut misplaced = patch_draft(vec![
            "atom-body",
            "atom-citation-open",
            "atom-citation-close",
        ]);
        misplaced.entries[0].protected_spans[0].translated_start = 0;
        let error = build_translation_patch(&page, misplaced)
            .expect_err("incorrect protected byte placement must reject the entry");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidProtectedSpanPlacement(_)
        ));
    }

    #[test]
    fn malformed_source_protected_span_is_rejected() {
        let mut page = page_graph();
        page.protected_spans[0].exact_text = "[2]".to_string();

        let error = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect_err("source protected span must match its atoms exactly");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidSourceProtectedSpan(_)
        ));

        let mut page = page_graph();
        page.protected_spans[0].atom_ids.reverse();
        let error = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect_err("source protected span atoms must remain in source order");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidSourceProtectedSpan(_)
        ));
    }

    #[test]
    fn page_identity_must_be_one_based_and_nonempty() {
        let mut page = page_graph();
        page.page_number = 0;
        let error = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect_err("page number zero must be rejected");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidMetadata("pageNumber")
        ));

        let mut page = page_graph();
        page.source_page_hash.clear();
        let error = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect_err("empty source page hash must be rejected");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidMetadata("sourcePageHash")
        ));
    }

    #[test]
    fn renderer_decision_is_part_of_patch_identity_and_is_validated() {
        let page = page_graph();
        let mut patch = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect("translation patch");
        patch.entries[0].renderer_decision = TranslationPatchRendererDecision::Fitted {
            strategy: TranslationPatchFitStrategy::SingleShowScale,
            fit_scale: 0.85,
        };
        patch.patch_id = patch_id(&patch).expect("updated patch id");
        validate_translation_patch(&page, &patch).expect("valid fitted decision");

        patch.entries[0].renderer_decision = TranslationPatchRendererDecision::Fitted {
            strategy: TranslationPatchFitStrategy::SingleShowScale,
            fit_scale: f32::NAN,
        };
        let error = validate_translation_patch(&page, &patch)
            .expect_err("non-finite fit scale must be rejected");
        assert!(matches!(
            error,
            TranslationPatchError::InvalidRendererDecision(_)
        ));
    }

    #[test]
    fn resolves_all_pending_decisions_before_rebuilding_patch_identity() {
        let page = page_graph();
        let patch = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect("translation patch");
        let original_id = patch.patch_id.clone();
        let decisions = BTreeMap::from([(
            patch.entries[0].entry_id.clone(),
            TranslationPatchRendererDecision::Fitted {
                strategy: TranslationPatchFitStrategy::SingleShowScale,
                fit_scale: 0.95,
            },
        )]);

        let resolved = resolve_translation_patch_renderer_decisions(&page, &patch, &decisions)
            .expect("resolved patch");
        assert_ne!(resolved.patch_id, original_id);
        assert!(matches!(
            resolved.entries[0].renderer_decision,
            TranslationPatchRendererDecision::Fitted { fit_scale, .. }
                if (fit_scale - 0.95).abs() < 0.0001
        ));
        assert!(matches!(
            resolve_translation_patch_renderer_decisions(&page, &resolved, &decisions)
                .expect_err("resolved patch cannot be resolved twice"),
            TranslationPatchError::RendererDecisionAlreadyResolved(_)
        ));

        let missing = BTreeMap::new();
        assert!(matches!(
            resolve_translation_patch_renderer_decisions(&page, &patch, &missing)
                .expect_err("every entry needs a decision"),
            TranslationPatchError::MissingRendererDecision(_)
        ));
        let unknown = BTreeMap::from([(
            "entry-unknown".to_string(),
            TranslationPatchRendererDecision::Preserved {
                reason_code: "unsupported".to_string(),
            },
        )]);
        assert!(matches!(
            resolve_translation_patch_renderer_decisions(&page, &patch, &unknown)
                .expect_err("unknown entry decision"),
            TranslationPatchError::UnknownRendererDecision(_)
        ));
    }

    #[test]
    fn encode_enforces_the_durable_patch_size_limit() {
        let page = page_graph();
        let mut patch = build_translation_patch(
            &page,
            patch_draft(vec![
                "atom-body",
                "atom-citation-open",
                "atom-citation-close",
            ]),
        )
        .expect("translation patch");
        patch.entries[0].translated_text = "x".repeat(MAX_PATCH_BYTES);

        let error = encode_translation_patch(&patch)
            .expect_err("durable encoding must reject oversized patches");
        assert!(matches!(error, TranslationPatchError::PatchTooLarge { .. }));
    }

    fn patch_draft(atom_ids: Vec<&str>) -> TranslationPatchDraft {
        TranslationPatchDraft {
            target_language: "zh-CN".to_string(),
            translation_revision: 1,
            provider_id: "rwkv-local".to_string(),
            model_id: "rwkv-test".to_string(),
            renderer_version: "pdf-v3-test".to_string(),
            entries: vec![TranslationPatchEntryDraft {
                atom_ids: atom_ids.into_iter().map(ToString::to_string).collect(),
                translated_text: "译文[1]".to_string(),
                protected_spans: vec![TranslationPatchProtectedSpanPlacement {
                    span_id: "span-citation".to_string(),
                    translated_start: u32::try_from("译文".len()).expect("fixture byte offset"),
                }],
            }],
        }
    }

    fn page_graph() -> PageGraph {
        let atoms = vec![
            atom(
                "atom-body",
                "Confidential source sentence",
                0,
                PageAtomKind::Body,
            ),
            atom("atom-citation-open", "[", 1, PageAtomKind::Citation),
            atom("atom-citation-close", "1]", 2, PageAtomKind::Citation),
        ];
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:page-one".to_string(),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms,
            styles: vec![PageStyle {
                style_id: "style-1".to_string(),
                font_resource: Some("F1".to_string()),
                font_size: 12.0,
                scaled_font_size: 12.0,
                font_weight: Some(400),
                italic: false,
                serif: false,
                fill_color: Some([0.0, 0.0, 0.0, 1.0]),
                stroke_color: None,
                opacity: Some(1.0),
                render_mode: Some("filled-unstroked".to_string()),
            }],
            groups: Vec::new(),
            protected_spans: vec![ProtectedSpan {
                span_id: "span-citation".to_string(),
                kind: ProtectedSpanKind::Citation,
                atom_ids: vec![
                    "atom-citation-open".to_string(),
                    "atom-citation-close".to_string(),
                ],
                exact_text: "[1]".to_string(),
            }],
            reconciliation: PageReconciliationSummary::unreconciled(3),
            warnings: Vec::new(),
        }
    }

    fn atom(id: &str, text: &str, order: u32, kind: PageAtomKind) -> PageAtom {
        PageAtom {
            atom_id: id.to_string(),
            source_text: text.to_string(),
            source_object_id: Some("object-1".to_string()),
            kind,
            style_id: Some("style-1".to_string()),
            bounds: [10.0 + order as f32 * 10.0, 20.0, 20.0, 32.0],
            loose_bounds: None,
            origin: Some([10.0 + order as f32 * 10.0, 20.0]),
            text_matrix: Some([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]),
            angle_degrees: Some(0.0),
            order,
            generated: false,
            hyphen: false,
            requires_translation: true,
            source_kind: PageAtomSourceKind::PdfiumVerified,
            source_provenance: None,
        }
    }
}
