# RWKV Runtime Fast Extraction

## Date

2026-05-08

## Summary

Fixed Settings page hangs when clicking `解压运行时`.

The extraction command previously called the full staged artifact scan first. That scan recalculated SHA-256 for both the 1.3GB runtime zip and the 3GB model file, then extraction recalculated the runtime zip hash again. This made the UI wait on repeated large-file reads.

## Changes

- `extract_rwkv_runtime_artifact` no longer calls `scan_staged_artifacts`.
- Extraction is now idempotent: if `runtime-bundle/rwkv_lightning.exe` already exists, the command returns immediately.
- Extraction performs fast runtime readiness checks:
  - runtime manifest validity when present
  - runtime zip existence
  - runtime zip size
- If the runtime manifest is missing but the expected runtime zip is present with the expected size, extraction writes the expected runtime manifest without hashing the file.
- Full SHA-256 verification remains available through the explicit `扫描文件` action.
- Added tests for fast return when the executable already exists and size mismatch before unzip.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
