use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use super::{
    font::{
        stage_document_translation_font_registry, stage_translation_fonts_page_context,
        DocumentTranslationFontRegistry, PreparedTranslationFont, TranslationFontError,
        TranslationFontWeight,
    },
    layout::TextShowGeometryKey,
    object_delta::{PdfObjectDelta, PdfObjectDeltaError},
    ownership::{PdfStreamOwnershipError, PdfStreamOwnershipIndex},
    page_context::{PdfPageContextError, PdfPageObjectContext},
    page_index::{PdfPageIndex, PdfPageIndexError},
    page_pdf::{serialize_single_page_pdf_from_view, PdfSinglePageError},
    region_layout::{
        layout_region_translation_patch, FlowContainerLayout, PreparedRegionFontMetrics,
        RegionLayoutError, RegionLayoutPolicy,
    },
    region_translation_patch::{
        ensure_region_translation_patch_resolved, resolve_region_translation_patch_decisions,
        RegionTranslationPatch, RegionTranslationPatchError,
        RegionTranslationPatchRendererDecision,
    },
    render_cache::{
        RenderCache, RenderCacheError, RenderCacheInsertOutcome, RenderCacheKey,
        RenderCacheOptions, RenderCacheOutputKind,
    },
    replacement::{
        preflight_text_show_replacement_transaction_with_page_index_and_cache,
        stage_text_show_replacement_batch_with_font_registry_and_cache,
        text_show_replacement_target_identity_with_cache, TextShowReplacementBatchResult,
        TextShowReplacementContentCache, TextShowReplacementError, TextShowReplacementRequest,
        TextShowReplacementTargetIdentity, TextShowReplacementTargetRequest,
    },
    source_object::{PdfObjectOverlay, PdfObjectView},
    types::{PageAtomSourceProvenance, PageGraph},
};

pub(crate) const REGION_TRANSLATION_RENDERER_VERSION: &str =
    "rosetta-pdf-v3-region-translation-renderer/2";

#[derive(Debug, Clone)]
pub(crate) struct RegionTranslationRenderResult {
    pub resolved_patch: RegionTranslationPatch,
    pub object_delta: PdfObjectDelta,
    pub neutralization_batch: Option<TextShowReplacementBatchResult>,
    pub rendered_container_count: usize,
    pub rendered_line_count: usize,
    pub preserved_container_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RegionTranslationPagePdf {
    source_fingerprint: String,
    patch: RegionTranslationPatch,
    pdf_bytes: Vec<u8>,
}

impl RegionTranslationPagePdf {
    pub(crate) fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub(crate) fn patch(&self) -> &RegionTranslationPatch {
        &self.patch
    }

