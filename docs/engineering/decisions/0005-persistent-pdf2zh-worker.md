# 0005. Persistent pdf2zh Worker Process

Date: 2026-06-11
Status: accepted

## Context

PDF translation runs measured with the per-run diagnostics profile
(`diagnostics/pdf-translation-profile-*.json`) showed that for an 18-page
document, RWKV model time was 75 s while pdf2zh-side processing added 58 s.
Decomposing that 58 s:

- ~13.3 s per pdf2zh invocation is `import doclayout_yolo` (torch +
  torchvision + opencv). The YOLO layout model load itself is 0.07 s.
- ~4 s per invocation is pdf2zh's own startup (pymupdf preprocess, font
  injection, saving an `-en.pdf` working copy).
- ~1.1 s per page is parse + YOLO layout inference (CPU; ~0.5 s of it YOLO).

Chunked batching (10 pages per `--pages` invocation) already removed the
worst per-page overhead, but every invocation still pays the ~17 s fixed
startup. The user's goal is PDF translation time approaching pure model
time.

## Decision

Keep one warm pdf2zh Python process per app session and feed it translate
jobs over a line-based JSON protocol (stdin for jobs, stdout for protocol
events, stderr for pdf2zh's own logs/progress, parsed exactly like the CLI
path).

Key choices:

1. **The worker script ships inside the Rosetta binary** (`include_str!`) and
   is written under `pdf2zh-sidecar/worker/` at spawn time. Existing
   installed packs get the worker without re-downloading anything; no pack
   rebuild, manifest, or sha pinning is involved.
2. **CLI fallback stays.** If the worker can't start (pack without a bundled
   `python/bin/python`, write failure, boot crash), the invocation falls
   back to the previous one-shot CLI path.
3. **Cancellation kills the worker's process group** (SIGTERM → 1.5 s →
   SIGKILL), reusing the run-level cancel plumbing. The next run pays one
   re-import. Per-job in-process cancellation was rejected: pdf2zh's
   translation threads have no abort seam, and a poisoned worker is worse
   than a 13 s respawn.
4. **Prewarm on PDF open.** The frontend fires `prewarm_pdf2zh_worker` when a
   PDF document becomes active, overlapping the import with page selection.
5. **Idle reaper.** The worker is shut down after 10 minutes of inactivity to
   reclaim torch's resident memory (several hundred MB).
6. **MPS layout inference: rejected after measurement, kept as opt-in.** The
   worker can monkeypatch `YOLOv10.predict` to upgrade pdf2zh's hardcoded
   `device="cpu"` to `"mps"`, gated on a page-sized probe inference. Measured
   on an M4 mini (18-page doc, 2026-06-11) MPS was ~0.8 s/page *slower* than
   CPU — the DocLayout model is small enough that per-call transfer +
   dispatch + fallback-op bouncing dominates. Default is CPU; set
   `ROSETTA_PDF2ZH_ENABLE_MPS=1` to re-test after torch/model upgrades. The
   pack source is never patched, so the CLI fallback keeps stock behavior.

## Consequences

- Steady-state PDF runs spend pdf2zh time only on parse/layout/render
  (~1.1 s/page) plus page-cache splitting; process startup disappears from
  every run after the first (and from the first too, when prewarm wins the
  race against the user's translate click).
- A long-lived Python process (torch resident) lives alongside the app while
  PDFs are being worked on; bounded by the idle reaper.
- Job isolation relies on per-job `chdir`/env updates inside one process. A
  failed job leaves the worker alive; a dead or cancelled worker is fully
  discarded and respawned.
- The worker protocol is versionless by design: the script is rewritten from
  the app binary on every spawn, so app and script can never drift.
