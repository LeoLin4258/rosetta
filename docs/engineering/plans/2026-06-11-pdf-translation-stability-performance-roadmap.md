# PDF Translation Stability and Performance Roadmap

> Historical roadmap. This document explains the problems and staged thinking
> before the durable PDF run/page-artifact refactor. The current implementation
> contract lives in `docs/engineering/pdf-pipeline.md` and ADR 0008. When this
> roadmap conflicts with those documents or current code, treat it as
> historical context only.

## Summary

This document records the current PDF translation problems reported in Rosetta, the working mental model behind them, and a staged repair route. It is intentionally a planning document, not an implementation patch.

The central finding is that Rosetta's PDF path is now a page-level visual PDF pipeline built around `pdf2zh`, page status files, and PNG preview rendering. It no longer behaves like the Markdown/TXT segment pipeline. Several visible problems come from treating a PDF translation run as one pseudo segment in the UI while the backend is actually processing pages and spawning `pdf2zh` work.

Before optimizing PDF translation speed, Rosetta needs a measurement mechanism that separates RWKV model time from PDF parsing, layout, process startup, page assembly, and preview rasterization. Without that, the "PDF is 10x slower than Markdown for similar text volume" problem cannot be fixed rigorously.

## Product Boundary

This work must preserve Rosetta's existing product scope:

- local-first document translation
- privacy-sensitive local files
- long-form document translation
- local or explicitly configured translation backend
- no chat, cloud upload, account, sync, collaboration, summarization, or document Q&A

PDF repair work should remain inside the document translation workflow. It should not turn PDF support into a separate product mode unless an ADR records a broader architecture change.

## Current Architecture Snapshot

PDF import and preview currently have two overlapping histories:

- The original PDF v1 plan described text extraction into `RosettaDocument`, `RosettaBlock[]`, and `Segment[]`.
- The current visual PDF path treats PDF as a layout-preserving document and uses `pdf2zh --pages` to generate page-level translated PDF files under `pdf-pages/`.

Relevant implementation areas:

- Frontend PDF run orchestration:
  - `rosetta-app/src/features/workspace/WorkspacePage.tsx`
- Frontend PDF status bar:
  - `rosetta-app/src/features/workspace/WorkspaceTopbar.tsx`
- PDF side-by-side preview:
  - `rosetta-app/src/features/preview/PdfDocumentPreview.tsx`
  - `rosetta-app/src/features/preview/PdfPane.tsx`