    pub(crate) fn pdf_bytes(&self) -> &[u8] {
        &self.pdf_bytes
    }
}

#[derive(Debug)]
pub(crate) enum RegionTranslationRenderError {
    MissingRegularFont,
    DuplicateFontWeight(TranslationFontWeight),
    MissingFontWeight(TranslationFontWeight),
    RendererVersionMismatch,
    InvalidProvenance(String),
    ContainerOwnershipIncomplete(String),
    SharedTextShow(String),
    PageContentsInvalid,
    ResourceConflict(String),
    ObjectNumberOverflow,
    ContentEncode(String),
    StreamWrite(String),
    Font(TranslationFontError),
    Layout(RegionLayoutError),
    Patch(RegionTranslationPatchError),
    Replacement(TextShowReplacementError),
    ObjectDelta(PdfObjectDeltaError),
    PageIndex(PdfPageIndexError),
    PageContext(PdfPageContextError),
    Ownership(PdfStreamOwnershipError),
    SinglePage(PdfSinglePageError),
    Cache(RenderCacheError),
    InvalidPagePdf(&'static str),
    PagePdfSerialization(String),
}

impl fmt::Display for RegionTranslationRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRegularFont => {
                formatter.write_str("region renderer requires a regular translation font")
            }
            Self::DuplicateFontWeight(weight) => {
                write!(formatter, "duplicate {weight:?} translation font")
            }
            Self::MissingFontWeight(weight) => {
                write!(formatter, "missing {weight:?} translation font")
            }
            Self::RendererVersionMismatch => {
                formatter.write_str("region translation renderer version mismatch")
            }
            Self::InvalidProvenance(id) => write!(formatter, "invalid source provenance for {id}"),
            Self::ContainerOwnershipIncomplete(id) => write!(
                formatter,
                "flow container {id} does not completely own its source text-shows"
            ),
            Self::SharedTextShow(id) => write!(
                formatter,
                "source text-show {id} is shared by multiple flow containers"
            ),
            Self::PageContentsInvalid => {
                formatter.write_str("selected PDF page has invalid /Contents")
            }
            Self::ResourceConflict(name) => write!(
                formatter,
                "page resource {name} conflicts with the region renderer"
            ),
            Self::ObjectNumberOverflow => {
                formatter.write_str("PDF object number overflow while staging region overlay")
            }
            Self::ContentEncode(message) | Self::StreamWrite(message) => {
                formatter.write_str(message)
            }
            Self::Font(error) => error.fmt(formatter),
            Self::Layout(error) => error.fmt(formatter),
            Self::Patch(error) => error.fmt(formatter),
            Self::Replacement(error) => error.fmt(formatter),
            Self::ObjectDelta(error) => error.fmt(formatter),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::PageContext(error) => error.fmt(formatter),
            Self::Ownership(error) => error.fmt(formatter),
            Self::SinglePage(error) => error.fmt(formatter),
            Self::Cache(error) => error.fmt(formatter),
            Self::InvalidPagePdf(reason) => {
                write!(formatter, "invalid region translated page PDF: {reason}")
            }
            Self::PagePdfSerialization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RegionTranslationRenderError {}

macro_rules! impl_from_error {
    ($variant:ident, $source:ty) => {
        impl From<$source> for RegionTranslationRenderError {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }
    };
}

impl_from_error!(Font, TranslationFontError);
impl_from_error!(Layout, RegionLayoutError);
impl_from_error!(Patch, RegionTranslationPatchError);
impl_from_error!(Replacement, TextShowReplacementError);
impl_from_error!(ObjectDelta, PdfObjectDeltaError);
impl_from_error!(PageIndex, PdfPageIndexError);
impl_from_error!(PageContext, PdfPageContextError);
impl_from_error!(Ownership, PdfStreamOwnershipError);
impl_from_error!(SinglePage, PdfSinglePageError);
impl_from_error!(Cache, RenderCacheError);

pub(crate) fn render_region_translation_patch(
    document: &mut Document,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
) -> Result<RegionTranslationRenderResult, RegionTranslationRenderError> {
    let staged = stage_region_translation_patch(document, page, patch, fonts, policy)?;
    staged.object_delta.apply_to(document);
    Ok(staged)
}

pub(crate) fn stage_region_translation_patch(
    source_objects: &dyn PdfObjectView,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
) -> Result<RegionTranslationRenderResult, RegionTranslationRenderError> {
    let page_index = PdfPageIndex::resolve_page(source_objects, page.page_number)?;
    let ownership_index = PdfStreamOwnershipIndex::resolve(
        source_objects,
        &page_index.selected_content_stream_ids(),
    )?;
    stage_region_translation_patch_with_context(
        source_objects,
        &page_index,
        &ownership_index,
        page,
        patch,
        fonts,
        policy,
    )
}

pub(crate) fn render_resolved_region_translation_patch_page_pdf_from_view(
    source_objects: &dyn PdfObjectView,
    source_fingerprint: &str,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
) -> Result<RegionTranslationPagePdf, RegionTranslationRenderError> {
    ensure_region_translation_patch_resolved(patch)?;
    validate_renderer_version(patch)?;
    let page_index = PdfPageIndex::resolve_page(source_objects, page.page_number)?;
    let ownership_index = PdfStreamOwnershipIndex::resolve(
        source_objects,
        &page_index.selected_content_stream_ids(),
    )?;
    let staged = stage_region_translation_patch_with_context(
        source_objects,
        &page_index,
        &ownership_index,
        page,
        patch,
        fonts,
        policy,
    )?;
    if staged.resolved_patch != *patch {
        return Err(RegionTranslationRenderError::Patch(
            RegionTranslationPatchError::PatchIdMismatch,
        ));
    }
    let overlay = PdfObjectOverlay::new(source_objects, &staged.object_delta);
    let indexed_page = page_index.page(page.page_number)?;
    let pdf_bytes = serialize_single_page_pdf_from_view(&overlay, indexed_page)?;
    validate_single_page_pdf(&pdf_bytes, "rendered")?;
    Ok(RegionTranslationPagePdf {
        source_fingerprint: source_fingerprint.to_string(),
        patch: staged.resolved_patch,
        pdf_bytes,
    })
}

pub(crate) fn region_translation_page_pdf_cache_key(
    source_fingerprint: &str,
    patch: &RegionTranslationPatch,
) -> Result<RenderCacheKey, RegionTranslationRenderError> {
    ensure_region_translation_patch_resolved(patch)?;
    validate_renderer_version(patch)?;
    Ok(RenderCacheKey {
        source_fingerprint: source_fingerprint.to_string(),
        page_number: patch.page_number,
        patch_id: patch.patch_id.clone(),
        translation_revision: patch.translation_revision,
        renderer_version: patch.renderer_version.clone(),
        options: RenderCacheOptions {
            output_kind: RenderCacheOutputKind::TranslatedPagePdf,
            pixel_width: None,
            scale_milli: None,
        },
    })
}

pub(crate) fn insert_region_translation_page_pdf_cache(
    cache: &RenderCache,
    artifact: &RegionTranslationPagePdf,
) -> Result<RenderCacheInsertOutcome, RegionTranslationRenderError> {
    let key =
        region_translation_page_pdf_cache_key(artifact.source_fingerprint(), artifact.patch())?;
    Ok(cache.insert(&key, artifact.pdf_bytes())?)
}

pub(crate) fn open_region_translation_page_pdf_cache(
    cache: &RenderCache,
    source_fingerprint: &str,
    patch: &RegionTranslationPatch,
) -> Result<Option<Vec<u8>>, RegionTranslationRenderError> {
    let key = region_translation_page_pdf_cache_key(source_fingerprint, patch)?;
    let Some(lease) = cache.open(&key)? else {
        return Ok(None);
    };
    match lease.read_bytes() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(RenderCacheError::CorruptArtifact { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn restore_region_translation_page_pdf(
    source_fingerprint: &str,
    patch: &RegionTranslationPatch,
    pdf_bytes: Vec<u8>,
) -> Result<RegionTranslationPagePdf, RegionTranslationRenderError> {
    ensure_region_translation_patch_resolved(patch)?;
    validate_renderer_version(patch)?;
    validate_single_page_pdf(&pdf_bytes, "cached")?;
    Ok(RegionTranslationPagePdf {
        source_fingerprint: source_fingerprint.to_string(),
        patch: patch.clone(),
        pdf_bytes,
    })
}

fn validate_renderer_version(
    patch: &RegionTranslationPatch,
) -> Result<(), RegionTranslationRenderError> {
    if patch.renderer_version != REGION_TRANSLATION_RENDERER_VERSION {
        return Err(RegionTranslationRenderError::RendererVersionMismatch);
    }
    Ok(())
}

fn validate_single_page_pdf(
    pdf_bytes: &[u8],
    kind: &'static str,
) -> Result<(), RegionTranslationRenderError> {
    if !pdf_bytes.starts_with(b"%PDF-") {
        return Err(RegionTranslationRenderError::InvalidPagePdf("signature"));
    }
    let document = Document::load_mem(pdf_bytes).map_err(|error| {
        RegionTranslationRenderError::PagePdfSerialization(format!(
            "failed to validate {kind} region single-page PDF: {error}"
        ))
    })?;
    if document.get_pages().len() != 1 {
        return Err(RegionTranslationRenderError::InvalidPagePdf(
            "page count is not one",
        ));
    }
    Ok(())
}

pub(crate) fn stage_region_translation_patch_with_context(
    source_objects: &dyn PdfObjectView,
    page_index: &PdfPageIndex,
    ownership_index: &PdfStreamOwnershipIndex,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
) -> Result<RegionTranslationRenderResult, RegionTranslationRenderError> {
    stage_region_translation_patch_internal(
        source_objects,
        source_objects,
        page_index,
        ownership_index,
        page,
        patch,
        fonts,
        fonts,
        policy,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_resolved_region_translation_patch_with_font_registry(
    source_objects: &dyn PdfObjectView,
    accumulated_objects: &dyn PdfObjectView,
    page_index: &PdfPageIndex,
    ownership_index: &PdfStreamOwnershipIndex,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    decision_fonts: &[&PreparedTranslationFont],
    output_fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
    font_registry: &DocumentTranslationFontRegistry,
) -> Result<RegionTranslationRenderResult, RegionTranslationRenderError> {
    ensure_region_translation_patch_resolved(patch)?;
    validate_renderer_version(patch)?;
    stage_region_translation_patch_internal(
        source_objects,
        accumulated_objects,
        page_index,
        ownership_index,
        page,
        patch,
        decision_fonts,
        output_fonts,
        policy,
        Some(font_registry),
    )
}

#[allow(clippy::too_many_arguments)]
fn stage_region_translation_patch_internal(
    source_objects: &dyn PdfObjectView,
    accumulated_objects: &dyn PdfObjectView,
    page_index: &PdfPageIndex,
    ownership_index: &PdfStreamOwnershipIndex,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    decision_fonts: &[&PreparedTranslationFont],
    output_fonts: &[&PreparedTranslationFont],
    policy: RegionLayoutPolicy,
    font_registry: Option<&DocumentTranslationFontRegistry>,
) -> Result<RegionTranslationRenderResult, RegionTranslationRenderError> {
    let patch_is_pending = patch.containers.iter().any(|container| {
        matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Pending
        )
    });
    if !patch_is_pending {
        ensure_region_translation_patch_resolved(patch)?;
    }
    if patch.containers.iter().all(|container| {
        matches!(
            container.renderer_decision,
            RegionTranslationPatchRendererDecision::Preserved { .. }
        )
    }) {
        return Ok(render_result(
            patch.clone(),
            PdfObjectDelta::empty(accumulated_objects.maximum_object_number()),
            None,
            Vec::new(),
        ));
    }
    let decision_fonts_by_weight = fonts_by_weight(decision_fonts)?;
    let regular = decision_fonts_by_weight
        .get(&TranslationFontWeight::Regular)
        .copied()
        .ok_or(RegionTranslationRenderError::MissingRegularFont)?;
    let metrics = PreparedRegionFontMetrics::new(
        regular,
        decision_fonts_by_weight
            .get(&TranslationFontWeight::Bold)
            .copied(),
    );
    let output_fonts_by_weight = fonts_by_weight(output_fonts)?;
    let mut layout_batch = layout_region_translation_patch(page, patch, &metrics, policy)?;
    preserve_unsupported_font_containers(
        &mut layout_batch.layouts,
        &mut layout_batch.decisions,
        &output_fonts_by_weight,
        patch_is_pending,
    )?;
    if layout_batch.layouts.is_empty() {
        let resolved_patch =
            finalize_patch(page, patch, &layout_batch.decisions, patch_is_pending)?;
        let maximum = source_objects.maximum_object_number();
        return Ok(render_result(
            resolved_patch,
            PdfObjectDelta::empty(maximum),
            None,
            Vec::new(),
        ));
    }

    let mut content_cache = TextShowReplacementContentCache::default();
    let (target_requests, unsafe_containers) = build_neutralization_targets(
        source_objects,
        page_index,
        page,
        patch,
        &layout_batch.layouts,
        decision_fonts,
        &mut content_cache,
    )?;
    if !patch_is_pending && !unsafe_containers.is_empty() {
        return Err(RegionTranslationRenderError::ContainerOwnershipIncomplete(
            unsafe_containers
                .into_keys()
                .next()
                .unwrap_or_else(|| "resolved-region".to_string()),
        ));
    }
    for (container_id, reason_code) in unsafe_containers {
        layout_batch.decisions.insert(
            container_id,
            RegionTranslationPatchRendererDecision::Preserved { reason_code },
        );
    }
    let effective_decisions = if patch_is_pending {
        layout_batch.decisions.clone()
    } else {
        patch
            .containers
            .iter()
            .map(|container| {
                (
                    container.container_id.clone(),
                    container.renderer_decision.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    layout_batch.layouts.retain(|layout| {
        matches!(
            effective_decisions.get(&layout.container_id),
            Some(RegionTranslationPatchRendererDecision::Reflowed { .. })
        )
    });
    let resolved_patch = finalize_patch(page, patch, &layout_batch.decisions, patch_is_pending)?;
    if layout_batch.layouts.is_empty() {
        return Ok(render_result(
            resolved_patch,
            PdfObjectDelta::empty(source_objects.maximum_object_number()),
            None,
            Vec::new(),
        ));
    }

    let active_container_ids = layout_batch
        .layouts
        .iter()
        .map(|layout| layout.container_id.as_str())
        .collect::<BTreeSet<_>>();
    let active_targets = target_requests
        .into_iter()
        .filter(|target| {
            target
                .container_ids
                .iter()
                .all(|id| active_container_ids.contains(id.as_str()))
        })
        .map(|target| TextShowReplacementTargetRequest {
            replacements: target.replacements,
        })
        .collect::<Vec<_>>();
    if active_targets.is_empty() {
        return Err(RegionTranslationRenderError::ContainerOwnershipIncomplete(
            "all-reflowed-containers".to_string(),
        ));
    }

    let (registry, font_delta) = match font_registry {
        Some(registry) => (
            registry.clone(),
            PdfObjectDelta::empty(accumulated_objects.maximum_object_number()),
        ),
        None => {
            let staged =
                stage_document_translation_font_registry(accumulated_objects, output_fonts)?;
            (staged.registry, staged.object_delta)
        }
    };
    let font_overlay = PdfObjectOverlay::new(accumulated_objects, &font_delta);
    let neutralized = stage_text_show_replacement_batch_with_font_registry_and_cache(
        source_objects,
        &font_overlay,
        page_index,
        ownership_index,
        page,
        &active_targets,
        decision_fonts,
        &registry,
        Some(&mut content_cache),
    )?;

    let mut accumulated = font_delta;
    accumulated.merge(neutralized.object_delta.clone())?;
    let accumulated_overlay = PdfObjectOverlay::new(accumulated_objects, &accumulated);
    let overlay_delta = stage_page_overlay(
        &accumulated_overlay,
        page_index,
        page.page_number,
        &layout_batch.layouts,
        &output_fonts_by_weight,
        &registry,
        &accumulated,
    )?;

    Ok(render_result(
        resolved_patch,
        overlay_delta,
        Some(neutralized.result),
        layout_batch.layouts,
    ))
}

fn preserve_unsupported_font_containers(
    layouts: &mut Vec<FlowContainerLayout>,
    decisions: &mut BTreeMap<String, RegionTranslationPatchRendererDecision>,
    fonts: &BTreeMap<TranslationFontWeight, &PreparedTranslationFont>,
    patch_is_pending: bool,
) -> Result<(), RegionTranslationRenderError> {
    let mut unsupported = BTreeSet::new();
    for layout in layouts.iter() {
        for line in &layout.lines {
            let font = fonts.get(&line.font_weight).copied().ok_or(
                RegionTranslationRenderError::MissingFontWeight(line.font_weight),
            )?;
            match font.encode_text(&line.text) {
                Ok(_) => {}
                Err(TranslationFontError::MissingPreparedGlyph(_)) if patch_is_pending => {
                    unsupported.insert(layout.container_id.clone());
                    break;
                }
                Err(error) => return Err(RegionTranslationRenderError::Font(error)),
            }
        }
    }
    for container_id in &unsupported {
        decisions.insert(
            container_id.clone(),
            RegionTranslationPatchRendererDecision::Preserved {
                reason_code: "region-font-glyph-unavailable".to_string(),
            },
        );
    }
    layouts.retain(|layout| !unsupported.contains(&layout.container_id));
    Ok(())
}

fn finalize_patch(
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    decisions: &BTreeMap<String, RegionTranslationPatchRendererDecision>,
    patch_is_pending: bool,
) -> Result<RegionTranslationPatch, RegionTranslationRenderError> {
    if patch_is_pending {
        Ok(resolve_region_translation_patch_decisions(
            page, patch, decisions,
        )?)
    } else {
        if !decisions.is_empty() {
            return Err(RegionTranslationRenderError::Patch(
                RegionTranslationPatchError::RendererDecisionAlreadyResolved(
                    decisions.keys().next().cloned().unwrap_or_default(),
                ),
            ));
        }
        Ok(patch.clone())
    }
}

#[derive(Debug)]
struct ContainerTargetRequest {
    container_ids: BTreeSet<String>,
    replacements: Vec<TextShowReplacementRequest>,
}

#[allow(clippy::too_many_arguments)]
fn build_neutralization_targets(
    source_objects: &dyn PdfObjectView,
    page_index: &PdfPageIndex,
    page: &PageGraph,
    patch: &RegionTranslationPatch,
    layouts: &[FlowContainerLayout],
    fonts: &[&PreparedTranslationFont],
    content_cache: &mut TextShowReplacementContentCache,
) -> Result<(Vec<ContainerTargetRequest>, BTreeMap<String, String>), RegionTranslationRenderError> {
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let containers_by_id = patch
        .containers
        .iter()
        .map(|container| (container.container_id.as_str(), container))
        .collect::<BTreeMap<_, _>>();
    let mut show_owner = BTreeMap::<String, String>::new();
    let mut grouped = BTreeMap::<TextShowReplacementTargetIdentity, ContainerTargetRequest>::new();
    let mut unsafe_containers = BTreeMap::new();

    for layout in layouts {
        let container = containers_by_id
            .get(layout.container_id.as_str())
            .copied()
            .ok_or_else(|| {
                RegionTranslationRenderError::ContainerOwnershipIncomplete(
                    layout.container_id.clone(),
                )
            })?;
        let container_atom_ids = container
            .atoms
            .iter()
            .map(|atom| atom.atom_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut provenances = BTreeMap::<String, &PageAtomSourceProvenance>::new();
        for atom_ref in &container.atoms {
            let atom = atoms_by_id
                .get(atom_ref.atom_id.as_str())
                .copied()
                .ok_or_else(|| {
                    RegionTranslationRenderError::InvalidProvenance(atom_ref.atom_id.clone())
                })?;
            let Some(provenance) = atom.source_provenance.as_ref() else {
                continue;
            };
            provenances
                .entry(provenance.text_show_id.clone())
                .or_insert(provenance);
        }
        if provenances.is_empty() {
            unsafe_containers.insert(
                layout.container_id.clone(),
                "region-source-provenance-missing".to_string(),
            );
            continue;
        }

        let mut container_requests = Vec::new();
        let mut container_safe = true;
        for (text_show_id, provenance) in provenances {
            let complete = page.atoms.iter().filter(|atom| {
                atom.source_provenance
                    .as_ref()
                    .is_some_and(|candidate| same_text_show(provenance, candidate))
            });
            if complete
                .clone()
                .any(|atom| !container_atom_ids.contains(atom.atom_id.as_str()))
            {
                container_safe = false;
                break;
            }
            if let Some(owner) =
                show_owner.insert(text_show_id.clone(), layout.container_id.clone())
            {
                if owner != layout.container_id {
                    return Err(RegionTranslationRenderError::SharedTextShow(text_show_id));
                }
            }
            let request = match neutralization_request(page, provenance) {
                Ok(request) => request,
                Err(RegionTranslationRenderError::InvalidProvenance(_)) => {
                    container_safe = false;
                    break;
                }
                Err(error) => return Err(error),
            };
            let identity = match text_show_replacement_target_identity_with_cache(
                source_objects,
                &request,
                content_cache,
            ) {
                Ok(identity) => identity,
                Err(error) if neutralization_preservation_reason(&error).is_some() => {
                    container_safe = false;
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            container_requests.push((identity, request));
        }
        if !container_safe {
            unsafe_containers.insert(
                layout.container_id.clone(),
                "region-source-ownership-incomplete".to_string(),
            );
            continue;
        }
        for (identity, request) in container_requests {
            let target = grouped
                .entry(identity)
                .or_insert_with(|| ContainerTargetRequest {
                    container_ids: BTreeSet::new(),
                    replacements: Vec::new(),
                });
            target.container_ids.insert(layout.container_id.clone());
            target.replacements.push(request);
        }
    }

    for target in grouped.values_mut() {
        target
            .replacements
            .sort_by_key(|request| request.geometry.operation_index);
        if let Err(error) = preflight_text_show_replacement_transaction_with_page_index_and_cache(
            source_objects,
            page_index,
            page,
            &target.replacements,
            fonts,
            Some(content_cache),
        ) {
            let Some(reason_code) = neutralization_preservation_reason(&error) else {
                return Err(error.into());
            };
            for container_id in &target.container_ids {
                unsafe_containers
                    .entry(container_id.clone())
                    .or_insert_with(|| reason_code.to_string());
            }
        }
    }
    loop {
        let previous = unsafe_containers.len();
        for target in grouped.values() {
            if target
                .container_ids
                .iter()
                .any(|id| unsafe_containers.contains_key(id))
            {
                let reason = target
                    .container_ids
                    .iter()
                    .find_map(|id| unsafe_containers.get(id).cloned())
                    .unwrap_or_else(|| "region-source-neutralization-unsupported".to_string());
                for container_id in &target.container_ids {
                    unsafe_containers
                        .entry(container_id.clone())
                        .or_insert_with(|| reason.clone());
                }
            }
        }
        if previous == unsafe_containers.len() {
            break;
        }
    }
    Ok((grouped.into_values().collect(), unsafe_containers))
}

fn neutralization_preservation_reason(error: &TextShowReplacementError) -> Option<&'static str> {
    match error {
        TextShowReplacementError::FitBounds(_) => Some("region-fit-bounds-unsupported"),
        TextShowReplacementError::Style(_)
        | TextShowReplacementError::MissingSourceFontState
        | TextShowReplacementError::SourcePaintStateUnsupported
        | TextShowReplacementError::SourceStyleMismatch => Some("region-source-style-unsupported"),
        TextShowReplacementError::LaterTextShowInTextObject
        | TextShowReplacementError::InvalidTextPositionReset
        | TextShowReplacementError::CrossTextObjectTransaction => {
            Some("region-text-anchor-unsupported")
        }
        TextShowReplacementError::UnsupportedOperator(_)
        | TextShowReplacementError::RepeatedPageStream => {
            Some("region-source-neutralization-unsupported")
        }
        _ => None,
    }
}

fn neutralization_request(
    page: &PageGraph,
    provenance: &PageAtomSourceProvenance,
) -> Result<TextShowReplacementRequest, RegionTranslationRenderError> {
    if !matches!(
        provenance.text_show_operator.as_str(),
        "Tj" | "TJ" | "'" | "\""
    ) || provenance.text_show_operand_hash.len() != 64
        || !provenance
            .text_show_operand_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RegionTranslationRenderError::InvalidProvenance(
            provenance.text_show_id.clone(),
        ));
    }
    let source_font_resource = provenance.source_font_resource.clone().ok_or_else(|| {
        RegionTranslationRenderError::InvalidProvenance(provenance.text_show_id.clone())
    })?;
    let source_font_size = provenance
        .source_font_size
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            RegionTranslationRenderError::InvalidProvenance(provenance.text_show_id.clone())
        })?;
    if !provenance.source_horizontal_scaling.is_finite()
        || provenance.source_horizontal_scaling <= 0.0
    {
        return Err(RegionTranslationRenderError::InvalidProvenance(
            provenance.text_show_id.clone(),
        ));
    }
    Ok(TextShowReplacementRequest {
        geometry: TextShowGeometryKey {
            page_number: page.page_number,
            text_show_id: provenance.text_show_id.clone(),
            form_invocation_path: provenance.form_invocation_path.clone(),
            stream_object_number: provenance.stream_object_number,
            stream_generation: provenance.stream_generation,
            operation_index: provenance.operation_index,
            source_font_resource,
            source_font_size,
            source_horizontal_scaling: provenance.source_horizontal_scaling,
        },
        expected_operator: provenance.text_show_operator.clone(),
        expected_operand_hash: provenance.text_show_operand_hash.clone(),
        translated_text: String::new(),
        minimum_fit_scale: 1.0,
    })
}

fn same_text_show(expected: &PageAtomSourceProvenance, actual: &PageAtomSourceProvenance) -> bool {
    expected.text_show_id == actual.text_show_id
        && expected.form_invocation_path == actual.form_invocation_path
        && expected.stream_object_number == actual.stream_object_number
        && expected.stream_generation == actual.stream_generation
        && expected.operation_index == actual.operation_index
        && expected.text_show_operator == actual.text_show_operator
        && expected.text_show_operand_hash == actual.text_show_operand_hash
}

fn fonts_by_weight<'a>(
    fonts: &'a [&'a PreparedTranslationFont],
) -> Result<
    BTreeMap<TranslationFontWeight, &'a PreparedTranslationFont>,
    RegionTranslationRenderError,
> {
    let mut by_weight = BTreeMap::new();
    for font in fonts {
        if by_weight.insert(font.weight(), *font).is_some() {
            return Err(RegionTranslationRenderError::DuplicateFontWeight(
                font.weight(),
            ));
        }
    }
    Ok(by_weight)
}

fn stage_page_overlay(
    accumulated_objects: &dyn PdfObjectView,
    page_index: &PdfPageIndex,
    page_number: u32,
    layouts: &[FlowContainerLayout],
    fonts: &BTreeMap<TranslationFontWeight, &PreparedTranslationFont>,
    registry: &DocumentTranslationFontRegistry,
    accumulated: &PdfObjectDelta,
) -> Result<PdfObjectDelta, RegionTranslationRenderError> {
    let indexed_page = page_index.page(page_number)?;
    let page_context = PdfPageObjectContext::resolve(accumulated_objects, indexed_page)?;
    let bindings = fonts
        .values()
        .map(|font| registry.binding_for(accumulated_objects, font))
        .collect::<Result<Vec<_>, _>>()?;
    let mut page_dictionary = stage_translation_fonts_page_context(
        &page_context,
        bindings.iter().map(|(name, id)| (*name, *id)),
    )?;
    let (content, opacity_states) = overlay_content(layouts, fonts)?;
    attach_opacity_resources(&mut page_dictionary, &opacity_states)?;

    let overlay_stream_id = (
        accumulated
            .maximum_object_number()
            .checked_add(1)
            .ok_or(RegionTranslationRenderError::ObjectNumberOverflow)?,
        0,
    );
    let mut overlay_stream = Stream::new(Dictionary::new(), content);
    overlay_stream
        .compress()
        .map_err(|error| RegionTranslationRenderError::StreamWrite(error.to_string()))?;
    append_page_content(&mut page_dictionary, overlay_stream_id)?;
    let maximum = overlay_stream_id.0;
    let mut objects = accumulated.objects().clone();
    objects.insert(overlay_stream_id, Object::Stream(overlay_stream));
    objects.insert(indexed_page.page_id(), Object::Dictionary(page_dictionary));
    PdfObjectDelta::try_from_objects(objects, maximum).map_err(Into::into)
}

fn overlay_content(
    layouts: &[FlowContainerLayout],
    fonts: &BTreeMap<TranslationFontWeight, &PreparedTranslationFont>,
) -> Result<(Vec<u8>, BTreeMap<Vec<u8>, f32>), RegionTranslationRenderError> {
    let mut operations = Vec::new();
    let mut opacity_states = BTreeMap::new();
    for layout in layouts {
        for line in &layout.lines {
            let font = fonts.get(&line.font_weight).copied().ok_or(
                RegionTranslationRenderError::MissingFontWeight(line.font_weight),
            )?;
            let encoded = font.encode_text(&line.text)?;
            let opacity_name = opacity_resource_name(line.opacity);
            opacity_states.insert(opacity_name.clone(), line.opacity.clamp(0.0, 1.0));
            let color = line.fill_color.map(|value| value.clamp(0.0, 1.0));
            operations.push(lopdf::content::Operation::new("q", Vec::new()));
            operations.push(lopdf::content::Operation::new(
                "gs",
                vec![Object::Name(opacity_name)],
            ));
            operations.push(lopdf::content::Operation::new(
                "rg",
                vec![
                    Object::Real(color[0]),
                    Object::Real(color[1]),
                    Object::Real(color[2]),
                ],
            ));
            operations.push(lopdf::content::Operation::new("BT", Vec::new()));
            operations.push(lopdf::content::Operation::new(
                "Tf",
                vec![
                    Object::Name(
                        super::font::translation_font_resource_name(line.font_weight).to_vec(),
                    ),
                    Object::Real(line.font_size),
                ],
            ));
            operations.push(lopdf::content::Operation::new(
                "Tm",
                vec![
                    Object::Integer(1),
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(1),
                    Object::Real(line.x),
                    Object::Real(line.baseline_y),
                ],
            ));
            operations.push(lopdf::content::Operation::new(
                "Tj",
                vec![Object::String(encoded, StringFormat::Hexadecimal)],
            ));
            operations.push(lopdf::content::Operation::new("ET", Vec::new()));
            operations.push(lopdf::content::Operation::new("Q", Vec::new()));
        }
    }
    let content = Content { operations }
        .encode()
        .map_err(|error| RegionTranslationRenderError::ContentEncode(error.to_string()))?;
    Ok((content, opacity_states))
}

fn opacity_resource_name(opacity: f32) -> Vec<u8> {
    format!(
        "RosettaOpacity{:04}",
        (opacity.clamp(0.0, 1.0) * 1_000.0).round() as u16
    )
    .into_bytes()
}

fn attach_opacity_resources(
    page: &mut Dictionary,
    opacity_states: &BTreeMap<Vec<u8>, f32>,
) -> Result<(), RegionTranslationRenderError> {
    let resources = page
        .get_mut(b"Resources")
        .and_then(Object::as_dict_mut)
        .map_err(|_| RegionTranslationRenderError::ResourceConflict("Resources".to_string()))?;
    let mut states = match resources.get(b"ExtGState") {
        Ok(Object::Dictionary(dictionary)) => dictionary.clone(),
        Ok(_) => {
            return Err(RegionTranslationRenderError::ResourceConflict(
                "ExtGState".to_string(),
            ))
        }
        Err(_) => Dictionary::new(),
    };
    for (name, opacity) in opacity_states {
        let expected = Object::Dictionary(Dictionary::from_iter([
            (b"Type".to_vec(), Object::Name(b"ExtGState".to_vec())),
            (b"ca".to_vec(), Object::Real(*opacity)),
            (b"CA".to_vec(), Object::Real(*opacity)),
        ]));
        if let Ok(existing) = states.get(name) {
            if existing != &expected {
                return Err(RegionTranslationRenderError::ResourceConflict(
                    String::from_utf8_lossy(name).into_owned(),
                ));
            }
        } else {
            states.set(name.clone(), expected);
        }
    }
    resources.set("ExtGState", Object::Dictionary(states));
    Ok(())
}

fn append_page_content(
    page: &mut Dictionary,
    overlay_stream_id: ObjectId,
) -> Result<(), RegionTranslationRenderError> {
    let overlay = Object::Reference(overlay_stream_id);
    let next = match page.get(b"Contents") {
        Err(_) | Ok(Object::Null) => overlay,
        Ok(Object::Reference(id)) => Object::Array(vec![Object::Reference(*id), overlay]),
        Ok(Object::Array(contents)) => {
            let mut contents = contents.clone();
            contents.push(overlay);
            Object::Array(contents)
        }
        Ok(_) => return Err(RegionTranslationRenderError::PageContentsInvalid),
    };
    page.set("Contents", next);
    Ok(())
}

fn render_result(
    resolved_patch: RegionTranslationPatch,
    object_delta: PdfObjectDelta,
    neutralization_batch: Option<TextShowReplacementBatchResult>,
    layouts: Vec<FlowContainerLayout>,
) -> RegionTranslationRenderResult {
    let rendered_line_count = layouts.iter().map(|layout| layout.lines.len()).sum();
    let rendered_container_count = layouts.len();
    let preserved_container_count = resolved_patch
        .containers
        .iter()
        .filter(|container| {
            matches!(
                container.renderer_decision,
                RegionTranslationPatchRendererDecision::Preserved { .. }
            )
        })
        .count();
    RegionTranslationRenderResult {
        resolved_patch,
        object_delta,
        neutralization_batch,
        rendered_container_count,
        rendered_line_count,
        preserved_container_count,
    }
}

#[cfg(test)]
mod tests {
    use super::{append_page_content, opacity_resource_name, REGION_TRANSLATION_RENDERER_VERSION};
    use lopdf::{Dictionary, Object};

    #[test]
    fn page_overlay_is_appended_after_existing_content() {
        let mut page = Dictionary::new();
        page.set("Contents", Object::Reference((7, 0)));
        append_page_content(&mut page, (9, 0)).expect("append overlay");
        assert_eq!(
            page.get(b"Contents").expect("page contents"),
            &Object::Array(vec![Object::Reference((7, 0)), Object::Reference((9, 0))])
        );
    }

    #[test]
    fn opacity_resource_names_are_stable_and_bounded() {
        assert_eq!(opacity_resource_name(-1.0), b"RosettaOpacity0000");
        assert_eq!(opacity_resource_name(0.3754), b"RosettaOpacity0375");
        assert_eq!(opacity_resource_name(2.0), b"RosettaOpacity1000");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows external PDF region-renderer probe"]
    fn manual_windows_external_region_renderer_probe() {
        use std::{fs, path::PathBuf};

        use crate::{
            pdf_v3::{
                document::DocumentHandle,
                font::{TranslationFontAsset, TranslationFontWeight, UnifiedTranslationFontPlan},
                paragraph_translation_plan::{
                    build_visual_paragraph_page_plan, resolve_visual_paragraph_results,
                },
                reconcile::build_reconciled_page_graph_from_handle,
                region_layout::RegionLayoutPolicy,
                region_translation_patch::{
                    build_region_translation_patch, RegionTranslationPatchDraft,
                },
                translation_plan::{TranslationPatchDraftMetadata, TranslationUnitResult},
            },
            rosetta_jobs::formats::pdf::test_helpers::{pdfium_test_lock, shared_pdfium},
        };

        let _guard = pdfium_test_lock();
        let source_path = std::env::var_os("ROSETTA_PDF_V3_REGION_RENDER_PROBE")
            .map(PathBuf::from)
            .expect("ROSETTA_PDF_V3_REGION_RENDER_PROBE");
        let page_number = std::env::var("ROSETTA_PDF_V3_REGION_RENDER_PAGE")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);
        let output_path = std::env::var_os("ROSETTA_PDF_V3_REGION_RENDER_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tmp/pdfs/pdf-v3-region-render-output.pdf")
            });
        let regular_path = std::env::var_os("ROSETTA_PDF_V3_REGION_REGULAR_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Regular.ttf"));
        let bold_path = std::env::var_os("ROSETTA_PDF_V3_REGION_BOLD_FONT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Leo\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\assets\babeldoc\fonts\SourceHanSansCN-Bold.ttf"));

        let handle = DocumentHandle::open(shared_pdfium(), &source_path).expect("document handle");
        let page =
            build_reconciled_page_graph_from_handle(&handle, page_number).expect("reconciled page");
        let plan = build_visual_paragraph_page_plan(&page).expect("paragraph plan");
        let results = plan
            .units
            .iter()
            .map(|unit| TranslationUnitResult {
                unit_id: unit.unit_id.clone(),
                translated_text: format!(
                    "这是用于验证整段回填的中文译文。{}",
                    "译".repeat((unit.source_text.chars().count() * 35 / 100).max(4))
                ),
            })
            .collect::<Vec<_>>();
        let translations = resolve_visual_paragraph_results(&plan, results, "zh-CN")
            .expect("resolved translations");
        let patch = build_region_translation_patch(
            &page,
            RegionTranslationPatchDraft {
                plan,
                translations,
                metadata: TranslationPatchDraftMetadata {
                    target_language: "zh-CN".to_string(),
                    translation_revision: 1,
                    provider_id: "region-render-probe".to_string(),
                    model_id: "synthetic-no-provider".to_string(),
                    renderer_version: REGION_TRANSLATION_RENDERER_VERSION.to_string(),
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
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = lopdf::Document::load_mem(&source).expect("lopdf source");
        let result = super::render_region_translation_patch(
            &mut document,
            &page,
            &patch,
            &[&regular, &bold],
            RegionLayoutPolicy::default(),
        )
        .expect("region render");
        assert!(result.rendered_container_count > 0);
        assert!(result.rendered_line_count > 0);
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save rendered PDF");
        let parent = output_path.parent().expect("output parent");
        fs::create_dir_all(parent).expect("create output directory");
        fs::write(&output_path, &output).expect("write rendered PDF");
        let output_text = shared_pdfium()
            .load_pdf_from_byte_slice(&output, None)
            .expect("PDFium output")
            .pages()
            .get((page_number - 1) as i32)
            .expect("output page")
            .text()
            .expect("output text")
            .all();
        assert!(output_text.contains("这是用于验证整段回填的中文译文"));
        println!(
            "pdf-v3 region-render-probe page={page_number} containers={} lines={} preserved={} neutralized={} outputBytes={} output={}",
            result.rendered_container_count,
            result.rendered_line_count,
            result.preserved_container_count,
            result
                .neutralization_batch
                .as_ref()
                .map(|batch| batch.replacement_count)
                .unwrap_or(0),
            output.len(),
            output_path.display(),
        );
    }
}
