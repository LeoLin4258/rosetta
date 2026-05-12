use std::collections::HashMap;

use crate::rosetta_jobs::model::{
    default_file_translation_status, RosettaBlock, RosettaDocument, RosettaJobSummary,
    RosettaSourceFile, Segment,
};

pub(crate) fn document_files(document: &RosettaDocument) -> Vec<RosettaSourceFile> {
    if !document.files.is_empty() {
        return document.files.clone();
    }

    vec![RosettaSourceFile {
        id: "file-1".to_string(),
        filename: document.filename.clone(),
        relative_path: document.filename.clone(),
        format: document.format.clone(),
        source_lang: document.source_lang.clone(),
        target_lang: Some(document.target_lang.clone()),
        translation_status: default_file_translation_status(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
        translating_segments: 0,
        block_ids: document
            .blocks
            .iter()
            .map(|block| block.id.clone())
            .collect(),
    }]
}

pub(crate) fn ensure_document_files(document: &mut RosettaDocument) {
    if document.files.is_empty() {
        document.files = document_files(document);
    }
}

pub(crate) fn effective_file_source_lang(
    source_file: &RosettaSourceFile,
    document: &RosettaDocument,
) -> Option<String> {
    source_file
        .source_lang
        .clone()
        .or_else(|| document.source_lang.clone())
}

pub(crate) fn effective_file_target_lang(
    source_file: &RosettaSourceFile,
    document: &RosettaDocument,
) -> String {
    source_file
        .target_lang
        .clone()
        .unwrap_or_else(|| document.target_lang.clone())
}

pub(crate) fn sync_document_default_language_from_files(document: &mut RosettaDocument) {
    let files = document_files(document);
    let Some(first_file) = files.first() else {
        return;
    };

    let first_source_lang = effective_file_source_lang(first_file, document);
    if files
        .iter()
        .all(|file| effective_file_source_lang(file, document) == first_source_lang)
    {
        document.source_lang = first_source_lang;
    }

    let first_target_lang = effective_file_target_lang(first_file, document);
    if files
        .iter()
        .all(|file| effective_file_target_lang(file, document) == first_target_lang)
    {
        document.target_lang = first_target_lang;
    }
}

pub(crate) fn sync_document_file_statuses(document: &mut RosettaDocument, segments: &[Segment]) {
    let mut stats_by_file: HashMap<String, (usize, usize, usize, usize)> = HashMap::new();
    for segment in segments {
        if segment.status == "skipped" || segment.source_text.trim().is_empty() {
            continue;
        }
        let file_id = segment.file_id.as_deref().unwrap_or("file-1").to_string();
        let entry = stats_by_file.entry(file_id).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        if matches!(segment.status.as_str(), "done" | "edited") {
            entry.1 += 1;
        }
        if segment.status == "failed" {
            entry.2 += 1;
        }
        if segment.status == "translating" {
            entry.3 += 1;
        }
    }

    for source_file in &mut document.files {
        let (segment_count, completed_segments, failed_segments, translating_segments) =
            stats_by_file
                .get(&source_file.id)
                .copied()
                .unwrap_or((0, 0, 0, 0));
        source_file.segment_count = segment_count;
        source_file.completed_segments = completed_segments;
        source_file.failed_segments = failed_segments;
        source_file.translating_segments = translating_segments;
        source_file.translation_status = if translating_segments > 0 {
            "translating".to_string()
        } else if failed_segments > 0 {
            "failed".to_string()
        } else if segment_count > 0 && completed_segments == segment_count {
            "translated".to_string()
        } else {
            "untranslated".to_string()
        };
    }
}

pub(crate) fn sync_job_source_files(job: &mut RosettaJobSummary, document: &RosettaDocument) {
    job.source_files = document.files.clone();
    job.file_count = document.files.len();
}

pub(crate) fn segments_by_block(segments: &[Segment]) -> HashMap<String, Vec<Segment>> {
    let mut grouped: HashMap<String, Vec<Segment>> = HashMap::new();
    for segment in segments {
        grouped
            .entry(segment.block_id.clone())
            .or_default()
            .push(segment.clone());
    }
    for grouped_segments in grouped.values_mut() {
        grouped_segments.sort_by_key(|segment| segment.segment_index_in_block.unwrap_or(0));
    }
    grouped
}

pub(crate) fn block_translation(
    block: &RosettaBlock,
    by_block: &HashMap<String, Vec<Segment>>,
    target_lang: &str,
) -> String {
    let Some(segments) = by_block.get(&block.id) else {
        return block.source_text.clone();
    };
    let translated = segments
        .iter()
        .map(|segment| {
            segment
                .translated_text
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or(&segment.source_text)
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(segment_joiner(
            segments
                .first()
                .map(|segment| segment.target_lang.as_str())
                .unwrap_or(target_lang),
        ));

    if translated.trim().is_empty() {
        block.source_text.clone()
    } else {
        translated
    }
}

fn segment_joiner(target_lang: &str) -> &'static str {
    if is_compact_language(target_lang) {
        ""
    } else {
        " "
    }
}

fn is_compact_language(target_lang: &str) -> bool {
    target_lang.starts_with("zh") || target_lang.starts_with("ja") || target_lang.starts_with("ko")
}

pub(crate) fn apply_segment_translations_to_document(
    document: &mut RosettaDocument,
    segments: &[Segment],
) {
    let by_block = segments_by_block(segments);
    for block in &mut document.blocks {
        if !block.should_translate {
            continue;
        }
        block.translated_text = Some(block_translation(block, &by_block, &document.target_lang));
        block.status = block_status(block, &by_block);
    }
}

pub(crate) fn block_status(
    block: &RosettaBlock,
    by_block: &HashMap<String, Vec<Segment>>,
) -> String {
    let Some(segments) = by_block.get(&block.id) else {
        return "pending".to_string();
    };
    if segments.iter().any(|segment| segment.status == "failed") {
        "failed".to_string()
    } else if segments.iter().all(|segment| segment.status == "done") {
        "done".to_string()
    } else if segments
        .iter()
        .any(|segment| segment.status == "translating")
    {
        "translating".to_string()
    } else {
        "pending".to_string()
    }
}

pub(crate) fn sync_job_counts(job: &mut RosettaJobSummary, segments: &[Segment]) {
    job.segment_count = segments.len();
    job.completed_segments = segments
        .iter()
        .filter(|segment| matches!(segment.status.as_str(), "done" | "edited" | "skipped"))
        .count();
    job.failed_segments = segments
        .iter()
        .filter(|segment| segment.status == "failed")
        .count();
    job.status = if segments
        .iter()
        .any(|segment| segment.status == "translating")
    {
        "translating".to_string()
    } else if job.failed_segments > 0 {
        "failed".to_string()
    } else if job.completed_segments == job.segment_count {
        "completed".to_string()
    } else {
        "ready".to_string()
    };
}
