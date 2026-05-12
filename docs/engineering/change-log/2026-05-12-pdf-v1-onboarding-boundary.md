# PDF v1 Onboarding Boundary

## Summary

Prepared Rosetta for multi-contributor development with PDF support as a required v1 scope item.

## Changes

- Added `docs/engineering/plans/2026-05-12-pdf-v1-support.md` to define PDF v1 scope, non-goals, module ownership, data model expectations, parser requirements, fixtures, and validation.
- Updated the root README to describe the current v1 milestone as TXT, Markdown, and text-based PDF import.
- Replaced the Tauri template README in `rosetta-app/` with Rosetta-specific contributor guidance.
- Clarified `AGENTS.md` validation rules so typecheck/check/test are the default validation commands, while dev servers and builds require explicit request.
- Updated data model conventions so PDF enters the existing Rosetta IR pipeline instead of becoming a separate workflow.
- Updated the project plan to move text-based PDF from a later phase into Phase 1 / MVP scope.
- Extended frontend source document format types to include `pdf`, while keeping PDF exports as text-like output for v1.
- Clarified that high-fidelity PDF format restoration is a nice-to-have enhancement path, not the v1 baseline acceptance gate.
- Moved the Rust job module from `rosetta_jobs.rs` to `rosetta_jobs/mod.rs` and split the backend job pipeline into model, path, format, import, export, store, translation file, revision, segmenter, document-helper, and test modules.

## Validation

No dev server or production build was run. This change is documentation and type-boundary preparation. Follow-up implementation should run:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

## Notes

PDF parser implementation is not complete in this change. Current runtime import still only accepts TXT and Markdown until the PDF importer and Tauri command integration are implemented.
