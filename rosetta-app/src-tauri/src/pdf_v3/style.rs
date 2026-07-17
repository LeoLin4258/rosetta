use std::fmt;

use super::{
    font::TranslationFontWeight,
    types::{PageGraph, PageStyle},
};

const COLOR_EPSILON: f32 = 0.0001;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextShowStylePlan {
    pub style_id: String,
    pub translation_font_weight: TranslationFontWeight,
    pub source_font_weight: u16,
    pub fill_color: [f32; 4],
    pub stroke_color: Option<[f32; 4]>,
    pub opacity: f32,
    pub render_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextShowStyleError {
    StyleMissing,
    DuplicateStyle,
    FontWeightUnavailable,
    UnsupportedItalic,
    UnsupportedRenderMode,
    FillColorUnavailable,
    InvalidColor,
    OpacityMismatch,
}

impl fmt::Display for TextShowStyleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StyleMissing => formatter.write_str("replacement PageStyle is missing"),
            Self::DuplicateStyle => formatter.write_str("replacement PageStyle ID is not unique"),
            Self::FontWeightUnavailable => {
                formatter.write_str("replacement source font weight cannot be classified safely")
            }
            Self::UnsupportedItalic => {
                formatter.write_str("italic source text has no approved translation font face")
            }
            Self::UnsupportedRenderMode => formatter
                .write_str("source text render mode is not safe for unified-font replacement"),
            Self::FillColorUnavailable => {
                formatter.write_str("source text fill color or opacity is unavailable")
            }
            Self::InvalidColor => {
                formatter.write_str("source text color is outside the normalized range")
            }
            Self::OpacityMismatch => {
                formatter.write_str("source text fill alpha and PageStyle opacity do not match")
            }
        }
    }
}

impl std::error::Error for TextShowStyleError {}

pub(crate) fn plan_text_show_style(
    page: &PageGraph,
    style_id: &str,
) -> Result<TextShowStylePlan, TextShowStyleError> {
    let mut matches = page
        .styles
        .iter()
        .filter(|style| style.style_id == style_id);
    let style = matches.next().ok_or(TextShowStyleError::StyleMissing)?;
    if matches.next().is_some() {
        return Err(TextShowStyleError::DuplicateStyle);
    }
    plan_page_style(style)
}

fn plan_page_style(style: &PageStyle) -> Result<TextShowStylePlan, TextShowStyleError> {
    if style.italic {
        return Err(TextShowStyleError::UnsupportedItalic);
    }
    let render_mode = style
        .render_mode
        .as_deref()
        .ok_or(TextShowStyleError::UnsupportedRenderMode)?;
    if render_mode != "FilledUnstroked" {
        return Err(TextShowStyleError::UnsupportedRenderMode);
    }
    let source_font_weight = style
        .font_weight
        .filter(|weight| (1..=1000).contains(weight))
        .ok_or(TextShowStyleError::FontWeightUnavailable)?;
    let fill_color = style
        .fill_color
        .filter(|color| valid_color(*color))
        .ok_or(TextShowStyleError::FillColorUnavailable)?;
    let opacity = style
        .opacity
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .ok_or(TextShowStyleError::FillColorUnavailable)?;
    if !approximately_equal(fill_color[3], opacity) {
        return Err(TextShowStyleError::OpacityMismatch);
    }
    let stroke_color = style.stroke_color;
    if stroke_color.is_some_and(|color| !valid_color(color)) {
        return Err(TextShowStyleError::InvalidColor);
    }
    let translation_font_weight = if source_font_weight >= 600
        || style
            .font_resource
            .as_deref()
            .is_some_and(font_name_has_bold_intent)
    {
        TranslationFontWeight::Bold
    } else {
        TranslationFontWeight::Regular
    };

    Ok(TextShowStylePlan {
        style_id: style.style_id.clone(),
        translation_font_weight,
        source_font_weight,
        fill_color,
        stroke_color,
        opacity,
        render_mode: render_mode.to_string(),
    })
}

