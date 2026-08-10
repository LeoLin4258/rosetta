use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::rosetta_jobs::model::RosettaBlock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderedMarkdownBlock {
    pub(crate) block_ids: Vec<String>,
    pub(crate) kind: String,
    pub(crate) markdown: String,
}

#[derive(Debug)]
struct TableCell<'a> {
    block: &'a RosettaBlock,
    table_id: String,
    row: usize,
    column: usize,
    row_span: usize,
    column_span: usize,
    header: bool,
    row_count: usize,
    column_count: usize,
}

pub(crate) fn is_pdf_markdown_block(block: &RosettaBlock) -> bool {
    block
        .style
        .as_ref()
        .and_then(|style| style.get("pdfMarkdown"))
        .is_some()
}

pub(crate) fn render_blocks(
    blocks: &[RosettaBlock],
    text_by_block: &HashMap<String, String>,
) -> Vec<RenderedMarkdownBlock> {
    let mut rendered = Vec::new();
    let mut index = 0usize;

    while index < blocks.len() {
        let block = &blocks[index];
        if block.block_type == "table_cell" {
            let table_id = table_cell(block)
                .map(|cell| cell.table_id)
                .unwrap_or_else(|| block.id.clone());
            let mut end = index + 1;
            while end < blocks.len()
                && blocks[end].block_type == "table_cell"
                && table_cell(&blocks[end]).is_some_and(|cell| cell.table_id == table_id)
            {
                end += 1;
            }
            rendered.push(render_table(&blocks[index..end], text_by_block));
            index = end;
            continue;
        }

        let markdown = render_block(block, resolved_text(block, text_by_block));
        if !markdown.trim().is_empty() {
            rendered.push(RenderedMarkdownBlock {
                block_ids: vec![block.id.clone()],
                kind: block.block_type.clone(),
                markdown,
            });
        }
        index += 1;
    }

    rendered
}

pub(crate) fn join_blocks(blocks: &[RenderedMarkdownBlock]) -> String {
    let mut output = String::new();
    let mut previous_kind: Option<&str> = None;

    for block in blocks {
        if !output.is_empty() {
            if previous_kind == Some("list_item") && block.kind == "list_item" {
                output.push('\n');
            } else {
                output.push_str("\n\n");
            }
        }
        output.push_str(block.markdown.trim_matches('\n'));
        previous_kind = Some(block.kind.as_str());
    }

    output.trim().to_string()
}

