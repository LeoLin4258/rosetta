# ADR 0010: Linux Managed llama.cpp Vulkan Runtime

Date: 2026-07-13

## Status

Accepted for Linux development testing

## Context

Rosetta's Linux PDF import and layout pipeline now runs on Ubuntu x64, but the
managed translation runtime had no Linux profile. The test host has NVIDIA
GPUs and a working graphics driver, while intentionally carrying no app
development environment or CUDA Toolkit.

llama.cpp release `b9775` publishes an official Ubuntu x64 Vulkan archive but
does not publish an Ubuntu CUDA archive. Building CUDA locally would add a
system toolchain requirement that is unnecessary for the current 0.4B GGUF.

## Decision

Linux x64 uses a managed llama.cpp Vulkan profile with CPU fallback. It reuses
the Windows Vulkan profile's provider contract and exact GGUF model:

`RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf`

The runtime launches `llama-server`, probes `/v1/models`, and translates via
the raw `/completion` endpoint. The sidecar binds only to `127.0.0.1`.

During development, the pinned upstream archive and model are checksum-verified
and staged directly into Rosetta's app-data directory by
`stage-llamacpp-linux-local.sh`. The profile deliberately has no online
runtime-pack URL until the Linux release artifact and installer path are
designed and validated.

## Consequences

- Linux translation testing does not require CUDA Toolkit, Codex, Node.js, or
  Rust to be installed on the UI test machine.
- Linux and Windows share the same GGUF and llama.cpp provider behavior.
- Vulkan-capable systems use GPU offload; llama.cpp can fall back to CPU when
  Vulkan initialization fails.
- Release packaging remains separate work. A distributable Linux runtime pack,
  its installer extraction format, and final artifact metadata are still
  required before release.