- Tauri PDF commands and page translation loop:
  - `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- PDF page state:
  - `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/page_state.rs`
- `pdf2zh` invocation and progress events:
  - `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`
- OpenAI-compatible shim used by `pdf2zh` to call RWKV:
  - `rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs`
- Welcome document creation:
  - `rosetta-app/src-tauri/src/rosetta_jobs/import.rs`
  - `rosetta-app/src/app/AppShell.tsx`

## Reported Problems

### 1. Large PDF Translation Cannot Be Stopped Reliably

User-visible symptom:

- When translating a large PDF, clicking stop does not reliably stop the translation.
- User may need to wait too long or restart the app.

Current mental model:

- Frontend `WorkspacePage` stores a cancel callback for PDF runs that invokes `cancel_rosetta_translated_pdf`.
- Backend `PdfTranslationCancelState` currently stores one global `oneshot::Sender`.
- The page translation loop creates a fresh cancellation channel for each page.
- `invoke_pdf2zh` listens for cancellation and calls `child.kill().await`.

Likely risk points:

- Cancellation is global, not keyed by run/job/page.
- Cancellation only targets the currently registered page invocation.
- If `pdf2zh` spawns child processes, killing only the immediate child may leave process descendants alive.
- There is no durable "run was cancelled" state distinct from page statuses.
- UI state can clear before backend state has fully settled.

Target behavior:

- Stop request exits the active PDF translation run quickly.
- Current translating page returns to `pending`.
- Completed pages stay `translated`.
- Failed pages stay `failed`.
- UI exits translating state without requiring app restart.
- No orphan `pdf2zh`, Python, or shim processes remain.

### 2. PDF Translation Is Much Slower Than Markdown for Similar Text

User-visible symptom:

- Similar text volume translates at least 10x faster as Markdown than as PDF.
- User suspects the model is not the bottleneck.

Current mental model:

- Markdown goes through Rosetta's segment batch translation path.
- PDF page translation currently loops pages and invokes `pdf2zh` for each page with `--pages <page>`.
- Per-page invocation may repeatedly start Python, start/prepare the shim, parse the PDF, recover layout, render output, and assemble page artifacts.

Primary hypothesis:

- RWKV time may be only a small portion of total PDF time.
- The large overhead is likely in repeated PDF parsing/layout/rendering and process startup.

Target behavior:

- Rosetta records where PDF translation time is spent.
- A single PDF run can report model time vs non-model time.
- Optimization work is driven by measured bottlenecks, not guesses.

### 3. PDF Status Bar Is Too Coarse and Sometimes Wrong

User-visible symptom:

- Status stays around "translating" or shows coarse information.
- After switching away and back, progress can reset to `0 / 1 · 0% · 00:02`.
- Total pages and current page can be wrong.

Current mental model:

- Frontend registers a PDF run as one pseudo target: `pdf-pages:<selection>`.
- Generic progress therefore sees one target and can display `0 / 1`.
- Real page progress is sent through `rosetta-pdf2zh-progress` events, but the live `pdfProgress` object is in component memory.
- When active file changes, the PDF progress subscription and in-memory state can be lost.

Target behavior:

- PDF progress uses page-level run state, not segment count.
- Switching files and returning restores progress from durable page state plus active run state.
- Status text reflects phases such as preparing engine, parsing layout, translating, rendering, assembling, and preview rendering.

### 4. PDF Translation State Resets When Switching Files

User-visible symptom:

- During PDF translation, switching to another file and coming back resets displayed state to an initial-looking state.
- Current page and total page values are wrong.

Current mental model:

- Page state exists in `pdf_page_translations.json`, but active run metadata does not appear to be a durable first-class object.
- The UI active translation run is stored in Zustand memory as pseudo segment IDs.
- `pdfProgress` is event-driven and not restored after navigation.

Target behavior:

- Page statuses survive navigation and reload.
- Active PDF run state can be reconstructed enough for UI display.
- If no active run exists, leftover `queued` or `translating` pages are safely restored to retryable state on load.

### 5. PDF Side-by-Side Scroll Sync Is Misaligned

User-visible symptom:

- Source and translated PDF panes drift out of alignment.

Current mental model:

- Sync currently maps scroll position by total scroll ratio between panes.
- Ratio sync breaks when:
  - translated pages are placeholders
  - source and translated pages have different rendered heights
  - PNG pages load at different times
  - page gaps or sticky controls differ

Target behavior:

- Scroll sync should align by page number and page-local offset.
- If translated page is not ready, sync to that page's placeholder.
- Page N source should line up with page N translation/placeholder.

### 6. PDF Translation Preview Is Blurry on Large Screens

User-visible symptom:

- Translated PDF preview is not clear on a large screen.
- User asks why the current PDF translation is an image, not a real PDF.

Current mental model:

- `PdfPane` intentionally rasterizes pages as PNGs via pdfium.
- Comments record that `pdfjs` had CJK/font subset issues for generated PDFs, and `<embed>` did not work reliably in Tauri WKWebView app mode.
- The exported PDF is still a real PDF; the preview pane is a rasterized display.
- Pane rasterization width is measured once per job, not continuously on resize.

Target behavior:

- UI clearly distinguishes exported PDF fidelity from preview rasterization.
- Preview rasterization adapts to large panes and high-DPI screens.
- User gets zoom controls and sharper previews without unbounded memory use.

### 7. Welcome Markdown Does Not Display Reliably on First Startup

User-visible symptom:

- First launch welcome Markdown sometimes displays and sometimes does not.
- User has not found a clear pattern.

Current mental model:

- `AppShell` creates the welcome document only when `listRosettaJobs()` returns an empty list.
- Once any job exists, welcome content is not created or shown.
- The welcome document is implemented as a fixed `job-welcome` Markdown job.
- The Markdown includes an absolute `/icon.png` image reference, which may be fragile in the Tauri webview depending on route/origin.

Target behavior:

- First-run welcome is deterministic.
- Empty workspace content is not dependent on the job index being empty in a fragile way.
- The welcome view should not pollute real user job semantics unless explicitly kept as a sample document.

## Measurement Mechanism Required Before Speed Fixes

### Goal

Add a local PDF translation profile that answers:

- How long did the full PDF translation run take?
- How much of that time was RWKV translation?
- How much was PDF parsing/layout/rendering/assembly/process startup?
- How does PDF compare to Markdown for similar text volume?

### Profile Output

Each PDF run should write a local diagnostics file under the job directory:

```txt
<job_dir>/diagnostics/pdf-translation-profile-<runId>.json
```

The profile should not include full document text. It may include counts, sizes, durations, phases, page numbers, and error strings.

Suggested shape:

```json
{
  "schemaVersion": 1,
  "runId": "run-pdf-pages-1710000000000-abcd12",
  "jobId": "job-...",
  "targetLang": "zh-CN",
  "sourceLang": "en",
  "pageSelection": "1-120",
  "pagesRequested": 120,
  "pagesProcessed": 120,
  "sourceCharsEstimated": 84000,
  "startedAt": "2026-06-11T10:00:00.000Z",
  "endedAt": "2026-06-11T10:10:20.000Z",
  "durationsMs": {
    "total": 620000,
    "pdf2zhWarmup": 18000,
    "pdfParseLayout": 240000,
    "rwkvTranslate": 52000,
    "pdfRenderAssemble": 285000,
    "pageArtifactAssembly": 12000,
    "other": 13000
  },
  "rwkv": {
    "requestCount": 96,
    "totalInputChars": 84000,
    "totalOutputChars": 72000,
    "averageRequestMs": 540,
    "maxRequestMs": 2100,
    "failedRequestCount": 0
  },
  "pages": [
    {
      "pageNumber": 1,
      "status": "translated",
      "totalMs": 5100,
      "rwkvMs": 430,
      "nonRwkvMs": 4670
    }
  ]
}
```

### Timing Boundaries

Record these boundaries:

- `total`: frontend command start to backend command completion.
- `pdf2zhWarmup`: status resolution, output directory setup, shim spawn, command spawn.
- `pdfParseLayout`: inferred from `pdf2zh` output lines for parse/layout phase.
- `rwkvTranslate`: measured inside `managed_pdf2zh/openai_shim.rs` around requests to RWKV.
- `pdfRenderAssemble`: inferred from `pdf2zh` render/save phase plus output collection.
- `pageArtifactAssembly`: extraction of a single translated page PDF and write to `pdf-pages/page-000N.pdf`.
- `previewRasterize`: measured separately when rendering PDF page PNGs for preview. Do not mix this into translation total.

### Markdown Comparison

Add a comparable Markdown translation profile, or at minimum a debug command that reports:

- source segment count
- source character count
- batch count
- total translation time
- RWKV request time
- non-RWKV orchestration time
- milliseconds per thousand source characters

The comparison should make the slow path visible:

```txt
PDF total:      620s
PDF RWKV:        52s
PDF non-RWKV:   568s

