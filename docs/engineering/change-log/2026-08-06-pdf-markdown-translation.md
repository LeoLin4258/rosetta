# PDF Markdown Translation

Date: 2026-08-06

Status: Checkpoint 0 Go; Checkpoint 1 model and migration implemented

## Scope

This aggregate change log tracks the implementation authorized by ADR 0078 and
`plans/2026-08-06-pdf-markdown-translation.md`. The delivered scope includes
the release-quality spike plus Checkpoint 1 data-model migration. It does not
add a managed component, extraction derivative or frontend output selector.

## Changes

- Added an exact-version, manifest-driven 24-document / 240-page corpus runner
  that calls `pymupdf4llm.to_json()` with OCR disabled, picture text excluded
  and image extraction enabled.
- Added sanitized diagnostic rendering and automated flags for page identity,
  unknown box classes, empty body pages, adjacent repetition and picture/body
  overlap. Diagnostic Markdown is inspection-only and is not export authority.
- Added deterministic Windows x64, macOS arm64 and Linux x64 overlay builders
  from pinned wheels, with exact archive hash, byte count, file count and
  unpacked size manifests.
- Reduced the Windows overlay from 63,229,287 bytes to 29,985,992 bytes by
  removing bytecode, MuPDF development files, optional Llama integration and
  model variants outside the pinned default layout runtime closure.
- Added a real Windows preflight proving CPU-only layout execution and PyMuPDF
  isolation between the Markdown overlay (`1.28.0`) and production `pdf2zh`
  (`1.25.2`).
- Kept the 8-page recovery boundary while invoking `to_json()` page by page
  inside a worker request, reducing peak RSS from 484,282,368 bytes to
  407,248,896 bytes.
- Added `outputFormat` to translation files with legacy PDF/Markdown/TXT
  inference, native ID preservation and output-qualified PDF Markdown sibling
  IDs.
- Changed translation-file uniqueness to
  `(sourceFileId, targetLang, outputFormat)` and added the narrow source/output
  validation matrix.
- Kept legacy PDF page counters isolated from secondary Markdown segment
  progress and updated existing frontend calls to request their native output
  explicitly, without adding mode-switch UI.
- Added persisted per-source output selection and output-qualified active
  translation lookup in the Zustand store, retaining a read fallback for old
  pair-key preferences.

## Validation

- `python rosetta-app/src-tauri/scripts/test-pdf-markdown-checkpoint0.py`:
  passed, 6 tests.
- Windows deterministic overlay build and real-PDF preflight: passed twice;
  both archives SHA-256
  `f2e01a2df1a4c5aaa74114dbb49f1473b2082104f1aee23eeb3407ded13ac2fc`.
- Windows 24-document / 240-page corpus: passed speed, peak-RSS and critical
  golden-page quality gates. Median was 0.333 seconds/page, p95 was 0.623
  seconds/page and peak RSS was 388.4 MiB.
- Windows cumulative managed PDF bytes: 396,059,375, below 400 MiB.
- Cross-platform target-wheel archive construction: macOS arm64 34,622,507
  bytes; Linux x64 36,480,503 bytes.
- Native macOS arm64 and Linux x64 builds reproduced the exact cross-built
  archive bytes and SHA-256 identities, passed real-PDF CPU-only preflight and
  kept the production worker on PyMuPDF `1.25.2`.
- Native macOS corpus: median 0.139 seconds/page, p95 0.300 seconds/page and
  peak RSS 1,148,796,928 bytes / 1,095.6 MiB.
- Native Linux corpus: median 0.275 seconds/page, p95 0.584 seconds/page and
  peak RSS 472,555,520 bytes / 450.7 MiB.
- Native structure reports reproduced the reviewed Windows defect counts with
  zero invalid page identities and zero unknown box classes.
- `pnpm typecheck`: passed after the output-qualified frontend/store changes.
- `cargo check`: passed with existing dead-code warnings only.
- `cargo test rosetta_jobs`: passed, 93 tests including legacy
  PDF/Markdown/TXT inference, output ID qualification, format-matrix rejection
  and PDF/Markdown progress isolation.

## Remaining Verification

The 400 MiB hard gate applies to cumulative downloaded managed PDF components,
not runtime RSS. Windows base plus overlay is 396,059,375 bytes and passes that
gate. Peak RSS remains recorded for capacity planning: a fresh
single-page macOS extraction already reaches 539,525,120 bytes / 514.5 MiB;
Linux grows from a 285,655,040-byte / 272.4 MiB single-page process to 450.7
MiB over the release corpus. Thread-limit environment variables did not
materially reduce either measurement, and the pinned package already disables
the ONNX CPU memory arena.

Checkpoint 1 model and migration work is implemented. Managed install, repair,
cancellation, offline restart and ordinary PDF regression remain for the later
worker/managed-component release checkpoint. ONNX session tuning and worker
recycling stay as a separate bounded optimization, not a release-size blocker.
