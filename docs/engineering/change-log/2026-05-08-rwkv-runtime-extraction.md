# RWKV Runtime Extraction

## Date

2026-05-08

## Summary

Added a managed runtime extraction phase for the verified RWKV Lightning Windows artifact. This keeps Rosetta on a local-first path and avoids requiring Python for the packaged runtime.

## Changes

- Added Tauri command `extract_rwkv_runtime_artifact`.
- Added a fixed extraction target under the managed runtime directory:

```txt
runtime/rwkv-lightning/runtime-bundle/
```

- The command verifies the expected runtime zip size and SHA-256 before extraction.
- Zip entries are rejected if they contain unsafe or unsupported path components.
- Extraction succeeds only if `rwkv_lightning.exe` exists after unpacking.
- Added Settings UI action for extracting the verified runtime.
- Added frontend invoke wrapper and shared TypeScript result type.
- Added runtime extraction unit tests.

## Local Spike Notes

The downloaded runtime zip was inspected and contains a packaged Windows executable plus DLLs, including:

```txt
rwkv_lightning.exe
rwkv_vocab_v20230424.txt
torch_cpu.dll
torch_cuda.dll
cudnn64_9.dll
```

This confirms the recommended Windows artifact does not require users to install Python for the packaged runtime path.

`rwkv_lightning.exe --help` and a missing-model launch probe both exited with code 1 and no CLI output. Startup management should therefore rely on process state, port readiness, and HTTP health/translation probes instead of help text or stderr.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
