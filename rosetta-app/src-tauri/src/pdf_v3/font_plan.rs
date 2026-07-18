use std::{collections::BTreeMap, fmt};

use super::{
    font::{
        PreparedTranslationFont, TranslationFontAsset, TranslationFontError, TranslationFontWeight,
        UnifiedTranslationFontPlan,
    },
    page_graph_store::{PageGraphStore, PageGraphStoreError},
    page_set::PageSet,
    patch_store::{TranslationPatchStore, TranslationPatchStoreError},
    style::{plan_text_show_style, TextShowStyleError},
    translation_patch::{
        ensure_translation_patch_renderer_resolved, validate_translation_patch,
        TranslationPatchError,
    },
    types::{PageGraph, TranslationPatch, TranslationPatchRendererDecision},
};

pub(crate) const MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT: usize = u16::MAX as usize;

#[derive(Debug, Clone, Default)]
pub(crate) struct TranslationFontCharacterPlan {
    by_weight: BTreeMap<TranslationFontWeight, UnifiedTranslationFontPlan>,
}

#[derive(Debug)]
pub(crate) enum TranslationFontPlanError {
    SourceIdentityMismatch,
    PageOutOfBounds {
        page_number: u32,
        page_count: u32,
    },
    MissingPageGraph(u32),
    MissingTranslationPatch(u32),
    UnexpectedRendererDecision(String),
    CharacterLimit {
        weight: TranslationFontWeight,
        count: usize,
        maximum: usize,
    },
    Patch(TranslationPatchError),
    Style(TextShowStyleError),
    PageGraphStore(PageGraphStoreError),
    PatchStore(TranslationPatchStoreError),
    Font(TranslationFontError),
}

