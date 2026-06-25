# 2026-06-25 beta.16 PDF release prep

## Summary

Prepared Rosetta `0.1.0-beta.16` for a PDF workflow beta release.

## Changes

- Bumped the app version to `0.1.0-beta.16` in the npm package, Tauri config,
  Cargo manifest, and Cargo lockfile.
- Added user-facing beta.16 release notes focused on PDF preview performance,
  PDF cache correctness, forced PDF retranslation, and long PDF text handling.
- Added in-app Settings release highlights for beta.16.

## Release focus

beta.16 focuses on making the visual PDF translation path feel stable on
longer files. PDF preview panes now render only nearby pages, cache recent
preview state, and refresh completed translated pages without remounting the
whole translated pane. Page-level translation artifacts are target-language
scoped, stale artifacts are cleaned on import, and users can force already
translated PDF pages to be regenerated.

The release also hardens pdf2zh shim behavior for long extracted PDF text
blocks by splitting oversized requests before they reach the local RWKV
backend, reducing context-limit failures with the small Windows llama.cpp
profile.

## Validation

- `pnpm typecheck` — passed
- `cargo check` — passed
- `cargo test rosetta_jobs` — passed
