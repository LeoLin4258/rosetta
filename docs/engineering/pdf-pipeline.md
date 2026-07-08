# PDF Pipeline

Last updated: 2026-07-03

This document describes the current PDF translation implementation. Older PDF
plans are historical background only when they conflict with this file and
ADR 0008 or ADR 0009.

Rosetta's PDF path is a local visual PDF translation pipeline:

1. Import copies the user's PDF into a job-local `source.pdf`.
2. The source PDF remains the authoritative source file.
3. Translation enters the Rosetta-native PDF engine contract in the
   PDFMathTranslate fork. The Python worker prepares a page window and returns
   typed translation units; it does not call RWKV, OpenAI-compatible shims, or
   Rosetta HTTP translation endpoints.
4. Rust translates the returned units through the selected local provider.
   Lightning keeps large unit batches inside the window. llama.cpp and other
   non-Lightning providers use the same Rust `translate_pdf_units` contract
   with strict chunking/retry and truncation rejection.
5. The Python worker renders the prepared window from `unitId -> translation`
   and emits formal `PageResult` records. Rust commits one page-level PDF
   artifact at a time only from those `PageResult` records.
6. Export assembles a full PDF from `source.pdf` plus completed page artifacts.

PDF translation does not use the TXT/Markdown `Segment[]` scheduler. PDF jobs
still have `document.json`, `segments.json`, and translation-file metadata so
they fit the workbench and sidebar model, but page translation state lives in
PDF-specific files.

## Job Layout

```txt
AppData/Rosetta/jobs/
  index.json
  delete_cleanup_tasks.json
  .trash/
    <jobId>-<timestamp>/
  <jobId>/
    source.pdf
    document.json
    segments.json
    translation_files.json
    translation_revisions.json
    pdf_source.json
    pdf_pages.<targetLang>.json
    pdf_run.<targetLang>.json
    diagnostics/
      pdf-timeline.jsonl
      pdf-translation-profile-<runId>.json
    translated-pages/
      <targetLang>/
        page-0001.pdf
        page-0002.pdf
    .tmp/
      pdf-runs/
        <runId>/
          chunk-0001/
    exports/
      <user-triggered exports>
```

PDF v1 beta files are detected only to reset derived state:

```txt
pdf_pages.<targetLang>.json       # schemaVersion < 2
pdf_page_translations.<targetLang>.json
pdf_page_translations.json
pdf-pages/<targetLang>/page-0001.pdf
pdf-pages/page-0001.pdf
```

PDF v2 does not preserve beta page translation state or beta translated
artifacts. When a v1 PDF page state is found, Rosetta removes derived
translated artifacts and page-state files, preserves `source.pdf`, and rebuilds
an empty v2 `pending` state so the document can be translated again.

## File Roles

Authoritative data:

- `source.pdf`: the imported PDF copied into the job directory.
- `document.json`: the workbench document record. For PDF this is a skeleton
  document with one PDF source file and empty blocks.
- `segments.json`: empty for the visual PDF path.
- `translation_files.json`: workbench-level target-language metadata.
- `pdf_source.json`: page count, fingerprint, imported filename, original path
  snapshot, import/update timestamps.
- `pdf_pages.<targetLang>.json`: durable page translation state.
- `pdf_run.<targetLang>.json`: current or latest durable PDF run state.

Translation artifacts:

- `translated-pages/<targetLang>/page-XXXX.pdf`: the authoritative translated
  PDF artifact for a page.

Temporary runtime files:

- `.tmp/pdf-runs/<runId>/...`: pdf2zh output before commit.

User exports:

- `exports/`: only user-triggered full-PDF exports belong here. The full export
  is rebuilt from page artifacts; it is not the source of page truth.

Diagnostics:

- `diagnostics/pdf-timeline.jsonl`: append-only lifecycle events for one PDF
  job, starting at import and continuing through translation runs. Events record
  timestamps, run IDs, page numbers, counts, durations, file sizes, provider
  IDs, and aggregate RWKV timings. They must not contain source text,
  translated text, prompts, model responses, or document content.
  Translation runs include prepare/translate/render chunk timing and formal
  page commit outcomes. Diagnostics can explain performance and failures, but
  they must not decide whether a page is committed; `PageResult` is the
  business contract.
