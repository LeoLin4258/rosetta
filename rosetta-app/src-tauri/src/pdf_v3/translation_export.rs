use std::{fmt, path::Path};

use serde::Serialize;

use super::{
    font::{
        stage_document_translation_font_registry, TranslationFontAsset, TranslationFontError,
        TranslationFontWeight,
    },
    font_plan::{
        plan_document_translation_fonts_with_cancel, TranslationFontCharacterPlan,
        TranslationFontPlanError,
    },
    incremental_export::{
        export_incremental_pdf_atomic, export_source_pdf_atomic, IncrementalExportBase,
        IncrementalExportCancellation, IncrementalExportError,
    },
    object_delta::PdfObjectDeltaError,
    ownership::{PdfStreamOwnershipError, PdfStreamOwnershipIndex},
    page_graph_store::{PageGraphStore, PageGraphStoreError},
    page_index::{PdfPageIndex, PdfPageIndexError},
    page_set::PageSet,
    patch_renderer::{
        stage_resolved_translation_patch_with_font_registry, TranslationPatchRenderError,
        TranslationPatchRenderPolicy,
    },
    patch_store::{TranslationPatchStore, TranslationPatchStoreError},
    source_object::{PdfObjectOverlay, PdfSourceObjectStore},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PdfV3TranslationExportCommitKind {
    Incremental,
    SourceCopy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfV3TranslationExportResult {
    pub schema: &'static str,
    pub commit_kind: PdfV3TranslationExportCommitKind,
    pub selected_page_count: usize,
    pub fitted_entry_count: usize,
    pub preserved_entry_count: usize,
    pub regular_character_count: usize,
    pub bold_character_count: usize,
    pub prepared_font_count: usize,
    pub font_subset_bytes: usize,
    pub font_object_count: usize,
    pub delta_object_count: usize,
    pub source_bytes: u64,
    pub appended_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug)]
pub(crate) enum PdfV3TranslationExportError {
    EmptyPageSet,
    Cancelled,
    SourceIdentityMismatch,
    SourcePageCountMismatch { source: u32, store: u32 },
    MissingPageGraph(u32),
    MissingTranslationPatch(u32),
    FontPlan(TranslationFontPlanError),
    Font(TranslationFontError),
    PageGraphStore(PageGraphStoreError),
    PatchStore(TranslationPatchStoreError),
    PageIndex(PdfPageIndexError),
    Ownership(PdfStreamOwnershipError),
    Render(TranslationPatchRenderError),
    ObjectDelta(PdfObjectDeltaError),
    Commit(IncrementalExportError),
}

impl fmt::Display for PdfV3TranslationExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPageSet => formatter.write_str("PDF v3 translation export PageSet is empty"),
            Self::Cancelled => formatter.write_str("PDF v3 translation export was cancelled"),
            Self::SourceIdentityMismatch => formatter.write_str(
                "PDF v3 translation export stores do not share the requested source identity",
            ),
            Self::SourcePageCountMismatch { source, store } => write!(
                formatter,
                "PDF v3 translation export source has {source} pages; PageGraph store has {store}"
            ),
            Self::MissingPageGraph(page) => {
                write!(
                    formatter,
                    "PDF v3 translation export is missing PageGraph page {page}"
                )
            }
            Self::MissingTranslationPatch(page) => write!(
                formatter,
                "PDF v3 translation export is missing TranslationPatch page {page}"
            ),
            Self::FontPlan(error) => error.fmt(formatter),
            Self::Font(error) => error.fmt(formatter),
            Self::PageGraphStore(error) => error.fmt(formatter),
            Self::PatchStore(error) => error.fmt(formatter),
            Self::PageIndex(error) => error.fmt(formatter),
            Self::Ownership(error) => error.fmt(formatter),
            Self::Render(error) => error.fmt(formatter),
            Self::ObjectDelta(error) => error.fmt(formatter),
            Self::Commit(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PdfV3TranslationExportError {}

macro_rules! export_error_from {
    ($source:ty, $variant:ident) => {
        impl From<$source> for PdfV3TranslationExportError {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }
    };
}

export_error_from!(TranslationFontError, Font);
export_error_from!(PageGraphStoreError, PageGraphStore);
export_error_from!(TranslationPatchStoreError, PatchStore);
export_error_from!(PdfPageIndexError, PageIndex);
export_error_from!(PdfStreamOwnershipError, Ownership);
export_error_from!(TranslationPatchRenderError, Render);
export_error_from!(PdfObjectDeltaError, ObjectDelta);

impl From<TranslationFontPlanError> for PdfV3TranslationExportError {
    fn from(value: TranslationFontPlanError) -> Self {
        match value {
            TranslationFontPlanError::Cancelled => Self::Cancelled,
            error => Self::FontPlan(error),
        }
    }
}

impl From<IncrementalExportError> for PdfV3TranslationExportError {
    fn from(value: IncrementalExportError) -> Self {
        match value {
            IncrementalExportError::Cancelled => Self::Cancelled,
            error => Self::Commit(error),
        }
    }
}

pub(crate) struct PdfV3TranslationExportRequest<'a> {
    pub source_fingerprint: &'a str,
    pub destination_path: &'a Path,
    pub pages: &'a PageSet,
    pub page_graph_store: &'a PageGraphStore,
    pub patch_store: &'a TranslationPatchStore,
    pub regular_font: &'a TranslationFontAsset,
    pub bold_font: Option<&'a TranslationFontAsset>,
    pub render_policy: TranslationPatchRenderPolicy,
    pub cancellation: &'a IncrementalExportCancellation,
}

pub(crate) fn export_translation_pdf_atomic(
    source: &PdfSourceObjectStore,
    request: PdfV3TranslationExportRequest<'_>,
) -> Result<PdfV3TranslationExportResult, PdfV3TranslationExportError> {
    validate_request(source, &request)?;
    let base = IncrementalExportBase::from_source_object_store(request.source_fingerprint, source)?;
    ensure_active(request.cancellation)?;

    let font_plan = plan_document_translation_fonts_with_cancel(
        request.page_graph_store,
        request.patch_store,
        request.pages,
        || request.cancellation.is_cancelled(),
    )?;
    let prepared_fonts =
        font_plan.prepare_document_fonts(request.regular_font, request.bold_font)?;
    let regular_character_count = font_plan.character_count(TranslationFontWeight::Regular);
    let bold_character_count = font_plan.character_count(TranslationFontWeight::Bold);
    let font_subset_bytes = prepared_fonts
        .iter()
        .map(|font| font.subset_bytes.len())
        .sum();
    let fonts = prepared_fonts.iter().collect::<Vec<_>>();

    ensure_active(request.cancellation)?;
    let page_index = PdfPageIndex::resolve(source, request.pages)?;
    let ownership_index =
        PdfStreamOwnershipIndex::resolve(source, &page_index.selected_content_stream_ids())?;
    let staged_fonts = stage_document_translation_font_registry(source, &fonts)?;
    let font_object_count = staged_fonts.object_delta.object_count();
    let mut export_delta = staged_fonts.object_delta;
    let mut fitted_entry_count = 0usize;
    let mut preserved_entry_count = 0usize;

    for &page_number in request.pages.pages() {
        ensure_active(request.cancellation)?;
        let stored_page = request
            .page_graph_store
            .load(page_number)?
            .ok_or(PdfV3TranslationExportError::MissingPageGraph(page_number))?;
        let stored_patch = request.patch_store.load(&stored_page.page)?.ok_or(
            PdfV3TranslationExportError::MissingTranslationPatch(page_number),
        )?;
        let replay_font_plan = TranslationFontCharacterPlan::for_resolved_replay(
            &stored_page.page,
            &stored_patch.patch,
        )?;
        let replay_fonts =
            replay_font_plan.prepare_available_fonts(request.regular_font, request.bold_font)?;
        let replay_font_refs = replay_fonts.iter().collect::<Vec<_>>();
        let overlay = PdfObjectOverlay::new(source, &export_delta);
        let staged = stage_resolved_translation_patch_with_font_registry(
            source,
            &overlay,
            &page_index,
            &ownership_index,
            &stored_page.page,
            &stored_patch.patch,
            &replay_font_refs,
            &fonts,
            request.render_policy,
            &staged_fonts.registry,
        )?;
        fitted_entry_count = fitted_entry_count.saturating_add(staged.render.fitted_entry_count);
        preserved_entry_count =
            preserved_entry_count.saturating_add(staged.render.preserved_entry_count);
        export_delta.merge(staged.object_delta)?;
    }
    ensure_active(request.cancellation)?;

    let delta_object_count = export_delta.object_count();
    let (commit_kind, source_bytes, appended_bytes, output_bytes) = if delta_object_count == 0 {
        let result = export_source_pdf_atomic(
            source.source_path(),
            request.destination_path,
            &base,
            request.cancellation,
        )?;
        (
            PdfV3TranslationExportCommitKind::SourceCopy,
            result.source_bytes,
            0,
            result.output_bytes,
        )
    } else {
        let result = export_incremental_pdf_atomic(
            source.source_path(),
            request.destination_path,
            &base,
            &export_delta,
            request.cancellation,
        )?;
        (
            PdfV3TranslationExportCommitKind::Incremental,
            result.source_bytes,
            result.appended_bytes,
            result.output_bytes,
        )
    };

    Ok(PdfV3TranslationExportResult {
        schema: "rosetta-pdf-v3-translation-export/1",
        commit_kind,
        selected_page_count: request.pages.pages().len(),
        fitted_entry_count,
        preserved_entry_count,
        regular_character_count,
        bold_character_count,
        prepared_font_count: prepared_fonts.len(),
        font_subset_bytes,
        font_object_count,
        delta_object_count,
        source_bytes,
        appended_bytes,
        output_bytes,
    })
}

fn validate_request(
    source: &PdfSourceObjectStore,
    request: &PdfV3TranslationExportRequest<'_>,
) -> Result<(), PdfV3TranslationExportError> {
    if request.pages.is_empty() {
        return Err(PdfV3TranslationExportError::EmptyPageSet);
    }
    if request.page_graph_store.source_fingerprint() != request.source_fingerprint
        || request.patch_store.source_fingerprint() != request.source_fingerprint
    {
        return Err(PdfV3TranslationExportError::SourceIdentityMismatch);
    }
    if source.page_count() != request.page_graph_store.source_page_count() {
        return Err(PdfV3TranslationExportError::SourcePageCountMismatch {
            source: source.page_count(),
            store: request.page_graph_store.source_page_count(),
        });
    }
    ensure_active(request.cancellation)
}

fn ensure_active(
    cancellation: &IncrementalExportCancellation,
) -> Result<(), PdfV3TranslationExportError> {
    if cancellation.is_cancelled() {
        Err(PdfV3TranslationExportError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod error_tests {
    use super::PdfV3TranslationExportError;
    use crate::pdf_v3::{
        font_plan::TranslationFontPlanError, incremental_export::IncrementalExportError,
    };

    #[test]
    fn cancellation_is_stable_across_export_phases() {
        assert!(matches!(
            PdfV3TranslationExportError::from(TranslationFontPlanError::Cancelled),
            PdfV3TranslationExportError::Cancelled
        ));
        assert!(matches!(
            PdfV3TranslationExportError::from(IncrementalExportError::Cancelled),
            PdfV3TranslationExportError::Cancelled
        ));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{
        collections::BTreeSet,
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use lopdf::{Document, Object};
    use sha2::{Digest, Sha256};

    use super::{
        export_translation_pdf_atomic, PdfV3TranslationExportCommitKind,
        PdfV3TranslationExportRequest,
    };
    use crate::{
        pdf_v3::{
            font::{TranslationFontAsset, TranslationFontWeight, UnifiedTranslationFontPlan},
            identity::{compare_images, render_page},
            incremental_export::IncrementalExportCancellation,
            page_graph_store::PageGraphStore,
            page_set::PageSet,
            patch_renderer::{
                render_translation_patch, TranslationPatchRenderPolicy,
                TRANSLATION_PATCH_RENDERER_VERSION,
            },
            patch_store::TranslationPatchStore,
            reconcile::build_reconciled_page_graph,
            source_object::PdfSourceObjectStore,
            translation_patch::{
                build_translation_patch, TranslationPatchDraft, TranslationPatchEntryDraft,
            },
            types::{
                PageAtomSourceKind, PageGraph, TranslationPatch, TranslationPatchRendererDecision,
            },
        },
        rosetta_jobs::formats::pdf::test_helpers::{fixture_path, pdfium_test_lock, shared_pdfium},
    };

    #[test]
    fn streams_resolved_pages_into_one_shared_font_incremental_export() {
        let _guard = pdfium_test_lock();
        let translations = ["Durable export page one", "Durable export page two"];
        let source_path = fixture_path("2305.13048v2.pdf");
        let source_bytes = fs::read(&source_path).expect("source PDF");
        let source_fingerprint = fingerprint(&source_bytes);
        let source = PdfSourceObjectStore::open(&source_path).expect("lazy source");
        let source_document = Document::load_mem(&source_bytes).expect("source document");
        let regular_asset = arial_asset();
        let mut font_plan = UnifiedTranslationFontPlan::default();
        for translation in translations {
            font_plan.add_text(translation);
        }
        let prepared = regular_asset
            .prepare(&font_plan)
            .expect("prepared test font");
        let pages = [
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("page one graph"),
            build_reconciled_page_graph(shared_pdfium(), &source_path, 2).expect("page two graph"),
        ];
        let patches = [
            resolved_renderable_patch(&source_bytes, &pages[0], &prepared, translations[0]),
            resolved_renderable_patch(&source_bytes, &pages[1], &prepared, translations[1]),
        ];
        let temp = TestDirectory::new("incremental");
        let page_store = PageGraphStore::new(
            &temp.path().join("pages"),
            &source_fingerprint,
            source.page_count(),
            "pdf-v3-translation-export-test",
        )
        .expect("PageGraph store");
        let patch_store = TranslationPatchStore::new(
            &temp.path().join("translations"),
            &source_fingerprint,
            "en",
        )
        .expect("TranslationPatch store");
        for (page, patch) in pages.iter().zip(&patches) {
            page_store.commit(page).expect("committed PageGraph");
            patch_store
                .commit(page, patch)
                .expect("committed TranslationPatch");
        }
        let selected_pages = PageSet::from_pages([1, 2]).expect("selected pages");
        let destination = temp.path().join("translated.pdf");

        let result = export_translation_pdf_atomic(
            &source,
            PdfV3TranslationExportRequest {
                source_fingerprint: &source_fingerprint,
                destination_path: &destination,
                pages: &selected_pages,
                page_graph_store: &page_store,
                patch_store: &patch_store,
                regular_font: &regular_asset,
                bold_font: None,
                render_policy: TranslationPatchRenderPolicy::default(),
                cancellation: &IncrementalExportCancellation::default(),
            },
        )
        .expect("translation export");

        let output_bytes = fs::read(&destination).expect("translated output");
        if let Some(probe_path) = env::var_os("ROSETTA_PDF_V3_TRANSLATION_EXPORT_PROBE_OUTPUT") {
            let probe_path = PathBuf::from(probe_path);
            if let Some(parent) = probe_path.parent() {
                fs::create_dir_all(parent).expect("probe output parent");
            }
            fs::write(probe_path, &output_bytes).expect("translation export probe");
        }
        assert_eq!(
            result.commit_kind,
            PdfV3TranslationExportCommitKind::Incremental
        );
        assert_eq!(result.selected_page_count, 2);
        assert_eq!(result.fitted_entry_count, 2);
        assert_eq!(result.preserved_entry_count, 0);
        assert_eq!(result.prepared_font_count, 1);
        assert_eq!(result.font_object_count, 6);
        assert_eq!(result.delta_object_count, 10);
        assert!(result.appended_bytes < source_bytes.len() as u64 / 4);
        assert!(output_bytes.starts_with(&source_bytes));

        let output_document = Document::load_mem(&output_bytes).expect("incremental document");
        assert_eq!(
            output_document.get_pages().len(),
            source.page_count() as usize
        );
        let type0_fonts = output_document
            .objects
            .values()
            .filter(|object| {
                object.as_dict().is_ok_and(|dictionary| {
                    dictionary
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|name| name == b"Type0")
                        && dictionary
                            .get(b"BaseFont")
                            .and_then(Object::as_name)
                            .is_ok_and(|name| name == prepared.subset_name.as_bytes())
                })
            })
            .count();
        assert_eq!(type0_fonts, 1);

        let source_pdfium = shared_pdfium()
            .load_pdf_from_byte_slice(&source_bytes, None)
            .expect("source PDFium document");
        let output_pdfium = shared_pdfium()
            .load_pdf_from_byte_slice(&output_bytes, None)
            .expect("output PDFium document");
        for (index, translation) in translations.iter().enumerate() {
            let output_page = output_pdfium
                .pages()
                .get(index as i32)
                .expect("translated page");
            assert!(output_page
                .text()
                .expect("translated text")
                .all()
                .contains(translation));
            let source_page = source_pdfium
                .pages()
                .get(index as i32)
                .expect("source page");
            let difference = compare_images(
                &render_page(&source_page, (index + 1) as u32, 1440).expect("source render"),
                &render_page(&output_page, (index + 1) as u32, 1440).expect("output render"),
            )
            .expect("page difference");
            assert!(difference.changed_pixel_count > 0);
            assert!(difference.changed_pixel_ratio < 0.05);
        }
        let source_page_three = source_pdfium.pages().get(2).expect("source page three");
        let output_page_three = output_pdfium.pages().get(2).expect("output page three");
        let unchanged = compare_images(
            &render_page(&source_page_three, 3, 1440).expect("source page three render"),
            &render_page(&output_page_three, 3, 1440).expect("output page three render"),
        )
        .expect("unchanged page difference");
        assert_eq!(unchanged.changed_pixel_count, 0);
        assert_eq!(source_document.get_pages().len(), 30);
    }

    #[test]
    fn all_preserved_export_is_a_verified_byte_exact_source_copy() {
        let _guard = pdfium_test_lock();
        let source_path = fixture_path("002-trivial-libre-office-writer.pdf");
        let source_bytes = fs::read(&source_path).expect("source PDF");
        let source_fingerprint = fingerprint(&source_bytes);
        let source = PdfSourceObjectStore::open(&source_path).expect("lazy source");
        let page =
            build_reconciled_page_graph(shared_pdfium(), &source_path, 1).expect("reconciled page");
        let regular_asset = arial_asset();
        let overflow = "W".repeat(4_096);
        let mut overflow_plan = UnifiedTranslationFontPlan::default();
        overflow_plan.add_text(&overflow);
        let prepared = regular_asset
            .prepare(&overflow_plan)
            .expect("prepared overflow font");
        let resolved = resolved_overflow_patch(&source_bytes, &page, &prepared, &overflow);
        let temp = TestDirectory::new("source-copy");
        let page_store = PageGraphStore::new(
            &temp.path().join("pages"),
            &source_fingerprint,
            source.page_count(),
            "pdf-v3-translation-export-test",
        )
        .expect("PageGraph store");
        let patch_store = TranslationPatchStore::new(
            &temp.path().join("translations"),
            &source_fingerprint,
            "en",
        )
        .expect("TranslationPatch store");
        page_store.commit(&page).expect("committed PageGraph");
        patch_store
            .commit(&page, &resolved)
            .expect("committed preserved patch");
        let selected_pages = PageSet::from_pages([1]).expect("selected page");
        let destination = temp.path().join("preserved.pdf");
        let result = export_translation_pdf_atomic(
            &source,
            PdfV3TranslationExportRequest {
                source_fingerprint: &source_fingerprint,
                destination_path: &destination,
                pages: &selected_pages,
                page_graph_store: &page_store,
                patch_store: &patch_store,
                regular_font: &regular_asset,
                bold_font: None,
                render_policy: TranslationPatchRenderPolicy::default(),
                cancellation: &IncrementalExportCancellation::default(),
            },
        )
        .expect("preserved export");

        assert_eq!(
            result.commit_kind,
            PdfV3TranslationExportCommitKind::SourceCopy
        );
        assert_eq!(result.fitted_entry_count, 0);
        assert_eq!(result.preserved_entry_count, 1);
        assert_eq!(result.prepared_font_count, 0);
        assert_eq!(result.font_object_count, 0);
        assert_eq!(result.delta_object_count, 0);
        assert_eq!(result.appended_bytes, 0);
        assert_eq!(
            fs::read(&destination).expect("preserved output"),
            source_bytes
        );
    }

    fn resolved_renderable_patch(
        source_bytes: &[u8],
        page: &PageGraph,
        prepared: &crate::pdf_v3::font::PreparedTranslationFont,
        translation: &str,
    ) -> TranslationPatch {
        for patch in buildable_patches(page, translation) {
            let mut document = Document::load_mem(source_bytes).expect("candidate document");
            let Ok(render) = render_translation_patch(
                &mut document,
                page,
                &patch,
                &[prepared],
                TranslationPatchRenderPolicy::default(),
            ) else {
                continue;
            };
            if render.fitted_entry_count == 1 {
                return render.resolved_patch;
            }
        }
        panic!("fixture must contain one renderable source object");
    }

    fn resolved_overflow_patch(
        source_bytes: &[u8],
        page: &PageGraph,
        prepared: &crate::pdf_v3::font::PreparedTranslationFont,
        translation: &str,
    ) -> TranslationPatch {
        for patch in buildable_patches(page, translation) {
            let mut document = Document::load_mem(source_bytes).expect("candidate document");
            let Ok(render) = render_translation_patch(
                &mut document,
                page,
                &patch,
                &[prepared],
                TranslationPatchRenderPolicy::default(),
            ) else {
                continue;
            };
            if render.resolved_patch.entries.iter().any(|entry| {
                matches!(
                    &entry.renderer_decision,
                    TranslationPatchRendererDecision::Preserved { reason_code }
                        if reason_code == "translation-overflow"
                )
            }) {
                return render.resolved_patch;
            }
        }
        panic!("fixture must contain one overflow-preserved source object");
    }

    fn buildable_patches(page: &PageGraph, translation: &str) -> Vec<TranslationPatch> {
        let mut visited = BTreeSet::new();
        let mut patches = Vec::new();
        for atom in &page.atoms {
            let Some(source_object_id) = atom.source_object_id.as_deref() else {
                continue;
            };
            if !visited.insert(source_object_id) || atom.source_provenance.is_none() {
                continue;
            }
            let atom_ids = page
                .atoms
                .iter()
                .filter(|candidate| candidate.source_object_id.as_deref() == Some(source_object_id))
                .filter(|candidate| {
                    !matches!(
                        candidate.source_kind,
                        PageAtomSourceKind::PdfiumSyntheticWhitespace
                            | PageAtomSourceKind::PreservedUnmapped
                    )
                })
                .map(|candidate| candidate.atom_id.clone())
                .collect::<Vec<_>>();
            if let Ok(patch) = build_translation_patch(
                page,
                TranslationPatchDraft {
                    target_language: "en".to_string(),
                    translation_revision: 1,
                    provider_id: "rwkv-local-test".to_string(),
                    model_id: "rwkv-model-test".to_string(),
                    renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                    entries: vec![TranslationPatchEntryDraft {
                        atom_ids,
                        translated_text: translation.to_string(),
                        protected_spans: Vec::new(),
                    }],
                },
            ) {
                patches.push(patch);
            }
        }
        patches
    }

    fn arial_asset() -> TranslationFontAsset {
        TranslationFontAsset::open_weighted(
            "ArialRegular",
            TranslationFontWeight::Regular,
            Path::new(r"C:\Windows\Fonts\arial.ttf"),
            0,
        )
        .expect("Windows Arial font")
    }

    fn fingerprint(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!(
            "sha256:{}",
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-translation-export-{label}-{}-{nanos}",
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
