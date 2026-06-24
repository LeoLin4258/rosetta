# ADR 0007: Windows Vulkan-First Managed RWKV Runtime

Date: 2026-06-24

## Status

Accepted

## Context

ADR 0006 selected RWKV Lightning CUDA as Rosetta's first Windows managed
runtime. Upstream has since reported larger Lightning bugs that require fixes
across upstream dependencies. That makes Lightning unsuitable as the default
Windows release runtime.

The RWKV team-recommended replacement path for Windows is llama.cpp with its
Vulkan backend. This covers Intel integrated GPUs, AMD GPUs, and NVIDIA GPUs
through one runtime package. NVIDIA users can still access Lightning as a
secondary option while it remains under active development.

## Decision

Rosetta's Windows x64 managed runtime defaults to llama.cpp Vulkan.

The Windows profile launches `llama-server.exe` as a managed sidecar and loads
the RWKV translation GGUF model:

`RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf`

The provider id is `llama-cpp-chat-completions`. Translation uses the raw
`/completion` endpoint with the RWKV translate model's role-based prompt
format (`{SourceLang}: {text}\n\n{TargetLang}:`), not the OpenAI chat
completions API. Rosetta owns batching by issuing parallel single-segment
requests to the local server (up to 16 concurrent).

Download sources are selected automatically by Rosetta. Users are not asked to
choose between ModelScope, HuggingFace, hf-mirror, or aifasthub.

## Consequences

- Windows support no longer requires NVIDIA CUDA or SM75.
- Hardware detection for the default runtime shifts from `nvidia-smi` to
  `llama-server.exe --list-devices`.
- RWKV Lightning remains a secondary NVIDIA-only runtime and must carry a
  development-stage warning in user-facing surfaces.
- PDF translation can reuse the same local llama.cpp provider through the
  OpenAI shim.
- ADR 0006 remains relevant for the Lightning secondary profile and historical
  CUDA runtime behavior, but it is no longer the default Windows path.