- `diagnostics/pdf-translation-profile-<runId>.json`: per-run aggregate profile
  for PDF translation. This remains the compact summary for one translation
  run; the timeline is the ordered event log used to reconstruct the chain.

The developer validation script `rosetta-app/scripts/check-pdf-translation-run.mjs`
reads a profile, page state, timeline, and provider diagnostics for one run.
It may fail local benchmark runs, but product commit logic must not depend on
diagnostic inference.

Diagnostic files are not job state. Repair, preview, export, and resume logic
must continue to use `pdf_source.json`, `pdf_pages.<targetLang>.json`,
`pdf_run.<targetLang>.json`, and page artifacts as the source of truth.

## Historical V1 Shim Notes

The notes below describe the v1 OpenAI-compatible shim/replay period and older
benchmark decisions. They are historical background only. The PDF v2 product
path does not spawn an OpenAI-compatible PDF shim, does not call
`/v1/rosetta/batch-translations` as the PDF engine communication layer, and
does not use a deferred/replay translator as Rosetta's architecture boundary.

For local OpenAI-shim providers that do not report a supported batch size,
Rosetta uses a default PDF paragraph batch width of 8. The Windows llama.cpp
Vulkan provider is the exception: by default it follows the managed
llama.cpp parallel setting, currently `16`, so the small 0.4B model can keep
all llama-server slots busy. The chosen batch width is also passed to the
persistent worker as pdf2zh's `thread` count, capped by the PDF worker ceiling.

The managed Windows llama.cpp runtime defaults to `--parallel 16` and
`--ctx-size 16384`, giving each concurrent slot about 1024 context tokens. This
is the current strict-correct PDF operating point for the Windows llama.cpp
Vulkan runtime: it keeps the 16-way throughput target, avoids the older
512-token slot truncation failures, and enables the adaptive PDF shim's
1024-slot chunk profile. Local benchmark runs can override these launch and
scheduling defaults with:

```txt
ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=<tokens>
ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=<slots>
```

The `PARALLEL` override also caps llama.cpp client-side batching in both the
PDF OpenAI shim and the regular text translation scheduler, keeping benchmark
experiments aligned with llama-server's slot count.

The llama.cpp PDF shim adapts its chunk budget to the effective per-slot
context. At the default `--ctx-size 16384 --parallel 16` operating point, body
and caption chunks target `72` prompt tokens while references target `42`, with
hard caps of `88`, `88`, and `56` respectively. This is the current
strict-correct benchmark default. If local benchmark runs lower the effective
slot context below `1024` tokens, the shim falls back to the more conservative
`56/72` body/caption and `42/56` reference profile. The shim also deterministically
passes through very short reference fragments such as compact `[N] ...` entries
so tiny bibliography shards are preserved without letting the model run away.
A wider `112/144` body profile was tested on 2026-06-29 and reduced completion
count, but reintroduced raw llama.cpp truncation and slowed the run through
split retries, so it remains available only through local env override
experiments. A follow-up `72/88` body plus `56/72` reference profile still hit
two raw reference-list failures, so references were returned to the conservative
budget. The resulting `72/88` body/caption, `42/56` reference, and short
reference passthrough profile passed the strict raw-completion checker on the
10-page benchmark with 304 completions and no raw truncation. If a llama.cpp
batch still fails, the shim retries through a smaller split backstop before
surfacing the failure to pdf2zh.

A local body/caption `80/96` sweep also passed strict correctness and reduced
raw completions to 288, but total runtime regressed because individual
completions were slower. Keep the default body/caption profile at `72/88`
unless a later benchmark shows a better tradeoff.

Local benchmark runs can override those llama.cpp PDF shim budgets with:

```txt
ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET=<tokens>
ROSETTA_PDF_SHIM_LLAMA_BODY_HARD=<tokens>
ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET=<tokens>
ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD=<tokens>
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET=<tokens>
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD=<tokens>
```

The hard cap is coerced to be at least the target. These knobs are for local
benchmark sweeps only; the strict checker must still reject any raw
`truncated=true`, `stop_type=limit`, or empty llama.cpp completion.

