# 2026-05-12 Store Selection and Enum Cleanup

## Context

The original quality report flagged two medium-risk maintenance issues:

- Jobs page selection was derived from route params, Zustand state, loaded bundle state, and fallback lists directly inside the component.
- Rust command inputs accepted broad strings for values that have a small fixed domain.

## Changes

- Added `rosetta-app/src/lib/rosettaSelection.ts` to centralize Jobs page selection resolution.
- Made route params the first selection source, followed by per-job store selection, loaded bundle fallback, and finally `null`.
- Kept the existing persisted Zustand fields in place to avoid localStorage migration in this phase.
- Added internal Rust enums for command input parsing:
  - `RosettaExportKind`
  - `TranslationRevisionReason`
- Kept persisted model fields as strings so existing cache files remain compatible.

## Compatibility

- No persistent data format migration is required.
- Existing routes remain the source of truth when present.
- Background bundle refresh should not override the route-selected source file.

## Validation

- `corepack pnpm typecheck`
- `cargo check`
- `cargo test rosetta_jobs`
- Manual checks should cover import navigation, source-file switching, bundle refresh, current-file deletion fallback, and independent preview windows.

## Deferred

Full store cleanup is still deferred. A future migration can remove duplicate `activeFileId*` / `activeSourceFileId*` fields after route-first behavior has been stable for a while.
