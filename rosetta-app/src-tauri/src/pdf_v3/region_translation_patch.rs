use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    paragraph_translation_plan::{
        build_visual_paragraph_page_plan, ResolvedVisualParagraphTranslation,
        VisualParagraphPagePlan, VisualParagraphPlanError,
    },
    translation_patch::{source_atom_hash, TranslationPatchError},
    translation_plan::TranslationPatchDraftMetadata,
    types::{
        PageGraph, PageGroupKind, TranslationPatchAtomRef, TranslationPatchProtectedSpan,
        TranslationPatchProvider, PAGE_GRAPH_SCHEMA_VERSION,
    },
};

pub(crate) const REGION_TRANSLATION_PATCH_SCHEMA_VERSION: u32 = 2;

const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONTAINERS_PER_PAGE: usize = 10_000;
const MAX_PARAGRAPHS_PER_PAGE: usize = 25_000;
const MAX_TRANSLATED_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionTranslationPatch {
    pub schema_version: u32,
    pub patch_id: String,
    pub page_number: u32,
    pub source_page_hash: String,
    pub target_language: String,
    pub translation_revision: u64,
    pub provider: TranslationPatchProvider,
    pub containers: Vec<RegionTranslationPatchContainer>,
    pub renderer_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionTranslationPatchContainer {
    pub container_id: String,
    pub flow_container_group_id: String,
    pub atoms: Vec<TranslationPatchAtomRef>,
    pub paragraphs: Vec<RegionTranslationPatchParagraph>,
    pub renderer_decision: RegionTranslationPatchRendererDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionTranslationPatchParagraph {
    pub unit_id: String,
    pub paragraph_group_id: String,
    pub atoms: Vec<TranslationPatchAtomRef>,
    pub translated_text: String,
    pub protected_spans: Vec<TranslationPatchProtectedSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum RegionTranslationPatchRendererDecision {
    Pending,
    Reflowed { line_count: u32, fit_scale: f32 },
    Preserved { reason_code: String },
}

#[derive(Debug, Clone)]
pub(crate) struct RegionTranslationPatchDraft {
    pub plan: VisualParagraphPagePlan,
    pub translations: Vec<ResolvedVisualParagraphTranslation>,
    pub metadata: TranslationPatchDraftMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegionTranslationPatchError {
    UnsupportedPageGraphSchema { expected: u32, actual: u32 },
    UnsupportedPatchSchema { expected: u32, actual: u32 },
    InvalidMetadata(&'static str),
    InvalidPlan,
    ResultCountMismatch { expected: usize, actual: usize },
    UnknownResult(String),
    DuplicateResult(String),
    MissingResult(String),
    UnknownGroup(String),
    GroupKindMismatch(String),
    GroupAtomMismatch(String),
    UnknownAtom(String),
    SourceAtomMismatch(String),
    DuplicateAtom(String),
    InvalidProtectedSpan(String),
    TooManyContainers { count: usize, maximum: usize },
    TooManyParagraphs { count: usize, maximum: usize },
    TranslationTooLarge { bytes: usize, maximum: usize },
    PatchTooLarge { bytes: usize, maximum: usize },
    PageMismatch { expected: u32, actual: u32 },
    SourcePageHashMismatch,
    PatchIdMismatch,
    InvalidRendererDecision(String),
    MissingRendererDecision(String),
    UnknownRendererDecision(String),
    RendererDecisionAlreadyResolved(String),
    RendererDecisionPending(String),
    Serialization(String),
}

impl fmt::Display for RegionTranslationPatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPageGraphSchema { expected, actual } => write!(
                formatter,
                "PageGraph schema mismatch: expected {expected}, found {actual}"
            ),
            Self::UnsupportedPatchSchema { expected, actual } => write!(
                formatter,
                "region patch schema mismatch: expected {expected}, found {actual}"
            ),
            Self::InvalidMetadata(field) => {
                write!(formatter, "region patch metadata field {field} is invalid")
            }
            Self::InvalidPlan => formatter.write_str("visual paragraph plan is not canonical"),
            Self::ResultCountMismatch { expected, actual } => write!(
                formatter,
                "region patch result count mismatch: expected {expected}, found {actual}"
            ),
            Self::UnknownResult(unit_id) => {
                write!(formatter, "region patch references unknown unit {unit_id}")
            }
            Self::DuplicateResult(unit_id) => {
                write!(formatter, "region patch repeats unit {unit_id}")
            }
            Self::MissingResult(unit_id) => {
                write!(formatter, "region patch is missing unit {unit_id}")
            }
            Self::UnknownGroup(group_id) => write!(formatter, "unknown PageGraph group {group_id}"),
            Self::GroupKindMismatch(group_id) => {
                write!(formatter, "PageGraph group {group_id} has the wrong kind")
            }
            Self::GroupAtomMismatch(group_id) => {
                write!(
                    formatter,
                    "PageGraph group {group_id} atom ownership changed"
                )
            }
            Self::UnknownAtom(atom_id) => write!(formatter, "unknown PageGraph atom {atom_id}"),
            Self::SourceAtomMismatch(atom_id) => {
                write!(formatter, "PageGraph atom {atom_id} content changed")
            }
            Self::DuplicateAtom(atom_id) => {
                write!(
                    formatter,
                    "PageGraph atom {atom_id} is owned more than once"
                )
            }
            Self::InvalidProtectedSpan(span_id) => {
                write!(
                    formatter,
                    "region patch protected span {span_id} is invalid"
                )
            }
            Self::TooManyContainers { count, maximum } => write!(
                formatter,
                "region patch has {count} containers, above maximum {maximum}"
            ),
            Self::TooManyParagraphs { count, maximum } => write!(
                formatter,
                "region patch has {count} paragraphs, above maximum {maximum}"
            ),
            Self::TranslationTooLarge { bytes, maximum } => write!(
                formatter,
                "region patch has {bytes} translated bytes, above maximum {maximum}"
            ),
            Self::PatchTooLarge { bytes, maximum } => write!(
                formatter,
                "region patch has {bytes} bytes, above maximum {maximum}"
            ),
            Self::PageMismatch { expected, actual } => write!(
                formatter,
                "region patch page mismatch: expected {expected}, found {actual}"
            ),
            Self::SourcePageHashMismatch => {
                formatter.write_str("region patch source page hash is stale")
            }
            Self::PatchIdMismatch => formatter.write_str("region patch ID does not match content"),
            Self::InvalidRendererDecision(container_id) => write!(
                formatter,
                "region patch container {container_id} has an invalid renderer decision"
            ),
            Self::MissingRendererDecision(container_id) => write!(
                formatter,
                "region patch container {container_id} has no renderer decision"
            ),
            Self::UnknownRendererDecision(container_id) => write!(
                formatter,
                "region patch renderer decision references unknown container {container_id}"
            ),
            Self::RendererDecisionAlreadyResolved(container_id) => write!(
                formatter,
                "region patch container {container_id} is already resolved"
            ),
            Self::RendererDecisionPending(container_id) => write!(
                formatter,
                "region patch container {container_id} is still pending"
            ),
            Self::Serialization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RegionTranslationPatchError {}

impl From<TranslationPatchError> for RegionTranslationPatchError {
    fn from(value: TranslationPatchError) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<VisualParagraphPlanError> for RegionTranslationPatchError {
    fn from(_: VisualParagraphPlanError) -> Self {
        Self::InvalidPlan
    }
}

pub(crate) fn build_region_translation_patch(
    page: &PageGraph,
    draft: RegionTranslationPatchDraft,
) -> Result<RegionTranslationPatch, RegionTranslationPatchError> {
    build_region_translation_patch_preserving_containers(page, draft, &BTreeMap::new())
}

pub(crate) fn build_region_translation_patch_preserving_containers(
    page: &PageGraph,
    draft: RegionTranslationPatchDraft,
    provider_preserved_containers: &BTreeMap<String, &'static str>,
) -> Result<RegionTranslationPatch, RegionTranslationPatchError> {
    validate_page(page)?;
    validate_metadata(&draft.metadata)?;
    if build_visual_paragraph_page_plan(page)? != draft.plan {
        return Err(RegionTranslationPatchError::InvalidPlan);
    }
    let expected_translation_count = draft
        .plan
        .units
        .iter()
        .filter(|unit| !provider_preserved_containers.contains_key(&unit.flow_container_group_id))
        .count();
    if draft.translations.len() != expected_translation_count {
        return Err(RegionTranslationPatchError::ResultCountMismatch {
            expected: expected_translation_count,
            actual: draft.translations.len(),
        });
    }

    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let groups_by_id = page
        .groups
        .iter()
        .map(|group| (group.group_id.as_str(), group))
        .collect::<BTreeMap<_, _>>();
    let spans_by_id = page
        .protected_spans
        .iter()
        .map(|span| (span.span_id.as_str(), span))
        .collect::<BTreeMap<_, _>>();
    let units_by_id = draft
        .plan
        .units
        .iter()
        .map(|unit| (unit.unit_id.as_str(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut translations_by_id = BTreeMap::new();
    for translation in draft.translations {
        if !units_by_id.contains_key(translation.unit_id.as_str()) {
            return Err(RegionTranslationPatchError::UnknownResult(
                translation.unit_id,
            ));
        }
        let unit_id = translation.unit_id.clone();
        if translations_by_id
            .insert(unit_id.clone(), translation)
            .is_some()
        {
            return Err(RegionTranslationPatchError::DuplicateResult(unit_id));
        }
    }

    let mut containers = Vec::new();
    let mut paragraph_count = 0usize;
    let mut translated_bytes = 0usize;
    let mut claimed_atoms = BTreeSet::new();
    let mut units_by_container = BTreeMap::<&str, Vec<&str>>::new();
    for unit in &draft.plan.units {
        units_by_container
            .entry(unit.flow_container_group_id.as_str())
            .or_default()
            .push(unit.unit_id.as_str());
    }

    for flow_container_group_id in provider_preserved_containers.keys() {
        if !units_by_container.contains_key(flow_container_group_id.as_str()) {
            return Err(RegionTranslationPatchError::UnknownGroup(
                flow_container_group_id.clone(),
            ));
        }
    }

    for (flow_container_group_id, unit_ids) in units_by_container {
        let group = group_of_kind(
            &groups_by_id,
            flow_container_group_id,
            PageGroupKind::FlowContainer,
        )?;
        let atoms = atom_refs(&group.atom_ids, &atoms_by_id)?;
        for atom in &atoms {
            if !claimed_atoms.insert(atom.atom_id.clone()) {
                return Err(RegionTranslationPatchError::DuplicateAtom(
                    atom.atom_id.clone(),
                ));
            }
        }
        if let Some(reason_code) = provider_preserved_containers.get(flow_container_group_id) {
            containers.push(RegionTranslationPatchContainer {
                container_id: container_id(&page.source_page_hash, flow_container_group_id, &atoms),
                flow_container_group_id: flow_container_group_id.to_string(),
                atoms,
                paragraphs: Vec::new(),
                renderer_decision: RegionTranslationPatchRendererDecision::Preserved {
                    reason_code: (*reason_code).to_string(),
                },
            });
            continue;
        }
        let mut paragraphs = Vec::with_capacity(unit_ids.len());
        for unit_id in unit_ids {
            let unit = units_by_id
                .get(unit_id)
                .copied()
                .ok_or_else(|| RegionTranslationPatchError::MissingResult(unit_id.to_string()))?;
            let translation = translations_by_id
                .remove(unit_id)
                .ok_or_else(|| RegionTranslationPatchError::MissingResult(unit_id.to_string()))?;
            if translation.paragraph_group_id != unit.paragraph_group_id
                || translation.flow_container_group_id != unit.flow_container_group_id
                || translation.atom_ids != unit.atom_ids
            {
                return Err(RegionTranslationPatchError::InvalidPlan);
            }
            let paragraph_group = group_of_kind(
                &groups_by_id,
                &unit.paragraph_group_id,
                PageGroupKind::Paragraph,
            )?;
            if paragraph_group.atom_ids != unit.atom_ids {
                return Err(RegionTranslationPatchError::GroupAtomMismatch(
                    paragraph_group.group_id.clone(),
                ));
            }
            let paragraph_atoms = atom_refs(&unit.atom_ids, &atoms_by_id)?;
            let protected_spans = build_protected_spans(
                &translation.translated_text,
                &translation.protected_spans,
                &unit.atom_ids,
                &spans_by_id,
            )?;
            translated_bytes = translated_bytes
                .checked_add(translation.translated_text.len())
                .ok_or(RegionTranslationPatchError::TranslationTooLarge {
                    bytes: usize::MAX,
                    maximum: MAX_TRANSLATED_TEXT_BYTES,
                })?;
            if translated_bytes > MAX_TRANSLATED_TEXT_BYTES {
                return Err(RegionTranslationPatchError::TranslationTooLarge {
                    bytes: translated_bytes,
                    maximum: MAX_TRANSLATED_TEXT_BYTES,
                });
            }
            paragraphs.push(RegionTranslationPatchParagraph {
                unit_id: unit.unit_id.clone(),
                paragraph_group_id: unit.paragraph_group_id.clone(),
                atoms: paragraph_atoms,
                translated_text: translation.translated_text,
                protected_spans,
            });
            paragraph_count += 1;
        }
        let container_id = container_id(&page.source_page_hash, flow_container_group_id, &atoms);
        containers.push(RegionTranslationPatchContainer {
            container_id,
            flow_container_group_id: flow_container_group_id.to_string(),
            atoms,
            paragraphs,
            renderer_decision: RegionTranslationPatchRendererDecision::Pending,
        });
    }

    for preserved in &draft.plan.preserved_containers {
        let group = group_of_kind(
            &groups_by_id,
            &preserved.flow_container_group_id,
            PageGroupKind::FlowContainer,
        )?;
        if group.atom_ids != preserved.atom_ids {
            return Err(RegionTranslationPatchError::GroupAtomMismatch(
                group.group_id.clone(),
            ));
        }
        let atoms = atom_refs(&group.atom_ids, &atoms_by_id)?;
        for atom in &atoms {
            if !claimed_atoms.insert(atom.atom_id.clone()) {
                return Err(RegionTranslationPatchError::DuplicateAtom(
                    atom.atom_id.clone(),
                ));
            }
        }
        let container_id = container_id(&page.source_page_hash, &group.group_id, &atoms);
        containers.push(RegionTranslationPatchContainer {
            container_id,
            flow_container_group_id: group.group_id.clone(),
            atoms,
            paragraphs: Vec::new(),
            renderer_decision: RegionTranslationPatchRendererDecision::Preserved {
                reason_code: preserved.reason_code.to_string(),
            },
        });
    }
    if !translations_by_id.is_empty() {
        return Err(RegionTranslationPatchError::UnknownResult(
            translations_by_id.into_keys().next().unwrap_or_default(),
        ));
    }
    if containers.len() > MAX_CONTAINERS_PER_PAGE {
        return Err(RegionTranslationPatchError::TooManyContainers {
            count: containers.len(),
            maximum: MAX_CONTAINERS_PER_PAGE,
        });
    }
    if paragraph_count > MAX_PARAGRAPHS_PER_PAGE {
        return Err(RegionTranslationPatchError::TooManyParagraphs {
            count: paragraph_count,
            maximum: MAX_PARAGRAPHS_PER_PAGE,
        });
    }
    containers.sort_by_key(|container| {
        page.groups
            .iter()
            .position(|group| group.group_id == container.flow_container_group_id)
            .unwrap_or(usize::MAX)
    });

    let mut patch = RegionTranslationPatch {
        schema_version: REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
        patch_id: String::new(),
        page_number: page.page_number,
        source_page_hash: page.source_page_hash.clone(),
        target_language: draft.metadata.target_language,
        translation_revision: draft.metadata.translation_revision,
        provider: TranslationPatchProvider {
            provider_id: draft.metadata.provider_id,
            model_id: draft.metadata.model_id,
        },
        containers,
        renderer_version: draft.metadata.renderer_version,
    };
    patch.patch_id = patch_id(&patch)?;
    encode_region_translation_patch(&patch)?;
    Ok(patch)
}

pub(crate) fn encode_region_translation_patch(
    patch: &RegionTranslationPatch,
) -> Result<Vec<u8>, RegionTranslationPatchError> {
    let bytes = serde_json::to_vec(patch).map_err(|error| {
        RegionTranslationPatchError::Serialization(format!(
            "failed to encode region TranslationPatch: {error}"
        ))
    })?;
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(RegionTranslationPatchError::PatchTooLarge {
            bytes: bytes.len(),
            maximum: MAX_PATCH_BYTES,
        });
    }
    Ok(bytes)
}

pub(crate) fn decode_and_validate_region_translation_patch(
    page: &PageGraph,
    bytes: &[u8],
) -> Result<RegionTranslationPatch, RegionTranslationPatchError> {
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(RegionTranslationPatchError::PatchTooLarge {
            bytes: bytes.len(),
            maximum: MAX_PATCH_BYTES,
        });
    }
    let patch = serde_json::from_slice::<RegionTranslationPatch>(bytes).map_err(|error| {
        RegionTranslationPatchError::Serialization(format!(
            "failed to decode region TranslationPatch: {error}"
        ))
    })?;
    validate_region_translation_patch(page, &patch)?;
    Ok(patch)
}

pub(crate) fn decode_and_validate_region_translation_patch_identity(
    bytes: &[u8],
) -> Result<RegionTranslationPatch, RegionTranslationPatchError> {
    if bytes.len() > MAX_PATCH_BYTES {
        return Err(RegionTranslationPatchError::PatchTooLarge {
            bytes: bytes.len(),
            maximum: MAX_PATCH_BYTES,
        });
    }
    let patch = serde_json::from_slice::<RegionTranslationPatch>(bytes).map_err(|error| {
        RegionTranslationPatchError::Serialization(format!(
            "failed to decode region TranslationPatch identity: {error}"
        ))
    })?;
    validate_patch_identity(&patch)?;
    ensure_region_translation_patch_resolved(&patch)?;
    if patch_id(&patch)? != patch.patch_id {
        return Err(RegionTranslationPatchError::PatchIdMismatch);
    }
    Ok(patch)
}

pub(crate) fn validate_region_translation_patch(
    page: &PageGraph,
    patch: &RegionTranslationPatch,
) -> Result<(), RegionTranslationPatchError> {
    validate_page(page)?;
    validate_patch_identity(patch)?;
    if patch.page_number != page.page_number {
        return Err(RegionTranslationPatchError::PageMismatch {
            expected: page.page_number,
            actual: patch.page_number,
        });
    }
    if patch.source_page_hash != page.source_page_hash {
        return Err(RegionTranslationPatchError::SourcePageHashMismatch);
    }
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let groups_by_id = page
        .groups
        .iter()
        .map(|group| (group.group_id.as_str(), group))
        .collect::<BTreeMap<_, _>>();
    let spans_by_id = page
        .protected_spans
        .iter()
        .map(|span| (span.span_id.as_str(), span))
        .collect::<BTreeMap<_, _>>();
    let mut claimed_atoms = BTreeSet::new();
    let mut paragraph_count = 0usize;
    let mut translated_bytes = 0usize;
    for container in &patch.containers {
        let group = group_of_kind(
            &groups_by_id,
            &container.flow_container_group_id,
            PageGroupKind::FlowContainer,
        )?;
        validate_atom_refs(&container.atoms, &group.atom_ids, &atoms_by_id)?;
        for atom in &container.atoms {
            if !claimed_atoms.insert(atom.atom_id.clone()) {
                return Err(RegionTranslationPatchError::DuplicateAtom(
                    atom.atom_id.clone(),
                ));
            }
        }
        if container.container_id
            != container_id(
                &page.source_page_hash,
                &container.flow_container_group_id,
                &container.atoms,
            )
        {
            return Err(RegionTranslationPatchError::GroupAtomMismatch(
                container.flow_container_group_id.clone(),
            ));
        }
        validate_renderer_decision(&container.container_id, &container.renderer_decision)?;
        if matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Pending
                | RegionTranslationPatchRendererDecision::Reflowed { .. }
        ) && container.paragraphs.is_empty()
        {
            return Err(RegionTranslationPatchError::InvalidRendererDecision(
                container.container_id.clone(),
            ));
        }
        for paragraph in &container.paragraphs {
            let paragraph_group = group_of_kind(
                &groups_by_id,
                &paragraph.paragraph_group_id,
                PageGroupKind::Paragraph,
            )?;
            validate_atom_refs(&paragraph.atoms, &paragraph_group.atom_ids, &atoms_by_id)?;
            let paragraph_ids = paragraph
                .atoms
                .iter()
                .map(|atom| atom.atom_id.as_str())
                .collect::<BTreeSet<_>>();
            let container_ids = container
                .atoms
                .iter()
                .map(|atom| atom.atom_id.as_str())
                .collect::<BTreeSet<_>>();
            if !paragraph_ids.is_subset(&container_ids) || paragraph.translated_text.is_empty() {
                return Err(RegionTranslationPatchError::GroupAtomMismatch(
                    paragraph.paragraph_group_id.clone(),
                ));
            }
            validate_persisted_protected_spans(
                &paragraph.translated_text,
                &paragraph.protected_spans,
                &paragraph_ids,
                &spans_by_id,
            )?;
            translated_bytes = translated_bytes
                .checked_add(paragraph.translated_text.len())
                .ok_or(RegionTranslationPatchError::TranslationTooLarge {
                    bytes: usize::MAX,
                    maximum: MAX_TRANSLATED_TEXT_BYTES,
                })?;
            paragraph_count += 1;
        }
    }
    if patch.containers.len() > MAX_CONTAINERS_PER_PAGE {
        return Err(RegionTranslationPatchError::TooManyContainers {
            count: patch.containers.len(),
            maximum: MAX_CONTAINERS_PER_PAGE,
        });
    }
    if paragraph_count > MAX_PARAGRAPHS_PER_PAGE {
        return Err(RegionTranslationPatchError::TooManyParagraphs {
            count: paragraph_count,
            maximum: MAX_PARAGRAPHS_PER_PAGE,
        });
    }
    if translated_bytes > MAX_TRANSLATED_TEXT_BYTES {
        return Err(RegionTranslationPatchError::TranslationTooLarge {
            bytes: translated_bytes,
            maximum: MAX_TRANSLATED_TEXT_BYTES,
        });
    }
    if patch_id(patch)? != patch.patch_id {
        return Err(RegionTranslationPatchError::PatchIdMismatch);
    }
    Ok(())
}

pub(crate) fn resolve_region_translation_patch_decisions(
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    decisions: &BTreeMap<String, RegionTranslationPatchRendererDecision>,
) -> Result<RegionTranslationPatch, RegionTranslationPatchError> {
    validate_region_translation_patch(page, patch)?;
    let container_ids = patch
        .containers
        .iter()
        .map(|container| container.container_id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(unknown) = decisions
        .keys()
        .find(|container_id| !container_ids.contains(container_id.as_str()))
    {
        return Err(RegionTranslationPatchError::UnknownRendererDecision(
            unknown.clone(),
        ));
    }
    let mut resolved = patch.clone();
    for container in &mut resolved.containers {
        if matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { .. }
        ) {
            if decisions.contains_key(&container.container_id) {
                return Err(
                    RegionTranslationPatchError::RendererDecisionAlreadyResolved(
                        container.container_id.clone(),
                    ),
                );
            }
            continue;
        }
        if !matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Pending
        ) {
            return Err(
                RegionTranslationPatchError::RendererDecisionAlreadyResolved(
                    container.container_id.clone(),
                ),
            );
        }
        let decision = decisions
            .get(&container.container_id)
            .cloned()
            .ok_or_else(|| {
                RegionTranslationPatchError::MissingRendererDecision(container.container_id.clone())
            })?;
        if matches!(decision, RegionTranslationPatchRendererDecision::Pending) {
            return Err(RegionTranslationPatchError::InvalidRendererDecision(
                container.container_id.clone(),
            ));
        }
        validate_renderer_decision(&container.container_id, &decision)?;
        container.renderer_decision = decision;
    }
    resolved.patch_id = patch_id(&resolved)?;
    validate_region_translation_patch(page, &resolved)?;
    encode_region_translation_patch(&resolved)?;
    Ok(resolved)
}

pub(crate) fn ensure_region_translation_patch_resolved(
    patch: &RegionTranslationPatch,
) -> Result<(), RegionTranslationPatchError> {
    if let Some(container) = patch.containers.iter().find(|container| {
        matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Pending
        )
    }) {
        return Err(RegionTranslationPatchError::RendererDecisionPending(
            container.container_id.clone(),
        ));
    }
    Ok(())
}

fn build_protected_spans(
    translated_text: &str,
    placements: &[super::translation_patch::TranslationPatchProtectedSpanPlacement],
    paragraph_atom_ids: &[String],
    spans_by_id: &BTreeMap<&str, &super::types::ProtectedSpan>,
) -> Result<Vec<TranslationPatchProtectedSpan>, RegionTranslationPatchError> {
    let paragraph_atoms = paragraph_atom_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut result = Vec::with_capacity(placements.len());
    let mut seen = BTreeSet::new();
    for placement in placements {
        let span = spans_by_id
            .get(placement.span_id.as_str())
            .copied()
            .ok_or_else(|| {
                RegionTranslationPatchError::InvalidProtectedSpan(placement.span_id.clone())
            })?;
        if !seen.insert(span.span_id.as_str())
            || !span
                .atom_ids
                .iter()
                .all(|atom_id| paragraph_atoms.contains(atom_id.as_str()))
        {
            return Err(RegionTranslationPatchError::InvalidProtectedSpan(
                span.span_id.clone(),
            ));
        }
        let translated_start = usize::try_from(placement.translated_start)
            .map_err(|_| RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone()))?;
        let translated_end = translated_start
            .checked_add(span.exact_text.len())
            .ok_or_else(|| {
                RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone())
            })?;
        if translated_text.get(translated_start..translated_end) != Some(span.exact_text.as_str()) {
            return Err(RegionTranslationPatchError::InvalidProtectedSpan(
                span.span_id.clone(),
            ));
        }
        result.push(TranslationPatchProtectedSpan {
            span_id: span.span_id.clone(),
            kind: span.kind,
            exact_text: span.exact_text.clone(),
            translated_start: placement.translated_start,
            translated_len: u32::try_from(span.exact_text.len()).map_err(|_| {
                RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone())
            })?,
        });
    }
    result.sort_by_key(|span| span.translated_start);
    Ok(result)
}

fn validate_persisted_protected_spans(
    translated_text: &str,
    persisted: &[TranslationPatchProtectedSpan],
    paragraph_atoms: &BTreeSet<&str>,
    spans_by_id: &BTreeMap<&str, &super::types::ProtectedSpan>,
) -> Result<(), RegionTranslationPatchError> {
    let mut previous_end = 0usize;
    let mut seen = BTreeSet::new();
    for span in persisted {
        let source = spans_by_id
            .get(span.span_id.as_str())
            .copied()
            .ok_or_else(|| {
                RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone())
            })?;
        let start = usize::try_from(span.translated_start)
            .map_err(|_| RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone()))?;
        let len = usize::try_from(span.translated_len)
            .map_err(|_| RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone()))?;
        let end = start.checked_add(len).ok_or_else(|| {
            RegionTranslationPatchError::InvalidProtectedSpan(span.span_id.clone())
        })?;
        if !seen.insert(span.span_id.as_str())
            || start < previous_end
            || span.kind != source.kind
            || span.exact_text != source.exact_text
            || len != source.exact_text.len()
            || translated_text.get(start..end) != Some(source.exact_text.as_str())
            || !source
                .atom_ids
                .iter()
                .all(|atom_id| paragraph_atoms.contains(atom_id.as_str()))
        {
            return Err(RegionTranslationPatchError::InvalidProtectedSpan(
                span.span_id.clone(),
            ));
        }
        previous_end = end;
    }
    Ok(())
}