impl fmt::Display for TranslationFontPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceIdentityMismatch => formatter
                .write_str("PDF v3 font planning stores do not share one source fingerprint"),
            Self::PageOutOfBounds {
                page_number,
                page_count,
            } => write!(
                formatter,
                "PDF v3 font planning page {page_number} is outside 1..={page_count}"
            ),
            Self::MissingPageGraph(page_number) => {
                write!(
                    formatter,
                    "PDF v3 font planning is missing PageGraph page {page_number}"
                )
            }
            Self::MissingTranslationPatch(page_number) => write!(
                formatter,
                "PDF v3 font planning is missing TranslationPatch page {page_number}"
            ),
            Self::UnexpectedRendererDecision(entry_id) => write!(
                formatter,
                "PDF v3 font planning entry {entry_id} has an unexpected renderer decision"
            ),
            Self::CharacterLimit {
                weight,
                count,
                maximum,
            } => write!(
                formatter,
                "PDF v3 {weight:?} font plan has {count} characters, above maximum {maximum}"
            ),
            Self::Patch(error) => error.fmt(formatter),
            Self::Style(error) => error.fmt(formatter),
            Self::PageGraphStore(error) => error.fmt(formatter),
            Self::PatchStore(error) => error.fmt(formatter),
            Self::Font(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranslationFontPlanError {}

impl From<TranslationPatchError> for TranslationFontPlanError {
    fn from(value: TranslationPatchError) -> Self {
        Self::Patch(value)
    }
}

impl From<TextShowStyleError> for TranslationFontPlanError {
    fn from(value: TextShowStyleError) -> Self {
        Self::Style(value)
    }
}

impl From<PageGraphStoreError> for TranslationFontPlanError {
    fn from(value: PageGraphStoreError) -> Self {
        Self::PageGraphStore(value)
    }
}

impl From<TranslationPatchStoreError> for TranslationFontPlanError {
    fn from(value: TranslationPatchStoreError) -> Self {
        Self::PatchStore(value)
    }
}

impl From<TranslationFontError> for TranslationFontPlanError {
    fn from(value: TranslationFontError) -> Self {
        Self::Font(value)
    }
}

impl TranslationFontCharacterPlan {
    pub(crate) fn for_pending_patch(
        page: &PageGraph,
        patch: &TranslationPatch,
    ) -> Result<Self, TranslationFontPlanError> {
        validate_translation_patch(page, patch)?;
        let mut plan = Self::default();
        for entry in &patch.entries {
            if !matches!(
                entry.renderer_decision,
                TranslationPatchRendererDecision::Pending
            ) {
                return Err(TranslationFontPlanError::UnexpectedRendererDecision(
                    entry.entry_id.clone(),
                ));
            }
            let Ok(style) = plan_text_show_style(page, &entry.style_id) else {
                continue;
            };
            plan.try_add_text(style.translation_font_weight, &entry.translated_text)?;
        }
        Ok(plan)
    }

    pub(crate) fn absorb_resolved_patch(
        &mut self,
        page: &PageGraph,
        patch: &TranslationPatch,
    ) -> Result<(), TranslationFontPlanError> {
        validate_translation_patch(page, patch)?;
        ensure_translation_patch_renderer_resolved(patch)?;
        let mut next = self.clone();
        for entry in &patch.entries {
            if !matches!(
                entry.renderer_decision,
                TranslationPatchRendererDecision::Fitted { .. }
            ) {
                continue;
            }
            let style = plan_text_show_style(page, &entry.style_id)?;
            next.try_add_text(style.translation_font_weight, &entry.translated_text)?;
        }
        *self = next;
        Ok(())
    }

    pub(crate) fn plan_for(
        &self,
        weight: TranslationFontWeight,
    ) -> Option<&UnifiedTranslationFontPlan> {
        self.by_weight.get(&weight).filter(|plan| !plan.is_empty())
    }

    pub(crate) fn character_count(&self, weight: TranslationFontWeight) -> usize {
        self.plan_for(weight)
            .map(UnifiedTranslationFontPlan::character_count)
            .unwrap_or(0)
    }

    pub(crate) fn prepare_available_fonts(
        &self,
        regular: &TranslationFontAsset,
        bold: Option<&TranslationFontAsset>,
    ) -> Result<Vec<PreparedTranslationFont>, TranslationFontPlanError> {
        let mut prepared = Vec::with_capacity(2);
        if let Some(plan) = self.plan_for(TranslationFontWeight::Regular) {
            prepared.push(regular.prepare(plan)?);
        }
        if let (Some(plan), Some(asset)) = (self.plan_for(TranslationFontWeight::Bold), bold) {
            prepared.push(asset.prepare(plan)?);
        }
        Ok(prepared)
    }

    fn try_add_text(
        &mut self,
        weight: TranslationFontWeight,
        text: &str,
    ) -> Result<(), TranslationFontPlanError> {
        self.by_weight
            .entry(weight)
            .or_default()
            .try_add_text(text, MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT)
            .map_err(|count| TranslationFontPlanError::CharacterLimit {
                weight,
                count,
                maximum: MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT,
            })
    }
}

pub(crate) fn plan_document_translation_fonts(
    page_graph_store: &PageGraphStore,
    patch_store: &TranslationPatchStore,
    pages: &PageSet,
) -> Result<TranslationFontCharacterPlan, TranslationFontPlanError> {
    if page_graph_store.source_fingerprint() != patch_store.source_fingerprint() {
        return Err(TranslationFontPlanError::SourceIdentityMismatch);
    }
    let page_count = page_graph_store.source_page_count();
    let mut plan = TranslationFontCharacterPlan::default();
    for &page_number in pages.pages() {
        if page_number == 0 || page_number > page_count {
            return Err(TranslationFontPlanError::PageOutOfBounds {
                page_number,
                page_count,
            });
        }
        let stored_page = page_graph_store
            .load(page_number)?
            .ok_or(TranslationFontPlanError::MissingPageGraph(page_number))?;
        let stored_patch = patch_store.load(&stored_page.page)?.ok_or(
            TranslationFontPlanError::MissingTranslationPatch(page_number),
        )?;
        plan.absorb_resolved_patch(&stored_page.page, &stored_patch.patch)?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        plan_document_translation_fonts, TranslationFontCharacterPlan, TranslationFontPlanError,
        MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT,
    };
    use crate::pdf_v3::{
        extract::page_source_hash,
        font::TranslationFontWeight,
        page_graph_store::PageGraphStore,
        page_set::PageSet,
        patch_renderer::TRANSLATION_PATCH_RENDERER_VERSION,
        patch_store::TranslationPatchStore,
        translation_patch::{
            build_translation_patch, resolve_translation_patch_renderer_decisions,
            TranslationPatchDraft, TranslationPatchEntryDraft,
        },
        types::{
            PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageReconciliationStatus,
            PageReconciliationSummary, PageStyle, TranslationPatchFitStrategy,
            TranslationPatchRendererDecision, PAGE_GRAPH_SCHEMA_VERSION,
        },
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-font-plan-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("font plan temp root");
            Self { path }
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn pending_patch_plans_regular_and_bold_characters_separately() {
        let page = page();
        let patch = pending_patch(&page);

        let plan = TranslationFontCharacterPlan::for_pending_patch(&page, &patch)
            .expect("pending font plan");

        assert_eq!(
            plan.character_count(TranslationFontWeight::Regular),
            unique_character_count("Regular 甲")
        );
        assert_eq!(
            plan.character_count(TranslationFontWeight::Bold),
            unique_character_count("Bold 乙")
        );
    }

    #[test]
    fn resolved_plan_excludes_preserved_entries() {
        let page = page();
        let pending = pending_patch(&page);
        let decisions = BTreeMap::from([
            (
                pending.entries[0].entry_id.clone(),
                TranslationPatchRendererDecision::Fitted {
                    strategy: TranslationPatchFitStrategy::SingleShowScale,
                    fit_scale: 1.0,
                },
            ),
            (
                pending.entries[1].entry_id.clone(),
                TranslationPatchRendererDecision::Preserved {
                    reason_code: "test-preservation".to_string(),
                },
            ),
        ]);
        let resolved = resolve_translation_patch_renderer_decisions(&page, &pending, &decisions)
            .expect("resolved patch");
        let mut plan = TranslationFontCharacterPlan::default();

        plan.absorb_resolved_patch(&page, &resolved)
            .expect("resolved font plan");

        assert_eq!(
            plan.character_count(TranslationFontWeight::Regular),
            unique_character_count("Regular 甲")
        );
        assert_eq!(plan.character_count(TranslationFontWeight::Bold), 0);
    }

    #[test]
    fn character_limit_rejection_does_not_partially_mutate_plan() {
        let text = (0..=char::MAX as u32)
            .filter_map(char::from_u32)
            .filter(|character| !character.is_control())
            .take(MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT + 1)
            .collect::<String>();
        let mut plan = TranslationFontCharacterPlan::default();

        let error = plan
            .try_add_text(TranslationFontWeight::Regular, &text)
            .expect_err("CID character limit must be enforced");

        assert!(matches!(
            error,
            TranslationFontPlanError::CharacterLimit {
                weight: TranslationFontWeight::Regular,
                maximum: MAX_TRANSLATION_FONT_CHARACTERS_PER_WEIGHT,
                ..
            }
        ));
        assert_eq!(plan.character_count(TranslationFontWeight::Regular), 0);
    }

    #[test]
    fn document_plan_streams_durable_page_and_patch_authorities() {
        const SOURCE_FINGERPRINT: &str = "sha256:font-plan-source";
        let temp = TempRoot::new();
        let page_store = PageGraphStore::new(
            &temp.path.join("pages"),
            SOURCE_FINGERPRINT,
            1,
            "pdf-v3-font-plan-test",
        )
        .expect("page store");
        let patch_store = TranslationPatchStore::new(
            &temp.path.join("translations"),
            SOURCE_FINGERPRINT,
            "zh-CN",
        )
        .expect("patch store");
        let mut page = page();
        page.source_page_hash = page_source_hash(SOURCE_FINGERPRINT, 1);
        page.reconciliation.status = PageReconciliationStatus::Complete;
        page_store.commit(&page).expect("committed PageGraph");
        let pending = pending_patch(&page);
        let decisions = pending
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.entry_id.clone(),
                    TranslationPatchRendererDecision::Fitted {
                        strategy: TranslationPatchFitStrategy::SingleShowScale,
                        fit_scale: 1.0,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let resolved = resolve_translation_patch_renderer_decisions(&page, &pending, &decisions)
            .expect("resolved patch");
        patch_store
            .commit(&page, &resolved)
            .expect("committed TranslationPatch");
        let pages = PageSet::from_pages([1]).expect("page set");

        let plan = plan_document_translation_fonts(&page_store, &patch_store, &pages)
            .expect("document font plan");

        assert_eq!(
            plan.character_count(TranslationFontWeight::Regular),
            unique_character_count("Regular 甲")
        );
        assert_eq!(
            plan.character_count(TranslationFontWeight::Bold),
            unique_character_count("Bold 乙")
        );
    }

    fn unique_character_count(text: &str) -> usize {
        text.chars()
            .filter(|character| !character.is_control())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn pending_patch(page: &PageGraph) -> crate::pdf_v3::types::TranslationPatch {
        build_translation_patch(
            page,
            TranslationPatchDraft {
                target_language: "zh-CN".to_string(),
                translation_revision: 1,
                provider_id: "provider-test".to_string(),
                model_id: "model-test".to_string(),
                renderer_version: TRANSLATION_PATCH_RENDERER_VERSION.to_string(),
                entries: vec![
                    TranslationPatchEntryDraft {
                        atom_ids: vec!["atom-regular".to_string()],
                        translated_text: "Regular 甲".to_string(),
                        protected_spans: Vec::new(),
                    },
                    TranslationPatchEntryDraft {
                        atom_ids: vec!["atom-bold".to_string()],
                        translated_text: "Bold 乙".to_string(),
                        protected_spans: Vec::new(),
                    },
                ],
            },
        )
        .expect("pending patch")
    }

    fn page() -> PageGraph {
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:font-plan-test".to_string(),
            page_width: 100.0,
            page_height: 100.0,
            rotation_degrees: 0,
            atoms: vec![
                atom("atom-regular", "style-regular", 0),
                atom("atom-bold", "style-bold", 1),
            ],
            styles: vec![style("style-regular", 400), style("style-bold", 700)],
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(2),
            warnings: Vec::new(),
        }
    }

    fn atom(atom_id: &str, style_id: &str, order: u32) -> PageAtom {
        PageAtom {
            atom_id: atom_id.to_string(),
            source_text: atom_id.to_string(),
            source_object_id: Some(format!("object-{order}")),
            kind: PageAtomKind::Body,
            style_id: Some(style_id.to_string()),
            bounds: [0.0, 0.0, 10.0, 10.0],
            loose_bounds: None,
            origin: None,
            text_matrix: None,
            angle_degrees: None,
            order,
            generated: false,
            hyphen: false,
            requires_translation: true,
            source_kind: PageAtomSourceKind::PdfiumVerified,
            source_provenance: None,
        }
    }

    fn style(style_id: &str, weight: u16) -> PageStyle {
        PageStyle {
            style_id: style_id.to_string(),
            font_resource: Some("TestFont".to_string()),
            font_size: 10.0,
            scaled_font_size: 10.0,
            font_weight: Some(weight),
            italic: false,
            serif: false,
            fill_color: Some([0.0, 0.0, 0.0, 1.0]),
            stroke_color: Some([0.0, 0.0, 0.0, 1.0]),
            opacity: Some(1.0),
            render_mode: Some("FilledUnstroked".to_string()),
        }
    }
}
