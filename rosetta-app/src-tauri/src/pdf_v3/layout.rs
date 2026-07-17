use std::{collections::BTreeSet, fmt};

use super::types::{
    FormInvocationStep, PageAtom, PageAtomSourceKind, PageGraph, PAGE_GRAPH_SCHEMA_VERSION,
};

const MATRIX_EPSILON: f32 = 0.0001;
const AXIS_ALIGNMENT_EPSILON: f32 = 0.0001;

#[derive(Debug, Clone)]
pub(crate) struct TextShowGeometryKey {
    pub page_number: u32,
    pub text_show_id: String,
    pub form_invocation_path: Vec<FormInvocationStep>,
    pub stream_object_number: u32,
    pub stream_generation: u16,
    pub operation_index: usize,
    pub source_font_resource: String,
    pub source_font_size: f32,
    pub source_horizontal_scaling: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextShowFitBounds {
    pub style_id: String,
    pub max_advance: f32,
    pub page_advance: f32,
    pub baseline_scale: f32,
    pub atom_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextShowFitBoundsError {
    UnsupportedPageGraphSchema { expected: u32, actual: u32 },
    PageMismatch { expected: u32, actual: u32 },
    InvalidSourceTextState,
    TargetAtomsMissing,
    AmbiguousSourceObject,
    SourceProvenanceMismatch,
    StyleMissing,
    InconsistentStyle,
    GeometryMissing,
    GeometryInvalid,
    InconsistentTransform,
    UnsupportedBaselineAngle,
}

impl fmt::Display for TextShowFitBoundsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPageGraphSchema { expected, actual } => write!(
                formatter,
                "PageGraph schema mismatch: expected {expected}, found {actual}"
            ),
            Self::PageMismatch { expected, actual } => write!(
                formatter,
                "PageGraph page mismatch: expected {expected}, found {actual}"
            ),
            Self::InvalidSourceTextState => {
                formatter.write_str("source text state cannot produce geometry fit bounds")
            }
            Self::TargetAtomsMissing => {
                formatter.write_str("PageGraph has no atoms for the replacement text-show")
            }
            Self::AmbiguousSourceObject => formatter
                .write_str("replacement text-show resolves to more than one PageGraph object"),
            Self::SourceProvenanceMismatch => formatter
                .write_str("PageGraph source provenance does not match the replacement target"),
            Self::StyleMissing => formatter.write_str("replacement text-show style is missing"),
            Self::InconsistentStyle => {
                formatter.write_str("replacement text-show atoms do not share one source style")
            }
            Self::GeometryMissing => {
                formatter.write_str("replacement text-show geometry is incomplete")
            }
            Self::GeometryInvalid => {
                formatter.write_str("replacement text-show geometry is invalid")
            }
            Self::InconsistentTransform => {
                formatter.write_str("replacement text-show atoms do not share one transform")
            }
            Self::UnsupportedBaselineAngle => {
                formatter.write_str("replacement text-show baseline is not page-axis aligned")
            }
        }
    }
}

impl std::error::Error for TextShowFitBoundsError {}

