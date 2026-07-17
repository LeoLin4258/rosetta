use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use lopdf::Document;

use super::{
    font::PreparedTranslationFont,
    layout::TextShowGeometryKey,
    replacement::{
        apply_text_show_replacement_batch, preflight_text_show_replacement_transaction,
        text_show_replacement_target_identity, TextShowReplacementBatchResult,
        TextShowReplacementError, TextShowReplacementRequest, TextShowReplacementTargetIdentity,
        TextShowReplacementTargetRequest,
    },
    translation_patch::{
        resolve_translation_patch_renderer_decisions, validate_translation_patch,
        TranslationPatchError,
    },
    types::{
        PageAtomSourceProvenance, PageGraph, TranslationPatch, TranslationPatchEntry,
        TranslationPatchFitStrategy, TranslationPatchRendererDecision,
    },
};

#[cfg(test)]
use super::types::{PageAtom, PageAtomSourceKind};

pub(crate) const DEFAULT_MINIMUM_FIT_SCALE: f32 = 0.9;

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

#[derive(Debug)]
pub(crate) enum TranslationPatchRenderError {
    InvalidPolicy,
    Patch(TranslationPatchError),
    Replacement(TextShowReplacementError),
}

impl fmt::Display for TranslationPatchRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("TranslationPatch render policy is invalid"),
            Self::Patch(error) => error.fmt(formatter),
            Self::Replacement(error) => error.fmt(formatter),
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

    let resolved_patch = resolve_translation_patch_renderer_decisions(page, patch, &decisions)?;
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
    use std::{collections::BTreeSet, fs, path::PathBuf};

    use lopdf::Document;

    use super::{
        render_translation_patch, request_for_entry, source_object_atoms,
        TranslationPatchRenderError, TranslationPatchRenderPolicy,
    };
    use crate::{
        pdf_v3::{
            font::{
                PreparedTranslationFont, TranslationFontAsset, TranslationFontWeight,
                UnifiedTranslationFontPlan,
            },
            identity::{compare_images, render_page},
            reconcile::build_reconciled_page_graph,
            replacement::{preflight_text_show_replacement_transaction, TextShowReplacementError},
            translation_patch::{
                build_translation_patch, TranslationPatchDraft, TranslationPatchEntryDraft,
            },
            types::{PageGraph, TranslationPatch, TranslationPatchRendererDecision},
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
            renderer_version: "pdf-v3-patch-renderer-test".to_string(),
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
}
