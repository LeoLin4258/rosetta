use std::{cmp::Ordering, collections::BTreeMap, time::Instant};

use super::types::{PageAtom, PageAtomSourceKind, PageGraph, PageGroup, PageGroupKind, PageStyle};

pub(crate) const MAX_VISUAL_ATOMS_PER_PAGE: usize = 200_000;
pub(crate) const MAX_VISUAL_LINES_PER_PAGE: usize = 50_000;
pub(crate) const MAX_VISUAL_PARAGRAPHS_PER_PAGE: usize = 25_000;
pub(crate) const MAX_FLOW_CONTAINERS_PER_PAGE: usize = 10_000;

const MIN_GEOMETRY_SIZE: f32 = 0.01;
const MAX_HORIZONTAL_ANGLE_DEGREES: f32 = 2.0;
const MAX_ACTIVE_GROUPING_CANDIDATES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisualGroupingSummary {
    pub eligible_atom_count: usize,
    pub line_count: usize,
    pub paragraph_count: usize,
    pub flow_container_count: usize,
    pub elapsed_us: u64,
    pub limit_exceeded: bool,
}

#[derive(Debug, Clone)]
struct AtomGeometry {
    atom_index: usize,
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
    baseline: f32,
    font_size: f32,
    whitespace: bool,
    emphasized: bool,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct VisualLine {
    atom_indices: Vec<usize>,
    bounds: [f32; 4],
    baseline: f32,
    font_size: f32,
    starts_emphasized: bool,
    ends_emphasized: bool,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct VisualParagraph {
    line_indices: Vec<usize>,
    bounds: [f32; 4],
    font_size: f32,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct FlowContainer {
    paragraph_indices: Vec<usize>,
    bounds: [f32; 4],
    font_size: f32,
    confidence: f32,
}

pub(crate) fn derive_visual_groups(page: &mut PageGraph) -> VisualGroupingSummary {
    let started = Instant::now();
    page.groups.clear();

    let styles = page
        .styles
        .iter()
        .map(|style| (style.style_id.as_str(), style))
        .collect::<BTreeMap<_, _>>();
    let mut atoms = page
        .atoms
        .iter()
        .enumerate()
        .filter_map(|(index, atom)| atom_geometry(index, atom, &styles))
        .take(MAX_VISUAL_ATOMS_PER_PAGE + 1)
        .collect::<Vec<_>>();
    let eligible_atom_count = atoms.len();
    let limit_exceeded = eligible_atom_count > MAX_VISUAL_ATOMS_PER_PAGE;

    if limit_exceeded {
        page.warnings
            .push("visual-grouping-atom-limit-exceeded".to_string());
        normalize_warnings(page);
        return summary(started, eligible_atom_count, 0, 0, 0, true);
    }

    atoms.sort_by(compare_atom_reading_order);
    let Some(lines) = derive_lines(&atoms, page.page_width) else {
        page.warnings
            .push("visual-grouping-line-limit-exceeded".to_string());
        normalize_warnings(page);
        return summary(started, eligible_atom_count, 0, 0, 0, true);
    };
    let Some(paragraphs) = derive_paragraphs(&lines) else {
        page.warnings
            .push("visual-grouping-paragraph-limit-exceeded".to_string());
        normalize_warnings(page);
        return summary(started, eligible_atom_count, lines.len(), 0, 0, true);
    };
    let Some(containers) = derive_flow_containers(&paragraphs) else {
        page.warnings
            .push("visual-grouping-container-limit-exceeded".to_string());
        normalize_warnings(page);
        return summary(
            started,
            eligible_atom_count,
            lines.len(),
            paragraphs.len(),
            0,
            true,
        );
    };

    page.groups.reserve(
        lines
            .len()
            .saturating_add(paragraphs.len())
            .saturating_add(containers.len()),
    );
    append_line_groups(page, &lines);
    append_paragraph_groups(page, &lines, &paragraphs);
    append_container_groups(page, &lines, &paragraphs, &containers);

    summary(
        started,
        eligible_atom_count,
        lines.len(),
        paragraphs.len(),
        containers.len(),
        limit_exceeded,
    )
}

fn summary(
    started: Instant,
    eligible_atom_count: usize,
    line_count: usize,
    paragraph_count: usize,
    flow_container_count: usize,
    limit_exceeded: bool,
) -> VisualGroupingSummary {
    VisualGroupingSummary {
        eligible_atom_count,
        line_count,
        paragraph_count,
        flow_container_count,
        elapsed_us: started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        limit_exceeded,
    }
}

fn atom_geometry(
    atom_index: usize,
    atom: &PageAtom,
    styles: &BTreeMap<&str, &PageStyle>,
) -> Option<AtomGeometry> {
    if !matches!(
        atom.source_kind,
        PageAtomSourceKind::PdfiumVerified
            | PageAtomSourceKind::ToUnicodeCorrected
            | PageAtomSourceKind::PdfiumSyntheticWhitespace
    ) || !horizontal_angle(atom.angle_degrees?)
    {
        return None;
    }

    let bounds = atom.loose_bounds.unwrap_or(atom.bounds);
    if bounds.iter().any(|value| !value.is_finite())
        || bounds[2] - bounds[0] < MIN_GEOMETRY_SIZE
        || bounds[3] - bounds[1] < MIN_GEOMETRY_SIZE
    {
        return None;
    }
    let baseline = atom
        .origin
        .filter(|origin| origin.iter().all(|value| value.is_finite()))
        .map(|origin| origin[1])
        .unwrap_or(bounds[1]);
    let style = atom
        .style_id
        .as_deref()
        .and_then(|style_id| styles.get(style_id))
        .copied();
    let style_size = style
        .map(|style| style.scaled_font_size.abs())
        .filter(|size| size.is_finite() && *size >= MIN_GEOMETRY_SIZE);
    let font_size = style_size.unwrap_or(bounds[3] - bounds[1]);
    let confidence = match atom.source_kind {
        PageAtomSourceKind::PdfiumVerified => 0.99,
        PageAtomSourceKind::ToUnicodeCorrected => 0.96,
        PageAtomSourceKind::PdfiumSyntheticWhitespace => 0.92,
        PageAtomSourceKind::PdfiumUnverified | PageAtomSourceKind::PreservedUnmapped => 0.0,
    };

    Some(AtomGeometry {
        atom_index,
        left: bounds[0],
        bottom: bounds[1],
        right: bounds[2],
        top: bounds[3],
        baseline,
        font_size,
        whitespace: atom.source_text.chars().all(char::is_whitespace),
        emphasized: style.and_then(|style| style.font_weight).unwrap_or(400) >= 600,
        confidence,
    })
}

fn horizontal_angle(angle: f32) -> bool {
    if !angle.is_finite() {
        return false;
    }
    let normalized = angle.rem_euclid(360.0);
    normalized <= MAX_HORIZONTAL_ANGLE_DEGREES
        || normalized >= 360.0 - MAX_HORIZONTAL_ANGLE_DEGREES
        || (normalized - 180.0).abs() <= MAX_HORIZONTAL_ANGLE_DEGREES
}

fn compare_atom_reading_order(left: &AtomGeometry, right: &AtomGeometry) -> Ordering {
    right
        .baseline
        .total_cmp(&left.baseline)
        .then_with(|| left.left.total_cmp(&right.left))
        .then_with(|| left.atom_index.cmp(&right.atom_index))
}

fn derive_lines(atoms: &[AtomGeometry], page_width: f32) -> Option<Vec<VisualLine>> {
    let mut lines = Vec::new();
    let mut row = Vec::<AtomGeometry>::new();
    let mut row_baseline = 0.0f32;
    let mut row_font_size = 0.0f32;

    for atom in atoms {
        let tolerance = (row_font_size.max(atom.font_size) * 0.35).max(0.75);
        if !row.is_empty() && (row_baseline - atom.baseline).abs() > tolerance {
            append_row_lines(&mut lines, &mut row, page_width)?;
        }
        if row.is_empty() {
            row_baseline = atom.baseline;
            row_font_size = atom.font_size;
        } else {
            let count = row.len() as f32;
            row_baseline = (row_baseline * count + atom.baseline) / (count + 1.0);
            row_font_size = row_font_size.max(atom.font_size);
        }
        row.push(atom.clone());
    }
    append_row_lines(&mut lines, &mut row, page_width)?;
    lines.sort_by(compare_line_reading_order);
    Some(lines)
}

fn append_row_lines(
    lines: &mut Vec<VisualLine>,
    row: &mut Vec<AtomGeometry>,
    page_width: f32,
) -> Option<()> {
    if row.is_empty() {
        return Some(());
    }
    row.sort_by(|left, right| {
        left.left
            .total_cmp(&right.left)
            .then_with(|| left.atom_index.cmp(&right.atom_index))
    });

    let mut segment = Vec::<AtomGeometry>::new();
    let mut last_visible_right = None::<f32>;
    for atom in row.drain(..) {
        if !atom.whitespace {
            if let Some(right) = last_visible_right {
                let gutter = ((atom.top - atom.bottom).abs() * 0.9)
                    .max(page_width.abs() * 0.008)
                    .max(4.0);
                if atom.left - right > gutter {
                    append_line(lines, &mut segment)?;
                }
            }
            last_visible_right = Some(atom.right);
        }
        segment.push(atom);
    }
    append_line(lines, &mut segment)
}

fn append_line(lines: &mut Vec<VisualLine>, segment: &mut Vec<AtomGeometry>) -> Option<()> {
    if !segment.iter().any(|atom| !atom.whitespace) {
        segment.clear();
        return Some(());
    }
    if lines.len() >= MAX_VISUAL_LINES_PER_PAGE {
        return None;
    }

    let first_visible = segment
        .iter()
        .position(|atom| !atom.whitespace)
        .expect("visible line atom");
    let last_visible = segment
        .iter()
        .rposition(|atom| !atom.whitespace)
        .expect("visible line atom");
    let mut trimmed = segment
        .drain(first_visible..=last_visible)
        .collect::<Vec<_>>();
    segment.clear();
    let visible = trimmed
        .iter()
        .filter(|atom| !atom.whitespace)
        .collect::<Vec<_>>();
    let bounds = union_atom_bounds(&visible);
    let baseline = median(visible.iter().map(|atom| atom.baseline).collect());
    let font_size = median(visible.iter().map(|atom| atom.font_size).collect());
    let confidence = visible
        .iter()
        .map(|atom| atom.confidence)
        .fold(1.0f32, f32::min);
    let starts_emphasized = visible.first().is_some_and(|atom| atom.emphasized);
    let ends_emphasized = visible.last().is_some_and(|atom| atom.emphasized);
    lines.push(VisualLine {
        atom_indices: trimmed.drain(..).map(|atom| atom.atom_index).collect(),
        bounds,
        baseline,
        font_size,
        starts_emphasized,
        ends_emphasized,
        confidence,
    });
    Some(())
}

fn derive_paragraphs(lines: &[VisualLine]) -> Option<Vec<VisualParagraph>> {
    let mut paragraphs = Vec::<VisualParagraph>::new();
    let mut active = Vec::<usize>::new();

    for (line_index, line) in lines.iter().enumerate() {
        active.retain(|paragraph_index| {
            let last = &lines[*paragraphs[*paragraph_index]
                .line_indices
                .last()
                .expect("paragraph line")];
            last.baseline - line.baseline <= (last.font_size.max(line.font_size) * 4.0).max(24.0)
        });

        let best = active
            .iter()
            .copied()
            .filter_map(|paragraph_index| {
                let last = &lines[*paragraphs[paragraph_index]
                    .line_indices
                    .last()
                    .expect("paragraph line")];
                paragraph_match_score(last, line).map(|score| (paragraph_index, score))
            })
            .min_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| left.0.cmp(&right.0))
            })
            .map(|(paragraph_index, _)| paragraph_index);

        if let Some(paragraph_index) = best {
            let paragraph = &mut paragraphs[paragraph_index];
            paragraph.line_indices.push(line_index);
            paragraph.bounds = union_bounds(paragraph.bounds, line.bounds);
            paragraph.font_size = (paragraph.font_size + line.font_size) * 0.5;
            paragraph.confidence = paragraph.confidence.min(line.confidence).min(0.96);
        } else {
            if paragraphs.len() >= MAX_VISUAL_PARAGRAPHS_PER_PAGE
                || active.len() >= MAX_ACTIVE_GROUPING_CANDIDATES
            {
                return None;
            }
            paragraphs.push(VisualParagraph {
                line_indices: vec![line_index],
                bounds: line.bounds,
                font_size: line.font_size,
                confidence: line.confidence.min(0.94),
            });
            active.push(paragraphs.len() - 1);
        }
    }
    paragraphs.sort_by(compare_paragraph_reading_order);
    Some(paragraphs)
}