pub(crate) fn derive_text_show_fit_bounds(
    page: &PageGraph,
    key: &TextShowGeometryKey,
) -> Result<TextShowFitBounds, TextShowFitBoundsError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION {
        return Err(TextShowFitBoundsError::UnsupportedPageGraphSchema {
            expected: PAGE_GRAPH_SCHEMA_VERSION,
            actual: page.schema_version,
        });
    }
    if page.page_number != key.page_number {
        return Err(TextShowFitBoundsError::PageMismatch {
            expected: key.page_number,
            actual: page.page_number,
        });
    }
    if !key.source_font_size.is_finite()
        || key.source_font_size <= 0.0
        || !key.source_horizontal_scaling.is_finite()
        || key.source_horizontal_scaling <= 0.0
    {
        return Err(TextShowFitBoundsError::InvalidSourceTextState);
    }

    let matched_atoms = page
        .atoms
        .iter()
        .filter(|atom| atom_matches_key(atom, key))
        .collect::<Vec<_>>();
    if matched_atoms.is_empty() {
        return Err(TextShowFitBoundsError::TargetAtomsMissing);
    }
    let source_object_ids = matched_atoms
        .iter()
        .filter_map(|atom| atom.source_object_id.as_deref())
        .collect::<BTreeSet<_>>();
    if source_object_ids.len() != 1 {
        return Err(TextShowFitBoundsError::AmbiguousSourceObject);
    }
    let source_object_id = source_object_ids
        .first()
        .copied()
        .ok_or(TextShowFitBoundsError::AmbiguousSourceObject)?;
    let object_atoms = page
        .atoms
        .iter()
        .filter(|atom| atom.source_object_id.as_deref() == Some(source_object_id))
        .collect::<Vec<_>>();
    if object_atoms.is_empty() {
        return Err(TextShowFitBoundsError::TargetAtomsMissing);
    }
    if object_atoms.iter().any(|atom| {
        atom.source_provenance
            .as_ref()
            .is_some_and(|_| !atom_matches_key(atom, key))
    }) {
        return Err(TextShowFitBoundsError::SourceProvenanceMismatch);
    }
    let style_ids = object_atoms
        .iter()
        .filter_map(|atom| atom.style_id.as_deref())
        .collect::<BTreeSet<_>>();
    if style_ids.is_empty() {
        return Err(TextShowFitBoundsError::StyleMissing);
    }
    if style_ids.len() != 1 || object_atoms.iter().any(|atom| atom.style_id.is_none()) {
        return Err(TextShowFitBoundsError::InconsistentStyle);
    }
    let style_id = style_ids
        .first()
        .copied()
        .ok_or(TextShowFitBoundsError::StyleMissing)?;

    let first_atom = object_atoms
        .iter()
        .min_by_key(|atom| atom.order)
        .copied()
        .ok_or(TextShowFitBoundsError::GeometryMissing)?;
    let first_origin = first_atom
        .origin
        .filter(|origin| origin.iter().all(|value| value.is_finite()))
        .ok_or(TextShowFitBoundsError::GeometryMissing)?;
    let matrix = first_atom
        .text_matrix
        .filter(|matrix| matrix.iter().all(|value| value.is_finite()))
        .ok_or(TextShowFitBoundsError::GeometryMissing)?;
    if object_atoms.iter().any(|atom| {
        atom.text_matrix
            .is_none_or(|candidate| !matrices_match(matrix, candidate))
    }) {
        return Err(TextShowFitBoundsError::InconsistentTransform);
    }

    let baseline_scale = matrix[0].hypot(matrix[1]);
    let determinant = matrix[0] * matrix[3] - matrix[1] * matrix[2];
    if !baseline_scale.is_finite()
        || baseline_scale <= MATRIX_EPSILON
        || !determinant.is_finite()
        || determinant.abs() <= MATRIX_EPSILON
    {
        return Err(TextShowFitBoundsError::GeometryInvalid);
    }
    let baseline = [matrix[0] / baseline_scale, matrix[1] / baseline_scale];
    if baseline[0].abs() > AXIS_ALIGNMENT_EPSILON && baseline[1].abs() > AXIS_ALIGNMENT_EPSILON {
        return Err(TextShowFitBoundsError::UnsupportedBaselineAngle);
    }

    let start_projection = project(first_origin, baseline);
    let mut terminal_projection = start_projection;
    for atom in &object_atoms {
        let bounds = atom.loose_bounds.unwrap_or(atom.bounds);
        if !valid_bounds(bounds) {
            return Err(TextShowFitBoundsError::GeometryInvalid);
        }
        for corner in bounds_corners(bounds) {
            terminal_projection = terminal_projection.max(project(corner, baseline));
        }
        if let Some(origin) = atom.origin {
            if !origin.iter().all(|value| value.is_finite()) {
                return Err(TextShowFitBoundsError::GeometryInvalid);
            }
            terminal_projection = terminal_projection.max(project(origin, baseline));
        }
    }
    let page_advance = terminal_projection - start_projection;
    let max_advance = page_advance / baseline_scale;
    if !page_advance.is_finite()
        || page_advance <= 0.0
        || !max_advance.is_finite()
        || max_advance <= 0.0
    {
        return Err(TextShowFitBoundsError::GeometryInvalid);
    }

    Ok(TextShowFitBounds {
        style_id: style_id.to_string(),
        max_advance,
        page_advance,
        baseline_scale,
        atom_count: object_atoms.len(),
    })
}