Markdown total:  58s
Markdown RWKV:   51s
Markdown other:   7s
```

If the numbers look like this, optimization should target PDF parsing/rendering/process strategy, not model inference.

## Repair Roadmap

### Phase 0: Add Diagnostics and Baseline Tests

Purpose:

- Make PDF performance measurable before changing behavior.
- Confirm whether RWKV or PDF processing dominates runtime.

Work items:

- Add a PDF translation profile writer in the Rust PDF path.
- Add RWKV request timing in `managed_pdf2zh/openai_shim.rs`.
- Add per-page timing in `translate_rosetta_pdf_pages`.
- Add preview PNG render timing in `render_rosetta_pdf_page_as_png` and `render_rosetta_pdf_translated_page_as_png`, recorded separately.
- Add a debug UI entry or diagnostics export command to open/copy the latest profile path.
- Add unit tests for profile aggregation math that do not require a real PDF.

Acceptance:

- Translating a PDF creates one profile JSON.
- Profile separates RWKV time from non-RWKV time.
- Cancelling a PDF run still writes a partial profile marked cancelled.
- No source text or translated text is written to diagnostics.

Validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

### Phase 1: Make PDF Translation Runs First-Class

Purpose:

- Stop representing a PDF run as one pseudo segment.
- Make progress and navigation restore behavior correct.

Work items:

- Introduce a frontend PDF run shape with page-level fields:
  - `runId`
  - `jobId`
  - `sourceFileId`
  - `translationFileId`
  - `pageSelection`
  - `pagesTotal`
  - `pagesCompleted`
  - `pagesFailed`
  - `currentPage`
  - `phase`
  - `startedAt`
  - `cancellable`
- Keep text/Markdown `ActiveTranslationRun` behavior intact.
- Hydrate PDF progress display from `pdf_page_translations.json` when switching back to a PDF file.
- Use `rosetta-pdf-page-progress` and `rosetta-pdf2zh-progress` as live updates, but not as the only source of truth.

Acceptance:

- PDF topbar never shows `0 / 1` for page translation.
- Switching away and back shows correct selected/translated/failed page state.
- During an active run, status displays real current phase and page progress.
- If the app reloads after a crash, stale `translating` pages become retryable.

Validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

### Phase 2: Harden PDF Cancellation

Purpose:

- Make "stop translation" reliable for large PDFs.

Work items:

- Key cancellation state by run/job instead of storing only one global sender if concurrent or stale runs are possible.
- Ensure cancel requests are idempotent.
- Confirm `child.kill()` terminates the full `pdf2zh` process tree on supported platforms.
- If needed, start `pdf2zh` in a process group and terminate the group.
- Ensure cancellation between pages exits the loop before starting the next page.
- Persist current page as `pending` on cancellation.
- Write a partial diagnostics profile with `status: "cancelled"`.

Acceptance:

- Stop exits UI translating state quickly.
- Current page becomes `pending`.
- Already translated pages remain translated.
- Re-running translation resumes from pending/failed pages.
- No orphan PDF translation subprocess remains after cancellation.

Validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Manual validation should include a large PDF because process cancellation is hard to prove with unit tests alone.

### Phase 3: Optimize PDF Processing Strategy

Purpose:

- Reduce the 10x gap against Markdown once diagnostics identify bottlenecks.

Likely strategies to evaluate:

- Batch several selected pages into one `pdf2zh --pages` invocation instead of invoking one page at a time.
- For full-document translation, invoke `pdf2zh` once and split the output into page caches.
- Cache parse/layout work if `pdf2zh` exposes stable artifacts.
- Avoid regenerating already translated pages unless `force` is enabled.
- Avoid preview rasterization work during translation if it blocks the translation path.

Decision rule:

- If RWKV time is similar between Markdown and PDF, optimize non-RWKV phases first.
- If `pdfParseLayout` dominates, reduce repeated parsing.
- If `pdfRenderAssemble` dominates, reduce repeated render/assembly.
- If `pdf2zhWarmup` dominates for small pages, reduce process startup count.

Acceptance:

- Same text volume PDF no longer spends most time in repeated startup/parse work.
- Page-level retry remains available.
- Force retranslating selected pages still works.
- Existing exported PDF behavior is preserved.

Validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Performance validation should compare profile output before and after the change using the same PDF and a Markdown text extraction of comparable size.

### Phase 4: Improve PDF Status Bar and User Feedback

Purpose:

- Make long PDF runs understandable.

Work items:

- Replace generic "翻译中" for PDF with phase-specific text:
  - preparing engine
  - parsing PDF layout
  - translating page X/Y
  - rendering translated PDF
  - assembling page cache
  - rendering preview
- Show elapsed time and percent when available.
- Show "RWKV Xs / PDF processing Ys" in diagnostics or an advanced detail view, not necessarily in the primary toolbar.
- Keep the primary toolbar concise.

Acceptance:

- User can tell whether the app is parsing, translating, rendering, or previewing.
- Progress remains meaningful when `pdf2zh` output is temporarily silent.
- No misleading segment counts appear for PDF.

Validation:

```bash
cd rosetta-app
pnpm typecheck
```

### Phase 5: Fix PDF Scroll Synchronization

Purpose:

- Align source and translated PDF pages predictably.

Work items:

- Replace whole-pane ratio sync with page-anchor sync.
- Track the first visible page and page-local offset in the scrolling pane.
- Scroll the other pane to the same page and proportional local offset.
- If the target page is not translated, scroll to its placeholder.
- Keep a small echo-suppression guard to avoid scroll feedback loops.

Acceptance:

- Page N aligns with page N in both panes.
- Loading translated images does not cause long-term drift.
- Placeholder pages still participate in sync.

Validation:

```bash
cd rosetta-app
pnpm typecheck
```

Manual validation should cover:

- untranslated PDF
- partially translated PDF
- fully translated PDF
- large window
- narrow window

### Phase 6: Improve Large-Screen PDF Preview Clarity

Purpose:

- Make rasterized PDF preview clear without changing exported PDF semantics.

Work items:

- Use `ResizeObserver` to update pane width when the preview area changes.
- Request raster width based on visible pane width and `devicePixelRatio`.
- Add a maximum raster width to avoid excessive memory use.
- Add zoom controls:
  - fit width
  - 100%
  - 125%
  - 150%
- Consider lazy rendering or virtualization for large PDFs before raising raster sizes too aggressively.

Acceptance:

- Large-screen preview is visibly sharper.
- Exported PDF remains a real PDF.
- Preview memory use remains bounded.
- User understands preview is rasterized while export is PDF.

Validation:

```bash
cd rosetta-app
pnpm typecheck
```

Manual validation should inspect pages on high-DPI and large external displays.

### Phase 7: Stabilize Welcome Markdown

Purpose:

- Make first-run welcome behavior deterministic.

Work items:

- Decide whether welcome content is:
  - an empty-home view, or
  - a sample document/job.
- Prefer an empty-home view if the content is onboarding/help, because it should not depend on the job index.
- If keeping `job-welcome`, exclude it from user job semantics or make its lifecycle explicit.
- Remove or replace fragile Markdown image reference `![Rosetta](/icon.png)`.
- Ensure first launch, returning launch with jobs, and cleared-local-data launch each have deterministic behavior.

Acceptance:

- First launch always shows welcome or empty workspace content.
- Existing user jobs do not suppress essential onboarding information accidentally.
- Welcome content does not depend on fragile absolute asset paths.

Validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
```