fn paragraph_match_score(previous: &VisualLine, current: &VisualLine) -> Option<f32> {
    let baseline_drop = previous.baseline - current.baseline;
    let size = previous.font_size.max(current.font_size).max(1.0);
    let previous_height = previous.bounds[3] - previous.bounds[1];
    let current_height = current.bounds[3] - current.bounds[1];
    let geometry_height = previous_height.max(current_height).max(1.0);
    if (current.starts_emphasized && !previous.ends_emphasized)
        || baseline_drop < previous_height.min(current_height) * 0.25
        || baseline_drop > geometry_height * 1.45
        || previous.font_size.min(current.font_size) / size < 0.55
    {
        return None;
    }
    let vertical_gap = previous.bounds[1] - current.bounds[3];
    if vertical_gap > size * 1.35 {
        return None;
    }
    let overlap = horizontal_overlap_ratio(previous.bounds, current.bounds);
    let left_delta = (previous.bounds[0] - current.bounds[0]).abs();
    if overlap < 0.25 && left_delta > size * 2.5 {
        return None;
    }
    Some(baseline_drop + left_delta * 0.2 - overlap * size)
}

fn derive_flow_containers(paragraphs: &[VisualParagraph]) -> Option<Vec<FlowContainer>> {
    let mut containers = Vec::<FlowContainer>::new();
    let mut active = Vec::<usize>::new();

    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        active.retain(|container_index| {
            let last = &paragraphs[*containers[*container_index]
                .paragraph_indices
                .last()
                .expect("container paragraph")];
            last.bounds[1] - paragraph.bounds[3]
                <= (last.font_size.max(paragraph.font_size) * 8.0).max(48.0)
        });
        let best = active
            .iter()
            .copied()
            .filter_map(|container_index| {
                let last = &paragraphs[*containers[container_index]
                    .paragraph_indices
                    .last()
                    .expect("container paragraph")];
                container_match_score(last, paragraph).map(|score| (container_index, score))
            })
            .min_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| left.0.cmp(&right.0))
            })
            .map(|(container_index, _)| container_index);

        if let Some(container_index) = best {
            let container = &mut containers[container_index];
            container.paragraph_indices.push(paragraph_index);
            container.bounds = union_bounds(container.bounds, paragraph.bounds);
            container.font_size = (container.font_size + paragraph.font_size) * 0.5;
            container.confidence = container.confidence.min(paragraph.confidence).min(0.92);
        } else {
            if containers.len() >= MAX_FLOW_CONTAINERS_PER_PAGE
                || active.len() >= MAX_ACTIVE_GROUPING_CANDIDATES
            {
                return None;
            }
            containers.push(FlowContainer {
                paragraph_indices: vec![paragraph_index],
                bounds: paragraph.bounds,
                font_size: paragraph.font_size,
                confidence: paragraph.confidence.min(0.90),
            });
            active.push(containers.len() - 1);
        }
    }
    containers.sort_by(compare_container_reading_order);
    Some(containers)
}

