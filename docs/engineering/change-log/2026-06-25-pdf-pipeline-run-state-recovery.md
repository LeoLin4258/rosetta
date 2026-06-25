# 2026-06-25 PDF pipeline run-state recovery

## Summary

Rebuilt the PDF translation pipeline around durable per-job/per-language run
state, durable page artifacts, repair-first snapshots, and two-phase deletion.
This replaces the previous patch-heavy behavior where long PDF runs could leave
pages permanently stuck in `translating` after pause, force quit, or partial
delete failures.

## Changes

- Added `pdf_source.json` as PDF source metadata with page count, source
  fingerprint, filename, original path snapshot, and timestamps.
- Added canonical `pdf_pages.<targetLang>.json` page state.
- Added canonical `translated-pages/<targetLang>/page-XXXX.pdf` page artifacts.
- Added `pdf_run.<targetLang>.json` durable PDF run state with owner session,
  requested/completed/failed pages, current chunk, pause/cancel flags, and run
  mode.
- Kept legacy `pdf_page_translations.*.json` and `pdf-pages/` readable for
  migration/repair, but new writes use the canonical layout.
- Changed page-state writes so `queued` and `translating` are not persisted.
  Durable page states are only `pending`, `translated`, and `failed`.
- Added PDF repair before list/load/snapshot operations. Repair can rebuild a
  minimal `document.json`, ensure `segments.json`, refresh `pdf_source.json`,
  recover stale runs to `paused`, migrate readable legacy artifacts, reset
  missing artifacts to `pending`, and sync sidebar summaries.
- Refactored page translation to process at most 10 pages per pdf2zh chunk.
- Committed page artifacts through a temp output directory, single-page PDF
  validation, artifact move, then atomic JSON page-state write.
- Added `get_rosetta_pdf_snapshot`, `pause_rosetta_pdf_run`, and
  `repair_rosetta_pdf_job`.
- Updated frontend PDF preview to load repair-first snapshots and ignore page
  progress events for other target languages.
- Changed PDF pause UI from old global cancel semantics to scoped
  job/language pause with an immediate "正在停止" state.
- Stopped clearing the active run when switching jobs, so a background PDF run
  remains visible when switching away and back.
- Changed default PDF continue behavior so translated pages are not silently
  overwritten. Explicit retranslation is required to clear translated page
  artifacts.
- Changed job deletion to remove the job from `index.json` first, request
  cancellation for an active PDF run, rename the job directory into `.trash/`,
  and record `delete_cleanup_tasks.json` if cleanup is blocked by file locks.
- Added visible sidebar errors for open repair failure and delete cleanup
  warnings.
- Removed the component-level PDF preview state cache that could show stale
  page state after switching jobs.

## Documentation

- Added ADR 0008: durable PDF runs, page artifacts, and delete semantics.
- Added `docs/engineering/pdf-pipeline.md`.
- Updated `docs/engineering/conventions/data-models.md`.
- Marked older PDF plans as historical where they conflict with the current
  implementation.

## Validation

- `pnpm typecheck` passed.
- `cargo check` passed.
- `cargo test rosetta_jobs` passed.
