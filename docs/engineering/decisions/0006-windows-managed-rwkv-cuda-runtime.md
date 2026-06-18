# 0006 Windows Managed RWKV CUDA Runtime

## Status

Accepted

## Date

2026-06-18

## Context

Rosetta beta.13 has a stable managed RWKV runtime on Apple Silicon. Windows
needs the same local-first product flow, but cannot reuse the macOS MLX
runtime. The confirmed Windows target is x64 with an NVIDIA GPU whose CUDA
compute capability is SM75 or newer.

The selected upstream runtime is `rwkv_lightning_cuda`. Its Windows release is
distributed separately from the model and exposes an OpenAI-compatible local
HTTP API. The upstream author has granted redistribution permission and is
preparing an official ZIP package.

## Decision

- Windows v1 supports x86_64, NVIDIA, and SM75+ only.
- Rosetta checks `nvidia-smi` before offering installation or startup.
- The runtime and model are separate, SHA256-pinned artifacts under
  `<app-local-data>/managed-rwkv/`.
- Runtime archives use ZIP and are extracted with Rust's `zip` crate. Rosetta
  does not require 7-Zip or a system `tar` command.
- Until the official upstream ZIP is published, development uses a locally
  produced ZIP with fixed size and SHA. Switching to upstream changes only
  profile metadata.
- The runtime binds to `127.0.0.1` on an ephemeral port.
- Windows child processes use `CREATE_NO_WINDOW`.
- Stop, cancel, stale-process cleanup, and app exit terminate the complete
  Windows process tree.
- The external translation API remains an explicit opt-in fallback. Choosing
  it in onboarding skips RWKV setup but still continues to PDF setup.

## Consequences

- AMD and Intel Windows machines can use an explicitly configured external
  API, but cannot install the CUDA runtime.
- NVIDIA runtime launch and translation still require validation on a clean
  SM75+ Windows machine.
- The temporary development ZIP is not a release artifact. Release metadata
  must be replaced after the official upstream ZIP is downloaded and verified.