fn container_match_score(previous: &VisualParagraph, current: &VisualParagraph) -> Option<f32> {
    let size = previous.font_size.max(current.font_size).max(1.0);
    let vertical_gap = previous.bounds[1] - current.bounds[3];
    if vertical_gap < -size * 0.5 || vertical_gap > size * 4.0 {
        return None;
    }
    let overlap = horizontal_overlap_ratio(previous.bounds, current.bounds);
    let previous_width = width(previous.bounds);
    let current_width = width(current.bounds);
    let width_ratio =
        previous_width.min(current_width) / previous_width.max(current_width).max(1.0);
    let center_delta =
        (horizontal_center(previous.bounds) - horizontal_center(current.bounds)).abs();
    let edge_tolerance = (size * 3.0).max(previous_width.max(current_width) * 0.15);
    let left_delta = (previous.bounds[0] - current.bounds[0]).abs();
    let right_delta = (previous.bounds[2] - current.bounds[2]).abs();
    if overlap < 0.65
        || width_ratio < 0.45
        || center_delta > previous_width.max(current_width) * 0.3
        || left_delta > edge_tolerance
        || right_delta > edge_tolerance
    {
        return None;
    }
    Some(vertical_gap.max(0.0) + center_delta * 0.15 - overlap * size)
}

