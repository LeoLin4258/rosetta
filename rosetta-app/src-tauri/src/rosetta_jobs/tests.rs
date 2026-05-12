use std::{collections::HashMap, fs, path::PathBuf};

use crate::rosetta_jobs::{
    export::*,
    formats::{markdown::parse_markdown, txt::parse_txt},
    model::*,
    path::*,
    revisions::*,
    segmenter::{split_long_text, translatable_block},
    store::{read_translation_revisions, write_translation_revisions},
    translation_files::*,
};

#[test]
fn txt_parser_splits_blank_line_paragraphs() {
    let (_document, segments) = parse_txt("doc", "One.\n\nTwo.\nStill two.");

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].source_text, "One.");
    assert_eq!(segments[1].source_text, "Two.\nStill two.");
    assert_eq!(segments[0].order, 1);
    assert_eq!(segments[1].order, 2);
}

#[test]
fn markdown_parser_recognizes_basic_blocks_and_skips_code() {
    let (blocks, segments) = parse_markdown(
        "doc",
        "# Title\n\nParagraph.\n\n- Item\n\n```rust\nfn main() {}\n```",
    );

    assert!(blocks.iter().any(|block| block.block_type == "heading"));
    assert!(blocks.iter().any(|block| block.block_type == "list_item"));
    assert!(blocks
        .iter()
        .any(|block| block.block_type == "code" && block.status == "skipped"));
    assert_eq!(segments.len(), 3);
}

#[test]
fn long_segmenter_splits_sentence_like_text() {
    let text = format!("{}.", "a".repeat(MAX_SEGMENT_CHARS));
    let text = format!("{text} {}.", "b".repeat(50));
    let chunks = split_long_text(&text);

    assert_eq!(chunks.len(), 2);
}

#[test]
fn export_translation_uses_original_for_untranslated_segments() {
    let (document, segments) = parse_txt("doc", "Hello.");
    let output = render_export(
        &RosettaDocument {
            schema_version: SCHEMA_VERSION,
            id: "doc".to_string(),
            filename: "demo.txt".to_string(),
            format: "txt".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            files: Vec::new(),
            blocks: document,
        },
        &segments,
        "translation",
    );

    assert_eq!(output, "Hello.");
}

#[test]
fn export_markdown_preserves_heading_marker() {
    let (document, mut segments) = parse_markdown("doc", "# Title");
    segments[0].translated_text = Some("标题".to_string());
    segments[0].status = "done".to_string();
    let output = render_export(
        &RosettaDocument {
            schema_version: SCHEMA_VERSION,
            id: "doc".to_string(),
            filename: "demo.md".to_string(),
            format: "markdown".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            files: Vec::new(),
            blocks: document,
        },
        &segments,
        "translation",
    );

    assert_eq!(output, "# 标题");
}

#[test]
fn file_export_rejects_pending_segments() {
    let segment = test_segment("pending", None);

    let result = ensure_file_ready_for_export(&[segment]);

    assert!(result.is_err());
}

#[test]
fn file_export_rejects_empty_translation() {
    let segment = test_segment("done", Some(""));

    let result = ensure_file_ready_for_export(&[segment]);

    assert!(result.is_err());
}

#[test]
fn file_export_allows_done_and_skipped_segments() {
    let translated = test_segment("done", Some("你好。"));
    let skipped = Segment {
        status: "skipped".to_string(),
        source_text: "```rust\nfn main() {}\n```".to_string(),
        translated_text: None,
        ..test_segment("skipped", None)
    };

    assert!(ensure_file_ready_for_export(&[translated, skipped]).is_ok());
}

#[test]
fn archive_segment_translation_preserves_current_translation() {
    let mut segment = test_segment("done", Some("你好。"));

    archive_segment_translation(&mut segment, "retranslation", "run-test");

    assert_eq!(segment.translation_history.len(), 1);
    assert_eq!(segment.translation_history[0].translated_text, "你好。");
    assert_eq!(segment.translation_history[0].reason, "retranslation");
    assert_eq!(
        segment.translation_history[0].run_id.as_deref(),
        Some("run-test")
    );
}

