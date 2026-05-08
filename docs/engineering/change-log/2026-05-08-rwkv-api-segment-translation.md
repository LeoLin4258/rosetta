# 2026-05-08 RWKV API Segment Translation

## Summary

Extended the external RWKV API work from a Settings probe into the first minimal segment translation path.

The engineer-provided `/v1/chat/completions` batch `contents[]` API is now treated as the current confirmed connector contract for `rwkv_lightning` and the translation model. Managed in-app RWKV runtime work remains paused.

## Changes

- Added a Tauri command that translates an arbitrary list of source texts through the external RWKV API.
- Reused the same non-streaming request body shape as the probe:
  - `contents[]`
  - fixed English -> Chinese prompt template
  - `stream: false`
  - user-provided body password
- Kept HTTP requests in Rust through `reqwest`; the frontend does not call the API with `fetch`.
- Kept response mapping based on `choices[index].message.content.trim()`.
- Added a Jobs page action that sends pending/failed preview segments to the configured RWKV API and writes ordered translations back into the preview segment state.
- Added failure handling that marks the attempted batch as failed when the connector reports HTTP, parse, empty content, or translation count errors.
- Updated engineering docs to mark `/v1/chat/completions` batch `contents[]` as the confirmed current connector path.

## Boundaries

- This is still a minimal demo segment path, not the final scheduler.
- Streaming chunks are still intentionally not implemented.
- TXT/Markdown import, persistent job cache, retry queue, and export are still future steps.
- Real API tokens and body passwords remain user-local settings only and are not written to code, docs, tests, or fixtures.

## Validation

Executed:

```txt
cargo fmt
cargo test rwkv_api
cargo check
corepack pnpm typecheck
```

Per project instruction, do not run dev server or build commands.
