# RWKV Runtime Fast Status

## Date

2026-05-08

## Summary

Fixed Settings page hangs after staging the large RWKV runtime/model artifacts.

The root cause was that normal runtime status and install-plan refreshes performed full SHA-256 reads of the staged 1.3GB runtime zip and 3GB model file. Settings loads several runtime queries, so entering the page could trigger repeated large-file reads and make the app appear frozen.

## Changes

- Runtime status and install-plan validation now use fast checks:
  - manifest JSON validity
  - expected ids and model metadata
  - SHA-256 string format
  - safe relative artifact filenames
  - file existence
  - file size when `sizeBytes` is present
- Full SHA-256 verification remains limited to explicit user actions:
  - `scan_rwkv_runtime_artifacts`
  - `extract_rwkv_runtime_artifact`
- Removed unused full-validation helpers to avoid accidental reintroduction on status refresh paths.
- Updated runtime tests to cover the fast refresh behavior.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
