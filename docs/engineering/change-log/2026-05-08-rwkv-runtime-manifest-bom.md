# RWKV Runtime Manifest BOM Handling

## Date

2026-05-08

## Summary

Fixed a Windows manifest parsing issue where a manually written JSON manifest could include a UTF-8 BOM. `serde_json` rejects the BOM at the start of the file, causing Settings to show `expected value at line 1 column 1`.

## Changes

- Rewrote the local workstation `model-manifest.json` as UTF-8 without BOM.
- Updated runtime manifest reading to tolerate a leading UTF-8 BOM.
- Added a unit test for BOM-prefixed manifest JSON.

## Validation

```txt
cargo fmt
cargo test rwkv_runtime
corepack pnpm typecheck
cargo check
```

Per project instruction, no dev server or build command was run.