fn atom_matches_key(atom: &PageAtom, key: &TextShowGeometryKey) -> bool {
    if !matches!(
        atom.source_kind,
        PageAtomSourceKind::PdfiumVerified | PageAtomSourceKind::ToUnicodeCorrected
    ) {
        return false;
    }
    atom.source_provenance.as_ref().is_some_and(|provenance| {
        provenance.text_show_id == key.text_show_id
            && provenance.stream_object_number == key.stream_object_number
            && provenance.stream_generation == key.stream_generation
            && provenance.operation_index == key.operation_index
            && provenance.form_invocation_path == key.form_invocation_path
            && provenance.source_font_resource.as_deref() == Some(key.source_font_resource.as_str())
            && provenance
                .source_font_size
                .is_some_and(|size| approximately_equal(size, key.source_font_size))
            && approximately_equal(
                provenance.source_horizontal_scaling,
                key.source_horizontal_scaling,
            )
    })
}

fn matrices_match(expected: [f32; 6], actual: [f32; 6]) -> bool {
    expected
        .into_iter()
        .zip(actual)
        .all(|(expected, actual)| approximately_equal(expected, actual))
}

fn approximately_equal(left: f32, right: f32) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= MATRIX_EPSILON * scale
}

fn valid_bounds(bounds: [f32; 4]) -> bool {
    bounds.iter().all(|value| value.is_finite()) && bounds[0] <= bounds[2] && bounds[1] <= bounds[3]
}