The native PDF v2 unit translator also keeps strict llama.cpp rejection in
place. If a llama.cpp unit batch fails with `truncated=true` or
`stop_type=limit`, Rosetta does not accept the partial response; it retries the
affected PDF unit chunks with narrower split budgets before surfacing the
failure. This mirrors the shim-era recovery boundary without weakening the
provider parser.

llama.cpp `/completion` requests use a translation-focused generation profile
instead of the server's generic sampling defaults. Rosetta sends low-entropy
sampling and repetition-control fields (`temperature`, `top_k`, `top_p`,
`min_p`, `repeat_penalty`, `repeat_last_n`) plus language-label stop strings.
This is intended to avoid the small-input repetition runaways that can hit the
request `n_predict` cap and produce `stop_type=limit` even when enough context
is available. Local benchmark runs can override these generation values with:

```txt
ROSETTA_LLAMA_CPP_TEMPERATURE=<float>
ROSETTA_LLAMA_CPP_TOP_K=<positive integer>
ROSETTA_LLAMA_CPP_TOP_P=<0.0-1.0>
ROSETTA_LLAMA_CPP_MIN_P=<0.0-1.0>
ROSETTA_LLAMA_CPP_REPEAT_PENALTY=<positive float>
ROSETTA_LLAMA_CPP_REPEAT_LAST_N=<positive integer>
ROSETTA_LLAMA_CPP_N_PREDICT=<positive integer>
```

Timeline diagnostics record the effective thread count in the worker
`job.started` stage and record every `page.processPage.translateRequest` or
`page.processPage.translateBatch`, making it possible to see whether a page
waited on one, two, or more TextConverter translation waves. In the native
Rosetta batch path, model time is expected to move from per-page
`translateBatch` events into a chunk-level `crossPageBatch.translate` event.
After cross-page collection, replay reuses the collect-pass layout masks and
pdfminer layout tree where available; replay-only work is surfaced through
`page.layoutMask.reuse` and `page.processPage.replayLayout`.
`page.saveSinglePdf` is split into `insertPage` and `writeFile` child stages so
page artifact serialization can be separated from page-object extraction.

For the native Rosetta batch path, page layout inference uses a speed-first
input size capped at `640` pixels by default instead of the page-height-derived
native value. On the 10-page Windows NVIDIA Lightning benchmark, ONNX batch
inference did not materially outperform serial inference, while reducing
`imgsz` from `768` to `640` cut YOLO time with nearly unchanged detected-box
counts. A more aggressive `576` input was faster but changed layout detections
more visibly, so it is not the default. Local diagnosis can restore or sweep the
layout inference size with:

```txt
ROSETTA_PDF_LAYOUT_IMGSZ=native
ROSETTA_PDF_LAYOUT_IMGSZ=768
ROSETTA_PDF_LAYOUT_IMGSZ=640
```

Single-page page artifacts are local cache files. The worker defaults to
speed-first artifact saving (`deflate=0`) because compressed PyMuPDF writes can
dominate warm-worker runtime after model batching has been fixed. Local
diagnosis can restore compressed page artifacts with:

```txt
ROSETTA_PDF_SINGLE_PAGE_DEFLATE=1
```

This is a speed/disk-space tradeoff for intermediate page artifacts, not a
change to source PDF state or translation correctness.

Rosetta keeps the speed-first write on the translation hot path and then
compresses committed page artifacts in a Rust-owned background maintenance
task on every supported platform with an installed pdf2zh component pack. The
background task uses the pack's PyMuPDF runtime with font subsetting,
`garbage=4`, stream/image/font deflate, and object streams. Font subsetting is
required because otherwise each single-page translated artifact can retain a
full CJK font copy; importing that lightweight PyMuPDF module does not touch
the warm pdf2zh worker or PyTorch/ONNX layout prewarm. Compression is
best-effort cache maintenance:

- the page remains `translated` even if compression fails;
- each candidate is guarded by `lastRunId`, `artifactVersion`, and
  `translatedPdfPath`, so an old compression task cannot commit over a newer
  force-retranslation result;
- compressed output is written to a sibling `.compressing.tmp.pdf`, validated
  as a one-page PDF, and only replaces the canonical page artifact when it is
  meaningfully smaller;
- replacement uses a temporary `.precompress.bak` backup and repair cleans or
  restores stale temp/backup files left by app exit or process termination;
