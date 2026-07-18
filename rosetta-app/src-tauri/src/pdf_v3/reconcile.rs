use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
};

use pdfium_render::prelude::Pdfium;

use super::{
    document::{DocumentHandle, DocumentHandleError},
    extract::{extract_pdfium_page_snapshot, PdfV3ExtractionError},
    mapping::{
        map_page_atoms_to_content_operands_from_snapshot, DecodedTextUnit, OperandMappingStatus,
        PageOperandMappingError, PageOperandMappingResult, TextObjectOperandMapping,
    },
    types::{
        PageAtomSourceKind, PageAtomSourceProvenance, PageGraph, PageReconciliationStatus,
        PageReconciliationSummary,
    },
};

#[derive(Debug)]
pub(crate) enum PageGraphReconciliationError {
    Document(DocumentHandleError),
    Extraction(PdfV3ExtractionError),
    Mapping(PageOperandMappingError),
}

impl fmt::Display for PageGraphReconciliationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(formatter),
            Self::Extraction(error) => error.fmt(formatter),
            Self::Mapping(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PageGraphReconciliationError {}

impl From<DocumentHandleError> for PageGraphReconciliationError {
    fn from(value: DocumentHandleError) -> Self {
        Self::Document(value)
    }
}

impl From<PdfV3ExtractionError> for PageGraphReconciliationError {
    fn from(value: PdfV3ExtractionError) -> Self {
        Self::Extraction(value)
    }
}

impl From<PageOperandMappingError> for PageGraphReconciliationError {
    fn from(value: PageOperandMappingError) -> Self {
        Self::Mapping(value)
    }
}

#[derive(Debug)]
struct DecodedScalar {
    character: char,
    operand_id: String,
    operand_index: usize,
    array_index: Option<usize>,
    encoded_start: usize,
    encoded_len: usize,
    source_unit_char_index: usize,
    source_unit_char_count: usize,
}

#[derive(Debug)]
struct AtomUpdate {
    atom_index: usize,
    source_text: String,
    source_kind: PageAtomSourceKind,
    source_provenance: Option<PageAtomSourceProvenance>,
    generated: bool,
    requires_translation: bool,
}

#[derive(Debug)]
struct ObjectPlan {
    updates: Vec<AtomUpdate>,
    unrepresented_source_whitespace_count: usize,
}

pub(crate) fn build_reconciled_page_graph(
    pdfium: &Pdfium,
    source_path: &Path,
    page_number: u32,
) -> Result<PageGraph, PageGraphReconciliationError> {
    let handle = DocumentHandle::open(pdfium, source_path)?;
    build_reconciled_page_graph_from_handle(&handle, page_number)
}

pub(crate) fn build_reconciled_page_graph_from_handle(
    handle: &DocumentHandle<'_>,
    page_number: u32,
) -> Result<PageGraph, PageGraphReconciliationError> {
    let snapshot = extract_pdfium_page_snapshot(handle, page_number)?;
    let mapping = map_page_atoms_to_content_operands_from_snapshot(handle, &snapshot)?;
    Ok(reconcile_page_graph(snapshot.page_graph, mapping))
}

fn reconcile_page_graph(mut page: PageGraph, mapping: PageOperandMappingResult) -> PageGraph {
    for atom in &mut page.atoms {
        atom.source_kind = PageAtomSourceKind::PreservedUnmapped;
        atom.source_provenance = None;
        if (atom.generated || atom.source_object_id.is_none())
            && atom.source_text.chars().all(char::is_whitespace)
        {
            atom.source_kind = PageAtomSourceKind::PdfiumSyntheticWhitespace;
            atom.generated = true;
            atom.requires_translation = false;
        }
    }

    let mut atom_indices_by_object = BTreeMap::<String, Vec<usize>>::new();
    for (atom_index, atom) in page.atoms.iter().enumerate() {
        if let Some(source_object_id) = atom.source_object_id.as_ref() {
            atom_indices_by_object
                .entry(source_object_id.clone())
                .or_default()
                .push(atom_index);
        }
    }

    let mut mapped_objects = BTreeSet::new();
    let mut fallback_reasons = BTreeSet::new();
    let mut unrepresented_source_whitespace_count = 0usize;
    for reason in &mapping.fallback_reasons {
        if matches!(
            reason.as_str(),
            "direct-form-xobject-stream-unsupported"
                | "form-xobject-depth-limit"
                | "form-xobject-invocation-count-mismatch"
                | "form-xobject-name-missing"
                | "form-xobject-reference-cycle"
        ) {
            fallback_reasons.insert(reason.clone());
        }
    }
    if !mapping.ordinal_alignment_valid {
        fallback_reasons.insert("text-object-show-count-mismatch".to_string());
    }

    for object_mapping in &mapping.mappings {
        let Some(atom_indices) = atom_indices_by_object.get(&object_mapping.source_object_id)
        else {
            fallback_reasons.insert("mapped-text-object-has-no-page-atoms".to_string());
            continue;
        };
        match plan_object_updates(&page, atom_indices, object_mapping) {
            Ok(plan) => {
                for update in plan.updates {
                    let atom = &mut page.atoms[update.atom_index];
                    atom.source_text = update.source_text;
                    atom.source_kind = update.source_kind;
                    atom.source_provenance = update.source_provenance;
                    atom.generated = update.generated;
                    atom.requires_translation = update.requires_translation;
                }
                unrepresented_source_whitespace_count += plan.unrepresented_source_whitespace_count;
                mapped_objects.insert(object_mapping.source_object_id.clone());
            }
            Err(reason) => {
                fallback_reasons.insert(reason.to_string());
            }
        }
    }

    let verified_atom_count = count_atoms(&page, PageAtomSourceKind::PdfiumVerified);
    let corrected_atom_count = count_atoms(&page, PageAtomSourceKind::ToUnicodeCorrected);
    let synthetic_whitespace_atom_count =
        count_atoms(&page, PageAtomSourceKind::PdfiumSyntheticWhitespace);
    let preserved_atom_count = count_atoms(&page, PageAtomSourceKind::PreservedUnmapped);
    let preserved_object_count = mapping
        .pdfium_text_object_count
        .saturating_sub(mapped_objects.len());
    let status =
        if (page.atoms.is_empty() || preserved_atom_count == 0) && fallback_reasons.is_empty() {
            PageReconciliationStatus::Complete
        } else if mapped_objects.is_empty() {
            PageReconciliationStatus::Preserved
        } else {
            PageReconciliationStatus::Partial
        };
    if corrected_atom_count > 0 {
        page.warnings
            .push("pdfium-unicode-corrected-from-to-unicode".to_string());
    }
    if synthetic_whitespace_atom_count > 0 {
        page.warnings
            .push("pdfium-synthetic-whitespace-reconciled".to_string());
    }
    if unrepresented_source_whitespace_count > 0 {
        page.warnings
            .push("source-whitespace-has-no-pdfium-atom".to_string());
    }
    page.warnings.extend(fallback_reasons.iter().cloned());
    page.warnings.sort();
    page.warnings.dedup();
    page.reconciliation = PageReconciliationSummary {
        status,
        mapped_object_count: mapped_objects.len(),
        preserved_object_count,
        verified_atom_count,
        corrected_atom_count,
        synthetic_whitespace_atom_count,
        unrepresented_source_whitespace_count,
        preserved_atom_count,
        fallback_reasons: fallback_reasons.into_iter().collect(),
    };

    page
}

fn plan_object_updates(
    page: &PageGraph,
    atom_indices: &[usize],
    mapping: &TextObjectOperandMapping,
) -> Result<ObjectPlan, &'static str> {
    match mapping.status {
        OperandMappingStatus::OrdinalCountMismatch => {
            return Err("text-object-show-count-mismatch");
        }
        OperandMappingStatus::OutsideTextObject => return Err("text-show-outside-bt-et"),
        OperandMappingStatus::DecodeUnavailable => return Err("text-show-decode-unavailable"),
        OperandMappingStatus::FontMismatch => return Err("font-resource-object-mismatch"),
        OperandMappingStatus::AtomCoverageMismatch => {
            return Err("pdfium-object-atom-coverage-mismatch");
        }
        OperandMappingStatus::Exact
        | OperandMappingStatus::WhitespaceEquivalent
        | OperandMappingStatus::DecodedTextMismatch
        | OperandMappingStatus::SharedContentStream => {}
    }
    if !mapping.font_name_match {
        return Err("font-resource-object-mismatch");
    }
    if !mapping.atom_coverage_match {
        return Err("pdfium-object-atom-coverage-mismatch");
    }
    let decoded = flatten_decoded_units(&mapping.decoded_units)?;
    if decoded.is_empty() && !atom_indices.is_empty() {
        return Err("text-show-decode-unavailable");
    }

    let mut updates = Vec::with_capacity(atom_indices.len());
    let mut decoded_index = 0usize;
    let mut unrepresented_source_whitespace_count = 0usize;
    for atom_index in atom_indices.iter().copied() {
        let atom = &page.atoms[atom_index];
        let mut atom_characters = atom.source_text.chars();
        let Some(pdfium_character) = atom_characters.next() else {
            return Err("pdfium-atom-has-empty-text");
        };
        if atom_characters.next().is_some() {
            return Err("pdfium-atom-has-multiple-characters");
        }

        if !pdfium_character.is_whitespace() {
            while decoded
                .get(decoded_index)
                .is_some_and(|scalar| scalar.character.is_whitespace())
            {
                decoded_index += 1;
                unrepresented_source_whitespace_count += 1;
            }
        }
        let next_decoded = decoded.get(decoded_index);
        if pdfium_character.is_whitespace()
            && next_decoded.is_none_or(|scalar| !scalar.character.is_whitespace())
        {
            updates.push(AtomUpdate {
                atom_index,
                source_text: atom.source_text.clone(),
                source_kind: PageAtomSourceKind::PdfiumSyntheticWhitespace,
                source_provenance: None,
                generated: true,
                requires_translation: false,
            });
            continue;
        }

        let Some(scalar) = next_decoded else {
            return Err("decoded-text-shorter-than-pdfium-atoms");
        };
        decoded_index += 1;
        let source_kind = if pdfium_character == scalar.character {
            PageAtomSourceKind::PdfiumVerified
        } else {
            PageAtomSourceKind::ToUnicodeCorrected
        };
        updates.push(AtomUpdate {
            atom_index,
            source_text: scalar.character.to_string(),
            source_kind,
            source_provenance: Some(PageAtomSourceProvenance {
                mapping_id: mapping.mapping_id.clone(),
                text_show_id: mapping.text_show_id.clone(),
                text_show_index: mapping.text_show_index,
                operand_id: scalar.operand_id.clone(),
                operand_index: scalar.operand_index,
                array_index: scalar.array_index,
                encoded_start: scalar.encoded_start,
                encoded_len: scalar.encoded_len,
                source_unit_char_index: scalar.source_unit_char_index,
                source_unit_char_count: scalar.source_unit_char_count,
                form_invocation_path: mapping.form_invocation_path.clone(),
                stream_object_number: mapping.stream_object_number,
                stream_generation: mapping.stream_generation,
                operation_index: mapping.operation_index,
                text_show_operator: mapping.text_show_operator.clone(),
                text_show_operand_hash: mapping.text_show_operand_hash.clone(),
                source_font_resource: mapping.source_font_resource.clone(),
                source_font_size: mapping.source_font_size,
                source_horizontal_scaling: mapping.source_horizontal_scaling,
            }),
            generated: atom.generated,
            requires_translation: !atom.generated
                && scalar.character != '\u{fffd}'
                && !scalar.character.is_whitespace(),
        });
    }
    while decoded
        .get(decoded_index)
        .is_some_and(|scalar| scalar.character.is_whitespace())
    {
        decoded_index += 1;
        unrepresented_source_whitespace_count += 1;
    }
    if decoded_index != decoded.len() {
        return Err("decoded-text-longer-than-pdfium-atoms");
    }
    Ok(ObjectPlan {
        updates,
        unrepresented_source_whitespace_count,
    })
}