fn append_line_groups(page: &mut PageGraph, lines: &[VisualLine]) {
    for (index, line) in lines.iter().enumerate() {
        page.groups.push(PageGroup {
            group_id: format!("page-{:04}-line-{:06}", page.page_number, index + 1),
            kind: PageGroupKind::Line,
            atom_ids: atom_ids(page, &line.atom_indices),
            bounds: line.bounds,
            confidence: line.confidence,
        });
    }
}

fn append_paragraph_groups(
    page: &mut PageGraph,
    lines: &[VisualLine],
    paragraphs: &[VisualParagraph],
) {
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let atom_indices = paragraph
            .line_indices
            .iter()
            .flat_map(|line_index| lines[*line_index].atom_indices.iter().copied())
            .collect::<Vec<_>>();
        page.groups.push(PageGroup {
            group_id: format!("page-{:04}-paragraph-{:06}", page.page_number, index + 1),
            kind: PageGroupKind::Paragraph,
            atom_ids: atom_ids(page, &atom_indices),
            bounds: paragraph.bounds,
            confidence: paragraph.confidence,
        });
    }
}

fn append_container_groups(
    page: &mut PageGraph,
    lines: &[VisualLine],
    paragraphs: &[VisualParagraph],
    containers: &[FlowContainer],
) {
    for (index, container) in containers.iter().enumerate() {
        let atom_indices = container
            .paragraph_indices
            .iter()
            .flat_map(|paragraph_index| paragraphs[*paragraph_index].line_indices.iter())
            .flat_map(|line_index| lines[*line_index].atom_indices.iter().copied())
            .collect::<Vec<_>>();
        page.groups.push(PageGroup {
            group_id: format!(
                "page-{:04}-flow-container-{:06}",
                page.page_number,
                index + 1
            ),
            kind: PageGroupKind::FlowContainer,
            atom_ids: atom_ids(page, &atom_indices),
            bounds: container.bounds,
            confidence: container.confidence,
        });
    }
}