- job deletion and force retranslation may race with compression; failures in
  those races are treated as skipped maintenance, not translation failures.

`pdf_pages.<targetLang>.json` records optional page artifact metadata:
`artifactCompression` (`fast`, `compressed`, or `skipped`), `artifactBytes`,
and `artifactCompressionError`. These fields are optional for backward
compatibility and must not be required to preview, export, repair, or resume
old jobs.

Local diagnosis can disable background page artifact compression with:

```txt
ROSETTA_PDF_PAGE_ARTIFACT_COMPRESSION=off
```

The managed pdf2zh pack is patched during staging/release builds so translated
text preserves source text color without using PDF faux-bold text stroke. For
simplified Chinese targets (`zh`, `zh-CN`, `zh-Hans`), normal translated text
uses BabelDOC's `SourceHanSansCN-Regular.ttf` instead of pdf2zh's upstream
`SourceHanSerifCN-Regular.ttf`. Paragraphs whose source text contains a
bold/medium font run use BabelDOC's `SourceHanSansCN-Bold.ttf` through a
separate PDF font resource named `notobold`. This keeps body text lighter,
restores source-like emphasis without text-rendering stroke, and uses the same
BabelDOC font assets on macOS and Windows.

## Worker Prewarm

App startup starts the persistent pdf2zh worker in the background after the
main window is shown. The worker prewarm now includes:

- importing pdf2zh and the ONNX layout runtime used by the Rosetta PDF
  component;
- checking the bundled ONNX DocLayout model path;
- optional MPS probing when explicitly enabled;
- loading the cached ONNX layout model and running one synthetic blank-page
  prediction at `imgsz=832`.

The synthetic prediction does not use document content. Its purpose is to move
YOLO's first predict-time setup out of the first translated page. If that
prediction fails, the worker still becomes ready and translation falls back to
the same behavior as before; the ready log records `yoloWarmupStatus`,
`yoloWarmupMs`, `yoloWarmupDevice`, and `yoloWarmupReason`.

In PDF v2 the ready event also reports the PDF engine `contractVersion`,
engine version, capabilities, and prewarm timings. Rosetta rejects an old PDF
component pack when the worker cannot report contract version `2`.

## Page State

`pdf_pages.<targetLang>.json` stores only durable statuses:

- `pending`: no committed translated page artifact.
- `translated`: the page is completed. `resultKind="translated"` has a valid
  `translatedPdfPath`; `resultKind="no_text"` intentionally has no translated
  PDF artifact and export keeps the source page for that page.
- `failed`: the last attempt for this page failed and can be retried.

Page commit is driven only by the formal `PageResult` returned by the PDF
engine. Diagnostics and timeline entries are not business inputs.

Commit rules:

- `status="translated"` requires a readable one-page PDF artifact.
- `sourceUnitCount > 0 && translatedChars == 0` fails the page.
- `emptyTranslationCount > 0` fails the page. The engine should only count
  required translation units in this field.
- `placeholderMismatchCount > 0` fails the page.
- `status="no_text"` completes the page with `resultKind="no_text"` and no
  translated artifact.
- `status="failed"`, provider failure, translation count mismatch,
  truncation, worker crash, and render failure all fail explicitly. They must
  never produce a successful blank artifact.

The UI may receive effective statuses:

- `pending`
- `queued`
- `translating`
- `translated`
- `failed`

`queued` and `translating` are derived from the active run. They are not
persisted as long-term facts. If an old state file contains them, reading the
file normalizes those pages to `pending`.

Page record:

```json
{
  "schemaVersion": 2,
  "pageNumber": 1,
  "status": "translated",
  "resultKind": "translated",
  "translatedPdfPath": "translated-pages/zh-CN/page-0001.pdf",
  "sourceUnitCount": 8,
  "translatedUnitCount": 8,
  "sourceChars": 1788,
  "translatedChars": 551,
  "artifactVersion": "1782369534004",
  "artifactCompression": "fast",
  "artifactBytes": 123456,
  "error": null,
  "updatedAt": "1782369534004",
  "lastRunId": "pdf-run-1782369534004"
}
```

## Run State

`pdf_run.<targetLang>.json` stores one current/latest run per job and target
language.

