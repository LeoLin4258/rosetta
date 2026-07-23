# PDF v3 Bounded Workbench Preview

Date: 2026-07-19

## Summary

Connected native PDF v3 run authority and lazy translated-page rendering to the
existing virtualized bilingual workbench without loading complete long-document
page state.

## Implementation

- added complete TypeScript contracts for v3 run-control and discriminated page
  state plus a bounded status invoke wrapper;
- selected only the newest native run for the active target language and kept
  the selection reconstructible rather than persistent;
- added visible 64-record status-window polling with a four-window LRU bound;
- handled sparse PageSets through compact range membership and recent-fetch
  conflict resolution for overlapping status windows;
- refreshed cached active windows once after terminal state transition;
- rendered completed pages through the lazy v3 PNG command and reused source
  preview for preserved pages;
- kept explicit placeholders for pending, extracted, leased, failed,
  non-requested and not-yet-loaded pages;
- suppressed legacy translated rendering during v3 discovery and retained the
  old path only after successful enumeration reports no v3 run;
- kept native enumeration errors visible instead of silently treating invalid
  state as an empty run list;
- isolated job/language context changes so an old run cannot issue preview work
  against a newly selected job.

## Validation

- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The workbench can now inspect and preview existing native v3 runs with bounded
frontend state. Primary translation actions still create legacy runs; native
v3 creation and pause/resume/cancel/retry/recovery controls remain the next UI
integration phase. Real complex 500/1,000-page end-to-end translation/export
stress validation also remains pending.
