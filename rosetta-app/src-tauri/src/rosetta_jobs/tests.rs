use std::{collections::HashMap, fs, path::PathBuf};

use crate::managed_pdf2zh::openai_shim::{
    LightningApiConfig, LlamaCppApiConfig, ShimProviderConfig,
};

use crate::rosetta_jobs::{
    document::{
        sync_document_file_statuses, sync_document_file_translation_statuses,
        sync_job_counts_from_source_files, sync_job_source_files,
    },
    export::*,
    formats::pdf::page_artifact_compression,
    formats::pdf::page_assemble::{
        assemble_pdf_with_page_translations, count_pdf_pages_lopdf, extract_pages_pdf,
    },
    formats::pdf::page_state::*,
    formats::pdf::run_state::*,
    formats::pdf::test_helpers::fixture_path,
    formats::{markdown::parse_markdown, txt::parse_txt},
    import::{build_blank_txt_bundle, rebuild_txt_source_file},
    model::*,
    path::*,
    revisions::*,
    segmenter::{split_long_text, translatable_block},
    store::{
        cleanup_pdf_translation_artifacts, read_translation_revisions, write_translation_revisions,
    },
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
fn blank_txt_bundle_uses_txt_format_and_starts_empty() {
    let bundle = build_blank_txt_bundle("job-txt-1", "1700000000000", "临时文本")
        .expect("build blank txt bundle");

    assert_eq!(bundle.document.format, "txt");
    assert_eq!(bundle.job.format, "txt");
    assert_eq!(bundle.job.source_kind, "file");
    assert_eq!(bundle.job.source_path, None);
    assert_eq!(bundle.document.files[0].format, "txt");
    assert_eq!(bundle.document.files[0].relative_path, "临时文本.txt");
    assert!(bundle.document.blocks.is_empty());
    assert!(bundle.segments.is_empty());
}

#[test]
fn txt_source_edit_rebuilds_blocks_and_segments() {
    let mut bundle = build_blank_txt_bundle("job-txt-1", "1700000000000", "临时文本")
        .expect("build blank txt bundle");

    rebuild_txt_source_file(
        &mut bundle.document,
        &mut bundle.segments,
        "file-1",
        "First paragraph.\n\nSecond paragraph.",
    )
    .expect("rebuild txt source");

    assert_eq!(bundle.document.blocks.len(), 2);
    assert_eq!(bundle.segments.len(), 2);
    assert_eq!(bundle.segments[0].source_text, "First paragraph.");
    assert_eq!(bundle.segments[1].source_text, "Second paragraph.");
    assert_eq!(bundle.document.files[0].block_ids.len(), 2);
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
            extraction_status: None,
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
            extraction_status: None,
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
            extraction_status: None,
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
fn missing_pdf_translation_segments_file_repairs_to_empty_compat_file() {
    let dir = unique_temp_dir("pdf-translation-segments-repair");
    fs::create_dir_all(&dir).expect("create temp dir");
    let mut document = test_document();
    document.format = "pdf".to_string();
    document.filename = "demo.pdf".to_string();
    document.files.truncate(1);
    document.files[0].filename = "demo.pdf".to_string();
    document.files[0].relative_path = "demo.pdf".to_string();
    document.files[0].format = "pdf".to_string();
    document.files[0].block_ids.clear();
    let translation_file = RosettaTranslationFile {
        id: translation_file_id("file-1", "zh-CN"),
        source_file_id: "file-1".to_string(),
        target_lang: "zh-CN".to_string(),
        status: "untranslated".to_string(),
        segment_count: 60,
        completed_segments: 0,
        failed_segments: 0,
        updated_at: "2000".to_string(),
        exported_at: None,
    };
    let path = translation_segments_path(&dir, &translation_file.id).expect("translation path");
    assert!(!path.exists());

    let segments = read_translation_segments_or_repair_pdf(&dir, &document, &translation_file)
        .expect("repair PDF compat translation file");

    assert!(segments.is_empty());
    assert!(path.is_file());
    let persisted = read_translation_segments(&dir, &translation_file.id)
        .expect("read repaired PDF compat translation file");
    assert!(persisted.is_empty());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn translation_file_status_drives_job_sidebar_summary() {
    let mut document = test_document();
    let segments = vec![
        test_segment_with("segment-1", "block-1", "file-1", "pending", None),
        test_segment_with("segment-2", "block-2", "file-1", "pending", None),
        test_segment_with("segment-3", "block-3", "file-2", "pending", None),
    ];
    let translation_files = vec![RosettaTranslationFile {
        id: translation_file_id("file-1", "zh-CN"),
        source_file_id: "file-1".to_string(),
        target_lang: "zh-CN".to_string(),
        status: "translated".to_string(),
        segment_count: 2,
        completed_segments: 2,
        failed_segments: 0,
        updated_at: "2000".to_string(),
        exported_at: None,
    }];
    let mut job = RosettaJobSummary {
        schema_version: SCHEMA_VERSION,
        id: "job-1".to_string(),
        filename: "fixture".to_string(),
        format: "txt".to_string(),
        source_path: None,
        source_filename: "fixture".to_string(),
        source_kind: default_source_kind(),
        file_count: 2,
        source_files: document.files.clone(),
        status: "ready".to_string(),
        created_at: "1000".to_string(),
        updated_at: "1000".to_string(),
        exported_at: None,
        last_error: None,
        target_lang: "zh-CN".to_string(),
        segment_count: 0,
        completed_segments: 0,
        failed_segments: 0,
    };

    sync_document_file_statuses(&mut document, &segments);
    sync_document_file_translation_statuses(&mut document, &translation_files);
    sync_job_source_files(&mut job, &document);
    sync_job_counts_from_source_files(&mut job);

    assert_eq!(job.source_files[0].translation_status, "translated");
    assert_eq!(job.source_files[0].completed_segments, 2);
    assert_eq!(job.source_files[1].translation_status, "untranslated");
    assert_eq!(job.status, "ready");
    assert_eq!(job.segment_count, 3);
    assert_eq!(job.completed_segments, 2);
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

#[test]
fn pdf_page_selection_accepts_ranges_and_dedupes() {
    let pages = parse_pdf_page_selection("1-3, 3,5", 5).expect("valid selection");

    assert_eq!(pages, vec![1, 2, 3, 5]);
}

#[test]
fn pdf_page_selection_rejects_out_of_range_pages() {
    let error = parse_pdf_page_selection("2,6", 5).expect_err("page 6 is invalid");

    assert!(error.contains("第 6 页超出范围"));
}

#[test]
fn lightning_pdf_chunk_policy_keeps_only_short_runs_wide() {
    let original_override = std::env::var_os(super::LIGHTNING_PDF_RUN_CHUNK_SIZE_ENV);
    std::env::remove_var(super::LIGHTNING_PDF_RUN_CHUNK_SIZE_ENV);

    let lightning = ShimProviderConfig::Lightning(LightningApiConfig {
        base_url: "http://127.0.0.1:8000".to_string(),
        endpoint: "/v1/batch/completions".to_string(),
        internal_token: String::new(),
        body_password: String::new(),
        timeout_ms: 120_000,
    });
    let llama = ShimProviderConfig::LlamaCpp(LlamaCppApiConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        timeout_ms: 120_000,
    });

    assert_eq!(
        super::pdf_run_chunk_size_for_provider(&lightning, 30),
        super::LIGHTNING_PDF_RUN_CHUNK_SIZE_DEFAULT,
    );
    assert_eq!(
        super::pdf_run_chunk_size_for_provider(&lightning, 31),
        super::LIGHTNING_PDF_LARGE_RUN_CHUNK_SIZE,
    );
    assert_eq!(
        super::pdf_run_chunk_size_for_provider(&llama, 400),
        PDF_RUN_CHUNK_SIZE,
    );

    if let Some(value) = original_override {
        std::env::set_var(super::LIGHTNING_PDF_RUN_CHUNK_SIZE_ENV, value);
    } else {
        std::env::remove_var(super::LIGHTNING_PDF_RUN_CHUNK_SIZE_ENV);
    }
}

#[test]
fn pdf_page_status_restores_stale_translating_pages() {
    let dir = unique_temp_dir("pdf-page-state");
    fs::create_dir_all(&dir).expect("create temp dir");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 2,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "translating".to_string(),
            translated_pdf_path: None,
            artifact_version: None,
            artifact_compression: None,
            artifact_bytes: None,
            artifact_compression_error: None,
            error: None,
            updated_at: "1".to_string(),
            last_run_id: None,
        }],
    };
    write_pdf_page_translation_state(&dir, &state).expect("write state");

    let restored = read_pdf_page_translation_state(&dir, 2, "zh-CN").expect("read state");

    assert_eq!(restored.pages[0].status, "pending");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_state_writes_canonical_file_and_only_durable_statuses() {
    let dir = unique_temp_dir("pdf-page-state-canonical");
    fs::create_dir_all(&dir).expect("create temp dir");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 2,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "queued".to_string(),
            translated_pdf_path: Some("translated-pages/zh-CN/page-0001.pdf".to_string()),
            artifact_version: Some("1".to_string()),
            artifact_compression: Some("fast".to_string()),
            artifact_bytes: Some(123),
            artifact_compression_error: Some("runtime".to_string()),
            error: Some("runtime".to_string()),
            updated_at: "1".to_string(),
            last_run_id: Some("run-1".to_string()),
        }],
    };

    write_pdf_page_translation_state(&dir, &state).expect("write state");

    assert!(dir.join("pdf_pages.zh-CN.json").is_file());
    assert!(!dir.join("pdf_page_translations.zh-CN.json").exists());
    let restored = read_pdf_page_translation_state(&dir, 2, "zh-CN").expect("read state");
    assert_eq!(restored.pages[0].status, "pending");
    assert_eq!(restored.pages[0].translated_pdf_path, None);
    assert_eq!(restored.pages[0].artifact_compression, None);
    assert_eq!(restored.pages[0].artifact_bytes, None);
    assert_eq!(restored.pages[0].artifact_compression_error, None);
    assert_eq!(restored.pages[0].error, None);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_state_preserves_artifact_compression_metadata() {
    let dir = unique_temp_dir("pdf-page-compression-metadata");
    fs::create_dir_all(&dir).expect("create temp dir");
    let mut state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 1,
        target_lang: "zh-CN".to_string(),
        pages: Vec::new(),
    };
    upsert_pdf_page_with_run(
        &mut state,
        1,
        "translated",
        Some("translated-pages/zh-CN/page-0001.pdf".to_string()),
        None,
        Some("run-1"),
    );
    set_pdf_page_artifact_metadata(
        &mut state,
        1,
        Some("compressed".to_string()),
        Some(2048),
        None,
    );
    write_pdf_page_translation_state(&dir, &state).expect("write state");

    let restored = read_pdf_page_translation_state(&dir, 1, "zh-CN").expect("read state");

    assert_eq!(
        restored.pages[0].artifact_compression.as_deref(),
        Some("compressed")
    );
    assert_eq!(restored.pages[0].artifact_bytes, Some(2048));
    assert_eq!(restored.pages[0].artifact_compression_error, None);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_run_recovery_pauses_stale_live_run() {
    let dir = unique_temp_dir("pdf-run-recovery");
    fs::create_dir_all(&dir).expect("create temp dir");
    let mut run = PdfTranslationRun::new(
        "run-1".to_string(),
        "job-1".to_string(),
        "zh-CN".to_string(),
        "continue".to_string(),
        vec![1, 2, 3],
        "old-session".to_string(),
    );
    run.current_chunk = vec![1, 2, 3];
    write_pdf_run_state(&dir, &run).expect("write run");

    let restored = recover_stale_run(&dir, "zh-CN", "new-session")
        .expect("recover run")
        .expect("run");

    assert_eq!(restored.state, "paused");
    assert_eq!(restored.current_chunk, Vec::<u32>::new());
    assert!(!restored.cancel_requested);
    assert!(restored.last_error.is_some());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_repair_tmp_cleanup_keeps_only_active_run_dirs() {
    let dir = unique_temp_dir("pdf-tmp-run-cleanup");
    let runs_dir = dir.join(".tmp").join("pdf-runs");
    fs::create_dir_all(runs_dir.join("run-active")).expect("create active temp run");
    fs::create_dir_all(runs_dir.join("run-stale")).expect("create stale temp run");
    let active = std::collections::BTreeSet::from(["run-active".to_string()]);

    let cleaned = super::cleanup_stale_pdf_run_tmp_dirs(&dir, &active).expect("cleanup tmp runs");

    assert!(cleaned);
    assert!(runs_dir.join("run-active").is_dir());
    assert!(!runs_dir.join("run-stale").exists());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_status_does_not_reuse_state_for_other_target_language() {
    let dir = unique_temp_dir("pdf-page-state-language");
    fs::create_dir_all(&dir).expect("create temp dir");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 1,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "translated".to_string(),
            translated_pdf_path: Some(pdf_page_relative_path(1)),
            artifact_version: Some("1".to_string()),
            artifact_compression: None,
            artifact_bytes: None,
            artifact_compression_error: None,
            error: None,
            updated_at: "1".to_string(),
            last_run_id: None,
        }],
    };
    write_pdf_page_translation_state(&dir, &state).expect("write state");

    let restored = read_pdf_page_translation_state(&dir, 1, "en").expect("read state");

    assert_eq!(restored.target_lang, "en");
    assert!(restored.pages.is_empty());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_status_does_not_trust_legacy_shared_page_path_for_english() {
    let dir = unique_temp_dir("pdf-page-state-legacy-en");
    fs::create_dir_all(&dir).expect("create temp dir");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 1,
        target_lang: "en".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "translated".to_string(),
            translated_pdf_path: Some("pdf-pages/page-0001.pdf".to_string()),
            artifact_version: Some("1".to_string()),
            artifact_compression: None,
            artifact_bytes: None,
            artifact_compression_error: None,
            error: None,
            updated_at: "1".to_string(),
            last_run_id: None,
        }],
    };
    write_pdf_page_translation_state(&dir, &state).expect("write state");

    let restored = read_pdf_page_translation_state(&dir, 1, "en").expect("read state");

    assert_eq!(restored.pages[0].status, "pending");
    assert_eq!(restored.pages[0].translated_pdf_path, None);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_import_cleanup_removes_derived_translation_artifacts() {
    let dir = unique_temp_dir("pdf-import-cleanup");
    fs::create_dir_all(dir.join("pdf-pages").join("zh-CN")).expect("create page cache");
    fs::create_dir_all(dir.join("translated-pages").join("zh-CN"))
        .expect("create translated pages");
    fs::create_dir_all(dir.join("pdf2zh-output")).expect("create output cache");
    fs::create_dir_all(dir.join("exports")).expect("create exports");
    fs::write(dir.join("source.pdf"), b"source").expect("write source");
    fs::write(dir.join("notes.txt"), b"keep").expect("write unrelated file");
    fs::write(
        dir.join("pdf_page_translations.zh-CN.json"),
        b"{\"pages\":[]}",
    )
    .expect("write scoped state");
    fs::write(dir.join("pdf_page_translations.json"), b"{\"pages\":[]}")
        .expect("write legacy state");
    fs::write(
        dir.join("pdf-pages").join("zh-CN").join("page-0001.pdf"),
        b"page",
    )
    .expect("write page cache");
    fs::write(
        dir.join("translated-pages")
            .join("zh-CN")
            .join("page-0001.pdf"),
        b"translated page",
    )
    .expect("write translated page");
    fs::write(dir.join("pdf2zh-output").join("page-0001.pdf"), b"output")
        .expect("write output cache");
    fs::write(dir.join("exports").join("translated.pdf"), b"translated")
        .expect("write translated pdf");

    cleanup_pdf_translation_artifacts(&dir).expect("cleanup pdf artifacts");

    assert!(dir.join("source.pdf").is_file());
    assert!(dir.join("notes.txt").is_file());
    assert!(dir.join("exports").is_dir());
    assert!(!dir.join("exports").join("translated.pdf").exists());
    assert!(!dir.join("pdf-pages").exists());
    assert!(!dir.join("translated-pages").exists());
    assert!(!dir.join("pdf2zh-output").exists());
    assert!(!dir.join("pdf_page_translations.zh-CN.json").exists());
    assert!(!dir.join("pdf_page_translations.json").exists());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_status_summary_marks_all_pages_translated() {
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 2,
        target_lang: "zh-CN".to_string(),
        pages: vec![
            PdfPageTranslation {
                page_number: 1,
                status: "translated".to_string(),
                translated_pdf_path: Some("translated-pages/zh-CN/page-0001.pdf".to_string()),
                artifact_version: Some("1".to_string()),
                artifact_compression: None,
                artifact_bytes: None,
                artifact_compression_error: None,
                error: None,
                updated_at: "1".to_string(),
                last_run_id: None,
            },
            PdfPageTranslation {
                page_number: 2,
                status: "translated".to_string(),
                translated_pdf_path: Some("translated-pages/zh-CN/page-0002.pdf".to_string()),
                artifact_version: Some("1".to_string()),
                artifact_compression: None,
                artifact_bytes: None,
                artifact_compression_error: None,
                error: None,
                updated_at: "1".to_string(),
                last_run_id: None,
            },
        ],
    };

    let (segment_count, completed_segments, failed_segments, status) =
        pdf_page_status_summary(&state);

    assert_eq!(segment_count, 2);
    assert_eq!(completed_segments, 2);
    assert_eq!(failed_segments, 0);
    assert_eq!(status, "translated");
}

