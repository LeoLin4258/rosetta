# 2026-06-29 beta.18 PDF release prep

## Summary

Prepared Rosetta `0.1.0-beta.18` for a PDF correctness and install diagnostics
beta release.

## Changes

- Bumped the app version to `0.1.0-beta.18` in the npm package, Tauri config,
  Cargo manifest, and Cargo lockfile.
- Added user-facing beta.18 release notes focused on llama.cpp PDF
  no-truncation behavior, safer PDF chunk retries, failed-run propagation,
  short reference passthrough, install diagnostics, and mirror selection.
- Added in-app Settings release highlights for beta.18.

## Release focus

beta.18 focuses on making Windows local PDF translation reject incomplete
llama.cpp output instead of silently accepting it. The managed runtime now uses
a larger default context, PDF translation chooses a llama.cpp-specific chunk
profile, and recovered or unrecovered provider failures are surfaced through
page/run state instead of being hidden by partial artifacts.

The release also improves first-install support by making diagnostics copyable
from the install step and simplifying mirror ranking so mainland-accessible
downloads are preferred before falling back.

## Validation

Pending for this release-prep change:

- `pnpm typecheck`
- `cargo check`
- `cargo test rosetta_jobs`