fn atom_ids(page: &PageGraph, atom_indices: &[usize]) -> Vec<String> {
    atom_indices
        .iter()
        .map(|index| page.atoms[*index].atom_id.clone())
        .collect()
}

fn compare_line_reading_order(left: &VisualLine, right: &VisualLine) -> Ordering {
    right
        .baseline
        .total_cmp(&left.baseline)
        .then_with(|| left.bounds[0].total_cmp(&right.bounds[0]))
}

fn compare_paragraph_reading_order(left: &VisualParagraph, right: &VisualParagraph) -> Ordering {
    right.bounds[3]
        .total_cmp(&left.bounds[3])
        .then_with(|| left.bounds[0].total_cmp(&right.bounds[0]))
}

fn compare_container_reading_order(left: &FlowContainer, right: &FlowContainer) -> Ordering {
    right.bounds[3]
        .total_cmp(&left.bounds[3])
        .then_with(|| left.bounds[0].total_cmp(&right.bounds[0]))
}

fn horizontal_overlap_ratio(left: [f32; 4], right: [f32; 4]) -> f32 {
    let overlap = (left[2].min(right[2]) - left[0].max(right[0])).max(0.0);
    overlap / width(left).min(width(right)).max(1.0)
}

fn horizontal_center(bounds: [f32; 4]) -> f32 {
    (bounds[0] + bounds[2]) * 0.5
}

fn width(bounds: [f32; 4]) -> f32 {
    (bounds[2] - bounds[0]).max(0.0)
}

fn union_bounds(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        left[0].min(right[0]),
        left[1].min(right[1]),
        left[2].max(right[2]),
        left[3].max(right[3]),
    ]
}

