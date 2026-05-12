# 2026-05-12 Code Quality Phase 1-2

## Summary

Rosetta completed the first two code-quality cleanup stages after the Tauri + React architecture review.

The goal was to reduce duplicated frontend workflow logic and clarify product boundaries without changing the job cache format, Tauri command contracts, or current TXT/Markdown translation behavior.

## Phase 1: Low-Risk Cleanup

Changes:

- Extracted shared language options and RWKV config readiness checks into `rosetta-app/src/lib/languages.ts`.
- Extracted export filename and export format helpers into `rosetta-app/src/lib/rosettaExport.ts`.
- Extracted translation segment status helpers and progress calculation into `rosetta-app/src/lib/translationSegments.ts`.
- Reused those helpers from the jobs workbench and translation preview window.
- Added visible job-page error feedback for project load, batch translation startup, and export failures.
- Clarified Settings copy: embedded local model runtime remains the long-term preferred path, while external RWKV APIs are the current development path and a future optional backend.
- Clarified Import copy: current imports support TXT and Markdown; PDF and Word are planned.
- Restored frontend typecheck by rebuilding `node_modules` with copied package files after `react-resizable-panels` package metadata became unreadable through the existing pnpm link/import state.

Behavior boundary:

- No dev server or production build was run.
- No Tauri command signatures changed.
- No persistent Rosetta job data format changed.
- No PDF or Word support was implemented in this pass.

## Phase 2: Shared Translation Runner

Changes:

- Added `rosetta-app/src/lib/translationRunner.ts`.
- Centralized the frontend batch translation loop used by the main jobs workbench and the independent translation preview window.
- The shared runner now owns:
  - batch chunking
  - marking translation segments as `translating`
  - saving in-progress batches
  - sending RWKV translation requests
  - restoring the active batch to `pending` when the user stops the run
  - marking successful batches as `done`
  - marking failed batches as `failed`
  - rejecting mismatched response counts
- The jobs workbench remains responsible for selecting files, creating translation files, queue display, and updating `activeTranslationRun`.
- The preview window remains responsible for selected-block retranslation UI and live refresh.

Behavior boundary:

- Cancellation remains a frontend stop signal. It restores the current persisted batch to `pending`, but it does not yet cancel the underlying Rust `reqwest` request once submitted.
- The runner still uses the existing `translate_rwkv_texts_with_api` and `save_rosetta_translation_segments` commands.
- Full Rust-side persistent queue, true request cancellation, retry policy, and structured command errors are deferred to a later higher-risk phase.

## Validation

Validated with:

```powershell
cd rosetta-app
corepack pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Expected result:

- TypeScript typecheck passes.
- Rust check passes.
- `rosetta_jobs` tests pass.

