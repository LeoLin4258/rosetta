use super::*;
use serde_json::json;
use std::{fs, path::PathBuf};

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rosetta-pdf-md-{name}-{}", now_nonce()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn shard(page: u32) -> PageShard {
    PageShard {
        schema: EXTRACTION_SCHEMA.into(),
        source_fingerprint:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        policy_version: POLICY_VERSION.into(),
        page_number: page,
        vendor: json!({"pages":[{"page_number":page,"boxes":[{"boxclass":"title","x0":1,"y0":2,"x1":30,"y1":10,"text":"  Stable   heading "},{"boxclass":"page-footer","text":"ignored"}]}]}),
        images: Vec::new(),
    }
}

#[test]
fn gzip_shard_roundtrips_and_ids_are_stable() {
    let dir = temp("stable");
    write_page_shard(&dir, &shard(1)).unwrap();
    let loaded = read_page_shard(&dir, 1).unwrap();
    let mut block_order = 1;
    let mut segment_order = 1;
    let (a, sa) = normalize_shard(
        &loaded,
        &mut block_order,
        &mut segment_order,
        Some("en".into()),
        "zh-CN",
    )
    .unwrap();
    let mut block_order = 1;
    let mut segment_order = 1;
    let (b, sb) = normalize_shard(
        &loaded,
        &mut block_order,
        &mut segment_order,
        Some("en".into()),
        "zh-CN",
    )
    .unwrap();
    assert_eq!(a[0].id, b[0].id);
    assert_eq!(sa[0].id, sb[0].id);
    assert_eq!(a[0].source_text, "Stable heading");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn stale_manifest_does_not_reuse_derivatives() {
    let manifest = ExtractionManifest {
        schema: EXTRACTION_SCHEMA.into(),
        source_fingerprint:
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        page_count: 2,
        engine: EngineIdentity::default(),
        policy_version: POLICY_VERSION.into(),
        use_ocr: false,
        force_text: false,
        write_images: true,
        committed_pages: vec![1],
    };
    assert!(!manifest_is_current(
        &manifest,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        2
    ));
    let mut policy_changed = manifest.clone();
    policy_changed.policy_version = "rosetta-pdf-markdown-normalizer/0".into();
    assert!(!manifest_is_current(
        &policy_changed,
        &manifest.source_fingerprint,
        2
    ));
}

#[test]
fn corrupt_gzip_shard_is_rejected() {
    let dir = temp("corrupt");
    let path = page_shard_path(&dir, 1);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"not-gzip").unwrap();
    assert_eq!(
        read_page_shard(&dir, 1).unwrap_err(),
        "page-shard-invalid-gzip"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn image_path_traversal_is_rejected_and_canonicalized() {
    let dir = temp("images");
    let run = dir.join("run");
    fs::create_dir_all(&run).unwrap();
    let image = run.join("picture.png");
    fs::write(&image, b"png").unwrap();
    let mut value = json!({"pages":[{"page_number":1,"boxes":[{"boxclass":"picture","image":image.to_string_lossy()}]}]});
    let refs = canonicalize_images(&dir, &run, 1, &mut value).unwrap();
    assert_eq!(refs, vec!["pdf-markdown/images/page-0001-picture-01.png"]);
    assert!(dir
        .join("pdf-markdown/images/page-0001-picture-01.png")
        .is_file());
    let mut escaped = json!({"pages":[{"page_number":1,"boxes":[{"boxclass":"picture","image":"../outside.png"}]}]});
    assert_eq!(
        canonicalize_images(&dir, &run, 1, &mut escaped).unwrap_err(),
        "image-path-invalid"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn preview_assets_enforce_flat_image_paths_types_and_size() {
    let dir = temp("preview-assets");
    let images = images_root(&dir);
    fs::create_dir_all(&images).unwrap();
    let valid = images.join("page-0001-picture-01.png");
    fs::write(&valid, b"png").unwrap();

    assert_eq!(
        resolve_preview_asset(&dir, "pdf-markdown/images/page-0001-picture-01.png").unwrap(),
        valid.canonicalize().unwrap()
    );
    assert_eq!(
        resolve_preview_asset(&dir, "pdf-markdown/images/../outside.png").unwrap_err(),
        "pdf-markdown-asset-path-invalid"
    );
    assert_eq!(
        resolve_preview_asset(&dir, "pdf-markdown/images/nested/picture.png").unwrap_err(),
        "pdf-markdown-asset-path-invalid"
    );
    assert_eq!(
        resolve_preview_asset(&dir, "pdf-markdown/images/picture.svg").unwrap_err(),
        "pdf-markdown-asset-type-invalid"
    );

    let oversized = images.join("oversized.webp");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_IMAGE_BYTES + 1)
        .unwrap();
    assert_eq!(
        resolve_preview_asset(&dir, "pdf-markdown/images/oversized.webp").unwrap_err(),
        "pdf-markdown-asset-invalid"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn public_progress_payload_is_content_free() {
    let status = PdfMarkdownExtractionStatus {
        job_id: "job-1".into(),
        state: "extracting".into(),
        completed_pages: 1,
        page_count: 3,
        error_code: None,
        run_id: Some("run-1".into()),
    };
    let encoded = serde_json::to_string(&status).unwrap();
    assert!(!encoded.contains("source"));
    assert!(!encoded.contains("text"));
    assert!(!encoded.contains("path"));
}

#[test]
fn pinned_vendor_shape_normalizes_render_metadata_and_skips_media() {
    let shard = PageShard {
        schema: EXTRACTION_SCHEMA.into(),
        source_fingerprint:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        policy_version: POLICY_VERSION.into(),
        page_number: 1,
        vendor: json!({"pages":[{"page_number":1,"boxes":[
            {"boxclass":"title","x0":10,"y0":10,"x1":90,"y1":20,"header_level":3,"text":"Title"},
            {"boxclass":"section-header","x0":10,"y0":30,"x1":90,"y1":40,"header_level":4,"text":"Section"},
            {"boxclass":"list-item","x0":10,"y0":50,"x1":90,"y1":60,"text":"• First"},
            {"boxclass":"list-item","x0":30,"y0":61,"x1":90,"y1":70,"text":"2. Nested"},
            {"boxclass":"table","x0":10,"y0":80,"x1":90,"y1":120,"table":{
                "row_count":2,
                "col_count":2,
                "cells":[[[10,80,50,100],[50,80,90,100]],[[10,100,50,120],[50,100,90,120]]],
                "extract":[["Name","Value"],["Alpha",""]],
                "markdown":"vendor markdown is not authority"
            }},
            {"boxclass":"picture","x0":10,"y0":130,"x1":50,"y1":170,"image":"pdf-markdown/images/page-0001-picture-01.png","text":"must not translate"},
            {"boxclass":"formula","x0":50,"y0":130,"x1":90,"y1":170,"image":"pdf-markdown/images/page-0001-picture-02.png","text":"must not translate"}
        ]}]}),
        images: vec![
            "pdf-markdown/images/page-0001-picture-01.png".into(),
            "pdf-markdown/images/page-0001-picture-02.png".into(),
        ],
    };
    let mut block_order = 1;
    let mut segment_order = 1;
    let (blocks, segments) = normalize_shard(
        &shard,
        &mut block_order,
        &mut segment_order,
        Some("en".into()),
        "zh-CN",
    )
    .unwrap();

    assert_eq!(
        blocks[0].style.as_ref().unwrap()["pdfMarkdown"]["extra"]["headingLevel"],
        1
    );
    assert_eq!(
        blocks[1].style.as_ref().unwrap()["pdfMarkdown"]["extra"]["headingLevel"],
        4
    );
    assert_eq!(blocks[2].source_text, "First");
    assert_eq!(
        blocks[2].style.as_ref().unwrap()["pdfMarkdown"]["extra"]["listLevel"],
        1
    );
    assert_eq!(blocks[3].source_text, "Nested");
    assert_eq!(
        blocks[3].style.as_ref().unwrap()["pdfMarkdown"]["extra"]["listLevel"],
        2
    );

    let table_cells = blocks
        .iter()
        .filter(|block| block.block_type == "table_cell")
        .collect::<Vec<_>>();
    assert_eq!(table_cells.len(), 4);
    assert_eq!(table_cells[0].source_text, "Name");
    assert_eq!(
        table_cells[0].style.as_ref().unwrap()["pdfMarkdown"]["extra"]["row"],
        0
    );
    assert_eq!(table_cells[3].source_text, "");
    assert!(!table_cells[3].should_translate);

    let media = blocks
        .iter()
        .filter(|block| matches!(block.block_type.as_str(), "metadata" | "code"))
        .collect::<Vec<_>>();
    assert_eq!(media.len(), 2);
    assert!(media
        .iter()
        .all(|block| !block.should_translate && block.source_text.is_empty()));
    assert!(segments
        .iter()
        .all(|segment| !media.iter().any(|block| block.id == segment.block_id)));
    let encoded_style = serde_json::to_string(&blocks[0].style).unwrap();
    assert!(!encoded_style.contains("textlines"));
    assert!(!encoded_style.contains("vendor markdown"));
}
