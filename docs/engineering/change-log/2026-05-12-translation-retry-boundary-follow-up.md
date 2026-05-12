# 2026-05-12 Translation Retry Boundary Follow-up

## Summary

Follow-up fixes from the code-quality review focused on translation retry safety and data minimization.

This pass intentionally keeps the development-stage external RWKV API default endpoint unchanged. Remote endpoint opt-in and URL policy work is deferred because the app-managed local RWKV runtime remains the planned long-term default path.

## Changes

- The independent translation preview now creates a file-level revision before selected-block retranslation.
- Selected-block retranslation now skips source or translation segments whose status is `skipped`, preventing code blocks, URL-only lines, and other intentionally skipped content from being marked as translated.
- The shared translation runner keeps skipped segments out of default target selection unless a future caller explicitly opts in.
- Ordinary document translation API calls no longer return raw backend response previews to the WebView. Probe calls still keep the limited redacted preview for diagnostics.
- Removed the unused opener dependencies from `package.json`, `pnpm-lock.yaml`, `Cargo.toml`, and `Cargo.lock` after the Tauri plugin initialization and capability had already been removed.

## Boundaries

- This does not implement true Rust-side HTTP cancellation. Frontend stop still restores the current persisted batch to `pending` while the submitted request may continue until the backend responds or times out.
- This does not change the default RWKV API base URL or add remote URL restrictions.
- This does not move RWKV credentials out of Zustand persist.
- This does not enable CSP; desktop runtime verification is still required before tightening it safely.

## Validation

Validate with:

```powershell
cd rosetta-app
corepack pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
cargo test rwkv_api
```