fn render_block(block: &RosettaBlock, text: &str) -> String {
    let extra = metadata_extra(block);
    match block.block_type.as_str() {
        "heading" => {
            let level = extra
                .and_then(|value| value.get("headingLevel"))
                .and_then(Value::as_u64)
                .unwrap_or(2)
                .clamp(1, 6);
            format!(
                "{} {}",
                "#".repeat(level as usize),
                escape_markdown_text(text)
            )
        }
        "list_item" => {
            let level = extra
                .and_then(|value| value.get("listLevel"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            let ordered = extra
                .and_then(|value| value.get("ordered"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let marker = extra
                .and_then(|value| value.get("listMarker"))
                .and_then(Value::as_str)
                .filter(|marker| valid_list_marker(marker, ordered))
                .unwrap_or(if ordered { "1." } else { "-" });
            format!(
                "{}{} {}",
                "    ".repeat(level.saturating_sub(1)),
                marker,
                escape_markdown_text(text)
            )
        }
        "caption" => format!("*{}*", escape_markdown_text(text)),
        "footnote" => format!("> {}", escape_markdown_text(text).replace('\n', "\n> ")),
        "metadata" if metadata_class(block) == Some("picture") => match asset_path(block) {
            Some(path) => format!("![Figure]({path})"),
            None => "**[Image unavailable]**".into(),
        },
        "code" if metadata_class(block) == Some("formula") => match asset_path(block) {
            Some(path) => format!("![Formula]({path})"),
            None => "**[Formula unavailable]**".into(),
        },
        _ => escape_markdown_text(text),
    }
}

fn render_table(
    blocks: &[RosettaBlock],
    text_by_block: &HashMap<String, String>,
) -> RenderedMarkdownBlock {
    let cells = blocks.iter().filter_map(table_cell).collect::<Vec<_>>();
    let markdown = if cells.len() != blocks.len() {
        render_invalid_table(blocks, text_by_block)
    } else if simple_table(&cells) {
        render_gfm_table(&cells, text_by_block)
    } else {
        render_html_table(&cells, text_by_block)
    };

    RenderedMarkdownBlock {
        block_ids: blocks.iter().map(|block| block.id.clone()).collect(),
        kind: "table".into(),
        markdown,
    }
}

fn render_invalid_table(
    blocks: &[RosettaBlock],
    text_by_block: &HashMap<String, String>,
) -> String {
    let mut output = String::from("<table>\n");
    for block in blocks {
        output.push_str("  <tr>\n    <td>");
        output.push_str(&escape_html_text(resolved_text(block, text_by_block)));
        output.push_str("</td>\n  </tr>\n");
    }
    output.push_str("</table>");
    output
}

fn simple_table(cells: &[TableCell<'_>]) -> bool {
    let Some(first) = cells.first() else {
        return false;
    };
    if first.row_count == 0 || first.column_count == 0 {
        return false;
    }
    let mut coordinates = HashSet::new();
    cells.len() == first.row_count.saturating_mul(first.column_count)
        && cells.iter().all(|cell| {
            cell.table_id == first.table_id
                && cell.row_count == first.row_count
                && cell.column_count == first.column_count
                && cell.row < first.row_count
                && cell.column < first.column_count
                && cell.row_span == 1
                && cell.column_span == 1
                && coordinates.insert((cell.row, cell.column))
        })
}

fn render_gfm_table(cells: &[TableCell<'_>], text_by_block: &HashMap<String, String>) -> String {
    let first = &cells[0];
    let by_coordinate = cells
        .iter()
        .map(|cell| ((cell.row, cell.column), cell))
        .collect::<HashMap<_, _>>();
    let mut lines = Vec::with_capacity(first.row_count + 1);
    for row in 0..first.row_count {
        let values = (0..first.column_count)
            .map(|column| {
                by_coordinate
                    .get(&(row, column))
                    .map(|cell| escape_table_text(resolved_text(cell.block, text_by_block)))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        lines.push(format!("| {} |", values.join(" | ")));
        if row == 0 {
            lines.push(format!(
                "| {} |",
                vec!["---"; first.column_count].join(" | ")
            ));
        }
    }
    lines.join("\n")
}

fn render_html_table(cells: &[TableCell<'_>], text_by_block: &HashMap<String, String>) -> String {
    let mut ordered = cells.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        (left.row, left.column, left.block.id.as_str()).cmp(&(
            right.row,
            right.column,
            right.block.id.as_str(),
        ))
    });
    let mut output = String::from("<table>\n");
    let mut active_row = None;
    for cell in ordered {
        if active_row != Some(cell.row) {
            if active_row.is_some() {
                output.push_str("  </tr>\n");
            }
            output.push_str("  <tr>\n");
            active_row = Some(cell.row);
        }
        let tag = if cell.header { "th" } else { "td" };
        output.push_str("    <");
        output.push_str(tag);
        if cell.row_span > 1 {
            output.push_str(&format!(" rowspan=\"{}\"", cell.row_span));
        }
        if cell.column_span > 1 {
            output.push_str(&format!(" colspan=\"{}\"", cell.column_span));
        }
        output.push('>');
        output.push_str(&escape_html_text(resolved_text(cell.block, text_by_block)));
        output.push_str("</");
        output.push_str(tag);
        output.push_str(">\n");
    }
    if active_row.is_some() {
        output.push_str("  </tr>\n");
    }
    output.push_str("</table>");
    output
}

fn table_cell(block: &RosettaBlock) -> Option<TableCell<'_>> {
    let extra = metadata_extra(block)?;
    Some(TableCell {
        block,
        table_id: extra.get("tableId")?.as_str()?.to_string(),
        row: extra.get("row")?.as_u64()?.try_into().ok()?,
        column: extra.get("column")?.as_u64()?.try_into().ok()?,
        row_span: extra.get("rowSpan")?.as_u64()?.try_into().ok()?,
        column_span: extra.get("columnSpan")?.as_u64()?.try_into().ok()?,
        header: extra.get("header")?.as_bool()?,
        row_count: extra.get("rowCount")?.as_u64()?.try_into().ok()?,
        column_count: extra.get("columnCount")?.as_u64()?.try_into().ok()?,
    })
}

fn metadata_extra(block: &RosettaBlock) -> Option<&Value> {
    block.style.as_ref()?.get("pdfMarkdown")?.get("extra")
}

fn metadata_class(block: &RosettaBlock) -> Option<&str> {
    block
        .style
        .as_ref()?
        .get("pdfMarkdown")?
        .get("boxClass")?
        .as_str()
}

fn asset_path(block: &RosettaBlock) -> Option<&str> {
    let path = metadata_extra(block)?.get("assetPath")?.as_str()?;
    let filename = path.strip_prefix("pdf-markdown/images/")?;
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    if filename.is_empty()
        || filename.contains('/')
        || !filename
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
        || path.contains('\\')
        || path.contains(':')
        || !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp")
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return None;
    }
    Some(path)
}

fn resolved_text<'a>(
    block: &'a RosettaBlock,
    text_by_block: &'a HashMap<String, String>,
) -> &'a str {
    text_by_block
        .get(&block.id)
        .map(String::as_str)
        .unwrap_or(&block.source_text)
}

fn valid_list_marker(marker: &str, ordered: bool) -> bool {
    if ordered {
        let number = marker.strip_suffix('.').unwrap_or("");
        !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
    } else {
        matches!(marker, "-" | "*" | "+")
    }
}

fn escape_markdown_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut escaped = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        match character {
            '\n' => escaped.push_str("<br>\n"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn escape_table_text(text: &str) -> String {
    escape_markdown_text(text).replace("<br>\n", "<br>")
}

fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rosetta_jobs::{
        export::render_export_blocks,
        model::{RosettaDocument, Segment, SCHEMA_VERSION},
    };
    use serde_json::json;

    fn block(id: &str, block_type: &str, class: &str, extra: Value, text: &str) -> RosettaBlock {
        RosettaBlock {
            id: id.into(),
            file_id: Some("file-1".into()),
            block_type: block_type.into(),
            source_text: text.into(),
            translated_text: None,
            should_translate: !matches!(class, "picture" | "formula"),
            order: 1,
            path: None,
            style: Some(
                json!({"pdfMarkdown":{"version":1,"page":1,"boxClass":class,"bbox":[0,0,10,10],"extra":extra}}),
            ),
            status: "pending".into(),
        }
    }

    fn cell(id: &str, row: usize, column: usize, row_span: usize, text: &str) -> RosettaBlock {
        block(
            id,
            "table_cell",
            "table",
            json!({
                "tableId":"table-1",
                "row":row,
                "column":column,
                "rowSpan":row_span,
                "columnSpan":1,
                "header":row == 0,
                "rowCount":2,
                "columnCount":2
            }),
            text,
        )
    }

    #[test]
    fn renders_structural_blocks_and_safe_asset_references() {
        let blocks = vec![
            block("h", "heading", "title", json!({"headingLevel":1}), "Title"),
            block(
                "l",
                "list_item",
                "list-item",
                json!({"listLevel":2,"listMarker":"3.","ordered":true}),
                "Item",
            ),
            block("c", "caption", "caption", json!({}), "Caption"),
            block("f", "footnote", "footnote", json!({}), "Footnote"),
            block(
                "p",
                "metadata",
                "picture",
                json!({"assetPath":"pdf-markdown/images/page-0001-picture-01.png"}),
                "",
            ),
            block(
                "m",
                "code",
                "formula",
                json!({"assetPath":"../formula.png"}),
                "",
            ),
        ];
        let rendered = join_blocks(&render_blocks(&blocks, &HashMap::new()));
        assert_eq!(rendered, "# Title\n\n    3. Item\n\n*Caption*\n\n> Footnote\n\n![Figure](pdf-markdown/images/page-0001-picture-01.png)\n\n**[Formula unavailable]**");
    }

    #[test]
    fn rectangular_tables_use_gfm_and_escape_translated_delimiters() {
        let blocks = vec![
            cell("a", 0, 0, 1, "Name"),
            cell("b", 0, 1, 1, "Value"),
            cell("c", 1, 0, 1, "Alpha"),
            cell("d", 1, 1, 1, "One"),
        ];
        let translated = HashMap::from([("d".to_string(), "A | B".to_string())]);
        let rendered = join_blocks(&render_blocks(&blocks, &translated));
        assert_eq!(
            rendered,
            "| Name | Value |\n| --- | --- |\n| Alpha | A \\| B |"
        );
    }

    #[test]
    fn spanned_or_unsafe_tables_use_deterministic_inline_html() {
        let blocks = vec![
            cell("a", 0, 0, 2, "A & B"),
            cell("b", 0, 1, 1, "Header"),
            cell("d", 1, 1, 1, "<value>"),
        ];
        let rendered = join_blocks(&render_blocks(&blocks, &HashMap::new()));
        assert_eq!(rendered, "<table>\n  <tr>\n    <th rowspan=\"2\">A &amp; B</th>\n    <th>Header</th>\n  </tr>\n  <tr>\n    <td>&lt;value&gt;</td>\n  </tr>\n</table>");
    }

    #[test]
    fn pdf_source_uses_ordinary_segment_translation_for_markdown_output() {
        let blocks = vec![
            block(
                "heading",
                "heading",
                "title",
                json!({"headingLevel":1}),
                "Source title",
            ),
            block(
                "picture",
                "metadata",
                "picture",
                json!({"assetPath":"pdf-markdown/images/page-0001-picture-01.png"}),
                "",
            ),
        ];
        let segments = vec![Segment {
            id: "segment-1".into(),
            block_id: "heading".into(),
            file_id: Some("file-1".into()),
            order: 1,
            source_text: "Source title".into(),
            translated_text: Some("Translated title".into()),
            source_lang: Some("en".into()),
            target_lang: "zh-CN".into(),
            kind: "heading".into(),
            preserve_whitespace: true,
            status: "done".into(),
            block_order: Some(1),
            segment_index_in_block: Some(0),
            error: None,
            translation_history: Vec::new(),
        }];
        let document = RosettaDocument {
            schema_version: SCHEMA_VERSION,
            id: "doc".into(),
            filename: "source.pdf".into(),
            format: "pdf".into(),
            source_lang: Some("en".into()),
            target_lang: "zh-CN".into(),
            files: Vec::new(),
            blocks: blocks.clone(),
            extraction_status: None,
        };

        let output = render_export_blocks(&document, &blocks, &segments, "translation", "markdown");
        assert_eq!(
            output,
            "# Translated title\n\n![Figure](pdf-markdown/images/page-0001-picture-01.png)"
        );
    }
}
