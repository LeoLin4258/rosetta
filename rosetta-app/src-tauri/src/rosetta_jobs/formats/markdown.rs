use serde_json::{json, Value};

use crate::rosetta_jobs::{
    model::{RosettaBlock, Segment},
    segmenter::{push_segments_for_block, translatable_block},
};

pub(crate) fn parse_markdown(
    document_id: &str,
    contents: &str,
) -> (Vec<RosettaBlock>, Vec<Segment>) {
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut block_order = 1;
    let mut segment_order = 1;
    let mut paragraph_lines: Vec<String> = Vec::new();
    let mut code_lines: Vec<String> = Vec::new();
    let mut in_code_block = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            code_lines.push(line.to_string());
            if in_code_block {
                push_skipped_markdown_block(
                    document_id,
                    &mut blocks,
                    &mut block_order,
                    "code",
                    &code_lines.join("\n"),
                    None,
                );
                code_lines.clear();
            }
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        if trimmed.is_empty() {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_skipped_markdown_block(
                document_id,
                &mut blocks,
                &mut block_order,
                "metadata",
                "",
                Some(json!({"markdownKind": "blank"})),
            );
            continue;
        }

        if let Some((marker, text)) = parse_heading(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "heading",
                text,
                json!({"marker": marker}),
            );
            continue;
        }

        if let Some((marker, text)) = parse_list_item(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "list_item",
                text,
                json!({"marker": marker}),
            );
            continue;
        }

        if let Some(text) = parse_blockquote(line) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_markdown_translatable(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                "blockquote",
                text,
                json!({"marker": ">"}),
            );
            continue;
        }

        if is_plain_url(trimmed) {
            flush_markdown_paragraph(
                document_id,
                &mut blocks,
                &mut segments,
                &mut block_order,
                &mut segment_order,
                &mut paragraph_lines,
            );
            push_skipped_markdown_block(
                document_id,
                &mut blocks,
                &mut block_order,
                "metadata",
                line,
                Some(json!({"markdownKind": "url"})),
            );
            continue;
        }

        paragraph_lines.push(line.to_string());
    }

    if in_code_block && !code_lines.is_empty() {
        push_skipped_markdown_block(
            document_id,
            &mut blocks,
            &mut block_order,
            "code",
            &code_lines.join("\n"),
            None,
        );
    }

    flush_markdown_paragraph(
        document_id,
        &mut blocks,
        &mut segments,
        &mut block_order,
        &mut segment_order,
        &mut paragraph_lines,
    );

    (blocks, segments)
}

fn flush_markdown_paragraph(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    segments: &mut Vec<Segment>,
    block_order: &mut usize,
    segment_order: &mut usize,
    paragraph_lines: &mut Vec<String>,
) {
    if paragraph_lines.is_empty() {
        return;
    }

    let text = paragraph_lines.join("\n");
    paragraph_lines.clear();
    push_markdown_translatable(
        document_id,
        blocks,
        segments,
        block_order,
        segment_order,
        "paragraph",
        &text,
        json!({"markdownKind": "paragraph"}),
    );
}

fn push_markdown_translatable(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    segments: &mut Vec<Segment>,
    block_order: &mut usize,
    segment_order: &mut usize,
    block_type: &str,
    text: &str,
    style: Value,
) {
    let block_id = format!("{document_id}-block-{block_order}");
    blocks.push(translatable_block(
        &block_id,
        block_type,
        text,
        *block_order,
        Some(style),
    ));
    push_segments_for_block(
        segments,
        &block_id,
        block_type,
        *block_order,
        text,
        segment_order,
    );
    *block_order += 1;
}

fn push_skipped_markdown_block(
    document_id: &str,
    blocks: &mut Vec<RosettaBlock>,
    block_order: &mut usize,
    block_type: &str,
    source_text: &str,
    style: Option<Value>,
) {
    let block_id = format!("{document_id}-block-{block_order}");
    blocks.push(RosettaBlock {
        id: block_id,
        file_id: None,
        block_type: block_type.to_string(),
        source_text: source_text.to_string(),
        translated_text: None,
        should_translate: false,
        order: *block_order,
        path: Some(format!("blocks.{}", *block_order)),
        style,
        status: "skipped".to_string(),
    });
    *block_order += 1;
}

fn parse_heading(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some(("#".repeat(hashes), rest.trim()))
}

fn parse_list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = trimmed.strip_prefix(marker) {
            return Some((marker.trim_end().to_string(), text.trim()));
        }
    }

    let dot_index = trimmed.find(". ")?;
    if trimmed[..dot_index]
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return Some((
            trimmed[..=dot_index].to_string(),
            trimmed[dot_index + 2..].trim(),
        ));
    }

    None
}

fn parse_blockquote(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix('>').map(str::trim)
}

fn is_plain_url(text: &str) -> bool {
    (text.starts_with("http://") || text.starts_with("https://"))
        && !text.chars().any(char::is_whitespace)
}
