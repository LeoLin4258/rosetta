# 2026-05-09 Workbench State Robustness

## Summary

Tightened the task workbench state model after file switching and loading bugs exposed competing sources of truth.

## Changes

- Made `/jobs/:jobId/files/:fileId` the primary route for current-file workbench state.
- Added a non-active-stealing bundle refresh path for async saves, exports, and rename refreshes.
- Kept `setActiveBundle` for explicit open/import flows only.
- Added run-id guards for translation run completion/failure/finish updates.
- Changed language updates from project-wide to current-file scope.
- Added optional file-level language fields to `RosettaSourceFile`.
- Added derived file-level translation status fields for sidebar file state icons.
- Updated translation revision snapshots to use the affected file language direction.
- Prevented explicit invalid file routes from silently falling back into first-file operations.
- Replaced full block rendering in document preview with block-level virtual scrolling.

## Validation

- `corepack pnpm typecheck`
- `cargo check`

## Notes

- Existing job caches remain readable because file-level language/status fields are optional or defaulted and fall back to document-level language fields plus segment-derived status.
- Project-level batch language changes are intentionally not exposed in the current workbench. If they return later, they need a separate project-level entry point and a clear impact warning.
