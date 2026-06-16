# 2026-06-16 PDF Pack Bundles DocLayout Model

## Context

Chinese mainland users reported that Rosetta could stay forever at "PDF 引擎预热中" after upgrading to beta.11. Reproduction: remove the HuggingFace cache for `juliozhao/DocLayout-YOLO-DocStructBench`, disable VPN/proxy, then start beta.11. The persistent pdf2zh worker imports `doclayout_yolo` and calls `hf_hub_download` during prewarm, while the worker process also clears proxy env vars to protect loopback shim calls. That combination makes prewarm block on direct HuggingFace access.

## Change

- The PDF pack is now required to include `models/doclayout_yolo_docstructbench_imgsz1024.pt`.
- `get_pdf2zh_status` and installer skip logic treat old packs that only contain `bin/pdf2zh` as not ready, so beta.11 users upgrade through the normal PDF component install flow.
- The persistent worker reads `ROSETTA_DOCLAYOUT_MODEL` and fails clearly if the bundled model is missing. It no longer downloads the layout model from HuggingFace.
- CLI fallback receives the same `ROSETTA_DOCLAYOUT_MODEL` env var.
- The local pack staging script downloads or copies the DocLayout-YOLO model and patches pdf2zh's CLI fallback to prefer that environment variable.
- The archive script refuses to package a PDF component that lacks the bundled model.

## Release Asset

- URL: `https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.06.16.1/rosetta-pdf2zh-macos-arm64.tar.gz`
- Mainland mirror: `https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.06.16.1/rosetta-pdf2zh-macos-arm64.tar.gz`
- Size: `318454908`
- SHA256: `35fcbc1485a3133008a3f556bd7a4303859a6edac8cfac959a5e3d6b2644be8c`

`managed_pdf2zh/profile.rs` points to this asset so old beta.11 packs are detected as needing update and the installer fetches the new self-contained pack.

## Validation

- `cargo test managed_pdf2zh::status::tests --lib`