Run fields:

```json
{
  "runId": "pdf-run-1782369534004",
  "jobId": "job-1782369534004-document",
  "targetLang": "zh-CN",
  "state": "running",
  "mode": "continue",
  "requestedPages": [1, 2, 3],
  "completedPages": [1],
  "failedPages": [],
  "currentChunk": [2, 3],
  "ownerSessionId": "session-1234-1782369534000",
  "leaseUpdatedAt": "1782369535000",
  "cancelRequested": false,
  "startedAt": "1782369534004",
  "updatedAt": "1782369535000",
  "lastError": null
}
```

Run states:

- `running`: backend owns the run in this app session.
- `pausing`: user requested pause; the backend is stopping the current worker.
- `paused`: run can be resumed from remaining pages.
- `failed`: run stopped because of an error.
- `completed`: all requested pages are committed or accounted for.

## Translation Flow

1. Frontend calls `translate_rosetta_pdf_pages` through the typed client.
2. Backend repairs the job first.
3. Backend parses the requested page selection and chooses a mode:
   - `continue`
   - `retranslate-selected`
   - `retranslate-all`
4. Backend creates `PdfTranslationRun` and writes `pdf_run.<targetLang>.json`.
5. Pages are processed in windows. Small PDFs can use a wide window up to 30
   pages. Runs above 30 pages use fixed 10-page windows.
6. Rust sends `prepare_pdf_window` to the persistent worker. The worker calls
   the fork-owned Rosetta engine `prepareRun` and `collectUnits`, then returns
   a typed `PreparedRun` plus ordered `TranslationUnit[]`.
   Large source PDFs are prepared as selected-page windows: `sourcePageCount`
   still reports the full original page count, but the prepared PDF document
   contains only the requested pages for the current chunk/window.
   PDFs that contain broad duplicate text layers keep those duplicate units in
   order for render alignment, but mark the repeated layer as
   `requiresTranslation=false` so Rosetta does not draw two translated layers
   on top of each other.
   The patched renderer also draws paragraph-level white masks before
   translated text and keeps CJK line spacing above a legible floor; this
   avoids common overlap failures when a source PDF classifies ordinary prose
   as table/formula-like visual content or when translated CJK text needs more
   vertical leading than the original Latin text.
7. Rust translates all required units in the window through
   `translate_pdf_units`. Lightning uses large ordered unit batches to keep
   RWKV fed. Non-Lightning providers use the same typed unit contract with
   strict chunking, split retry, and truncation/empty-output rejection.
8. Rust sends `render_pdf_window` with `unitId -> translation`. The worker
   calls the engine `renderPages`, which emits one formal `PageResult` per
   page in page order.
9. Rust commits each `PageResult` immediately as it arrives. Translated pages
   are validated as readable one-page PDFs, moved to
   `translated-pages/<targetLang>/page-XXXX.pdf`, recorded with v2 metadata,
   and emitted to the UI. `no_text` pages are completed without pretending to
   have translated text or a translated artifact.
10. Rust sends `dispose_pdf_window` after render or cancellation. If worker
    state is not trustworthy, the worker is killed and the next run prewarms a
    fresh worker.
11. The run file is updated as pages complete, fail, pause, or finish. Job
    summary and translation-file summary are synced from reconciled page state.

The default continue path never overwrites translated pages. Explicit
retranslation clears the relevant page artifacts first.

## Long PDF Stability Policy

The primary PDF product target is a fast, live 1-30 page workflow. Larger PDFs
are supported, but stability and avoiding app stalls take priority over maximum
throughput.

Frontend behavior:

- PDFs with 30 pages or fewer default to selecting all pages.
- PDFs with more than 30 pages default to selecting the first 10 pages.
- The topbar exposes a first-10-pages shortcut next to all/clear selection.
- Translation requests with more than 50 selected pages require confirmation.
  The confirmation gives the user a one-click path back to the first 10 pages.
- During active PDF runs with more than 30 requested pages, the preview keeps
  page status updates live but pauses translated-page PNG rendering until the
  run stops or completes. Small runs keep live translated-page preview.

Backend behavior:

- All providers use the typed prepare/translate/render window contract.
- Lightning windows aggregate units across the window to keep RWKV batch size
  high; they do not fall back to per-page or per-paragraph tiny requests.