fn union_atom_bounds(atoms: &[&AtomGeometry]) -> [f32; 4] {
    let first = atoms.first().expect("visible line atom");
    atoms.iter().skip(1).fold(
        [first.left, first.bottom, first.right, first.top],
        |bounds, atom| union_bounds(bounds, [atom.left, atom.bottom, atom.right, atom.top]),
    )
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

fn normalize_warnings(page: &mut PageGraph) {
    page.warnings.sort();
    page.warnings.dedup();
}

#[cfg(test)]
mod tests {
    use super::derive_visual_groups;
    use crate::pdf_v3::types::{
        PageAtom, PageAtomKind, PageAtomSourceKind, PageGraph, PageGroupKind,
        PageReconciliationStatus, PageReconciliationSummary, PageStyle, PAGE_GRAPH_SCHEMA_VERSION,
    };

    fn two_column_page() -> PageGraph {
        let mut atoms = Vec::new();
        let mut order = 0u32;
        for (column, left) in [40.0f32, 330.0].into_iter().enumerate() {
            for row in 0..3 {
                let baseline = 700.0 - row as f32 * 14.0;
                for character in 0..8 {
                    let atom_left = left + character as f32 * 7.0;
                    atoms.push(PageAtom {
                        atom_id: format!("atom-{column}-{row}-{character}"),
                        source_text: char::from(b'a' + character as u8).to_string(),
                        source_object_id: Some(format!("object-{column}-{row}")),
                        kind: PageAtomKind::Body,
                        style_id: Some("body".to_string()),
                        bounds: [atom_left, baseline - 2.0, atom_left + 6.0, baseline + 9.0],
                        loose_bounds: None,
                        origin: Some([atom_left, baseline]),
                        text_matrix: None,
                        angle_degrees: Some(0.0),
                        order,
                        generated: false,
                        hyphen: false,
                        requires_translation: true,
                        source_kind: PageAtomSourceKind::PdfiumVerified,
                        source_provenance: None,
                    });
                    order += 1;
                }
            }
        }
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:test".to_string(),
            page_width: 612.0,
            page_height: 792.0,
            rotation_degrees: 0,
            atoms,
            styles: vec![PageStyle {
                style_id: "body".to_string(),
                font_resource: Some("F1".to_string()),
                font_size: 10.0,
                scaled_font_size: 10.0,
                font_weight: Some(400),
                italic: false,
                serif: false,
                fill_color: Some([0.0, 0.0, 0.0, 1.0]),
                stroke_color: None,
                opacity: Some(1.0),
                render_mode: Some("fill".to_string()),
            }],
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary {
                status: PageReconciliationStatus::Complete,
                mapped_object_count: 6,
                preserved_object_count: 0,
                verified_atom_count: 48,
                corrected_atom_count: 0,
                synthetic_whitespace_atom_count: 0,
                unrepresented_source_whitespace_count: 0,
                preserved_atom_count: 0,
                fallback_reasons: Vec::new(),
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn keeps_two_columns_in_separate_paragraphs_and_containers() {
        let mut page = two_column_page();
        let summary = derive_visual_groups(&mut page);

        assert_eq!(summary.line_count, 6);
        assert_eq!(summary.paragraph_count, 2);
        assert_eq!(summary.flow_container_count, 2);
        for kind in [PageGroupKind::Paragraph, PageGroupKind::FlowContainer] {
            let groups = page
                .groups
                .iter()
                .filter(|group| group.kind == kind)
                .collect::<Vec<_>>();
            assert_eq!(groups.len(), 2);
            assert!(groups.iter().all(|group| group.atom_ids.len() == 24));
            assert!(groups.iter().all(|group| {
                let first_column = group.atom_ids[0].split('-').nth(1);
                group
                    .atom_ids
                    .iter()
                    .all(|atom_id| atom_id.split('-').nth(1) == first_column)
            }));
        }
    }

    #[test]
    fn grouping_ids_and_atom_order_are_deterministic() {
        let mut first = two_column_page();
        let mut second = first.clone();

        derive_visual_groups(&mut first);
        derive_visual_groups(&mut second);

        assert_eq!(first.groups, second.groups);
        assert_eq!(
            first.groups.first().expect("first line").atom_ids,
            (0..8)
                .map(|character| format!("atom-0-0-{character}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn emphasized_line_start_splits_translation_paragraphs_but_not_columns() {
        let mut page = two_column_page();
        let mut bold = page.styles[0].clone();
        bold.style_id = "bold".to_string();
        bold.font_weight = Some(700);
        page.styles.push(bold);
        for atom in &mut page.atoms {
            if atom.atom_id.ends_with("-1-0") {
                atom.style_id = Some("bold".to_string());
            }
        }

        let summary = derive_visual_groups(&mut page);

        assert_eq!(summary.paragraph_count, 4);
        assert_eq!(summary.flow_container_count, 2);
    }

    #[test]
    fn diagnostics_do_not_contain_document_text() {
        let mut page = two_column_page();
        page.atoms[0].source_text = "PRIVATE-SOURCE-TEXT".to_string();

        let summary = derive_visual_groups(&mut page);

        assert!(!format!("{summary:?}").contains("PRIVATE-SOURCE-TEXT"));
    }
}
