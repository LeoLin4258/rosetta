# 2026-07-14 PDF Background Preparse

## Summary

Moved the expensive first PDF prepare pass ahead of the translate click by
preparing the selected document window in the background after import, page
selection, or language-direction changes.

## Change

- The PDF workspace debounces stable page selections for `750ms` and invokes a
  narrow Tauri preparse command without blocking the UI.
- The backend validates the job-local source PDF and prepares only the first
  window that the selected provider would translate.
- Lightning prepares selections up to 30 pages as one window and caps longer
  selections to the first 10 pages. Other providers retain the normal 10-page
  window.
- Preparse uses the existing source metadata, page selection, language pair,
  and thread cache identity, so the foreground translation receives a normal
  prepared-cache hit.
- A running PDF translation takes priority: background preparse is skipped
  while a run is active.
- Preparse does not call RWKV, render pages, or write PDF page/run state.
  Content-free start/completion/failure records are appended to the PDF
  timeline for diagnosis.

No persistent data schema or PDF engine contract changed.

## Validation

- `pnpm typecheck`
- `cargo fmt -- --check`
- `cargo check`
- `cargo test rosetta_jobs`

Ubuntu RTX 4090 validation with a newly imported 10-page PDF:

- background preparse completed in `13.770s`, including `9.377s` layout and
  `3.749s` unit collection;
- the first user-triggered translation reported `preparedCacheHits=1` and
  spent only `4ms` in foreground prepare;
- translation completed in `5.972s`, including `3.704s` Lightning and
  `2.205s` page rendering;
- all 10 committed artifacts reopened as readable single-page PDFs with
  extractable text.