fn atom_refs(
    atom_ids: &[String],
    atoms_by_id: &BTreeMap<&str, &super::types::PageAtom>,
) -> Result<Vec<TranslationPatchAtomRef>, RegionTranslationPatchError> {
    atom_ids
        .iter()
        .map(|atom_id| {
            let atom = atoms_by_id
                .get(atom_id.as_str())
                .copied()
                .ok_or_else(|| RegionTranslationPatchError::UnknownAtom(atom_id.clone()))?;
            Ok(TranslationPatchAtomRef {
                atom_id: atom.atom_id.clone(),
                source_atom_hash: source_atom_hash(atom)?,
            })
        })
        .collect()
}

fn validate_atom_refs(
    refs: &[TranslationPatchAtomRef],
    expected_atom_ids: &[String],
    atoms_by_id: &BTreeMap<&str, &super::types::PageAtom>,
) -> Result<(), RegionTranslationPatchError> {
    if refs.len() != expected_atom_ids.len()
        || refs
            .iter()
            .map(|atom| atom.atom_id.as_str())
            .ne(expected_atom_ids.iter().map(String::as_str))
    {
        return Err(RegionTranslationPatchError::GroupAtomMismatch(
            "atom-order".to_string(),
        ));
    }
    for atom_ref in refs {
        let atom = atoms_by_id
            .get(atom_ref.atom_id.as_str())
            .copied()
            .ok_or_else(|| RegionTranslationPatchError::UnknownAtom(atom_ref.atom_id.clone()))?;
        if source_atom_hash(atom)? != atom_ref.source_atom_hash {
            return Err(RegionTranslationPatchError::SourceAtomMismatch(
                atom_ref.atom_id.clone(),
            ));
        }
    }
    Ok(())
}

