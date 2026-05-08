# RWKV Runtime Settings Extracted State

## Date

2026-05-08

## Summary

Made the Settings page show whether the RWKV runtime executable is already extracted.

## Changes

- Runtime status now includes:
  - `runtimeBundleDir`
  - `runtimeBundleExists`
  - `runtimeExecutablePath`
  - `runtimeExecutableExists`
- Settings now displays the runtime bundle path and executable path.
- The extraction button is disabled and labeled `已解压` when `rwkv_lightning.exe` already exists.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
