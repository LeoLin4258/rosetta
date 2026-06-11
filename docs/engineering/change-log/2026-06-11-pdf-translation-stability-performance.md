# 2026-06-11 PDF Translation Stability & Performance Fixes

Implements the stability milestone from
`docs/engineering/plans/2026-06-11-pdf-translation-stability-performance-roadmap.md`,
with deviations noted below where the roadmap's assumptions didn't match the
root causes found in code.

## Backend (Rust)

### Cancellation hardened (root cause: lost cancel signals + orphan process tree)

- `PdfTranslationCancelState` is now a level-triggered `AtomicBool` plus the
  per-invocation oneshot sender. A stop request that lands between two pdf2zh
  invocations (when no sender is registered) is no longer silently dropped —
  the run loop checks the flag before each chunk.
- The state also acts as a single-flight guard: a second
  `translate_rosetta_pdf_pages` / `generate_rosetta_translated_pdf` call while
  one is running returns an error instead of fighting over the global sender.
- pdf2zh is started in its own process group (`process_group(0)`, unix) and
  cancellation kills the whole group: SIGTERM, 1.5 s grace, then SIGKILL.
  Previously only the immediate Python launcher was killed; its
  multiprocessing workers survived and kept hitting the RWKV server, which is
  why "stop" appeared to do nothing on large PDFs.
- Cancelled chunks persist their pages back to `pending`; completed pages stay
  `translated`.

### Per-page invocation replaced with chunked batch invocation (root cause of the 10x gap)

- `translate_rosetta_pdf_pages` now passes up to 10 pages per
  `pdf2zh --pages a,b,c-d` invocation (`PDF_PAGES_PER_INVOCATION`) instead of
  one process per page. Each invocation pays several seconds of fixed
  overhead (Python startup, shim spawn, RWKV role-set HTTP, full-document
  parse); per-page invocation made that overhead dominate runtime.
- Chunk output is split into the existing `pdf-pages/page-NNNN.pdf` cache via
  `extract_pages_pdf` (loads the output PDF once, clones per page). Page
  mapping is defensive: a full-document output maps by original page number,
  an output with exactly the selected pages maps positionally, anything else
  fails the chunk with a clear error.
- Page-level retry/force semantics are unchanged; chunks persist state after
  each invocation so partial progress survives cancel/crash.

### Diagnostics profile (roadmap Phase 0, simplified)

- Each run writes `<job_dir>/diagnostics/pdf-translation-profile-<runId>.json`
  with wall-clock totals, per-phase durations (warmup / pdf2zh process /
  page artifact assembly) and aggregated RWKV request stats (count, total /
  average / max ms, char counts) collected inside the OpenAI shim
  (`ShimRwkvMetrics`). Cancelled and failed runs write a partial profile with
  `status` set accordingly. No source or translated text is recorded
  (unit-tested guard).
- Simplification vs roadmap: pdf2zh's stdout doesn't reliably delimit
  parse/layout vs render phases, so the profile records the whole process
  wall time; `rwkv.totalRequestMs` subtracted from it gives the non-model
  bound. This is enough to answer "is the model the bottleneck".

## Frontend

- pdf2zh progress events are now subscribed app-level (AppShell) and stored
  in Zustand keyed by jobId (`pdfRunProgressByJobId`), so switching files
  mid-run no longer resets the PDF status display. Cleared when the run
  finishes.
- Topbar PDF branch no longer falls back to the misleading `0 / 1` pseudo
  segment count; before the first progress event it shows 准备翻译引擎. The
  elapsed timer is anchored to the run's `startedAt` instead of component
  mount time, so it survives remounts.
- PDF scroll sync switched from whole-pane ratio to page-anchor sync: the
  driving pane's first visible page row + page-local offset is mirrored to
  the same page row in the other pane. Placeholder pages participate, so
  partially translated documents stay aligned.
