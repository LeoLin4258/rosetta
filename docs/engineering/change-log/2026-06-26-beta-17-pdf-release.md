# 2026-06-26 beta.17 PDF release prep

## Summary

Prepared Rosetta `0.1.0-beta.17` for a PDF durability and responsiveness beta
release.

## Changes

- Bumped the app version to `0.1.0-beta.17` in the npm package, Tauri config,
  Cargo manifest, and Cargo lockfile.
- Added user-facing beta.17 release notes focused on durable PDF run recovery,
  safer PDF deletion, translated-page loading behavior, first-page latency,
  local diagnostics, paragraph batching, and per-job pdf2zh cache handling.
- Added in-app Settings release highlights for beta.17.

## Release focus

beta.17 focuses on long PDF runs that need to survive real desktop behavior:
pause, force quit, task switching, repair, and Windows file locks. PDF page
state is now durable by task and target language, stale transient states can be
recovered, and page artifacts are committed through a safer canonical path.

The release also improves perceived PDF responsiveness by showing source-page
backdrops while translated pages load and by prewarming the layout model before
the worker reports ready. New local timeline diagnostics make slow PDF tasks
easier to inspect without recording document text, translated text, prompts, or
model responses.

## Validation

Pending for this release-prep change:

- `pnpm typecheck`
- `cargo check`
- `cargo test rosetta_jobs`
