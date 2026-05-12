# 2026-05-12 RWKV Translation Run Cancel

## Context

The previous frontend translation runner stopped the UI loop with `Promise.race`, but the Rust `reqwest` request could continue waiting until response or timeout. This made the stop button misleading for long or stuck model calls.

## Changes

- Added an in-memory Rust translation run registry managed by Tauri state.
- Added commands:
  - `start_rwkv_translation_run`
  - `cancel_rwkv_translation_run`
  - `get_rwkv_translation_run_status`
- Moved document translation batch execution into Rust for the main runner path.
- Kept `translate_rwkv_texts_with_api` for settings probes and low-level API checks.
- On cancel, current in-flight batch segments are restored from `translating` to `pending`.
- Added cancellation-aware request waiting so a stuck send future can be aborted instead of waiting for the configured timeout.
- Kept the run registry memory-only; no `translation_runs.json`, SQLite, or persistent queue was introduced.

## Compatibility

- Existing translation file JSON remains unchanged.
- Existing stale `translating -> pending` recovery remains useful after app restarts or crashes.
- Skipped segments remain excluded from translation targets.
- Frontend UI behavior remains the same: full-file translation, selected-block retranslation, stop, retry, and export still operate through the existing pages.

## Known Boundary

The Rust run state is intentionally small. It is not a durable background queue and does not try to recover an active run after the app process exits. That larger orchestration layer should only be added when long multi-file jobs need crash-resumable execution.

## Validation

- `cargo check`
- `cargo test rwkv_api`
- `cargo test rosetta_jobs`
- Manual runtime validation should verify that stop exits quickly and that pending work can be continued afterward.
