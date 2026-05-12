use std::collections::HashMap;

use serde_json::Value;

use crate::rosetta_jobs::model::{RosettaBlock, Segment, MAX_SEGMENT_CHARS};

pub(crate) fn apply_file_id(blocks: &mut [RosettaBlock], segments: &mut [Segment], file_id: &str) {
    for block in blocks {
        block.file_id = Some(file_id.to_string());
    }
    for segment in segments {
        segment.file_id = Some(file_id.to_string());
    }
}

pub(crate) fn renumber_blocks_and_segments(
    blocks: &mut [RosettaBlock],
    segments: &mut [Segment],
    next_block_order: &mut usize,
    next_segment_order: &mut usize,
) {
    let mut block_order_by_id = HashMap::new();
    for block in blocks {
        block.order = *next_block_order;
        block.path = Some(format!("blocks.{}", block.order));
        block_order_by_id.insert(block.id.clone(), block.order);
        *next_block_order += 1;
    }

    segments.sort_by_key(|segment| (segment.block_order.unwrap_or(0), segment.order));
    for segment in segments {
        segment.order = *next_segment_order;
        segment.block_order = block_order_by_id.get(&segment.block_id).copied();
        *next_segment_order += 1;
    }
}

pub(crate) fn translatable_block(
    id: &str,
    block_type: &str,
    source_text: &str,
    order: usize,
    style: Option<Value>,
) -> RosettaBlock {
    RosettaBlock {
        id: id.to_string(),
        file_id: None,
        block_type: block_type.to_string(),
        source_text: source_text.to_string(),
        translated_text: None,
        should_translate: true,
        order,
        path: Some(format!("blocks.{order}")),
        style,
        status: "pending".to_string(),
    }
}

pub(crate) fn push_segments_for_block(
    segments: &mut Vec<Segment>,
    block_id: &str,
    kind: &str,
    block_order: usize,
    source_text: &str,
    segment_order: &mut usize,
) {
    for (index, chunk) in split_long_text(source_text).into_iter().enumerate() {
        segments.push(Segment {
            id: format!("{block_id}-segment-{}", index + 1),
            block_id: block_id.to_string(),
            file_id: None,
            order: *segment_order,
            source_text: chunk,
            translated_text: None,
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            kind: kind.to_string(),
            preserve_whitespace: true,
            status: "pending".to_string(),
            block_order: Some(block_order),
            segment_index_in_block: Some(index),
            error: None,
            translation_history: Vec::new(),
        });
        *segment_order += 1;
    }
}

pub(crate) fn split_long_text(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_SEGMENT_CHARS {
        return vec![text.trim().to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in split_sentence_like(text) {
        let next_len = current.chars().count() + sentence.chars().count();
        if !current.is_empty() && next_len > MAX_SEGMENT_CHARS {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sentence.trim());
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks
    }
}

pub(crate) fn split_sentence_like(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;

    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '?' | '!' | ';' | '。' | '？' | '！' | '；') {
            let end = index + character.len_utf8();
            if start < end {
                sentences.push(&text[start..end]);
            }
            start = end;
        }
    }

    if start < text.len() {
        sentences.push(&text[start..]);
    }

    sentences
}