fn flatten_decoded_units(units: &[DecodedTextUnit]) -> Result<Vec<DecodedScalar>, &'static str> {
    let mut scalars = Vec::new();
    for unit in units {
        let characters = unit.text.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            return Err("decoded-source-unit-is-empty");
        }
        let source_unit_char_count = characters.len();
        scalars.extend(characters.into_iter().enumerate().map(
            |(source_unit_char_index, character)| DecodedScalar {
                character,
                operand_id: unit.operand_id.clone(),
                operand_index: unit.operand_index,
                array_index: unit.array_index,
                encoded_start: unit.encoded_start,
                encoded_len: unit.encoded_len,
                source_unit_char_index,
                source_unit_char_count,
            },
        ));
    }
    Ok(scalars)
}

fn count_atoms(page: &PageGraph, source_kind: PageAtomSourceKind) -> usize {
    page.atoms
        .iter()
        .filter(|atom| atom.source_kind == source_kind)
        .count()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::{
        build_reconciled_page_graph, build_reconciled_page_graph_from_handle, reconcile_page_graph,
    };
    use crate::{
        pdf_v3::{
            content_stream::probe_content_stream_save_only,
            document::DocumentHandle,
            extract::extract_pdfium_page_snapshot,
            mapping::map_page_atoms_to_content_operands_from_snapshot,
            types::{PageAtomSourceKind, PageReconciliationStatus},
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn one_document_handle_reconciles_sparse_pages() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");

        let first = build_reconciled_page_graph_from_handle(&handle, 1).expect("page 1");
        let third = build_reconciled_page_graph_from_handle(&handle, 3).expect("page 3");

        assert_eq!(first.page_number, 1);
        assert_eq!(third.page_number, 3);
        assert!(!first.atoms.is_empty());
        assert!(!third.atoms.is_empty());
        assert_ne!(first.source_page_hash, third.source_page_hash);
    }

    #[test]
    #[ignore = "manual Windows reused DocumentHandle timing probe"]
    fn manual_windows_reused_document_handle_probe() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");

        let first_started = Instant::now();
        let first = build_reconciled_page_graph_from_handle(&handle, 1).expect("page 1");
        let first_ms = first_started.elapsed().as_millis();
        let third_started = Instant::now();
        let third = build_reconciled_page_graph_from_handle(&handle, 3).expect("page 3");
        let third_ms = third_started.elapsed().as_millis();

        println!(
            "pdf-v3 reused handle source_bytes={} pages={} open={}ms page1={}ms page3={}ms atoms={}/{} statuses={:?}/{:?}",
            handle.source_bytes(),
            handle.page_count(),
            handle.open_elapsed().as_millis(),
            first_ms,
            third_ms,
            first.atoms.len(),
            third.atoms.len(),
            first.reconciliation.status,
            third.reconciliation.status,
        );
    }

    #[test]
    #[ignore = "manual Windows ten-page PageGraph timing benchmark"]
    fn manual_windows_ten_page_reconciliation_benchmark() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("2305.13048v2.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let started = Instant::now();
        let mut page_ms = Vec::new();
        let mut extraction_ms = Vec::new();
        let mut setup_ms = 0u64;
        let mut object_snapshot_ms = 0u64;
        let mut object_text_us = 0u64;
        let mut object_identity_us = 0u64;
        let mut character_geometry_ms = 0u64;
        let mut mapping_ms = Vec::new();
        let mut mapping_page_lookup_us = 0u64;
        let mut mapping_collect_us = 0u64;
        let mut mapping_stream_decode_us = 0u64;
        let mut mapping_font_inspection_us = 0u64;
        let mut mapping_text_show_decode_us = 0u64;
        let mut mapping_object_prepare_us = 0u64;
        let mut mapping_pair_us = 0u64;
        let mut mapping_stream_decode_cache_hits = 0usize;
        let mut reconciliation_ms = Vec::new();
        let mut atom_count = 0usize;
        for page_number in 1..=10 {
            let page_started = Instant::now();
            let extraction_started = Instant::now();
            let snapshot = extract_pdfium_page_snapshot(&handle, page_number)
                .unwrap_or_else(|error| panic!("page {page_number} extraction: {error}"));
            extraction_ms.push(extraction_started.elapsed().as_millis());
            setup_ms += snapshot.timing.setup_ms;
            object_snapshot_ms += snapshot.timing.object_snapshot_ms;
            object_text_us += snapshot.timing.object_text_us;
            object_identity_us += snapshot.timing.object_identity_us;
            character_geometry_ms += snapshot.timing.character_geometry_ms;
            let mapping_started = Instant::now();
            let mapping = map_page_atoms_to_content_operands_from_snapshot(&handle, &snapshot)
                .unwrap_or_else(|error| panic!("page {page_number} mapping: {error}"));
            mapping_ms.push(mapping_started.elapsed().as_millis());
            mapping_page_lookup_us += mapping.timing.page_lookup_us;
            mapping_collect_us += mapping.timing.collect_text_shows_us;
            mapping_stream_decode_us += mapping.timing.stream_decode_us;
            mapping_font_inspection_us += mapping.timing.font_inspection_us;
            mapping_text_show_decode_us += mapping.timing.text_show_decode_us;
            mapping_object_prepare_us += mapping.timing.object_prepare_us;
            mapping_pair_us += mapping.timing.pair_mappings_us;
            mapping_stream_decode_cache_hits += mapping.timing.stream_decode_cache_hits;
            let reconciliation_started = Instant::now();
            let page = reconcile_page_graph(snapshot.page_graph, mapping);
            reconciliation_ms.push(reconciliation_started.elapsed().as_millis());
            page_ms.push(page_started.elapsed().as_millis());
            atom_count += page.atoms.len();
        }
        let total_ms = started.elapsed().as_millis();
        let mut sorted_ms = page_ms.clone();
        sorted_ms.sort_unstable();
        println!(
            "pdf-v3 ten-page source_bytes={} source_pages={} open={}ms total={}ms median={}ms min={}ms max={}ms extraction_total={}ms setup={}ms object_snapshot={}ms object_text={}us object_identity={}us character_geometry={}ms mapping_total={}ms mapping_page_lookup={}us mapping_collect={}us mapping_stream_decode={}us mapping_stream_decode_cache_hits={} mapping_font_inspection={}us mapping_text_show_decode={}us mapping_object_prepare={}us mapping_pair={}us reconciliation_total={}ms atoms={} page_ms={:?}",
            handle.source_bytes(),
            handle.page_count(),
            handle.open_elapsed().as_millis(),
            total_ms,
            sorted_ms[sorted_ms.len() / 2],
            sorted_ms[0],
            sorted_ms[sorted_ms.len() - 1],
            extraction_ms.iter().sum::<u128>(),
            setup_ms,
            object_snapshot_ms,
            object_text_us,
            object_identity_us,
            character_geometry_ms,
            mapping_ms.iter().sum::<u128>(),
            mapping_page_lookup_us,
            mapping_collect_us,
            mapping_stream_decode_us,
            mapping_stream_decode_cache_hits,
            mapping_font_inspection_us,
            mapping_text_show_decode_us,
            mapping_object_prepare_us,
            mapping_pair_us,
            reconciliation_ms.iter().sum::<u128>(),
            atom_count,
            page_ms,
        );
    }

    #[test]
    fn simple_page_reconciles_every_atom_to_an_encoded_operand() {
        let _guard = pdfium_test_lock();
        let page = build_reconciled_page_graph(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
        )
        .expect("reconciled page");

        assert_eq!(
            page.reconciliation.status,
            PageReconciliationStatus::Complete
        );
        assert_eq!(page.reconciliation.preserved_atom_count, 0);
        assert!(page.reconciliation.verified_atom_count > 0);
        assert!(page.atoms.iter().all(|atom| match atom.source_kind {
            PageAtomSourceKind::PdfiumVerified => atom.source_provenance.is_some(),
            PageAtomSourceKind::PdfiumSyntheticWhitespace => {
                atom.source_provenance.is_none() && !atom.requires_translation
            }
            _ => false,
        }));

        let content = probe_content_stream_save_only(
            shared_pdfium(),
            &fixture_path("002-trivial-libre-office-writer.pdf"),
            1,
            400,
        )
        .expect("content provenance");
        let operand_lengths = content
            .streams
            .iter()
            .flat_map(|stream| &stream.text_operands)
            .map(|operand| (&operand.operand_id, operand.encoded_byte_count))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert!(page
            .atoms
            .iter()
            .filter_map(|atom| atom.source_provenance.as_ref())
            .all(|provenance| {
                operand_lengths
                    .get(&provenance.operand_id)
                    .is_some_and(|operand_len| {
                        provenance
                            .encoded_start
                            .checked_add(provenance.encoded_len)
                            .is_some_and(|end| end <= *operand_len)
                    })
            }));
    }

    #[test]
    fn real_page_corrects_to_unicode_and_keeps_synthetic_whitespace_explicit() {
        let _guard = pdfium_test_lock();
        let page =
            build_reconciled_page_graph(shared_pdfium(), &fixture_path("2305.13048v2.pdf"), 1)
                .expect("reconciled real page");

        assert!(page.reconciliation.corrected_atom_count >= 15);
        assert!(page.reconciliation.synthetic_whitespace_atom_count > 0);
        assert_eq!(page.reconciliation.mapped_object_count, 242);
        assert_eq!(page.reconciliation.preserved_object_count, 16);
        assert_eq!(page.reconciliation.preserved_atom_count, 57);
        assert!(!page
            .reconciliation
            .fallback_reasons
            .iter()
            .any(|reason| reason == "form-xobject-requires-recursive-mapping"));
        assert!(page
            .atoms
            .iter()
            .filter(|atom| { atom.source_kind == PageAtomSourceKind::ToUnicodeCorrected })
            .all(|atom| atom.source_provenance.is_some()));
        assert!(page
            .atoms
            .iter()
            .filter(|atom| { atom.source_kind == PageAtomSourceKind::PdfiumSyntheticWhitespace })
            .all(|atom| atom.source_provenance.is_none() && atom.generated));
    }

    #[test]
    fn repeated_reconciliation_keeps_atom_provenance_stable() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let first =
            build_reconciled_page_graph(shared_pdfium(), &source, 1).expect("first reconciliation");
        let second = build_reconciled_page_graph(shared_pdfium(), &source, 1)
            .expect("second reconciliation");

        assert_eq!(first.atoms, second.atoms);
        assert_eq!(first.reconciliation, second.reconciliation);
    }

    #[test]
    fn fixture_corpus_reconciliation_is_total_and_conservative() {
        let _guard = pdfium_test_lock();
        for fixture in [
            "simple-one-page.pdf",
            "pdflatex-image.pdf",
            "multicolumn.pdf",
            "google-doc-document.pdf",
            "GeoTopo.pdf",
        ] {
            let page = build_reconciled_page_graph(shared_pdfium(), &fixture_path(fixture), 1)
                .unwrap_or_else(|error| panic!("reconciliation failed for {fixture}: {error}"));
            let summary = &page.reconciliation;

            assert_eq!(
                summary.verified_atom_count
                    + summary.corrected_atom_count
                    + summary.synthetic_whitespace_atom_count
                    + summary.preserved_atom_count,
                page.atoms.len(),
                "atom accounting failed for {fixture}"
            );
            assert!(page
                .atoms
                .iter()
                .all(|atom| atom.source_kind != PageAtomSourceKind::PdfiumUnverified));
            assert!(page
                .atoms
                .iter()
                .filter_map(|atom| atom.source_provenance.as_ref())
                .all(|provenance| {
                    provenance.encoded_len > 0
                        && provenance.source_unit_char_count > 0
                        && provenance.source_unit_char_index < provenance.source_unit_char_count
                }));
            println!(
                "pdf-v3 reconcile fixture={fixture} status={:?} atoms={} verified={} corrected={} synthetic_whitespace={} unrepresented_source_whitespace={} preserved={} objects={}/{} fallbacks={:?}",
                summary.status,
                page.atoms.len(),
                summary.verified_atom_count,
                summary.corrected_atom_count,
                summary.synthetic_whitespace_atom_count,
                summary.unrepresented_source_whitespace_count,
                summary.preserved_atom_count,
                summary.mapped_object_count,
                summary.mapped_object_count + summary.preserved_object_count,
                summary.fallback_reasons
            );
        }
    }

    #[test]
    #[ignore = "manual Windows real-page PageGraph reconciliation probe"]
    fn manual_windows_real_page_reconciliation_probe() {
        let _guard = pdfium_test_lock();
        let started = std::time::Instant::now();
        let page =
            build_reconciled_page_graph(shared_pdfium(), &fixture_path("2305.13048v2.pdf"), 1)
                .expect("real-page reconciliation");
        let summary = &page.reconciliation;

        println!(
            "pdf-v3 reconcile page={} status={:?} atoms={} verified={} corrected={} synthetic_whitespace={} unrepresented_source_whitespace={} preserved={} objects={}/{} fallbacks={:?} elapsed={}ms",
            page.page_number,
            summary.status,
            page.atoms.len(),
            summary.verified_atom_count,
            summary.corrected_atom_count,
            summary.synthetic_whitespace_atom_count,
            summary.unrepresented_source_whitespace_count,
            summary.preserved_atom_count,
            summary.mapped_object_count,
            summary.mapped_object_count + summary.preserved_object_count,
            summary.fallback_reasons,
            started.elapsed().as_millis()
        );
    }
}
