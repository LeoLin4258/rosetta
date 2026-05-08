# RWKV Runtime Manual Artifact Scan

## Date

2026-05-08

## Summary

Added a managed offline scan path for RWKV runtime artifacts. Rosetta still does not download large files automatically; users can place the expected ModelScope files into the app-managed runtime/model directories, then ask the app to scan and verify them.

## Changes

- Added Tauri command `scan_rwkv_runtime_artifacts`.
- Added exact filename, size, and SHA-256 verification for the expected Windows amd64 RWKV Lightning runtime zip.
- Added exact filename, size, and SHA-256 verification for the expected RWKV v7 G1 Translate 1.5B model file.
- Added manifest generation after a staged artifact passes verification.
- Added Settings UI action for scanning staged files.
- Added frontend invoke wrapper and shared TypeScript result type.
- Added unit tests for empty scan, successful manifest write, and hash mismatch rejection.

## Large Files

Rosetta does not download these files in this phase:

```txt
rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
sizeBytes: 1321825122
sha256: e4957c0dc771ea949d24f1d15123848dc2243546db62f4928c695c799c99e881
```

```txt
RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
sizeBytes: 3055445546
sha256: b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa
```

The app only scans files already placed in the managed target directories and writes manifests for files that match the expected metadata.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
