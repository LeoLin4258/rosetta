use crate::rosetta_jobs::{
    model::{RosettaBlock, Segment},
    segmenter::{push_segments_for_block, translatable_block},
};

pub(crate) fn parse_txt(document_id: &str, contents: &str) -> (Vec<RosettaBlock>, Vec<Segment>) {
    let mut blocks = Vec::new();
    let mut segments = Vec::new();
    let mut block_order = 1;
    let mut segment_order = 1;

    for paragraph in split_txt_paragraphs(contents) {
        let block_id = format!("{document_id}-block-{block_order}");
        blocks.push(translatable_block(
            &block_id,
            "paragraph",
            &paragraph,
            block_order,
            None,
        ));
        push_segments_for_block(
            &mut segments,
            &block_id,
            "paragraph",
            block_order,
            &paragraph,
            &mut segment_order,
        );
        block_order += 1;
    }

    (blocks, segments)
}

fn split_txt_paragraphs(contents: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();

    for line in contents.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join("\n").trim().to_string());
                current.clear();
            }
        } else {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        paragraphs.push(current.join("\n").trim().to_string());
    }

    paragraphs
}