fn group_of_kind<'a>(
    groups_by_id: &BTreeMap<&str, &'a super::types::PageGroup>,
    group_id: &str,
    kind: PageGroupKind,
) -> Result<&'a super::types::PageGroup, RegionTranslationPatchError> {
    let group = groups_by_id
        .get(group_id)
        .copied()
        .ok_or_else(|| RegionTranslationPatchError::UnknownGroup(group_id.to_string()))?;
    if group.kind != kind {
        return Err(RegionTranslationPatchError::GroupKindMismatch(
            group_id.to_string(),
        ));
    }
    Ok(group)
}

fn validate_page(page: &PageGraph) -> Result<(), RegionTranslationPatchError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(RegionTranslationPatchError::UnsupportedPageGraphSchema {
            expected: PAGE_GRAPH_SCHEMA_VERSION,
            actual: page.schema_version,
        });
    }
    if page.page_number == 0 || page.source_page_hash.is_empty() {
        return Err(RegionTranslationPatchError::InvalidMetadata("pageIdentity"));
    }
    Ok(())
}

fn validate_metadata(
    metadata: &TranslationPatchDraftMetadata,
) -> Result<(), RegionTranslationPatchError> {
    if metadata.translation_revision == 0 {
        return Err(RegionTranslationPatchError::InvalidMetadata(
            "translationRevision",
        ));
    }
    for (value, field) in [
        (&metadata.target_language, "targetLanguage"),
        (&metadata.provider_id, "providerId"),
        (&metadata.model_id, "modelId"),
        (&metadata.renderer_version, "rendererVersion"),
    ] {
        validate_identifier(value, field)?;
    }
    Ok(())
}

