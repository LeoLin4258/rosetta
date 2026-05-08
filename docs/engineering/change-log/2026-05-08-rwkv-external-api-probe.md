# 2026-05-08 RWKV External API Probe

## Summary

Added the first connector path for an already deployed RWKV translation API.

Managed in-app RWKV runtime work remains paused. This probe is the current development path for validating batch translation before building the document scheduler.

## Changes

- Added a Tauri command for probing an external RWKV translation API.
- The probe uses a non-streaming `/v1/chat/completions` style request with `contents`.
- Requests are sent from Rust instead of frontend `fetch`, avoiding browser CORS and keeping API credentials out of the webview network layer.
- Settings now shows the RWKV API card before the parked local runtime card.
- Settings can store, locally and explicitly:
  - API base URL
  - endpoint
  - internal token
  - body password
  - timeout
- The API probe displays status, latency, translations, and a redacted raw response preview.

## Boundaries

- Streaming responses are documented but not implemented in this step.
- The real internal token is not committed to code, docs, tests, or fixtures.
- Remote/cloud API use remains explicit opt-in and does not change Rosetta's local-first default.
- The local runtime panel remains parked and is not a dependency for the translation connector.

## Validation

Executed:

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_api
cargo check
```

Per project instruction, no dev server or build command was run.
