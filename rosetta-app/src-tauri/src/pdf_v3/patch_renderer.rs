use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::{Document, Object};

use super::{
    font::PreparedTranslationFont,
    layout::TextShowGeometryKey,
    render_cache::{
        RenderCache, RenderCacheError, RenderCacheInsertOutcome, RenderCacheKey,
        RenderCacheOptions, RenderCacheOutputKind,
    },
    replacement::{
        apply_text_show_replacement_batch, preflight_text_show_replacement_transaction,
        text_show_replacement_target_identity, TextShowReplacementBatchResult,
        TextShowReplacementError, TextShowReplacementRequest, TextShowReplacementTargetIdentity,
        TextShowReplacementTargetRequest,
    },
    translation_patch::{
        ensure_translation_patch_renderer_resolved, resolve_translation_patch_renderer_decisions,
        validate_translation_patch, TranslationPatchError,
    },
    types::{
        PageAtomSourceProvenance, PageGraph, TranslationPatch, TranslationPatchEntry,
        TranslationPatchFitStrategy, TranslationPatchRendererDecision,
    },
};

#[cfg(test)]
use super::types::{PageAtom, PageAtomSourceKind};

pub(crate) const DEFAULT_MINIMUM_FIT_SCALE: f32 = 0.9;
pub(crate) const TRANSLATION_PATCH_RENDERER_VERSION: &str =
    "rosetta-pdf-v3-translation-patch-renderer/1";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TranslationPatchRenderPolicy {
    pub minimum_fit_scale: f32,
}