fn validate_patch_identity(
    patch: &RegionTranslationPatch,
) -> Result<(), RegionTranslationPatchError> {
    if patch.schema_version != REGION_TRANSLATION_PATCH_SCHEMA_VERSION {
        return Err(RegionTranslationPatchError::UnsupportedPatchSchema {
            expected: REGION_TRANSLATION_PATCH_SCHEMA_VERSION,
            actual: patch.schema_version,
        });
    }
    if patch.page_number == 0
        || patch.source_page_hash.is_empty()
        || patch.translation_revision == 0
    {
        return Err(RegionTranslationPatchError::InvalidMetadata(
            "patchIdentity",
        ));
    }
    for (value, field) in [
        (&patch.target_language, "targetLanguage"),
        (&patch.provider.provider_id, "providerId"),
        (&patch.provider.model_id, "modelId"),
        (&patch.renderer_version, "rendererVersion"),
    ] {
        validate_identifier(value, field)?;
    }
    Ok(())
}

fn validate_identifier(
    value: &str,
    field: &'static str,
) -> Result<(), RegionTranslationPatchError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(RegionTranslationPatchError::InvalidMetadata(field));
    }
    Ok(())
}

fn validate_renderer_decision(
    container_id: &str,
    decision: &RegionTranslationPatchRendererDecision,
) -> Result<(), RegionTranslationPatchError> {
    match decision {
        RegionTranslationPatchRendererDecision::Pending => Ok(()),
        RegionTranslationPatchRendererDecision::Reflowed {
            line_count,
            fit_scale,
        } if *line_count > 0
            && fit_scale.is_finite()
            && (0.0..=1.0).contains(fit_scale)
            && *fit_scale > 0.0 =>
        {
            Ok(())
        }
        RegionTranslationPatchRendererDecision::Preserved { reason_code }
            if !reason_code.is_empty()
                && reason_code.len() <= 128
                && reason_code.trim() == reason_code
                && !reason_code.chars().any(char::is_control) =>
        {
            Ok(())
        }
        _ => Err(RegionTranslationPatchError::InvalidRendererDecision(
            container_id.to_string(),
        )),
    }
}

