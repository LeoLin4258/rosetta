# Long PDF Stability UX Plan

Date: 2026-07-02

Status: implemented first pass, keep measuring.

## Context

Rosetta's PDF performance pass made 10-18 page PDFs much faster on Windows
NVIDIA Lightning after the PDF component fork, persistent worker prewarm,
Lightning direct batch path, page artifact hot-path writes, and background
compression.

The next observed failure mode was not small-PDF throughput. It was very large
PDF stability:

- A 483-page forced PDF run completed in about 478 seconds.
- The same run used 100-page Lightning chunks, causing about 95 seconds before
  the first page artifact was committed.
- Page commits arrived in 80-100 page bursts near the end of each chunk.
- Intermediate translated page artifacts totalled about 4.18 GB, around 8.7 MB
  per page before background compression.
- The webview could become sluggish or freeze when the user tried a 400-page
  PDF with all pages selected.

The product goal is therefore:

- 1-30 page PDFs should feel fast, live, and easy to inspect.
- Huge PDFs should remain possible, but the app must not freeze or crash.
- Huge-PDF speed is secondary to stable scheduling, clear user control, and
  bounded preview pressure.

## Policy

### Default Page Selection

When a PDF is opened:

- 30 pages or fewer: select all pages.
- More than 30 pages: select the first 10 pages.

This keeps the common "inspect the first few pages" workflow cheap. Users can
still select all pages explicitly from the topbar.

### Long Range Confirmation

Before translating more than 50 selected pages, the workspace shows a
confirmation dialog.

The dialog tells the user that the run may take longer, use more disk, and lower
live translated-preview rendering while it runs. It offers:

- cancel;
- translate the first 10 pages instead;
- continue with the selected long range.

This is a product guardrail, not a hard limit.

### Stable Preview Mode

For active PDF runs with more than 30 requested pages:

- source-page preview stays virtualized and visible;
- page status events still update live;
- translated-page PNG rendering is paused until the run stops or completes.

For active runs of 30 pages or fewer, live translated-page preview remains
enabled so the small-PDF experience keeps showing each page as it is committed.

### Backend Chunk Policy

Non-Lightning providers keep the page-local path and durable 10-page chunk
default.

Lightning policy:

- 30 requested pages or fewer: 100-page chunk ceiling, so small PDFs keep the
  wide cross-page batch speed path.
- More than 30 requested pages: 10-page chunks, so large PDFs reduce
  time-to-first-page, event burst size, and webview pressure.
- `ROSETTA_PDF_LIGHTNING_PAGE_CHUNK_SIZE` remains an explicit local diagnostic
  override.

## Implemented Changes

- Added `rosetta-app/src/lib/pdfPageSelectionPolicy.ts` as the shared threshold
  source for default selection and long-range confirmation.
- Changed PDF preview default selection to use the policy above.
- Added a topbar first-N-pages shortcut for long PDFs.
- Added a confirmation dialog for PDF translations above the long-range
  threshold.
- Added stable preview mode for active PDF runs above 30 requested pages.
- Changed the Lightning long-run chunk threshold from more than 50 pages to more
  than 30 pages.
- Added a Rust boundary test for the Lightning PDF chunk policy.
- Updated `docs/engineering/pdf-pipeline.md` with the current behavior.

## Validation Plan

Automated:

```powershell
cd rosetta-app
.\node_modules\.bin\tsc.cmd --noEmit
cd src-tauri
cargo fmt -- --check
cargo test rosetta_jobs
cargo test managed_pdf2zh
cd ..\..
git diff --check
```

Manual:

- Open a 10-page PDF and confirm all pages are selected by default.
- Force translate the 10-page PDF and confirm translated pages appear as each
  page commits.
- Open a 400+ page PDF and confirm only the first 10 pages are selected by
  default.
- Click all pages, then translate or force retranslate, and confirm the long
  range dialog appears.
- Choose first 10 pages from the dialog and confirm the run starts with 10
  requested pages.
- Choose continue for a long range and confirm the app remains responsive while
  statuses update.
- Pause a long PDF run and confirm already committed pages are available.

## Follow-Up Work

1. Add a proper page range editor.

   The current topbar supports all, clear, first 10, and individual checkboxes.
   For huge PDFs, users also need an efficient range entry such as `1-20, 45-60`.

2. Add persistent long-PDF preference.

   A future setting could default huge PDFs to first 5, first 10, first 30, or no
   pages selected, but the current fixed first-10 policy is simpler for release.

3. Throttle page progress patching if needed.

   Chunk size 10 should reduce event bursts. If a later profile still shows
   render pressure, coalesce page progress events in the frontend before
   updating React state.

4. Add export-only or background mode.

   For 300+ page PDFs, a user may want translation and export without live
   preview. That should be an explicit mode, not the default for small PDFs.

5. Continue PDFMathTranslate fork work.

   The largest remaining PDF-side costs are still collect/replay/layout work.
   Deep fork work could expose a Rosetta-native collect/replay API, avoid
   driving the full conversion machinery twice, and add a reusable layout cache.

## Non-Goals

- Do not optimize huge PDFs by regressing 1-30 page live preview.
- Do not make huge-PDF full translation impossible.
- Do not log document text, prompts, translated text, or layout content in
  diagnostics.
- Do not add cloud upload, account, sync, telemetry, summarization, document QA,
  or generic assistant behavior.
