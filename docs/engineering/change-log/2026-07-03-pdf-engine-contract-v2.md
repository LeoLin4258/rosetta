# 2026-07-03 PDF Engine Contract v2

## Summary

Reworked PDF translation around a Rosetta-native typed PDF engine contract.
The product path no longer asks pdf2zh to translate through an
OpenAI-compatible shim. Python prepares and renders PDF windows; Rust owns
translation provider orchestration and page commit decisions.

## Changes

- Added the PDF Translation Engine Contract v2 ADR.
- Added a PDFMathTranslate fork API (`pdf2zh.rosetta_engine`) for:
  `prewarm`, `prepareRun`, `collectUnits`, `renderPages`, and `disposeRun`.
- Replaced the Rosetta PDF worker protocol with typed
  `prepare_pdf_window`, `render_pdf_window`, and `dispose_pdf_window`
  commands.
- Added Rust-side PDF unit translation orchestration for Lightning,
  mobile-batch, and llama.cpp provider paths.
- Removed the product dependency on PDF OpenAI-compatible shim translation,
  `/v1/rosetta/batch-translations`, replay translators, and silent CLI
  fallback.
- Bumped PDF page state to schema version 2 and added `resultKind`,
  unit counts, char counts, artifact byte size, and artifact compression
  metadata.
- Changed page commit to accept only formal `PageResult` data. Diagnostics are
  no longer used to decide page success.
- Added reset behavior for beta v1 PDF page state: derived translated artifacts
  and old page-state files are removed, while `source.pdf` is preserved.
- Kept long-PDF stability policy: small PDFs can use wider windows, large PDFs
  use 10-page windows, and long active runs pause translated PNG live raster.

## Operational Notes

- Updated PDF component packs must include the fork's v2 `rosetta_engine`.
- PDF component build scripts install the local PDFMathTranslate fork and
  smoke-test `rosetta_engine.ENGINE_CONTRACT_VERSION == 2`.
- Worker startup rejects packs that do not report PDF engine contract version
  `2`.
- Existing beta PDF translations must be regenerated after v2 state reset.
- Fast page artifacts remain valid on the hot path; byte size is recorded and
  background compression handles disk pressure by subsetting embedded fonts
  before deflate/object-stream saving.

## Validation

Relevant validation:

```powershell
cd rosetta-app
.\node_modules\.bin\tsc.cmd --noEmit

cd src-tauri
cargo fmt -- --check
cargo check
cargo test rosetta_jobs
cargo test managed_pdf2zh
cargo test managed_rwkv
```

PDFMathTranslate fork:

```powershell
python -m py_compile pdf2zh/rosetta_engine.py
python -m pytest test
```