- Preview rasterization width now tracks pane size via a debounced
  `ResizeObserver` (10% change threshold) with a `MAX_RASTER_WIDTH = 1800`
  clamp matching the backend's, instead of measuring once per job.
- Welcome document: removed the fragile `![Rosetta](/icon.png)` absolute
  asset reference; job-list bootstrap (list + create-welcome-if-empty) is a
  shared helper that also re-runs after `rosetta-onboarding-completed`, so
  first-run welcome no longer depends on event timing between the onboarding
  and main windows.

## Persistent worker + MPS (second pass, same day)

Profile data from real runs (3-page and 18-page documents) pinned the
remaining non-model cost: ~13.3 s per invocation is `import doclayout_yolo`
(torch chain) — the model load itself is 0.07 s — plus ~4 s pdf2zh startup
and ~1.1 s/page parse+layout (CPU YOLO ≈ 0.5 s of it).

- Added a persistent pdf2zh worker (ADR 0005): one warm Python process per
  session, jobs over stdin/stdout JSON, pdf2zh logs still on stderr feeding
  the same progress parsing. Worker script is embedded in the app binary and
  written to `pdf2zh-sidecar/worker/` at spawn — works with already-installed
  packs, no pack rebuild. CLI invocation remains as fallback.
- `prewarm_pdf2zh_worker` command + frontend prewarm when a PDF document
  becomes active, hiding the first import behind page selection.
- Cancellation kills the worker process group and respawns next run; idle
  reaper stops the worker after 10 min.
- MPS layout inference: probe-gated `YOLOv10.predict` monkeypatch in the
  worker (`PYTORCH_ENABLE_MPS_FALLBACK=1`); falls back to stock CPU when the
  probe fails. Pack source untouched.

Measured on the same 18-page document: 137.7 s → 113.2 s with the worker warm
(non-model time 58 s → 36.6 s; RWKV is now 67% of the run).

MPS verdict (measured, then reverted to CPU default): with the probe fixed to
use a page-sized image, MPS enabled cleanly but the run came back ~14 s
SLOWER (non-model 36.6 s → 50.9 s). The DocLayout YOLO model is small enough
that per-call transfer + dispatch + fallback-op bouncing outweighs the GPU
win on M-series CPUs. MPS is now opt-in via `ROSETTA_PDF2ZH_ENABLE_MPS=1`.

`PDF_PAGES_PER_INVOCATION` raised 10 → 25: with the worker warm the remaining
per-invocation cost is pdf2zh's whole-document pymupdf preprocess (~5 s),
so typical papers should run as a single invocation.

## Live status bar (third pass, same day)

Final measured result after worker + prewarm + chunk 25: 18-page run 93.7 s
(RWKV 74%; baseline was 137.7 s chunked / ~8 min per-page).

Two fixes so long runs no longer read as "frozen":

- pdf2zh's tqdm progress redraws with `\r` and never newlines, so the
  line-based stderr reader delivered the whole bar only when it finished —
  page progress was frozen for the entire invocation. The worker's stderr
  reader now splits on `\r` too, and `parse_tqdm_fraction` (denominator must
  match the chunk size) turns each redraw into a live "第 X/Y 页" update.
- The shim's RWKV metrics now feed a 已翻译 N 字 counter: every progress
  event carries the cumulative translated character count, and a 500 ms
  heartbeat task emits it even while pdf2zh's own output is quiet. The count
  is offset across chunks so it's monotonic for the whole run.

## Not done (deliberate)

- Roadmap Phase 1's full "PDF run as first-class durable object" — the store
  progress slice + durable `pdf_page_translations.json` (which already resets
  stale `translating` → `pending` on load) covers the reported symptoms with
  much less surface.
- Zoom controls for the preview pane (roadmap Phase 6) — clarity fix shipped;
  zoom is UI scope to be decided separately.
- Markdown-side comparison profile — the PDF profile alone already separates
  model vs non-model time.