fn font_name_has_bold_intent(font_name: &str) -> bool {
    let normalized = font_name
        .rsplit_once('+')
        .map_or(font_name, |(_, base)| base)
        .to_ascii_lowercase();
    ["bold", "black", "heavy", "demi", "semi", "cmbx", "-medi"]
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn valid_color(color: [f32; 4]) -> bool {
    color
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

fn approximately_equal(left: f32, right: f32) -> bool {
    (left - right).abs() <= COLOR_EPSILON
}

#[cfg(test)]
mod tests {
    use super::{plan_text_show_style, TextShowStyleError};
    use crate::pdf_v3::{
        font::TranslationFontWeight,
        types::{PageGraph, PageReconciliationSummary, PageStyle, PAGE_GRAPH_SCHEMA_VERSION},
    };

    #[test]
    fn colored_regular_fill_produces_a_regular_style_plan() {
        let page = page_with_style(style(
            Some("AAAAAA+ArialMT"),
            Some(225),
            false,
            [0.0, 0.0, 0.5019608, 1.0],
            "FilledUnstroked",
        ));

        let plan = plan_text_show_style(&page, "style-1").expect("style plan");

        assert_eq!(plan.translation_font_weight, TranslationFontWeight::Regular);
        assert_eq!(plan.fill_color, [0.0, 0.0, 0.5019608, 1.0]);
        assert_eq!(plan.opacity, 1.0);
    }

    #[test]
    fn explicit_bold_name_overrides_unreliable_numeric_weight() {
        for font_name in ["CAAAAA+Arial-BoldMT", "CMBX12", "NimbusRomNo9L-Medi"] {
            let page = page_with_style(style(
                Some(font_name),
                Some(380),
                false,
                [0.0, 0.0, 0.0, 1.0],
                "FilledUnstroked",
            ));

            let plan = plan_text_show_style(&page, "style-1").expect("bold style plan");

            assert_eq!(plan.translation_font_weight, TranslationFontWeight::Bold);
        }
    }

    #[test]
    fn high_numeric_weight_produces_bold_without_a_name_marker() {
        let page = page_with_style(style(
            Some("Subset+UnknownFace"),
            Some(700),
            false,
            [0.0, 0.0, 0.0, 1.0],
            "FilledUnstroked",
        ));

        let plan = plan_text_show_style(&page, "style-1").expect("bold style plan");

        assert_eq!(plan.translation_font_weight, TranslationFontWeight::Bold);
    }

    #[test]
    fn italic_and_clipping_styles_are_preserved() {
        let italic = page_with_style(style(
            Some("Arial-ItalicMT"),
            Some(400),
            true,
            [0.0, 0.0, 0.0, 1.0],
            "FilledUnstroked",
        ));
        let clipping = page_with_style(style(
            Some("ArialMT"),
            Some(400),
            false,
            [0.0, 0.0, 0.0, 1.0],
            "FilledUnstrokedClipping",
        ));

        assert_eq!(
            plan_text_show_style(&italic, "style-1"),
            Err(TextShowStyleError::UnsupportedItalic)
        );
        assert_eq!(
            plan_text_show_style(&clipping, "style-1"),
            Err(TextShowStyleError::UnsupportedRenderMode)
        );
    }

    fn page_with_style(style: PageStyle) -> PageGraph {
        PageGraph {
            schema_version: PAGE_GRAPH_SCHEMA_VERSION,
            page_number: 1,
            source_page_hash: "sha256:test".to_string(),
            page_width: 100.0,
            page_height: 100.0,
            rotation_degrees: 0,
            atoms: Vec::new(),
            styles: vec![style],
            groups: Vec::new(),
            protected_spans: Vec::new(),
            reconciliation: PageReconciliationSummary::unreconciled(0),
            warnings: Vec::new(),
        }
    }

    fn style(
        font_resource: Option<&str>,
        font_weight: Option<u16>,
        italic: bool,
        fill_color: [f32; 4],
        render_mode: &str,
    ) -> PageStyle {
        PageStyle {
            style_id: "style-1".to_string(),
            font_resource: font_resource.map(str::to_string),
            font_size: 10.0,
            scaled_font_size: 10.0,
            font_weight,
            italic,
            serif: false,
            fill_color: Some(fill_color),
            stroke_color: Some(fill_color),
            opacity: Some(fill_color[3]),
            render_mode: Some(render_mode.to_string()),
        }
    }
}