fn bounds_corners(bounds: [f32; 4]) -> [[f32; 2]; 4] {
    [
        [bounds[0], bounds[1]],
        [bounds[0], bounds[3]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
    ]
}

fn project(point: [f32; 2], axis: [f32; 2]) -> f32 {
    point[0] * axis[0] + point[1] * axis[1]
}

#[cfg(test)]
mod tests {
    use super::{derive_text_show_fit_bounds, TextShowFitBoundsError, TextShowGeometryKey};
    use crate::pdf_v3::types::{
        PageAtom, PageAtomKind, PageAtomSourceKind, PageAtomSourceProvenance, PageGraph,
        PageReconciliationSummary, PAGE_GRAPH_SCHEMA_VERSION,
    };

    #[test]
    fn horizontal_geometry_converts_page_width_to_text_space() {
        let page = page_with_atoms(vec![
            atom(
                0,
                "A",
                [10.0, 20.0],
                [10.0, 18.0, 20.0, 30.0],
                [2.0, 0.0, 0.0, 1.0, 10.0, 20.0],
                true,
            ),
            atom(
                1,
                "B",
                [20.0, 20.0],
                [20.0, 18.0, 50.0, 30.0],
                [2.0, 0.0, 0.0, 1.0, 10.0, 20.0],
                true,
            ),
        ]);

        let bounds = derive_text_show_fit_bounds(&page, &key()).expect("fit bounds");

        assert_eq!(bounds.page_advance, 40.0);
        assert_eq!(bounds.baseline_scale, 2.0);
        assert_eq!(bounds.max_advance, 20.0);
        assert_eq!(bounds.atom_count, 2);
    }

    #[test]
    fn vertical_geometry_uses_rotated_baseline_scale() {
        let page = page_with_atoms(vec![
            atom(
                0,
                "A",
                [20.0, 10.0],
                [15.0, 10.0, 25.0, 20.0],
                [0.0, 2.0, -1.0, 0.0, 20.0, 10.0],
                true,
            ),
            atom(
                1,
                "B",
                [20.0, 20.0],
                [15.0, 20.0, 25.0, 50.0],
                [0.0, 2.0, -1.0, 0.0, 20.0, 10.0],
                true,
            ),
        ]);

        let bounds = derive_text_show_fit_bounds(&page, &key()).expect("fit bounds");

        assert_eq!(bounds.page_advance, 40.0);
        assert_eq!(bounds.baseline_scale, 2.0);
        assert_eq!(bounds.max_advance, 20.0);
    }

    #[test]
    fn reverse_horizontal_geometry_follows_the_text_direction() {
        let page = page_with_atoms(vec![
            atom(
                0,
                "A",
                [50.0, 20.0],
                [40.0, 18.0, 50.0, 30.0],
                [-2.0, 0.0, 0.0, -1.0, 50.0, 20.0],
                true,
            ),
            atom(
                1,
                "B",
                [40.0, 20.0],
                [10.0, 18.0, 40.0, 30.0],
                [-2.0, 0.0, 0.0, -1.0, 50.0, 20.0],
                true,
            ),
        ]);

        let bounds = derive_text_show_fit_bounds(&page, &key()).expect("fit bounds");

        assert_eq!(bounds.page_advance, 40.0);
        assert_eq!(bounds.baseline_scale, 2.0);
        assert_eq!(bounds.max_advance, 20.0);
    }

    #[test]
    fn synthetic_trailing_space_extends_object_geometry() {
        let mut trailing_space = atom(
            1,
            " ",
            [20.0, 20.0],
            [20.0, 18.0, 30.0, 30.0],
            [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
            false,
        );
        trailing_space.source_kind = PageAtomSourceKind::PdfiumSyntheticWhitespace;
        let page = page_with_atoms(vec![
            atom(
                0,
                "A",
                [10.0, 20.0],
                [10.0, 18.0, 20.0, 30.0],
                [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
                true,
            ),
            trailing_space,
        ]);

        let bounds = derive_text_show_fit_bounds(&page, &key()).expect("fit bounds");

        assert_eq!(bounds.max_advance, 20.0);
        assert_eq!(bounds.atom_count, 2);
    }

    #[test]
    fn angled_axis_aligned_boxes_are_rejected() {
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        let page = page_with_atoms(vec![atom(
            0,
            "A",
            [10.0, 20.0],
            [10.0, 18.0, 20.0, 30.0],
            [diagonal, diagonal, -diagonal, diagonal, 10.0, 20.0],
            true,
        )]);

        let error = derive_text_show_fit_bounds(&page, &key())
            .expect_err("diagonal AABB cannot produce safe fit bounds");

        assert_eq!(error, TextShowFitBoundsError::UnsupportedBaselineAngle);
    }

    fn key() -> TextShowGeometryKey {
        TextShowGeometryKey {
            page_number: 1,
            text_show_id: "show-1".to_string(),
            form_invocation_path: Vec::new(),
            stream_object_number: 4,
            stream_generation: 0,
            operation_index: 7,
            source_font_resource: "F1".to_string(),
            source_font_size: 10.0,
            source_horizontal_scaling: 100.0,
        }
    }

    fn page_with_atoms(atoms: Vec<PageAtom>) -> PageGraph {
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:test".to_string(),
            page_width: 100.0,
            page_height: 100.0,
            rotation_degrees: 0,
            reconciliation: PageReconciliationSummary::unreconciled(atoms.len()),
            atoms,
            styles: Vec::new(),
            groups: Vec::new(),
            protected_spans: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn atom(
        order: u32,
        text: &str,
        origin: [f32; 2],
        loose_bounds: [f32; 4],
        text_matrix: [f32; 6],
        with_provenance: bool,
    ) -> PageAtom {
        PageAtom {
            atom_id: format!("atom-{order}"),
            source_text: text.to_string(),
            source_object_id: Some("object-1".to_string()),
            kind: PageAtomKind::Body,
            style_id: Some("style-1".to_string()),
            bounds: loose_bounds,
            loose_bounds: Some(loose_bounds),
            origin: Some(origin),
            text_matrix: Some(text_matrix),
            angle_degrees: Some(0.0),
            order,
            generated: !with_provenance,
            hyphen: false,
            requires_translation: with_provenance,
            source_kind: if with_provenance {
                PageAtomSourceKind::PdfiumVerified
            } else {
                PageAtomSourceKind::PdfiumSyntheticWhitespace
            },
            source_provenance: with_provenance.then(|| PageAtomSourceProvenance {
                mapping_id: "mapping-1".to_string(),
                text_show_id: "show-1".to_string(),
                text_show_index: 0,
                operand_id: "operand-1".to_string(),
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
                source_font_resource: Some("F1".to_string()),
                source_font_size: Some(10.0),
                source_horizontal_scaling: 100.0,
            }),
        }
    }
}