## Test Strategy

### Unit Tests

Add Rust unit tests for:

- PDF page selection parsing edge cases.
- Page status summary after translated/failed/pending/cancelled states.
- Diagnostics profile duration aggregation.
- Diagnostics privacy guard that rejects full source/translated text fields.
- Stale `queued` / `translating` page states loading as `pending`.

Add TypeScript tests if the project already has a test harness available for:

- PDF topbar progress formatting.
- PDF run state restoration helpers.
- Page-anchor scroll calculation helpers.

Do not introduce a new frontend test framework solely for this work unless it is clearly worth the maintenance cost.

### Integration Tests

Use existing Rust/Tauri seams where possible:

- Run PDF page translation with a mocked or fake `pdf2zh` command that emits known phase lines.
- Verify page state writes and profile output.
- Verify cancellation marks current page pending and writes a cancelled profile.

If mocking `pdf2zh` is not currently easy, document that gap and keep the first pass to pure profile/page-state unit tests plus manual validation.

### Manual Performance Protocol

Use the same content in two forms:

- A Markdown file containing the extracted text.
- A PDF containing equivalent text volume.

For each run record:

- total wall time
- RWKV time
- non-RWKV time
- source character count
- pages
- requests
- milliseconds per thousand source characters

Run at least:

- one short 1-3 page PDF
- one medium 20-30 page PDF
- one large PDF similar to the user-reported case
- one comparable Markdown file