#[test]
fn pdf_force_retranslate_clears_existing_page_artifact() {
    let dir = unique_temp_dir("pdf-force-clear-page");
    let page_dir = dir.join("translated-pages").join("zh-CN");
    fs::create_dir_all(&page_dir).expect("create page cache");
    let page_path = page_dir.join("page-0002.pdf");
    fs::write(&page_path, b"old translated page").expect("write old page");
    let mut state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 3,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 2,
            status: "translated".to_string(),
            translated_pdf_path: Some("translated-pages/zh-CN/page-0002.pdf".to_string()),
            artifact_version: Some("1".to_string()),
            artifact_compression: Some("compressed".to_string()),
            artifact_bytes: Some(42),
            artifact_compression_error: None,
            error: None,
            updated_at: "1".to_string(),
            last_run_id: None,
        }],
    };

    super::clear_pdf_page_artifacts(&dir, &mut state, "zh-CN", 2);

    assert!(!page_path.exists());
    assert_eq!(state.pages[0].status, "pending");
    assert_eq!(state.pages[0].translated_pdf_path, None);
    assert_eq!(state.pages[0].artifact_compression, None);
    assert_eq!(state.pages[0].artifact_bytes, None);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_repair_cleans_stale_page_compression_temps() {
    let dir = unique_temp_dir("pdf-compress-cleanup");
    let page_dir = dir.join("translated-pages").join("zh-CN");
    fs::create_dir_all(&page_dir).expect("create page cache");
    fs::write(page_dir.join("page-0001.pdf"), b"current").expect("write current page");
    fs::write(
        page_dir.join(".page-0001.pdf.100.compressing.tmp.pdf"),
        b"tmp",
    )
    .expect("write temp");
    fs::write(
        page_dir.join(".page-0001.pdf.100.precompress.bak"),
        b"backup",
    )
    .expect("write backup");

    let cleaned = page_artifact_compression::cleanup_stale_compression_files_in_dir(&page_dir)
        .expect("cleanup compression temps");

    assert!(cleaned);
    assert!(page_dir.join("page-0001.pdf").is_file());
    assert!(!page_dir
        .join(".page-0001.pdf.100.compressing.tmp.pdf")
        .exists());
    assert!(!page_dir.join(".page-0001.pdf.100.precompress.bak").exists());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_artifact_path_is_stable() {
    assert_eq!(pdf_page_filename(1), "page-0001.pdf");
    assert_eq!(pdf_page_filename(42), "page-0042.pdf");
    assert_eq!(pdf_page_relative_path(42), "translated-pages/page-0042.pdf");
    assert_eq!(pdf_page_language_dir("en"), "en");
    assert_eq!(pdf_page_language_dir("zh-CN"), "zh-CN");
    assert_eq!(pdf_page_language_dir("../en"), "en");
    assert_eq!(
        pdf_page_relative_path_for_lang("en", 42),
        "translated-pages/en/page-0042.pdf"
    );
    assert_eq!(
        pdf_page_translation_state_filename("en"),
        "pdf_pages.en.json"
    );
    assert_eq!(
        legacy_pdf_page_translation_state_filename("en"),
        "pdf_page_translations.en.json"
    );
}

#[test]
fn pdf_page_export_preserves_source_page_count_without_translations() {
    let source = fixture_path("002-trivial-libre-office-writer.pdf");
    let source_page_count = lopdf::Document::load(&source)
        .expect("load source pdf")
        .get_pages()
        .len();
    let dir = unique_temp_dir("pdf-page-export-source");
    fs::create_dir_all(&dir).expect("create temp dir");
    let target = dir.join("export.pdf");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: source_page_count as u32,
        target_lang: "zh-CN".to_string(),
        pages: Vec::new(),
    };

    assemble_pdf_with_page_translations(&source, &dir, &state, &target).expect("assemble pdf");

    let output_pages = lopdf::Document::load(&target)
        .expect("load output pdf")
        .get_pages()
        .len();
    assert_eq!(output_pages, source_page_count);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_export_substitutes_translated_page_and_keeps_full_length() {
    let source = fixture_path("002-trivial-libre-office-writer.pdf");
    let translated = fixture_path("simple-one-page.pdf");
    let source_page_count = lopdf::Document::load(&source)
        .expect("load source pdf")
        .get_pages()
        .len();
    let dir = unique_temp_dir("pdf-page-export-substitute");
    let pages_dir = dir.join("translated-pages").join("zh-CN");
    fs::create_dir_all(&pages_dir).expect("create page cache dir");
    fs::copy(&translated, pages_dir.join("page-0001.pdf")).expect("copy translated page");
    let target = dir.join("export.pdf");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: source_page_count as u32,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "translated".to_string(),
            translated_pdf_path: Some("translated-pages/zh-CN/page-0001.pdf".to_string()),
            artifact_version: Some("1".to_string()),
            artifact_compression: None,
            artifact_bytes: None,
            artifact_compression_error: None,
            error: None,
            updated_at: "1".to_string(),
            last_run_id: None,
        }],
    };

    assemble_pdf_with_page_translations(&source, &dir, &state, &target).expect("assemble pdf");

    let output_pages = lopdf::Document::load(&target)
        .expect("load output pdf")
        .get_pages()
        .len();
    assert_eq!(output_pages, source_page_count);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_cache_extracts_requested_page_from_full_pdf_output() {
    let source = fixture_path("2305.13048v2.pdf");
    let source_pages = lopdf::Document::load(&source)
        .expect("load source pdf")
        .get_pages()
        .len();
    assert!(source_pages >= 2);
    let dir = unique_temp_dir("pdf-page-cache-extract");
    fs::create_dir_all(&dir).expect("create temp dir");
    let target = dir.join("page-0002.pdf");

    extract_pages_pdf(&source, &[(2, target.clone())]).expect("extract second page");

    let output_pages = lopdf::Document::load(&target)
        .expect("load extracted page")
        .get_pages()
        .len();
    assert_eq!(output_pages, 1);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_cache_extracts_multiple_pages_in_one_pass() {
    let source = fixture_path("2305.13048v2.pdf");
    let source_pages = count_pdf_pages_lopdf(&source).expect("count source pages");
    assert!(source_pages >= 3);
    let dir = unique_temp_dir("pdf-page-cache-extract-multi");
    fs::create_dir_all(&dir).expect("create temp dir");

    let extractions = vec![
        (1, dir.join("page-0001.pdf")),
        (3, dir.join("page-0003.pdf")),
    ];
    extract_pages_pdf(&source, &extractions).expect("extract pages 1 and 3");

    for (_, target) in &extractions {
        let pages = lopdf::Document::load(target)
            .expect("load extracted page")
            .get_pages()
            .len();
        assert_eq!(pages, 1);
    }
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pdf_page_cache_accepts_single_page_pdf_output_for_any_requested_page() {
    let source = fixture_path("simple-one-page.pdf");
    let dir = unique_temp_dir("pdf-page-cache-single-output");
    fs::create_dir_all(&dir).expect("create temp dir");
    let target = dir.join("page-0002.pdf");

    extract_pages_pdf(&source, &[(2, target.clone())]).expect("extract fallback page");

    let output_pages = lopdf::Document::load(&target)
        .expect("load extracted page")
        .get_pages()
        .len();
    assert_eq!(output_pages, 1);
    fs::remove_dir_all(dir).ok();
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
        extraction_status: None,
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rosetta-{name}-{}", timestamp_ms_string()))
}
