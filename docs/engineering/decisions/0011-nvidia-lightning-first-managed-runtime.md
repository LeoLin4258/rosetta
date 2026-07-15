# ADR 0011: NVIDIA Lightning-First Managed Runtime

Date: 2026-07-13

## Status

Accepted for Windows and Linux development testing

## Context

Rosetta supports both RWKV Lightning CUDA and llama.cpp Vulkan on NVIDIA
systems. Real Linux testing on four RTX 4090 GPUs showed that llama.cpp Vulkan
works, but its batch translation path is not the preferred NVIDIA runtime.
Upstream `rwkv_lightning_cuda` V1.0.3 now publishes a self-contained Linux x64
SM75+ ZIP with an explicit loopback `--host` option and the existing
`/v1/batch/completions` API contract.

## Decision

- On Windows and Linux, select RWKV Lightning first when an SM75+ NVIDIA GPU
  is detected. Keep an explicitly saved user selection authoritative.
- Fall back to the platform's llama.cpp Vulkan profile when Lightning hardware
  support is unavailable or the user selects the fallback.
- Pin the upstream Linux V1.0.3 ZIP by exact filename, size, and SHA-256. It is
  allowed as the Linux development artifact because it supports loopback
  binding without binary patching.
- Linux launches the self-contained runtime with its bundled `lib/` directory
  in `LD_LIBRARY_PATH`; no CUDA Toolkit installation is required.
- Linux ZIP extraction restores the runtime executable bit.

## Consequences

- ADR 0007's unconditional Windows Vulkan-first default is superseded for
  supported NVIDIA hardware. Vulkan remains the broad hardware fallback.
- Linux can share the Windows Lightning provider and PTH model contract while
  using a platform-specific runtime archive.
- Release packaging is still out of scope. The Linux artifact must complete
  release-policy and redistribution review before a production build ships.
