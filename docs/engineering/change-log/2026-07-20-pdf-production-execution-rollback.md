# PDF Production Execution Rollback

Date: 2026-07-20

## Summary

Restored the pre-v3 PDF workbench execution path after the native region path
failed the exact ten-page performance and visible-translation benchmark.

## Changes

- Restored pdf2zh worker readiness and progress handling in the app shell.
- Restored legacy PDF preparse and cross-page translation dispatch in the
  workspace.
- Restored legacy pause, force-retranslate and translated-PDF export controls.
- Restored translated page preview from durable legacy page artifacts.
- Removed native v3 run discovery/control from the production PDF workbench
  without deleting native v3 backend infrastructure, stores or tests.
- Recorded the production-routing decision in ADR 0077.

## Validation

Baseline validation before the rollback passed:

```text
pnpm typecheck
cargo check
cargo test pdf_v3 --lib -- --nocapture
  214 passed, 25 ignored
cargo test rosetta_jobs --lib -- --nocapture
  131 passed
```

The same validation set must pass after the rollback. The exact ten-page local
RWKV benchmark remains a user-run acceptance gate; automated success is not a
substitute for the user's visual and elapsed-time approval.
