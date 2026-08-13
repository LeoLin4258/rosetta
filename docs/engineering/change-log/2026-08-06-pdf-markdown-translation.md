# PDF Markdown Translation

Date: 2026-08-06

Status: Checkpoint 0 Go; Checkpoints 1-5 implemented; Checkpoint 2 artifacts published; visual regression pending

## Scope

This aggregate change log tracks the implementation authorized by ADR 0078 and
`plans/2026-08-06-pdf-markdown-translation.md`. The delivered scope includes
the release-quality spike, output-qualified data model, managed
component/isolated worker, extraction derivative, deterministic Markdown
rendering and workbench integration. Multi-file Markdown asset export remains
outside the delivered scope until Checkpoint 6.

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
- Published release `pdf-markdown-overlay-v2026.08.06.1` in
  `LeoLin4258/rosetta-assets` with the exact Windows x64, macOS arm64 and Linux
  x64 overlay archives referenced by the managed-component profiles.
- Added the versioned extraction manifest, bounded gzip page shards,
  canonical image references, resumable window commits and recoverable atomic
  source-IR projection for PDF Markdown derivatives.
- Normalized the pinned `to_json()` table matrix into complete row/column cell
  blocks, retaining empty cells for structure while scheduling only non-empty
  textual cells. Pictures and formulas remain non-translatable blocks with
  checked job-relative asset references. Advanced the normalizer policy to
  `/2` so earlier derivatives are invalidated instead of mixed with the new IR.
- Added a deterministic, serializable Markdown block renderer for headings,
  nested lists, captions, footnotes, pictures, formulas, GFM rectangular
  tables and inline-HTML complex/unsafe tables. Text and asset paths are
  escaped or validated at the renderer boundary.
- Changed translation-file export rendering to use the selected
  `outputFormat`, so a PDF source's Markdown sibling consumes the ordinary
  translated source segments without changing PDF source identity or native
  PDF behavior.
- Added a compact persisted `PDF | Markdown` output selector for PDF sources.
  Native PDF mode keeps the existing page navigation, page-range selection,
  force-retranslate, progress, preview and export behavior; Markdown mode uses
  whole-document extraction and ordinary segment translation.
- Added explicit Markdown component install, repair and cancellation actions,
  download progress, and extraction idle/extracting/ready/stale/failed/cancelled
  states. Installation and extraction start only from explicit user actions.
- Made workspace selection and active-run state output-qualified, including an
  exact target-language translation-file match. Switching modes does not
  cancel the sibling run or force a completed extraction to change the user's
  current output selection.
- Added a narrow shared-renderer preview command and a virtualized two-pane
  Markdown workbench preview. Table render groups preserve all block IDs for
  selection and pending groups never substitute source text as translated
  content.
- Added bounded Markdown image preview reads through job-relative byte IPC.
  The backend accepts only flat PNG/JPEG/WebP references under the canonical
  per-job image root, rejects traversal, nesting and files over 32 MiB, and
  never exposes absolute document paths.
- Changed the enabled single-file translation export write to stage and flush
  a same-directory temporary file before atomic replacement. Checkpoint 6
  now extends that boundary to the multi-file Markdown-plus-assets transaction.
- Added PDF Markdown `.md` plus sibling `.assets/` export with strict output
  identity and renderer-link validation, percent-encoded relative links,
  SHA-256 image deduplication, aggregate result counts and recoverable
  same-directory destination backups.
- Streamed image hashing and staging through fixed-size buffers instead of
  retaining every exported image in memory. Each asset remains capped at 32
  MiB and is rehashed while copied so a concurrent cache change cannot produce
  a mismatched staged export.
- Added fault-injection coverage for rollback after the Markdown file commits,
  plus invalid/missing/oversized asset, stale destination, deduplication and
  staging-cleanup coverage.
- File deletion now cancels the job's PDF and Markdown work before removing a
  source and deletes PDF Markdown shards and images with the removed PDF.
- Hardened first extraction after component installation: vendor stdout is
  isolated from the JSONL protocol at both Python and native file-descriptor
  levels, transient worker-start failures receive one bounded retry, failed
  runs clean their temporary image directories, and cancellation uses the
  worker's actual protocol-close error spelling.
- Fixed the Rust JSONL event contract so camel-case worker payload fields such
  as `integrationBoundary` and `cpuOnly` deserialize during the ready
  handshake. Added a regression fixture for the exact 194-byte ready event.
- Preserved page totals and committed-page progress on extraction failure and
  surfaced the durable error code as actionable workbench copy instead of a
  generic retry-only state.

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
- `cargo test rosetta_jobs`: passed, 104 tests including legacy
  PDF/Markdown/TXT inference, output ID qualification, format-matrix rejection
  and PDF/Markdown progress isolation, pinned `to_json()` normalization,
  translation compatibility and deterministic Markdown rendering.
- Checkpoint 6 Windows validation: `pnpm typecheck` passed; `cargo check`
  passed with existing dead-code warnings only; `cargo test rosetta_jobs`
  passed 111 tests, including five multi-file export/rollback cases, one PDF
  Markdown derivative-cleanup case and cancellation error classification.
- `python rosetta-app/src-tauri/scripts/test-rosetta-pdf-markdown-worker.py`:
  passed, 5 worker protocol/path tests.
- Focused `cargo test managed_pdf_markdown`: 15 passed; the exact native
  release-archive test remains ignored unless its explicit artifact environment
  variable is supplied. Worker protocol tests passed 5/5 and the Checkpoint 0
  Python suite passed 6/6.
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
- GitHub reported all three release assets as uploaded with the expected byte
  counts and SHA-256 digests. Full public downloads through each profile's
  primary configured URL reproduced the exact archive bytes and hashes.

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
exact release artifacts. The configured release tag is published, and the
primary online download path has passed full byte/hash verification on all
three assets. The ordinary PDF end-to-end visual regression with the overlay
installed and absent remains pending; PyMuPDF/process isolation has passed but
is not a substitute for visual comparison. ONNX session tuning and worker
recycling stay as a separate bounded optimization, not a release-size blocker.

Checkpoint 3 adds the extraction store and normalizer. Each PDF job now has a
versioned, fingerprint-bound manifest and bounded gzip page shards under
`pdf-markdown/`, with deterministic canonical image copies and resumable
missing/corrupt-page recovery. The isolated worker remains the only vendor
boundary (`to_json()`), and a page-window is committed only after identity,
coordinate, path, size and defensive-count validation.

Normalization produces stable page/box/table-cell block and segment IDs,
omits headers/footers, preserves pictures/formulas as non-translatable
metadata/code, and projects `document.json` plus `segments.json` through a
recoverable staged replacement. Narrow extraction status/start/cancel commands
and content-free progress events were registered; deleting a job stops its
Markdown extraction before cleanup. No UI or production `pdf2zh` behavior was
changed.

Checkpoint 4 reuses the ordinary translation-file runner and makes
`translationFile.outputFormat` the export rendering authority. The shared
renderer emits deterministic structured block groups and covers headings,
lists, captions, footnotes, media placeholders and both GFM and inline-HTML
table modes.

Checkpoint 5 adds the output-qualified Workbench path, managed component and
extraction lifecycle controls, and the virtualized shared-renderer preview.
Automated validation covers compilation, existing job behavior, managed
component isolation and bounded asset-path reads. Manual runtime/visual
verification was explicitly deferred for this checkpoint. Atomic multi-file
Markdown plus image export, destination rollback and PDF derivative deletion
are implemented for Checkpoint 6. Final release-corpus export inspection and
the ordinary PDF visual regression with the overlay installed and absent
remain pending manual acceptance gates.
