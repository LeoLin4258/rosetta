# PDF v3 Long-Document Acceptance

Date: 2026-07-19

## Summary

- Added isolated ignored Windows tests for 20-, 500- and 1,000-page durable PDF
  v3 translation/export acceptance.
- Repeats one repository-owned renderable LibreOffice page while retaining
  shared content/resource ownership, then exercises the bounded scheduler,
  PageGraph and patch stores, deterministic translation planning, real fit
  decisions and atomic incremental export.
- Uses only a temporary directory and the Windows Arial font. It does not read
  or mutate user jobs, AppData resources or local provider state.
- Verifies translated text samples, preserved-page text, output page count,
  source-prefix retention, atomic destination replacement and temp cleanup.
- Records stage timing, Windows process memory, logical and physical store
  bytes, patch counts, font subset bytes and PDF append/output bytes.
- Replacement planning now de-duplicates page-local content scans and decoded
  content streams without retaining cache state across pages or jobs.
- TranslationPatch files now use bounded gzip storage while preserving the
  canonical JSON identity and renderer validation contract.
- Page-local content streams are now shared between replacement identity,
  preflight and final staging, keyed by stream identity plus Form invocation
  path and released after each page render.
- Translation worker outcomes now expose the first committed patch page and
  batch-relative authority time for first-visible latency measurement.
- The acceptance path separately measures a complete cold translated preview
  without including final document export.
- New runs now prioritize the currently visible selected page by rotating the
  existing bounded extraction and translation cursors before worker startup.
  No additional page queue or durable authority is introduced.

## Validation

- The 20-page smoke acceptance passed with 18 translated pages, 2 preserved
  pages and 126 fitted entries.
- The 500-page acceptance passed with 495 translated pages, 5 preserved pages,
  3,465 fitted entries and a peak working set below 40 MB.
- The 1,000-page entry point is implemented but was not run in this change.
- The 20-page rerun measured an 11,786 ms complete pipeline and 4,318 ms
  replacement render stage after the de-duplication change, versus roughly
  12,100 ms and 4,590 ms in the preceding smoke baseline. This is a debug
  diagnostic, not a long-document throughput claim.
- A subsequent 20-page rerun measured 1,306,935 B of logical patch JSON,
  455,253 B of compressed patch payloads and 462,586 B for the complete patch
  directory, a 65.2% reduction in patch payload bytes.
- Two smoke reruns after the shared content cache measured 3,638 ms and
  3,468 ms for replacement rendering, versus 4,234 ms before this change.
  The measured renderer-stage reduction is roughly 14%-18%; full pipeline time
  remains a debug diagnostic.
- Two first-visible probes measured the first durable translated patch at
  911-945 ms and the cold 1,200-pixel translated preview at 588-807 ms. Their
  non-provider sum is 1,499-1,752 ms; real latency additionally includes the
  first RWKV result and frontend display delay.
- Scheduler and trusted-creation tests verify that page 7 is claimed first for
  a sparse requested PageSet, while an unrequested preferred page is rejected
  before a run becomes visible.

This change updates native replacement planning, TranslationPatch storage,
acceptance measurement and the narrow run-creation scheduling hint. It does
not change Tauri permissions or add a second translation authority.
