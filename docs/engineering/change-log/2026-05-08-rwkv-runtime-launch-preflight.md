# 2026-05-08 RWKV Runtime Launch Preflight

## Scope

Added the first controlled launch path for the packaged RWKV Lightning runtime, then added hardware compatibility preflight after testing on the current AMD workstation.

## Changes

- Added Tauri commands:
  - `get_rwkv_runtime_process_status`
  - `start_rwkv_runtime`
  - `probe_rwkv_runtime_translation`
- Runtime launch now uses the packaged executable with required arguments:

```txt
rwkv_lightning.exe --model-path <model> --vocab-path <vocab> --port 8000 --password <local-token>
```

- The local password is generated under app data and redacted from UI command previews.
- Runtime process status now reports:
  - PID and stale PID handling
  - TCP port readiness
  - HTTP API readiness
  - HTTP status code
  - log tail
- Settings now shows process status, API readiness, log tail, and a minimal translation probe action.
- Settings now shows runtime hardware compatibility.
- The current installed runtime artifact is identified as CUDA/NVIDIA:

```txt
rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
```

- On Windows, Rosetta reads display adapters through the narrow `pnputil /enum-devices /class Display` query. If the installed runtime is CUDA/NVIDIA and no NVIDIA adapter is detected, `start_rwkv_runtime` is blocked before spawning the process.

## Local Finding

The current development machine has AMD graphics:

```txt
AMD Radeon 780M Graphics
```

This machine is not compatible with the currently staged CUDA/NVIDIA RWKV Lightning artifact. A manual launch attempt created a `rwkv_lightning.exe` process, but `127.0.0.1:8000` did not become reachable after several minutes and no useful stdout/stderr diagnostics were produced.

## Implication

The next runtime spike should not continue on the CUDA/NVIDIA artifact for this workstation. To run RWKV locally on this hardware, Rosetta needs a Vulkan/CPU-capable runtime path, likely through an AI00 or llama.cpp RWKV spike, before returning to the one-click runtime launch flow.

## Validation

```txt
cargo fmt
corepack pnpm typecheck
cargo test rwkv_runtime
cargo check
```

Per project instruction, no dev server or build command was run.
