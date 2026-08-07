# PDF Markdown Translation

Date: 2026-08-06

Status: Checkpoint 0 Go; Checkpoints 1-2 implemented; Checkpoint 2 artifact publication pending

## Scope

This aggregate change log tracks the implementation authorized by ADR 0078 and
`plans/2026-08-06-pdf-markdown-translation.md`. The delivered scope includes
the release-quality spike, Checkpoint 1 data-model migration and Checkpoint 2
managed component/isolated worker. It does not add an extraction derivative or
frontend output selector.

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
- Added a standalone `managed_pdf_markdown` component with exact Windows x64,
  macOS arm64 and Linux x64 profiles for PyMuPDF4LLM/Layout/PyMuPDF 1.28.0.
- Added install, repair, status, progress, cancellation and offline manifest
  validation without changing the production `pdf2zh` pack. Archive installs
  enforce exact hash/inventory, compressed and unpacked limits, path/link
  safety, staging writes and recoverable atomic replacement.
- Added a private isolated worker using `to_json()` as its sole integration
  boundary, exact CPU-only version preflight, a sanitized process environment,
  64 KiB request and 64 MiB response limits, strict event schemas and checked
  jobs-root paths.
- Added process-group/process-tree cancellation through an atomic PID outside
  the worker mutex, plus a stopping gate that prevents prewarm/shutdown races.
  Public state and error surfaces do not expose document content or absolute
  document paths, and worker stderr is discarded.
- Added protocol/path tests, native runtime-isolation tooling and focused Rust
  coverage for status, install/repair rollback, cancellation, traversal,
  offline reopen and production-pack separation.

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
- Windows `cargo test managed_pdf_markdown`: 15 passed, 1 exact-artifact test
  ignored by default; the ignored exact Windows archive test passed when given
  the 29,985,992-byte release artifact.
- Native macOS arm64 and Linux x64 `cargo check` and focused
  `managed_pdf_markdown` tests passed. Each platform passed 15 tests with the
  exact-artifact test ignored by default, then passed that test explicitly
  against its exact release archive and reopened from only the local manifest.
- The worker protocol/path suite passed 5 tests on Windows, macOS and Linux.
- Native concurrent isolation passed on all three platforms: production
  PyMuPDF remained 1.25.2 before/during/after, while the Markdown worker loaded
  1.28.0 and reported only `CPUExecutionProvider`.

## Remaining Verification

The 400 MiB hard gate applies to cumulative downloaded managed PDF components,
not runtime RSS. Windows base plus overlay is 396,059,375 bytes and passes that
gate. Peak RSS remains recorded for capacity planning: a fresh
single-page macOS extraction already reaches 539,525,120 bytes / 514.5 MiB;
Linux grows from a 285,655,040-byte / 272.4 MiB single-page process to 450.7
MiB over the release corpus. Thread-limit environment variables did not
materially reduce either measurement, and the pinned package already disables
the ONNX CPU memory arena.

Checkpoint 2 managed install, repair, cancellation, offline restart and
cross-platform runtime isolation are implemented and verified against the
exact local release artifacts. The configured release tag
`pdf-markdown-overlay-v2026.08.06.1` has not been published, so the default
online download path remains a release gate rather than a claimed pass. The
ordinary PDF end-to-end visual regression with the overlay installed and
absent also remains pending; PyMuPDF/process isolation has passed but is not a
substitute for visual comparison. ONNX session tuning and worker recycling stay
as a separate bounded optimization, not a release-size blocker.
