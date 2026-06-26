# PDF Pipeline

Last updated: 2026-06-26

This document describes the current PDF translation implementation. Older PDF
plans are historical background only when they conflict with this file and
ADR 0008.

Rosetta's PDF path is a local visual PDF translation pipeline:

1. Import copies the user's PDF into a job-local `source.pdf`.
2. The source PDF remains the authoritative source file.
3. Translation uses the local `pdf2zh` component through Rosetta's local
   OpenAI-compatible shim.
4. Translation commits one page-level PDF artifact at a time.
5. Export assembles a full PDF from `source.pdf` plus completed page artifacts.

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

Legacy files are read for compatibility:

```txt
pdf_page_translations.<targetLang>.json
pdf_page_translations.json
pdf-pages/<targetLang>/page-0001.pdf
pdf-pages/page-0001.pdf
```

New writes use `pdf_pages.<targetLang>.json` and `translated-pages/`.

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
  Translation runs also include `worker.stage` events emitted by the persistent
  pdf2zh worker for internal phases such as PDF preprocessing, YOLO layout
  inference, pdfminer page processing, patch application, single-page PDF save,
  and page event emission. `page.processPage` is further split into pdfminer
  `beginPage`, `renderStreams`, `endPage`, `receiveLayout`,
  `translateRequest`, and `patchStreams` events so the first visible
  page can be analyzed without treating pdfminer/TextConverter as one opaque
  block.
- `diagnostics/pdf-translation-profile-<runId>.json`: per-run aggregate profile
  for PDF translation. This remains the compact summary for one translation
  run; the timeline is the ordered event log used to reconstruct the chain.

Diagnostic files are not job state. Repair, preview, export, and resume logic
must continue to use `pdf_source.json`, `pdf_pages.<targetLang>.json`,
`pdf_run.<targetLang>.json`, and page artifacts as the source of truth.

## PDF Translation Concurrency

For local OpenAI-shim providers that do not report a supported batch size,
Rosetta uses a default PDF paragraph batch width of 8. The same value is passed
to the persistent worker as pdf2zh's `thread` count. Timeline diagnostics record
the effective thread count in the worker `job.started` stage and record every
`page.processPage.translateRequest`, making it possible to see whether a page
waited on one, two, or more TextConverter translation waves.

## Worker Prewarm

App startup starts the persistent pdf2zh worker in the background after the
main window is shown. The worker prewarm now includes:

- importing PyTorch, DocLayout-YOLO, and pdf2zh;
- checking the bundled DocLayout model path;
- optional MPS probing when explicitly enabled;
- loading the cached YOLO model and running one synthetic blank-page
  prediction at 596x842 px (`imgsz=832`).

The synthetic prediction does not use document content. Its purpose is to move
YOLO's first predict-time setup out of the first translated page. If that
prediction fails, the worker still becomes ready and translation falls back to
the same behavior as before; the ready log records `yoloWarmupStatus`,
`yoloWarmupMs`, `yoloWarmupDevice`, and `yoloWarmupReason`.

## Page State

`pdf_pages.<targetLang>.json` stores only durable statuses:

- `pending`: no committed translated page artifact.
- `translated`: `translatedPdfPath` points to a valid page PDF.
- `failed`: the last attempt for this page failed and can be retried.

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
  "pageNumber": 1,
  "status": "translated",
  "translatedPdfPath": "translated-pages/zh-CN/page-0001.pdf",
  "artifactVersion": "1782369534004",
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
5. Pages are processed in chunks of 10.
6. Each chunk writes pdf2zh output under `.tmp/pdf-runs/<runId>/...`.
7. Each completed page is validated as a PDF, moved to
   `translated-pages/<targetLang>/page-XXXX.pdf`, then written to
   `pdf_pages.<targetLang>.json`.
8. The run file is updated as pages complete, fail, pause, or finish.
9. Job summary and translation-file summary are synced from reconciled page
   state.

The default continue path never overwrites translated pages. Explicit
retranslation clears the relevant page artifacts first.

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
