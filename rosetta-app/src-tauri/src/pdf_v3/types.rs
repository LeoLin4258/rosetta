use serde::{Deserialize, Serialize};

pub(crate) const PDF_V3_CONTRACT_VERSION: u32 = 1;
pub(crate) const PAGE_GRAPH_SCHEMA_VERSION: u32 = 5;
pub(crate) const TRANSLATION_PATCH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormInvocationStep {
    pub parent_stream_object_number: u32,
    pub parent_stream_generation: u16,
    pub operation_index: usize,
    pub form_stream_object_number: u32,
    pub form_stream_generation: u16,
}

impl FormInvocationStep {
    pub(crate) fn parent_stream_id(&self) -> lopdf::ObjectId {
        (
            self.parent_stream_object_number,
            self.parent_stream_generation,
        )
    }

    pub(crate) fn form_stream_id(&self) -> lopdf::ObjectId {
        (self.form_stream_object_number, self.form_stream_generation)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageGraph {
    pub schema_version: u32,
    pub page_number: u32,
    pub source_page_hash: String,
    pub page_width: f32,
    pub page_height: f32,
    pub rotation_degrees: i32,
    pub atoms: Vec<PageAtom>,
    pub styles: Vec<PageStyle>,
    pub groups: Vec<PageGroup>,
    pub protected_spans: Vec<ProtectedSpan>,
    pub reconciliation: PageReconciliationSummary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageAtom {
    pub atom_id: String,
    pub source_text: String,
    pub source_object_id: Option<String>,
    pub kind: PageAtomKind,
    pub style_id: Option<String>,
    pub bounds: [f32; 4],
    pub loose_bounds: Option<[f32; 4]>,
    pub origin: Option<[f32; 2]>,
    pub text_matrix: Option<[f32; 6]>,
    pub angle_degrees: Option<f32>,
    pub order: u32,
    pub generated: bool,
    pub hyphen: bool,
    pub requires_translation: bool,
    pub source_kind: PageAtomSourceKind,
    pub source_provenance: Option<PageAtomSourceProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PageAtomSourceKind {
    PdfiumUnverified,
    PdfiumVerified,
    ToUnicodeCorrected,
    PdfiumSyntheticWhitespace,
    PreservedUnmapped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageAtomSourceProvenance {
    pub mapping_id: String,
    pub text_show_id: String,
    pub text_show_index: usize,
    pub operand_id: String,
    pub operand_index: usize,
    pub array_index: Option<usize>,
    pub encoded_start: usize,
    pub encoded_len: usize,
    pub source_unit_char_index: usize,
    pub source_unit_char_count: usize,
    pub form_invocation_path: Vec<FormInvocationStep>,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub text_show_operator: String,
    pub text_show_operand_hash: String,
    pub source_font_resource: Option<String>,
    pub source_font_size: Option<f32>,
    pub source_horizontal_scaling: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageReconciliationSummary {
    pub status: PageReconciliationStatus,
    pub mapped_object_count: usize,
    pub preserved_object_count: usize,
    pub verified_atom_count: usize,
    pub corrected_atom_count: usize,
    pub synthetic_whitespace_atom_count: usize,
    pub unrepresented_source_whitespace_count: usize,
    pub preserved_atom_count: usize,
    pub fallback_reasons: Vec<String>,
}

impl PageReconciliationSummary {
    pub(crate) fn unreconciled(atom_count: usize) -> Self {
        Self {
            status: PageReconciliationStatus::Unreconciled,
            mapped_object_count: 0,
            preserved_object_count: 0,
            verified_atom_count: 0,
            corrected_atom_count: 0,
            synthetic_whitespace_atom_count: 0,
            unrepresented_source_whitespace_count: 0,
            preserved_atom_count: atom_count,
            fallback_reasons: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PageReconciliationStatus {
    Unreconciled,
    Complete,
    Partial,
    Preserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PageAtomKind {
    Body,
    Citation,
    Formula,
    TableCell,
    Caption,
    Header,
    Footer,
    Annotation,
    Preserved,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageStyle {
    pub style_id: String,
    pub font_resource: Option<String>,
    pub font_size: f32,
    pub scaled_font_size: f32,
    pub font_weight: Option<u16>,
    pub italic: bool,
    pub serif: bool,
    pub fill_color: Option<[f32; 4]>,
    pub stroke_color: Option<[f32; 4]>,
    pub opacity: Option<f32>,
    pub render_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageGroup {
    pub group_id: String,
    pub kind: PageGroupKind,
    pub atom_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PageGroupKind {
    Line,
    Paragraph,
    Column,
    Table,
    TableCell,
    Caption,
    VisualRegion,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtectedSpan {
    pub span_id: String,
    pub kind: ProtectedSpanKind,
    pub atom_ids: Vec<String>,
    pub exact_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProtectedSpanKind {
    Citation,
    Url,
    Number,
    Formula,
    Symbol,
    Style,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatch {
    pub schema_version: u32,
    pub patch_id: String,
    pub page_number: u32,
    pub source_page_hash: String,
    pub target_language: String,
    pub translation_revision: u64,
    pub provider: TranslationPatchProvider,
    pub entries: Vec<TranslationPatchEntry>,
    pub renderer_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchProvider {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchEntry {
    pub entry_id: String,
    pub atoms: Vec<TranslationPatchAtomRef>,
    pub translated_text: String,
    pub protected_spans: Vec<TranslationPatchProtectedSpan>,
    pub style_id: String,
    pub renderer_decision: TranslationPatchRendererDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchAtomRef {
    pub atom_id: String,
    pub source_atom_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationPatchProtectedSpan {
    pub span_id: String,
    pub kind: ProtectedSpanKind,
    pub exact_text: String,
    pub translated_start: u32,
    pub translated_len: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum TranslationPatchRendererDecision {
    Pending,
    Fitted {
        strategy: TranslationPatchFitStrategy,
        fit_scale: f32,
    },
    Preserved {
        reason_code: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TranslationPatchFitStrategy {
    SingleShowScale,
    AnchoredTransaction,
    ParagraphReflow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageResult {
    pub page_number: u32,
    pub kind: PageResultKind,
    pub artifact_path: Option<String>,
    pub preserved_region_count: u32,
    pub warning_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PageResultKind {
    Translated,
    Preserved,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{
        PageAtom, PageAtomKind, PageAtomSourceKind, PageAtomSourceProvenance, PageGraph,
        PageReconciliationSummary, PageResult, PageResultKind,
    };

    #[test]
    fn page_graph_round_trips_through_json() {
        let graph = PageGraph {
            schema_version: super::PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 7,
            source_page_hash: "sha256:test".to_string(),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms: vec![PageAtom {
                atom_id: "page-7-atom-1".to_string(),
                source_text: "Citation [1]".to_string(),
                source_object_id: Some("obj-42".to_string()),
                kind: PageAtomKind::Citation,
                style_id: Some("style-1".to_string()),
                bounds: [10.0, 20.0, 80.0, 32.0],
                loose_bounds: Some([9.0, 19.0, 81.0, 33.0]),
                origin: Some([10.0, 20.0]),
                text_matrix: Some([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]),
                angle_degrees: Some(0.0),
                order: 0,
                generated: false,
                hyphen: false,
                requires_translation: false,
                source_kind: PageAtomSourceKind::PdfiumVerified,
                source_provenance: Some(PageAtomSourceProvenance {
                    mapping_id: "page-0007-map-000000".to_string(),
                    text_show_id: "page-0007-stream-00000042-00000-op-00000001".to_string(),
                    text_show_index: 0,
                    operand_id: "page-0007-stream-00000042-00000-op-00000001-arg-000".to_string(),
                    operand_index: 0,
                    array_index: None,
                    encoded_start: 0,
                    encoded_len: 1,
                    source_unit_char_index: 0,
                    source_unit_char_count: 1,
                    form_invocation_path: Vec::new(),
                    stream_object_number: 42,
                    stream_generation: 0,
                    operation_index: 1,
                    text_show_operator: "Tj".to_string(),
                    text_show_operand_hash: "sha256:operand".to_string(),
                    source_font_resource: Some("F1".to_string()),
                    source_font_size: Some(12.0),
                    source_horizontal_scaling: 100.0,
                }),
            }],
            styles: Vec::new(),
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(1),
            warnings: Vec::new(),
        };

        let encoded = serde_json::to_string(&graph).expect("encode page graph");
        let decoded: PageGraph = serde_json::from_str(&encoded).expect("decode page graph");

        assert_eq!(decoded, graph);
    }

    #[test]
    fn page_result_preserves_explicit_fallback_kind() {
        let result = PageResult {
            page_number: 3,
            kind: PageResultKind::Preserved,
            artifact_path: None,
            preserved_region_count: 2,
            warning_codes: vec!["complex-visual-region".to_string()],
        };

        let encoded = serde_json::to_string(&result).expect("encode page result");

        assert!(encoded.contains("\"kind\":\"preserved\""));
        assert!(encoded.contains("complex-visual-region"));
    }
}
