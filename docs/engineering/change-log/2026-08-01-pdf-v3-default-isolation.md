# 2026-08-01 PDF v3 Default Isolation

## Summary

Removed the unused PDF v3 frontend control surface from the production app and
moved its native command, lifecycle, preview, export, and worker implementation
behind a default-off Cargo feature. Production PDF translation, source preview,
translated-page preview, export, and shared source-identity primitives remain
enabled.

## Changes

- Added the default-off `experimental-pdf-v3` Cargo feature.
- Stopped registering eleven unused PDF v3 Tauri commands in default builds.
- Removed the unused frontend PDF v3 hooks, command wrappers, and response
  types.
- Kept native PDF v3 history recoverable through an explicit feature build.
- Replaced unconditional PDF v3 shutdown state dependencies with feature-aware
  helpers so app exit, local-data reset, and job deletion behave correctly in
  both build modes.
- Narrowed broad dead-code suppression to the shared primitives still consumed
  by the production PDF path.
- Added a static isolation check and CI gates for both the default production
  boundary and the experimental recovery build.

This is the first isolation step only. It does not delete the feature-gated
native implementation, change persistent job formats, migrate existing data,
or alter the production PDF translation workflow.

## Managed PDF Compatibility

The same integration restores the managed PDF runtime minimum to engine
revision 1. `resource-manager-reuse` remains a build-time AST verification for
new packs, not a runtime protocol requirement, so the frozen Windows, macOS,
and Linux release packs are not incorrectly treated as outdated. The managed
PDF test suite now runs in main application CI.

## Validation

- `pnpm typecheck`
- `pnpm check:pdf-v3-isolation`
- `cargo fmt --all -- --check`
- `cargo check`
- `cargo check --features experimental-pdf-v3`
- `cargo test rosetta_jobs`
- `cargo test managed_pdf2zh`
- `cargo test --features experimental-pdf-v3 rosetta_jobs::formats::pdf::v3_run_list`
- `python scripts/test-pdf2zh-patches.py -q`
- `git diff --check`

No development server or production build was run.