#[test]
fn missing_translation_revisions_file_returns_empty() {
    let dir = unique_temp_dir("missing-revisions");
    fs::create_dir_all(&dir).expect("create temp dir");

    let revisions = read_translation_revisions(&dir).expect("read revisions");

    assert!(revisions.is_empty());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn translation_revision_roundtrip_restores_snapshot() {
    let dir = unique_temp_dir("revision-roundtrip");
    fs::create_dir_all(&dir).expect("create temp dir");
    let mut translations = HashMap::new();
    translations.insert("segment-1".to_string(), "你好。".to_string());
    let revision = TranslationRevision {
        id: "revision-1".to_string(),
        job_id: "job-1".to_string(),
        file_id: "file-1".to_string(),
        created_at: "1".to_string(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        reason: "file-retranslation".to_string(),
        scope_block_ids: None,
        segment_translations: translations,
    };

    write_translation_revisions(&dir, &[revision]).expect("write revisions");
    let restored = read_translation_revisions(&dir).expect("read revisions");

    assert_eq!(restored.len(), 1);
    assert_eq!(
        restored[0].segment_translations.get("segment-1"),
        Some(&"你好。".to_string())
    );
    fs::remove_dir_all(dir).ok();
}

#[test]
fn revision_snapshot_contains_current_file_translations() {
    let document = test_document();
    let segments = vec![
        test_segment_with("segment-1", "block-1", "file-1", "done", Some("你好。")),
        test_segment_with("segment-2", "block-2", "file-1", "done", Some("世界。")),
        test_segment_with("segment-3", "block-3", "file-2", "done", Some("忽略。")),
    ];

    let revision = create_revision_snapshot(
        "job-1",
        "file-1",
        "file-retranslation",
        None,
        &document,
        &segments,
    )
    .expect("create snapshot")
    .expect("snapshot exists");

    assert_eq!(revision.file_id, "file-1");
    assert_eq!(revision.segment_translations.len(), 2);
    assert_eq!(
        revision.segment_translations.get("segment-1"),
        Some(&"你好。".to_string())
    );
    assert!(!revision.segment_translations.contains_key("segment-3"));
}

#[test]
fn empty_translation_snapshot_is_not_created() {
    let document = test_document();
    let segments = vec![test_segment_with(
        "segment-1",
        "block-1",
        "file-1",
        "pending",
        None,
    )];

    let revision = create_revision_snapshot(
        "job-1",
        "file-1",
        "file-retranslation",
        None,
        &document,
        &segments,
    )
    .expect("create snapshot");

    assert!(revision.is_none());
}

#[test]
fn export_translation_uses_current_segments_not_revision_history() {
    let (blocks, mut segments) = parse_txt("doc", "Hello.");
    segments[0].translated_text = Some("当前译文。".to_string());
    segments[0].status = "done".to_string();
    segments[0]
        .translation_history
        .push(TranslationHistoryEntry {
            id: "history-1".to_string(),
            run_id: Some("run-1".to_string()),
            translated_text: "旧译文。".to_string(),
            created_at: "1".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            reason: "retranslation".to_string(),
        });
    let output = render_export(
        &RosettaDocument {
            schema_version: SCHEMA_VERSION,
            id: "doc".to_string(),
            filename: "demo.txt".to_string(),
            format: "txt".to_string(),
            source_lang: Some("en".to_string()),
            target_lang: "zh-CN".to_string(),
            files: Vec::new(),
            blocks,
        },
        &segments,
        "translation",
    );

    assert_eq!(output, "当前译文。");
}

#[test]
fn legacy_segments_migrate_to_translation_file() {
    let dir = unique_temp_dir("translation-file-migration");
    fs::create_dir_all(&dir).expect("create temp dir");
    let document = test_document();
    let segments = vec![
        test_segment_with("segment-1", "block-1", "file-1", "done", Some("你好。")),
        test_segment_with("segment-2", "block-2", "file-1", "pending", None),
        test_segment_with("segment-3", "block-3", "file-2", "done", Some("忽略。")),
    ];

    let translation_files =
        read_or_migrate_translation_files(&dir, &document, &segments).expect("migrate");

    assert_eq!(translation_files.len(), 2);
    let file_one = translation_files
        .iter()
        .find(|file| file.source_file_id == "file-1")
        .expect("file-1 translation file");
    assert_eq!(file_one.target_lang, "zh-CN");
    assert_eq!(file_one.segment_count, 2);
    assert_eq!(file_one.completed_segments, 1);
    let restored = read_translation_segments(&dir, &file_one.id).expect("read migrated segments");
    assert_eq!(restored[0].translated_text.as_deref(), Some("你好。"));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn translation_file_export_uses_selected_translation_segments() {
    let mut document = test_document();
    for block in &mut document.blocks {
        block.file_id = if block.id == "block-3" {
            Some("file-2".to_string())
        } else {
            Some("file-1".to_string())
        };
    }
    let source_segments = vec![
        test_segment_with("segment-1", "block-1", "file-1", "pending", None),
        test_segment_with("segment-2", "block-2", "file-1", "pending", None),
        test_segment_with("segment-3", "block-3", "file-2", "done", Some("忽略。")),
    ];
    let translation_segments = vec![
        translation_segment("segment-1", "ja", "done", Some("こんにちは。")),
        translation_segment("segment-2", "ja", "done", Some("世界。")),
    ];
    let file_blocks = document
        .blocks
        .iter()
        .filter(|block| block.file_id.as_deref().unwrap_or("file-1") == "file-1")
        .cloned()
        .collect::<Vec<_>>();
    let export_segments =
        translated_source_segments(&source_segments, &translation_segments, "file-1", "ja");

    ensure_translation_file_ready_for_export(&translation_segments)
        .expect("translation file should export");
    let output = render_export_blocks(
        &document,
        &file_blocks,
        &export_segments,
        "translation",
        "txt",
    );

    assert_eq!(output, "こんにちは。\n\n世界。");
}

#[test]
fn path_safety_rejects_traversal_job_id() {
    assert!(is_safe_job_id("job-123_demo"));
    assert!(!is_safe_job_id("../job"));
    assert!(!is_safe_job_id("job/123"));
}

fn test_segment(status: &str, translated_text: Option<&str>) -> Segment {
    Segment {
        id: "segment-1".to_string(),
        block_id: "block-1".to_string(),
        file_id: Some("file-1".to_string()),
        order: 1,
        source_text: "Hello.".to_string(),
        translated_text: translated_text.map(str::to_string),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        kind: "paragraph".to_string(),
        preserve_whitespace: false,
        status: status.to_string(),
        block_order: Some(1),
        segment_index_in_block: Some(0),
        error: None,
        translation_history: Vec::new(),
    }
}

fn test_segment_with(
    id: &str,
    block_id: &str,
    file_id: &str,
    status: &str,
    translated_text: Option<&str>,
) -> Segment {
    Segment {
        id: id.to_string(),
        block_id: block_id.to_string(),
        file_id: Some(file_id.to_string()),
        order: 1,
        source_text: "Hello.".to_string(),
        translated_text: translated_text.map(str::to_string),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        kind: "paragraph".to_string(),
        preserve_whitespace: false,
        status: status.to_string(),
        block_order: Some(1),
        segment_index_in_block: Some(0),
        error: None,
        translation_history: Vec::new(),
    }
}

fn translation_segment(
    source_segment_id: &str,
    target_lang: &str,
    status: &str,
    translated_text: Option<&str>,
) -> TranslationSegment {
    TranslationSegment {
        source_segment_id: source_segment_id.to_string(),
        translated_text: translated_text.map(str::to_string),
        target_lang: target_lang.to_string(),
        status: status.to_string(),
        error: None,
        translation_history: Vec::new(),
    }
}

fn test_document() -> RosettaDocument {
    RosettaDocument {
        schema_version: SCHEMA_VERSION,
        id: "document-1".to_string(),
        filename: "demo".to_string(),
        format: "txt".to_string(),
        source_lang: Some("en".to_string()),
        target_lang: "zh-CN".to_string(),
        files: vec![
            RosettaSourceFile {
                id: "file-1".to_string(),
                filename: "one.txt".to_string(),
                relative_path: "one.txt".to_string(),
                format: "txt".to_string(),
                source_lang: Some("en".to_string()),
                target_lang: Some("zh-CN".to_string()),
                translation_status: default_file_translation_status(),
                segment_count: 0,
                completed_segments: 0,
                failed_segments: 0,
                translating_segments: 0,
                block_ids: vec!["block-1".to_string(), "block-2".to_string()],
            },
            RosettaSourceFile {
                id: "file-2".to_string(),
                filename: "two.txt".to_string(),
                relative_path: "two.txt".to_string(),
                format: "txt".to_string(),
                source_lang: Some("en".to_string()),
                target_lang: Some("zh-CN".to_string()),
                translation_status: default_file_translation_status(),
                segment_count: 0,
                completed_segments: 0,
                failed_segments: 0,
                translating_segments: 0,
                block_ids: vec!["block-3".to_string()],
            },
        ],
        blocks: vec![
            translatable_block("block-1", "paragraph", "Hello.", 1, None),
            translatable_block("block-2", "paragraph", "World.", 2, None),
            translatable_block("block-3", "paragraph", "Ignored.", 3, None),
        ],
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rosetta-{name}-{}", timestamp_ms_string()))
}
