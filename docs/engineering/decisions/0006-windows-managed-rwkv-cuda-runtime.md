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
- Upstream V1.0.0 only publishes a `.7z`. Rosetta's staging script verifies
  that archive's SHA256, removes the bundled build-machine Windows DLL
  snapshot using a fixed allowlist, and produces a deterministic ZIP.
- Upstream V1.0.0 hard-codes `0.0.0.0` and crashes on an unknown `--host`
  argument. The pinned staging step replaces the two equal-length bind
  literals with IPv6 loopback `::1`, verifies the patched executable SHA256,
  and Rosetta connects through `http://[::1]:<port>`.
- Windows child processes use `CREATE_NO_WINDOW`.
- Stop, cancel, stale-process cleanup, and app exit terminate the complete
  Windows process tree.
- The external translation API remains an explicit opt-in fallback. Choosing
  it in onboarding skips RWKV setup but still continues to PDF setup.

## Consequences

- AMD and Intel Windows machines can use an explicitly configured external
  API, but cannot install the CUDA runtime.
- The NVIDIA installation, runtime launch, translation, PDF, clean first-run,
  and application-exit paths passed real-device validation on an SM75+ Windows
  machine before the first Windows release.
- The staged ZIP must be uploaded as a Rosetta-controlled release artifact
  before Windows distribution; the app must never download and execute an
  unverified runtime archive.
