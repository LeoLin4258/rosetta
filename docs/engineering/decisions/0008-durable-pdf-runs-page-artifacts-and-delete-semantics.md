# 0008. Durable PDF Runs, Page Artifacts, and Delete Semantics

Date: 2026-06-25
Status: accepted

## Context

The PDF translation path had drifted into a patch-heavy model:

- A large PDF run could mark hundreds of pages as `translating` before any
  page artifact was committed.
- `queued` and `translating` were persisted as if they were durable facts,
  so force quitting the app could leave a job permanently in progress.
- Page state and page artifacts were not reconciled. A page could say
  `translated` while the corresponding page PDF no longer existed.
- Delete removed files directly from the active job directory. On Windows,
  a worker or preview file lock could leave the job half-deleted while the
  sidebar still pointed at it.
- Several older documents describe earlier PDF approaches. They are useful
  history, but the implementation now uses a visual `pdf2zh` pipeline with
  per-page PDF artifacts, not the original text-segment PDF plan.

The broken job reported as `原则-（中文版）瑞·达利欧.pdf` matched this failure
mode: the index entry still existed, `document.json` was missing, many pages
were stuck in `translating`, and some `translated` pages pointed at missing
artifacts.

## Decision

PDF translation jobs now use a durable per-job, per-target-language model.

### File Contract

Each PDF job has four kinds of files:

- Authoritative source and metadata:
  - `source.pdf`
  - `document.json`
  - `segments.json`
  - `translation_files.json`
  - `pdf_source.json`
  - `pdf_pages.<targetLang>.json`
  - `pdf_run.<targetLang>.json`
- Translation artifacts:
  - `translated-pages/<targetLang>/page-0001.pdf`
- Temporary run files:
  - `.tmp/pdf-runs/<runId>/...`
- User exports:
  - `exports/...`

Legacy `pdf_page_translations.<targetLang>.json`,
`pdf_page_translations.json`, `pdf-pages/<targetLang>/...`, and
`pdf-pages/page-000N.pdf` remain readable for migration and repair. New writes
use the `pdf_pages.*.json` and `translated-pages/` layout.

### Page State

`pdf_pages.<targetLang>.json` is the durable page-state authority. It only
persists:

- `pending`
- `translated`
- `failed`

`queued` and `translating` are effective UI states computed from the active
run. They are never written as long-term facts. Reading an old state file that
contains `queued` or `translating` normalizes those pages back to `pending`.

Each page record contains:

- `pageNumber`
- `status`
- `translatedPdfPath`
- `artifactVersion`
- `error`
- `updatedAt`
- `lastRunId`

### Run State

Each `jobId + targetLang` has one latest run file:

- `pdf_run.<targetLang>.json`

The run records:

- `runId`
- `jobId`
- `targetLang`
- `state`
- `mode`
- `requestedPages`
- `completedPages`
- `failedPages`
- `currentChunk`
- `ownerSessionId`
- `leaseUpdatedAt`
- `cancelRequested`
- `startedAt`
- `updatedAt`
- `lastError`

A live run belongs to one app session. App startup creates a new session id.
If a PDF job contains an old `running` or `pausing` run owned by another
session, repair changes it to `paused`, clears `currentChunk`, and leaves
completed page artifacts intact.

### Scheduling

Page translation runs process at most 10 pages per `pdf2zh` invocation. A
500-page PDF is therefore handled as many small chunks rather than one
uninterruptible worker call.

For each page artifact:

1. `pdf2zh` writes the page output into `.tmp/pdf-runs/<runId>/...`.
2. Rosetta validates that the output is readable as a single-page PDF.
3. The page PDF is moved to `translated-pages/<targetLang>/page-XXXX.pdf`.
4. Only after the artifact is committed does Rosetta atomically write the
   page state as `translated`.

JSON writes use a temporary file plus rename, so app termination should not
leave half-written state files.

### Pause And Recovery

Pause is a run-level operation, not a global app-level toggle. The frontend
calls `pause_rosetta_pdf_run(jobId, targetLang, runId?)`.

When pause is requested:

- The durable run enters `pausing`.
- The active pdf2zh worker/process tree receives cancellation.
- Pages already committed as `translated` remain translated.
- Pages in the current uncommitted chunk return to effective `pending`.
- The run ends as `paused`.

If the app is force-quit, startup repair uses the same durable files to make
the job usable again. Translated pages with valid artifacts remain translated;
translated pages whose artifacts are missing become pending.

### Re-translate Semantics

Default continue mode translates only `pending` and `failed` pages.
Translated pages are not overwritten.

Explicit overwrite modes are separate:

- `retranslate-selected` clears only the selected page artifacts and state,
  then starts a new run.
- `retranslate-all` clears all page artifacts and state for the target
  language, then starts a new run.

### Delete Semantics

Deleting a job is two-phase:

1. Remove the job from `index.json`, so the sidebar stops pointing at it.
2. Request cancellation for any active PDF run owned by the job.
3. Rename the job directory to `.trash/<jobId>-<timestamp>`.
4. Delete the trash directory.

If rename or delete fails, Rosetta records `delete_cleanup_tasks.json` and
retries on future job-list loads. The index must not continue to reference
the half-deleted job.

## Consequences

- A force-quit cannot leave permanent `translating` pages because those
  states are not durable facts.
- Large PDFs become pausable at chunk boundaries, and active workers are
  interrupted on pause/delete.
- Repeated import of the same PDF creates independent jobs. The fingerprint
  in `pdf_source.json` is diagnostic metadata, not implicit de-duplication.
- Sidebar and preview state are keyed by job id and target language, not file
  name.
- Rasterized preview remains an adapter for WebView rendering. It is not a
  translation authority. The canonical source and translated artifacts are PDF
  files.

## Follow-up

- If native PDF rendering becomes reliable in the Tauri WebView for both
  source and pdf2zh output fixtures, replace the raster preview adapter with a
  native PDF preview.
- If multiple simultaneous PDF runs are needed, replace the single in-memory
  cancel slot with a keyed run registry. The durable file model already uses
  `jobId + targetLang`.