fn container_id(
    source_page_hash: &str,
    flow_container_group_id: &str,
    atoms: &[TranslationPatchAtomRef],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rosetta-pdf-v3-region-container/1\0");
    hasher.update(source_page_hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(flow_container_group_id.as_bytes());
    for atom in atoms {
        hasher.update(b"\0");
        hasher.update(atom.atom_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(atom.source_atom_hash.as_bytes());
    }
    format!("container-{}", hex_digest(hasher.finalize()))
}

fn patch_id(patch: &RegionTranslationPatch) -> Result<String, RegionTranslationPatchError> {
    let mut canonical = patch.clone();
    canonical.patch_id.clear();
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        RegionTranslationPatchError::Serialization(format!(
            "failed to calculate region patch ID: {error}"
        ))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("region-patch-{}", hex_digest(hasher.finalize())))
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
        build_region_translation_patch, build_region_translation_patch_preserving_containers,
        decode_and_validate_region_translation_patch, ensure_region_translation_patch_resolved,
        resolve_region_translation_patch_decisions, RegionTranslationPatchDraft,
        RegionTranslationPatchError, RegionTranslationPatchRendererDecision,
    };
    use crate::pdf_v3::{
        paragraph_translation_plan::{
            build_visual_paragraph_page_plan, resolve_visual_paragraph_results,
            resolve_visual_paragraph_results_preserving_invalid_containers,
        },
        region_renderer::REGION_TRANSLATION_RENDERER_VERSION,
        translation_plan::{TranslationPatchDraftMetadata, TranslationUnitResult},
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind,
            PageReconciliationSummary, PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    #[test]
    fn region_patch_round_trips_without_persisting_source_text_or_layout_lines() {
        let page = page();
        let patch = patch(&page);
        let bytes = super::encode_region_translation_patch(&patch).expect("encode");
        let decoded = decode_and_validate_region_translation_patch(&page, &bytes).expect("decode");

        assert_eq!(decoded, patch);
        let json = String::from_utf8(bytes).expect("UTF-8 JSON");
        assert!(!json.contains("Source paragraph across objects"));
        assert!(!json.contains("lineBreak"));
        assert!(json.contains("跨对象完整段落"));
        assert_eq!(decoded.containers.len(), 1);
        assert_eq!(decoded.containers[0].paragraphs.len(), 1);
    }

    #[test]
    fn region_patch_rejects_source_atom_drift_and_requires_all_decisions() {
        let mut page = page();
        let patch = patch(&page);
        let container_id = patch.containers[0].container_id.clone();
        let error = resolve_region_translation_patch_decisions(&page, &patch, &BTreeMap::new())
            .expect_err("missing decision");
        assert_eq!(
            error,
            RegionTranslationPatchError::MissingRendererDecision(container_id.clone())
        );

        let resolved = resolve_region_translation_patch_decisions(
            &page,
            &patch,
            &BTreeMap::from([(
                container_id,
                RegionTranslationPatchRendererDecision::Reflowed {
                    line_count: 2,
                    fit_scale: 0.9,
                },
            )]),
        )
        .expect("resolved decisions");
        ensure_region_translation_patch_resolved(&resolved).expect("resolved patch");

        page.atoms[0].source_text = "Tampered".to_string();
        let error =
            super::validate_region_translation_patch(&page, &resolved).expect_err("source drift");
        assert!(matches!(
            error,
            RegionTranslationPatchError::SourceAtomMismatch(_)
        ));
    }

    #[test]
    fn provider_invalid_container_is_durable_preservation_not_page_failure() {
        let page = page();
        let plan = build_visual_paragraph_page_plan(&page).expect("plan");
        let unit = &plan.units[0];
        let resolved = resolve_visual_paragraph_results_preserving_invalid_containers(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: "W欢迎 e来到 v我们的中文译文，这是一段完全破碎的译文。"
                    .to_string(),
            }],
            "zh-CN",
        )
        .expect("container resolution");
        let patch = build_region_translation_patch_preserving_containers(
            &page,
            RegionTranslationPatchDraft {
                plan,
                translations: resolved.translations,
                metadata: TranslationPatchDraftMetadata {
                    target_language: "zh-CN".to_string(),
                    translation_revision: 1,
                    provider_id: "provider".to_string(),
                    model_id: "model".to_string(),
                    renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
                },
            },
            &resolved.preserved_containers,
        )
        .expect("preserved region patch");

        assert_eq!(patch.containers.len(), 1);
        assert!(patch.containers[0].paragraphs.is_empty());
        assert!(matches!(
            &patch.containers[0].renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "translation-fragmented-mixed-language"
        ));
        ensure_region_translation_patch_resolved(&patch).expect("resolved patch");
    }

    fn patch(page: &PageGraph) -> super::RegionTranslationPatch {
        let plan = build_visual_paragraph_page_plan(page).expect("plan");
        let unit = &plan.units[0];
        let translations = resolve_visual_paragraph_results(
            &plan,
            vec![TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: "跨对象完整段落".to_string(),
            }],
            "zh-CN",
        )
        .expect("resolved translation");
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
                    renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
                },
            },
        )
        .expect("region patch")
    }

    fn page() -> PageGraph {
        let source = "Source paragraph spanning several separate objects";
        let atoms = source
            .chars()
            .enumerate()
            .map(|(index, character)| PageAtom {
                atom_id: format!("atom-{index}"),
                source_text: character.to_string(),
                source_object_id: Some(format!("object-{}", index / 5)),
                kind: PageAtomKind::Body,
                style_id: Some("regular".to_string()),
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
            source_page_hash: "sha256:region-patch-page".to_string(),
            page_width: 200.0,
            page_height: 200.0,
            rotation_degrees: 0,
            atoms,
            styles: Vec::new(),
            groups: vec![
                PageGroup {
                    group_id: "line-1".to_string(),
                    kind: PageGroupKind::Line,
                    atom_ids: atom_ids.clone(),
                    bounds: [0.0, 10.0, 100.0, 20.0],
                    confidence: 0.99,
                },
                PageGroup {
                    group_id: "paragraph-1".to_string(),
                    kind: PageGroupKind::Paragraph,
                    atom_ids: atom_ids.clone(),
                    bounds: [0.0, 10.0, 100.0, 20.0],
                    confidence: 0.98,
                },
                PageGroup {
                    group_id: "flow-container-1".to_string(),
                    kind: PageGroupKind::FlowContainer,
                    atom_ids,
                    bounds: [0.0, 10.0, 100.0, 20.0],
                    confidence: 0.97,
                },
            ],
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(source.chars().count()),
            warnings: Vec::new(),
        }
    }
}