## Documentation Updates

Update engineering docs when implementation begins:

- If the PDF run state model changes, update `docs/engineering/conventions/data-models.md`.
- If a new durable diagnostics file is added, document it in the data model conventions.
- If process-group cancellation or batch PDF invocation becomes a long-term architecture decision, add an ADR under `docs/engineering/decisions/`.
- For substantial implementation work, add a change-log entry under `docs/engineering/change-log/`.

## Open Questions

- Should PDF diagnostics be exposed in the UI, or only saved under job diagnostics for support/debugging?
- Should `pdf_page_translations.json` become target-language keyed if multiple PDF target languages are supported concurrently?
- Can `pdf2zh` efficiently translate multiple selected pages in one invocation without losing page-level retry semantics?
- Does killing the immediate `pdf2zh` process terminate all child processes on macOS and Windows, or do we need explicit process group handling?
- Should the welcome screen remain a generated Markdown document, or become a dedicated empty workspace view?

## Recommended Implementation Order

1. Add PDF diagnostics and baseline performance profile.
2. Make PDF run state page-level and restorable.
3. Harden cancellation.
4. Optimize PDF invocation strategy using measured profile output.
5. Improve status bar copy and phase display.
6. Replace ratio scroll sync with page-anchor sync.
7. Improve raster preview clarity and zoom.
8. Stabilize welcome Markdown / first-run behavior.

The first three phases should be treated as one stability milestone. Performance optimization should not start before profile output can prove where time is being spent.