impl Default for TranslationPatchRenderPolicy {
    fn default() -> Self {
        Self {
            minimum_fit_scale: DEFAULT_MINIMUM_FIT_SCALE,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TranslationPatchRenderResult {
    pub resolved_patch: TranslationPatch,
    pub batch: Option<TextShowReplacementBatchResult>,
    pub fitted_entry_count: usize,
    pub preserved_entry_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TranslationPatchPagePdf {
    pub source_fingerprint: String,
    pub render: TranslationPatchRenderResult,
    pub pdf_bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum TranslationPatchRenderError {
    InvalidPolicy,
    RendererVersionMismatch { actual: String },
    MixedRendererDecisionState,
    ResolvedRendererDecisionMismatch(String),
    PageOutOfBounds { page: u32, page_count: u32 },
    PagePdfSerialization(String),
    InvalidPagePdf(&'static str),
    Patch(TranslationPatchError),
    Replacement(TextShowReplacementError),
    Cache(RenderCacheError),
}

impl fmt::Display for TranslationPatchRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("TranslationPatch render policy is invalid"),
            Self::RendererVersionMismatch { actual } => write!(
                formatter,
                "TranslationPatch renderer version mismatch: expected {TRANSLATION_PATCH_RENDERER_VERSION}, found {actual}"
            ),
            Self::MixedRendererDecisionState => formatter.write_str(
                "TranslationPatch renderer decisions must be either all pending or all resolved",
            ),
            Self::ResolvedRendererDecisionMismatch(entry_id) => write!(
                formatter,
                "TranslationPatch entry {entry_id} resolved renderer decision is not reproducible"
            ),
            Self::PageOutOfBounds { page, page_count } => {
                write!(formatter, "PDF page {page} is outside 1..={page_count}")
            }
            Self::PagePdfSerialization(message) => formatter.write_str(message),
            Self::InvalidPagePdf(reason) => {
                write!(formatter, "rendered single-page PDF is invalid: {reason}")
            }
            Self::Patch(error) => error.fmt(formatter),
            Self::Replacement(error) => error.fmt(formatter),
            Self::Cache(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranslationPatchRenderError {}

impl From<TranslationPatchError> for TranslationPatchRenderError {
    fn from(value: TranslationPatchError) -> Self {
        Self::Patch(value)
    }
}

impl From<TextShowReplacementError> for TranslationPatchRenderError {
    fn from(value: TextShowReplacementError) -> Self {
        Self::Replacement(value)
    }
}

impl From<RenderCacheError> for TranslationPatchRenderError {
    fn from(value: RenderCacheError) -> Self {
        Self::Cache(value)
    }
}

#[derive(Debug)]
struct RenderableEntry {
    entry_id: String,
    request: TextShowReplacementRequest,
}

pub(crate) fn render_translation_patch(
    document: &mut Document,
    page: &PageGraph,
    patch: &TranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: TranslationPatchRenderPolicy,
) -> Result<TranslationPatchRenderResult, TranslationPatchRenderError> {
    if !policy.minimum_fit_scale.is_finite() || !(0.0..=1.0).contains(&policy.minimum_fit_scale) {
        return Err(TranslationPatchRenderError::InvalidPolicy);
    }
    validate_translation_patch(page, patch)?;
    validate_renderer_version(patch)?;
    let pending_entry_count = patch
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.renderer_decision,
                TranslationPatchRendererDecision::Pending
            )
        })
        .count();
    if pending_entry_count != 0 && pending_entry_count != patch.entries.len() {
        return Err(TranslationPatchRenderError::MixedRendererDecisionState);
    }

    let mut decisions = BTreeMap::<String, TranslationPatchRendererDecision>::new();
    let mut grouped = BTreeMap::<TextShowReplacementTargetIdentity, Vec<RenderableEntry>>::new();
    for entry in &patch.entries {
        let request = match request_for_entry(page, entry, policy.minimum_fit_scale) {
            Ok(request) => request,
            Err(reason_code) => {
                decisions.insert(
                    entry.entry_id.clone(),
                    TranslationPatchRendererDecision::Preserved {
                        reason_code: reason_code.to_string(),
                    },
                );
                continue;
            }
        };
        let renderable = RenderableEntry {
            entry_id: entry.entry_id.clone(),
            request,
        };
        let identity = match text_show_replacement_target_identity(document, &renderable.request) {
            Ok(identity) => identity,
            Err(error) => {
                if let Some(reason_code) = preservation_reason(&error) {
                    preserve_entries(
                        &mut decisions,
                        std::slice::from_ref(&renderable),
                        reason_code,
                    );
                    continue;
                }
                return Err(error.into());
            }
        };
        grouped.entry(identity).or_default().push(renderable);
    }

    let mut targets = Vec::new();
    for entries in grouped.into_values() {
        let text_show_ids = entries
            .iter()
            .map(|entry| entry.request.geometry.text_show_id.as_str())
            .collect::<BTreeSet<_>>();
        if text_show_ids.len() != entries.len() {
            preserve_entries(&mut decisions, &entries, "duplicate-text-show-entry");
            continue;
        }
        let requests = entries
            .iter()
            .map(|entry| entry.request.clone())
            .collect::<Vec<_>>();
        let preflight =
            match preflight_text_show_replacement_transaction(document, page, &requests, fonts) {
                Ok(preflight) => preflight,
                Err(error) => {
                    if let Some(reason_code) = preservation_reason(&error) {
                        preserve_entries(&mut decisions, &entries, reason_code);
                        continue;
                    }
                    return Err(error.into());
                }
            };
        if preflight.len() != entries.len() {
            return Err(TextShowReplacementError::SourceIdentityMismatch(
                "replacement preflight result count changed".to_string(),
            )
            .into());
        }
        let fit_by_show = preflight
            .into_iter()
            .map(|result| (result.text_show_id, result.fit_scale))
            .collect::<BTreeMap<_, _>>();
        let strategy = if entries.len() == 1 {
            TranslationPatchFitStrategy::SingleShowScale
        } else {
            TranslationPatchFitStrategy::AnchoredTransaction
        };
        for entry in &entries {
            let fit_scale = fit_by_show
                .get(&entry.request.geometry.text_show_id)
                .copied()
                .ok_or_else(|| {
                    TextShowReplacementError::SourceIdentityMismatch(
                        "replacement preflight lost a text-show identity".to_string(),
                    )
                })?;
            decisions.insert(
                entry.entry_id.clone(),
                TranslationPatchRendererDecision::Fitted {
                    strategy,
                    fit_scale,
                },
            );
        }
        targets.push(TextShowReplacementTargetRequest {
            replacements: requests,
        });
    }

    let resolved_patch = if pending_entry_count == patch.entries.len() {
        resolve_translation_patch_renderer_decisions(page, patch, &decisions)?
    } else {
        ensure_translation_patch_renderer_resolved(patch)?;
        for entry in &patch.entries {
            if decisions.get(&entry.entry_id) != Some(&entry.renderer_decision) {
                return Err(
                    TranslationPatchRenderError::ResolvedRendererDecisionMismatch(
                        entry.entry_id.clone(),
                    ),
                );
            }
        }
        patch.clone()
    };
    let batch = if targets.is_empty() {
        None
    } else {
        Some(apply_text_show_replacement_batch(
            document, page, &targets, fonts,
        )?)
    };
    let fitted_entry_count = resolved_patch
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.renderer_decision,
                TranslationPatchRendererDecision::Fitted { .. }
            )
        })
        .count();
    let preserved_entry_count = resolved_patch.entries.len() - fitted_entry_count;
    Ok(TranslationPatchRenderResult {
        resolved_patch,
        batch,
        fitted_entry_count,
        preserved_entry_count,
    })
}

pub(crate) fn render_translation_patch_page_pdf(
    mut document: Document,
    source_fingerprint: &str,
    page: &PageGraph,
    patch: &TranslationPatch,
    fonts: &[&PreparedTranslationFont],
    policy: TranslationPatchRenderPolicy,
) -> Result<TranslationPatchPagePdf, TranslationPatchRenderError> {
    let render = render_translation_patch(&mut document, page, patch, fonts, policy)?;
    let pdf_bytes = serialize_single_page_pdf(document, page.page_number)?;
    Ok(TranslationPatchPagePdf {
        source_fingerprint: source_fingerprint.to_string(),
        render,
        pdf_bytes,
    })
}

pub(crate) fn translation_patch_page_pdf_cache_key(
    source_fingerprint: &str,
    patch: &TranslationPatch,
) -> Result<RenderCacheKey, TranslationPatchRenderError> {
    ensure_translation_patch_renderer_resolved(patch)?;
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

fn validate_renderer_version(patch: &TranslationPatch) -> Result<(), TranslationPatchRenderError> {
    if patch.renderer_version != TRANSLATION_PATCH_RENDERER_VERSION {
        return Err(TranslationPatchRenderError::RendererVersionMismatch {
            actual: patch.renderer_version.clone(),
        });
    }
    Ok(())
}

pub(crate) fn insert_translation_patch_page_pdf_cache(
    cache: &RenderCache,
    artifact: &TranslationPatchPagePdf,
) -> Result<RenderCacheInsertOutcome, TranslationPatchRenderError> {
    let key = translation_patch_page_pdf_cache_key(
        &artifact.source_fingerprint,
        &artifact.render.resolved_patch,
    )?;
    Ok(cache.insert(&key, &artifact.pdf_bytes)?)
}

pub(crate) fn open_translation_patch_page_pdf_cache(
    cache: &RenderCache,
    source_fingerprint: &str,
    patch: &TranslationPatch,
) -> Result<Option<Vec<u8>>, TranslationPatchRenderError> {
    let key = translation_patch_page_pdf_cache_key(source_fingerprint, patch)?;
    let Some(lease) = cache.open(&key)? else {
        return Ok(None);
    };
    match lease.read_bytes() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(RenderCacheError::CorruptArtifact { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn serialize_single_page_pdf(
    mut document: Document,
    page_number: u32,
) -> Result<Vec<u8>, TranslationPatchRenderError> {
    let pages = document.get_pages();
    let page_count = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    if !pages.contains_key(&page_number) {
        return Err(TranslationPatchRenderError::PageOutOfBounds {
            page: page_number,
            page_count,
        });
    }
    let delete_pages = pages
        .keys()
        .copied()
        .filter(|candidate| *candidate != page_number)
        .collect::<Vec<_>>();
    document.delete_pages(&delete_pages);
    strip_single_page_navigation(&mut document);
    document.prune_objects();
    document.renumber_objects();
    document.compress();

    let mut bytes = Vec::new();
    document.save_to(&mut bytes).map_err(|error| {
        TranslationPatchRenderError::PagePdfSerialization(format!(
            "failed to serialize rendered single-page PDF: {error}"
        ))
    })?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(TranslationPatchRenderError::InvalidPagePdf("signature"));
    }
    let validated = Document::load_mem(&bytes).map_err(|error| {
        TranslationPatchRenderError::PagePdfSerialization(format!(
            "failed to validate rendered single-page PDF: {error}"
        ))
    })?;
    if validated.get_pages().len() != 1 {
        return Err(TranslationPatchRenderError::InvalidPagePdf(
            "page count is not one",
        ));
    }
    Ok(bytes)
}

fn strip_single_page_navigation(document: &mut Document) {
    let Ok(root_id) = document.trailer.get(b"Root").and_then(Object::as_reference) else {
        return;
    };
    let Ok(catalog) = document
        .get_object_mut(root_id)
        .and_then(Object::as_dict_mut)
    else {
        return;
    };
    for key in [
        b"Outlines".as_slice(),
        b"PageMode".as_slice(),
        b"OpenAction".as_slice(),
        b"PageLabels".as_slice(),
        b"Names".as_slice(),
        b"StructTreeRoot".as_slice(),
    ] {
        catalog.remove(key);
    }
}

fn request_for_entry(
    page: &PageGraph,
    entry: &TranslationPatchEntry,
    minimum_fit_scale: f32,
) -> Result<TextShowReplacementRequest, &'static str> {
    if entry.translated_text.chars().any(char::is_control) {
        return Err("translation-control-character");
    }
    let atoms_by_id = page
        .atoms
        .iter()
        .map(|atom| (atom.atom_id.as_str(), atom))
        .collect::<BTreeMap<_, _>>();
    let atoms = entry
        .atoms
        .iter()
        .filter_map(|atom| atoms_by_id.get(atom.atom_id.as_str()).copied())
        .collect::<Vec<_>>();
    if atoms.len() != entry.atoms.len() {
        return Err("entry-atom-missing");
    }
    let source_object_ids = atoms
        .iter()
        .filter_map(|atom| atom.source_object_id.as_deref())
        .collect::<BTreeSet<_>>();
    if source_object_ids.len() != 1 || atoms.iter().any(|atom| atom.source_object_id.is_none()) {
        return Err("entry-spans-source-objects");
    }
    let source_object_id = source_object_ids
        .first()
        .copied()
        .ok_or("entry-source-object-missing")?;
    let complete_atom_ids = page
        .atoms
        .iter()
        .filter(|atom| atom.source_object_id.as_deref() == Some(source_object_id))
        .filter(|atom| atom.source_provenance.is_some())
        .map(|atom| atom.atom_id.as_str())
        .collect::<BTreeSet<_>>();
    let entry_atom_ids = atoms
        .iter()
        .map(|atom| atom.atom_id.as_str())
        .collect::<BTreeSet<_>>();
    if complete_atom_ids.is_empty() || complete_atom_ids != entry_atom_ids {
        return Err("entry-source-object-incomplete");
    }

    let provenance = atoms
        .iter()
        .filter_map(|atom| atom.source_provenance.as_ref())
        .next()
        .ok_or("entry-provenance-missing")?;
    if atoms.iter().any(|atom| {
        atom.source_provenance
            .as_ref()
            .is_none_or(|candidate| !same_text_show(provenance, candidate))
    }) {
        return Err("entry-spans-text-shows");
    }
    if !matches!(
        provenance.text_show_operator.as_str(),
        "Tj" | "TJ" | "'" | "\""
    ) || !is_lower_hex_sha256(&provenance.text_show_operand_hash)
    {
        return Err("entry-provenance-invalid");
    }
    let source_font_resource = provenance
        .source_font_resource
        .clone()
        .ok_or("entry-source-font-missing")?;
    let source_font_size = provenance
        .source_font_size
        .filter(|size| size.is_finite() && *size > 0.0)
        .ok_or("entry-source-font-missing")?;
    if !provenance.source_horizontal_scaling.is_finite()
        || provenance.source_horizontal_scaling <= 0.0
    {
        return Err("entry-source-scaling-invalid");
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
        translated_text: entry.translated_text.clone(),
        minimum_fit_scale,
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
        && expected.source_font_resource == actual.source_font_resource
        && expected.source_font_size == actual.source_font_size
        && expected.source_horizontal_scaling == actual.source_horizontal_scaling
}

fn preserve_entries(
    decisions: &mut BTreeMap<String, TranslationPatchRendererDecision>,
    entries: &[RenderableEntry],
    reason_code: &'static str,
) {
    for entry in entries {
        decisions.insert(
            entry.entry_id.clone(),
            TranslationPatchRendererDecision::Preserved {
                reason_code: reason_code.to_string(),
            },
        );
    }
}

fn preservation_reason(error: &TextShowReplacementError) -> Option<&'static str> {
    match error {
        TextShowReplacementError::FitBounds(_) => Some("fit-bounds-unsupported"),
        TextShowReplacementError::Style(_) => Some("source-style-unsupported"),
        TextShowReplacementError::MissingTranslationFontFace(_) => {
            Some("translation-font-face-unavailable")
        }
        TextShowReplacementError::MissingSourceFontState => Some("source-font-state-unsupported"),
        TextShowReplacementError::SourcePaintStateUnsupported => {
            Some("source-paint-state-unsupported")
        }
        TextShowReplacementError::LaterTextShowInTextObject
        | TextShowReplacementError::InvalidTextPositionReset
        | TextShowReplacementError::CrossTextObjectTransaction => Some("text-anchor-unsupported"),
        TextShowReplacementError::UnsupportedOperator(_) => Some("text-operator-unsupported"),
        TextShowReplacementError::EmptyTranslation => Some("translation-empty"),
        TextShowReplacementError::InvalidFitBounds => Some("fit-bounds-invalid"),
        TextShowReplacementError::Overflow { .. } => Some("translation-overflow"),
        _ => None,
    }
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
fn source_object_atoms<'a>(page: &'a PageGraph, atom: &PageAtom) -> Vec<&'a PageAtom> {
    page.atoms
        .iter()
        .filter(|candidate| candidate.source_object_id == atom.source_object_id)
        .filter(|candidate| {
            !matches!(
                candidate.source_kind,
                PageAtomSourceKind::PdfiumSyntheticWhitespace
                    | PageAtomSourceKind::PreservedUnmapped
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use lopdf::Document;

    use super::{
        insert_translation_patch_page_pdf_cache, open_translation_patch_page_pdf_cache,
        render_translation_patch, render_translation_patch_page_pdf, request_for_entry,
        source_object_atoms, translation_patch_page_pdf_cache_key, TranslationPatchRenderError,
        TranslationPatchRenderPolicy, TRANSLATION_PATCH_RENDERER_VERSION,
    };
    use crate::{
        pdf_v3::{
            font::{
                PreparedTranslationFont, TranslationFontAsset, TranslationFontWeight,
                UnifiedTranslationFontPlan,
            },
            identity::{compare_images, render_page},
            reconcile::build_reconciled_page_graph,
            render_cache::{RenderCache, RenderCacheConfig, RenderCacheInsertKind},
            replacement::{preflight_text_show_replacement_transaction, TextShowReplacementError},
            translation_patch::{
                build_translation_patch, resolve_translation_patch_renderer_decisions,
                TranslationPatchDraft, TranslationPatchEntryDraft,
            },
            types::{
                PageGraph, TranslationPatch, TranslationPatchFitStrategy,
                TranslationPatchRendererDecision,
            },
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[cfg(target_os = "windows")]
    #[test]
    fn renders_pending_patch_to_searchable_pdf_and_resolved_identity() {
        let _guard = pdfium_test_lock();
        let replacement = "Unified patch renderer";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let patch = first_renderable_patch(&document, &page, &prepared, replacement);
        let pending_patch_id = patch.patch_id.clone();

        let result = render_translation_patch(
            &mut document,
            &page,
            &patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("render patch");
        assert_eq!(result.fitted_entry_count, 1);
        assert_eq!(result.preserved_entry_count, 0);
        assert_eq!(
            result
                .batch
                .as_ref()
                .expect("replacement batch")
                .replacement_count,
            1
        );
        assert_ne!(result.resolved_patch.patch_id, pending_patch_id);
        assert!(matches!(
            result.resolved_patch.entries[0].renderer_decision,
            TranslationPatchRendererDecision::Fitted { fit_scale, .. }
                if fit_scale >= 0.9
        ));

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save rendered PDF");
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
        assert!(output_text.contains(replacement));

        let source_document = shared_pdfium()
            .load_pdf_from_byte_slice(&source, None)
            .expect("PDFium source");
        let source_page = source_document.pages().get(0).expect("source page");
        let output_page = output_document.pages().get(0).expect("output page");
        let source_image = render_page(&source_page, 1, 1440).expect("source render");
        let output_image = render_page(&output_page, 1, 1440).expect("output render");
        let difference = compare_images(&source_image, &output_image).expect("image difference");
        assert!(difference.changed_pixel_count > 0);
        assert!(difference.changed_pixel_ratio < 0.05);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolved_patch_rerenders_with_the_same_identity_and_page_bytes() {
        let _guard = pdfium_test_lock();
        let replacement = "Resolved patch replay";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let source_document = Document::load_mem(&source).expect("source document");
        let pending = first_renderable_patch(&source_document, &page, &prepared, replacement);

        let first = render_translation_patch_page_pdf(
            source_document,
            "sha256:resolved-replay-source",
            &page,
            &pending,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("initial page render");
        let replay = render_translation_patch_page_pdf(
            Document::load_mem(&source).expect("replay document"),
            "sha256:resolved-replay-source",
            &page,
            &first.render.resolved_patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("resolved page replay");

        assert_eq!(replay.render.resolved_patch, first.render.resolved_patch);
        assert_eq!(replay.pdf_bytes, first.pdf_bytes);
        assert_eq!(
            Document::load_mem(&replay.pdf_bytes)
                .expect("replayed page PDF")
                .get_pages()
                .len(),
            1
        );
        let output = shared_pdfium()
            .load_pdf_from_byte_slice(&replay.pdf_bytes, None)
            .expect("PDFium replay output");
        assert!(output
            .pages()
            .get(0)
            .expect("output page")
            .text()
            .expect("output text")
            .all()
            .contains(replacement));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolved_decision_drift_is_rejected_before_document_mutation() {
        let _guard = pdfium_test_lock();
        let replacement = "Decision drift";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let pending = first_renderable_patch(&document, &page, &prepared, replacement);
        let decisions = BTreeMap::from([(
            pending.entries[0].entry_id.clone(),
            TranslationPatchRendererDecision::Fitted {
                strategy: TranslationPatchFitStrategy::SingleShowScale,
                fit_scale: 0.5,
            },
        )]);
        let drifted = resolve_translation_patch_renderer_decisions(&page, &pending, &decisions)
            .expect("valid but drifted resolved patch");
        let before = document.clone();

        assert!(matches!(
            render_translation_patch(
                &mut document,
                &page,
                &drifted,
                &[&prepared],
                TranslationPatchRenderPolicy::default(),
            )
            .expect_err("resolved fit decision must be reproducible"),
            TranslationPatchRenderError::ResolvedRendererDecisionMismatch(_)
        ));
        assert_eq!(document.objects, before.objects);
        assert_eq!(document.max_id, before.max_id);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn renderer_version_mismatch_cannot_render_or_address_current_cache() {
        let _guard = pdfium_test_lock();
        let replacement = "Old renderer patch";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let current = first_renderable_patch(&document, &page, &prepared, replacement);
        let atom_ids = current.entries[0]
            .atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let mut old_draft = patch_draft(vec![(atom_ids, replacement)]);
        old_draft.renderer_version = "rosetta-pdf-v3-translation-patch-renderer/0".to_string();
        let old_pending = build_translation_patch(&page, old_draft).expect("old pending patch");
        let before = document.clone();

        assert!(matches!(
            render_translation_patch(
                &mut document,
                &page,
                &old_pending,
                &[&prepared],
                TranslationPatchRenderPolicy::default(),
            )
            .expect_err("old renderer version"),
            TranslationPatchRenderError::RendererVersionMismatch { .. }
        ));
        assert_eq!(document.objects, before.objects);
        assert_eq!(document.max_id, before.max_id);

        let old_resolved = resolve_translation_patch_renderer_decisions(
            &page,
            &old_pending,
            &BTreeMap::from([(
                old_pending.entries[0].entry_id.clone(),
                TranslationPatchRendererDecision::Preserved {
                    reason_code: "version-test".to_string(),
                },
            )]),
        )
        .expect("old resolved patch");
        assert!(matches!(
            translation_patch_page_pdf_cache_key("sha256:source", &old_resolved)
                .expect_err("old cache namespace"),
            TranslationPatchRenderError::RendererVersionMismatch { .. }
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn translated_single_page_pdf_round_trips_through_bounded_cache() {
        let _guard = pdfium_test_lock();
        let replacement = "Bounded cached page";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("2305.13048v2.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let source_document = Document::load_mem(&source).expect("source document");
        assert!(source_document.get_pages().len() > 1);
        let pending = first_renderable_patch(&source_document, &page, &prepared, replacement);
        let source_fingerprint = "sha256:cache-bridge-source";
        let artifact = render_translation_patch_page_pdf(
            source_document,
            source_fingerprint,
            &page,
            &pending,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("rendered page artifact");
        assert_eq!(
            Document::load_mem(&artifact.pdf_bytes)
                .expect("page artifact")
                .get_pages()
                .len(),
            1
        );
        assert!(artifact.pdf_bytes.len() < source.len());

        let temp = TestDirectory::new("cache-bridge");
        let cache = RenderCache::new(
            temp.path(),
            RenderCacheConfig {
                max_bytes: 8 * 1024 * 1024,
                max_entries: 8,
            },
        )
        .expect("render cache");
        assert!(open_translation_patch_page_pdf_cache(
            &cache,
            source_fingerprint,
            &artifact.render.resolved_patch,
        )
        .expect("cache miss")
        .is_none());
        assert!(matches!(
            translation_patch_page_pdf_cache_key(source_fingerprint, &pending)
                .expect_err("pending patch has no cache identity"),
            TranslationPatchRenderError::Patch(
                crate::pdf_v3::translation_patch::TranslationPatchError::RendererDecisionPending(_)
            )
        ));

        let inserted =
            insert_translation_patch_page_pdf_cache(&cache, &artifact).expect("cache insert");
        assert_eq!(inserted.kind, RenderCacheInsertKind::Written);
        assert_eq!(cache.snapshot().expect("cache snapshot").entry_count, 1);
        let cached = open_translation_patch_page_pdf_cache(
            &cache,
            source_fingerprint,
            &artifact.render.resolved_patch,
        )
        .expect("cache hit")
        .expect("cached bytes");
        assert_eq!(cached, artifact.pdf_bytes);

        let replay = render_translation_patch_page_pdf(
            Document::load_mem(&source).expect("replay source"),
            source_fingerprint,
            &page,
            &artifact.render.resolved_patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("cache miss rebuild");
        assert_eq!(replay.pdf_bytes, artifact.pdf_bytes);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn renders_safe_entry_while_preserving_incomplete_source_object() {
        let _guard = pdfium_test_lock();
        let safe_translation = "Safe translated row";
        let preserved_translation = "Must remain source";
        let prepared = prepared_arial(&[safe_translation, preserved_translation]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let safe_patch = first_renderable_patch(&document, &page, &prepared, safe_translation);
        let safe_atom_ids = safe_patch.entries[0]
            .atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let safe_ids = safe_atom_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let partial_atom = page
            .atoms
            .iter()
            .find(|atom| {
                atom.source_provenance.is_some()
                    && !safe_ids.contains(atom.atom_id.as_str())
                    && source_object_atoms(&page, atom).len() > 1
            })
            .expect("partial object atom");
        let patch = build_translation_patch(
            &page,
            patch_draft(vec![
                (safe_atom_ids, safe_translation),
                (vec![partial_atom.atom_id.clone()], preserved_translation),
            ]),
        )
        .expect("mixed patch");

        let result = render_translation_patch(
            &mut document,
            &page,
            &patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("mixed render");
        assert_eq!(result.fitted_entry_count, 1);
        assert_eq!(result.preserved_entry_count, 1);
        assert_eq!(
            result
                .batch
                .as_ref()
                .expect("replacement batch")
                .replacement_count,
            1
        );
        assert!(matches!(
            &result.resolved_patch.entries[1].renderer_decision,
            TranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "entry-source-object-incomplete"
        ));

        let mut output = Vec::new();
        document.save_to(&mut output).expect("save mixed output");
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
        assert!(output_text.contains(safe_translation));
        assert!(!output_text.contains(preserved_translation));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stale_operand_identity_leaves_document_unmodified() {
        let _guard = pdfium_test_lock();
        let replacement = "Stale replacement";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mut page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let base_patch = first_renderable_patch(&document, &page, &prepared, replacement);
        let atom_ids = base_patch.entries[0]
            .atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        for atom in &mut page.atoms {
            if atom_ids.contains(&atom.atom_id) {
                atom.source_provenance
                    .as_mut()
                    .expect("mapped atom")
                    .text_show_operand_hash = "0".repeat(64);
            }
        }
        let stale_patch =
            build_translation_patch(&page, patch_draft(vec![(atom_ids, replacement)]))
                .expect("stale identity patch");
        let before = document.clone();

        let error = render_translation_patch(
            &mut document,
            &page,
            &stale_patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect_err("stale source must fail");
        assert!(matches!(
            error,
            TranslationPatchRenderError::Replacement(
                TextShowReplacementError::SourceIdentityMismatch(_)
            )
        ));
        assert_eq!(document.objects, before.objects);
        assert_eq!(document.max_id, before.max_id);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn target_outside_text_object_is_preserved_without_mutation() {
        let _guard = pdfium_test_lock();
        let replacement = "Preserved replacement";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let mut page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let base_patch = first_renderable_patch(&document, &page, &prepared, replacement);
        let atom_ids = base_patch.entries[0]
            .atoms
            .iter()
            .map(|atom| atom.atom_id.clone())
            .collect::<Vec<_>>();
        let provenance = page
            .atoms
            .iter()
            .find(|atom| atom_ids.contains(&atom.atom_id))
            .and_then(|atom| atom.source_provenance.as_ref())
            .expect("mapped atom provenance");
        let stream = document
            .get_object((
                provenance.stream_object_number,
                provenance.stream_generation,
            ))
            .and_then(lopdf::Object::as_stream)
            .expect("source stream");
        let content = lopdf::content::Content::decode(
            &stream.get_plain_content().expect("plain source stream"),
        )
        .expect("decoded source stream");
        let first_text_object = content
            .operations
            .iter()
            .position(|operation| operation.operator == "BT")
            .expect("BT operation");
        assert!(first_text_object > 0);
        let outside_text_object = first_text_object - 1;
        for atom in &mut page.atoms {
            if atom_ids.contains(&atom.atom_id) {
                atom.source_provenance
                    .as_mut()
                    .expect("mapped atom")
                    .operation_index = outside_text_object;
            }
        }
        let patch = build_translation_patch(&page, patch_draft(vec![(atom_ids, replacement)]))
            .expect("unsupported target patch");
        let before = document.clone();

        let result = render_translation_patch(
            &mut document,
            &page,
            &patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("unsupported target preserves source");
        assert_eq!(result.fitted_entry_count, 0);
        assert_eq!(result.preserved_entry_count, 1);
        assert!(result.batch.is_none());
        assert!(matches!(
            &result.resolved_patch.entries[0].renderer_decision,
            TranslationPatchRendererDecision::Preserved { reason_code }
                if reason_code == "text-anchor-unsupported"
        ));
        assert_eq!(document.objects, before.objects);
        assert_eq!(document.max_id, before.max_id);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows TranslationPatch renderer Poppler probe"]
    fn manual_windows_translation_patch_renderer_probe() {
        let _guard = pdfium_test_lock();
        let replacement = "Unified patch renderer";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let mut document = Document::load_mem(&source).expect("source document");
        let patch = first_renderable_patch(&document, &page, &prepared, replacement);
        let result = render_translation_patch(
            &mut document,
            &page,
            &patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("render patch");
        let mut output = Vec::new();
        document.save_to(&mut output).expect("save output");

        let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/pdfs");
        fs::create_dir_all(&output_dir).expect("create PDF probe directory");
        fs::write(output_dir.join("pdf-v3-patch-render-source.pdf"), &source)
            .expect("write probe source");
        fs::write(output_dir.join("pdf-v3-patch-render-output.pdf"), &output)
            .expect("write probe output");
        fs::write(
            output_dir.join("pdf-v3-patch-render-resolved.json"),
            serde_json::to_vec_pretty(&result.resolved_patch).expect("resolved patch JSON"),
        )
        .expect("write resolved patch");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows TranslationPatch cache bridge Poppler probe"]
    fn manual_windows_translation_patch_cache_probe() {
        let _guard = pdfium_test_lock();
        let replacement = "Bounded cached page";
        let prepared = prepared_arial(&[replacement]);
        let source_path = fixture_path("2305.13048v2.pdf");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let source = fs::read(&source_path).expect("source PDF");
        let document = Document::load_mem(&source).expect("source document");
        let patch = first_renderable_patch(&document, &page, &prepared, replacement);
        let artifact = render_translation_patch_page_pdf(
            document,
            "sha256:manual-cache-bridge-source",
            &page,
            &patch,
            &[&prepared],
            TranslationPatchRenderPolicy::default(),
        )
        .expect("render page artifact");

        let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/pdfs");
        fs::create_dir_all(&output_dir).expect("create PDF probe directory");
        fs::write(output_dir.join("pdf-v3-cache-bridge-source.pdf"), &source)
            .expect("write probe source");
        fs::write(
            output_dir.join("pdf-v3-cache-bridge-output.pdf"),
            &artifact.pdf_bytes,
        )
        .expect("write probe output");
        fs::write(
            output_dir.join("pdf-v3-cache-bridge-resolved.json"),
            serde_json::to_vec_pretty(&artifact.render.resolved_patch)
                .expect("resolved patch JSON"),
        )
        .expect("write resolved patch");
    }

    #[cfg(target_os = "windows")]
    fn first_renderable_patch(
        document: &Document,
        page: &PageGraph,
        prepared: &PreparedTranslationFont,
        translation: &str,
    ) -> TranslationPatch {
        let mut visited = BTreeSet::new();
        for atom in &page.atoms {
            let Some(source_object_id) = atom.source_object_id.as_deref() else {
                continue;
            };
            if !visited.insert(source_object_id) || atom.source_provenance.is_none() {
                continue;
            }
            let atom_ids = source_object_atoms(page, atom)
                .into_iter()
                .map(|atom| atom.atom_id.clone())
                .collect::<Vec<_>>();
            let Ok(patch) =
                build_translation_patch(page, patch_draft(vec![(atom_ids, translation)]))
            else {
                continue;
            };
            let Ok(request) = request_for_entry(page, &patch.entries[0], 0.9) else {
                continue;
            };
            if preflight_text_show_replacement_transaction(document, page, &[request], &[prepared])
                .is_ok()
            {
                return patch;
            }
        }
        panic!("fixture must contain one renderable source object");
    }

    #[cfg(target_os = "windows")]
    fn prepared_arial(translations: &[&str]) -> PreparedTranslationFont {
        let asset = TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            PathBuf::from(r"C:\Windows\Fonts\arial.ttf").as_path(),
            0,
        )
        .expect("Windows Arial font");
        let mut plan = UnifiedTranslationFontPlan::default();
        for translation in translations {
            plan.add_text(translation);
        }
        asset.prepare(&plan).expect("prepared Arial subset")
    }

    fn patch_draft(entries: Vec<(Vec<String>, &str)>) -> TranslationPatchDraft {
        TranslationPatchDraft {
            target_language: "en".to_string(),
            translation_revision: 1,
            provider_id: "rwkv-local".to_string(),
            model_id: "rwkv-test".to_string(),
            renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
            entries: entries
                .into_iter()
                .map(|(atom_ids, translated_text)| TranslationPatchEntryDraft {
                    atom_ids,
                    translated_text: translated_text.to_string(),
                    protected_spans: Vec::new(),
                })
                .collect(),
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-patch-renderer-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
