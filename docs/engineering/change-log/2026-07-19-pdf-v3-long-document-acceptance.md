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

## Validation

- The 20-page smoke acceptance passed with 18 translated pages, 2 preserved
  pages and 126 fitted entries.
- The 500-page acceptance passed with 495 translated pages, 5 preserved pages,
  3,465 fitted entries and a peak working set below 40 MB.
- The 1,000-page entry point is implemented but was not run in this change.

This change adds test and measurement infrastructure only. It does not change
runtime commands, persistent schemas, product UI or Tauri permissions.
