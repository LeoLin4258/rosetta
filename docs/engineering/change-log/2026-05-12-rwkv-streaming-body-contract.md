# RWKV Streaming Body Contract

## Summary

Updated the external RWKV translation connector request body to match the model backend's current `/v1/chat/completions` contract.

## Changes

- Removed `stop_tokens` from the request body.
- Updated generation parameters to the backend-provided values: `max_tokens: 8292`, `temperature: 1`, `top_k: 1`, `top_p: 0`, `alpha_presence: 0`, `alpha_frequency: 0`, `alpha_decay: 0.99`.
- Set `stream: true`.
- Kept `password` sourced from local user settings instead of hardcoding any credential.
- Added response parsing compatibility for SSE-style `data:` chunks while preserving existing plain JSON response parsing.

## Validation

Run:

```powershell
cd rosetta-app/src-tauri
cargo test rwkv_api
cargo check
```