- Runs above 30 requested pages use 10-page windows. This sacrifices some
  maximum batch width on huge documents, but reduces first-visible page
  latency, event bursts, memory pressure, and webview render pressure.
- The first visible page is not blocked on the full document. Once a window's
  translations return, pages render and commit in order.

## Pause

Frontend uses:

```txt
pause_rosetta_pdf_run(jobId, targetLang, runId?)
```

The UI immediately enters a stopping state. The backend marks the run as
`pausing`, signals the active worker/process tree, preserves already committed
pages, and returns uncommitted pages to effective `pending`. The final run state
is `paused`.

## Force Quit Recovery

App startup creates a new `appSessionId`. When list/load/snapshot/repair sees a
PDF run that is `running` or `pausing` and owned by a different session, it:

- changes the run to `paused`
- clears `currentChunk`
- records a recovery warning
- validates page artifacts
- keeps valid artifacts as `translated`
- resets missing or damaged translated artifacts to `pending`

This is why force quitting during a 500-page PDF run should not leave permanent
`translating` pages.

## Repair

Repair runs when listing jobs, loading a PDF job, getting a PDF snapshot, or
calling `repair_rosetta_pdf_job(jobId)`.

Repair can:

- rebuild a minimal `document.json` from the index when `source.pdf` exists
- ensure `segments.json` exists
- write or update `pdf_source.json`
- recover stale live runs to `paused`
- copy readable legacy `pdf-pages/` artifacts into `translated-pages/`
- mark `translated` pages without valid artifacts as `pending`
- sync sidebar summary counts

Repair cannot recover a PDF job if `source.pdf` is gone.

## Duplicate Imports

Import does not implicitly de-duplicate PDFs. Importing the same file twice
creates two independent `jobId` directories, independent `source.pdf` copies,
independent page state files, and independent artifacts.

`sourceFingerprint` exists for diagnostics and future explicit de-duplication.
It does not alter import behavior.

## Delete

Delete is two-phase:

1. Remove the job from `index.json`.
2. Request cancellation for any active PDF run for that job.
3. Rename the job directory to `.trash/<jobId>-<timestamp>`.
4. Delete the trash directory.

If file locks prevent cleanup, Rosetta records a task in
`delete_cleanup_tasks.json`. Job listing retries pending cleanup. The sidebar
must not keep showing a job that has already been removed from the index.

The delete API returns:

```json
{
  "jobs": [],
  "cleanupStatus": "deleted",
  "warning": null
}
```

`cleanupStatus` may also be `pending-cleanup`, `not-found`, or `no-cache`.

## Preview

The canonical PDF data remains PDF:

- source: `source.pdf`
- translated pages: `translated-pages/<targetLang>/page-XXXX.pdf`

Current preview rendering uses a raster adapter because WebView-native PDF
rendering has not been proven reliable for Rosetta's source and pdf2zh output
fixtures. The raster adapter is a preview-only boundary:

- it does not write page translation state
- it does not decide export readiness
- it does not participate in repair
- it can be cleared without losing translation progress

If native PDF rendering is later verified on supported platforms, the raster
adapter should be replaced and this document updated.

## Backend API

PDF-specific commands:

- `get_rosetta_pdf_snapshot(jobId, targetLang?)`
- `translate_rosetta_pdf_pages(jobId, pageSelection, targetLang, ...)`
- `pause_rosetta_pdf_run(jobId, targetLang, runId?)`
- `repair_rosetta_pdf_job(jobId)`
- `delete_rosetta_job(jobId)`
- `export_rosetta_translated_pdf(jobId, targetPath, targetLang?)`

`get_rosetta_pdf_page_status` remains as a compatibility wrapper around the
snapshot command.

## Frontend Rules

- Use job id as the PDF identity. File names are display text only.
- Store PDF progress keyed by job id.
- Page-progress events must be ignored when their `jobId` or `targetLang` does
  not match the current view.
- Switching away from a PDF must not clear the active backend run.
- Delete, pause, repair failure, and open failure need visible feedback.
- The UI must not introduce chat, summarization, document Q&A, cloud sync, or
  account flows while working on PDF translation.

## Validation

Relevant commands:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```
