# Windows llama.cpp Vulkan Runtime Plan

Date: 2026-06-24

## Summary

Rosetta's Windows managed runtime will move from CUDA-first RWKV Lightning to
Vulkan-first llama.cpp. The first Windows llama.cpp release targets Windows x64
machines with Intel integrated GPUs, AMD GPUs, or NVIDIA GPUs. NVIDIA users may
still use RWKV Lightning as a secondary option, but it must be labeled as a
development-stage runtime that may have bugs.

The default Windows model is:

- `RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf`
- size: `501498208` bytes
- SHA256: `f0f1c64455d075236df309457e4730fe763489e5fc8c038ce3f29d9963dec96b`

## Implementation Changes

- Add a Windows x64 `llama.cpp Vulkan` managed runtime profile and make it the
  default Windows profile.
- Launch the runtime through `llama-server.exe` with a local loopback server,
  fixed model alias `rwkv-translate`, context size `4096`, automatic Vulkan GPU
  layers, and a small parallel request count.
- Add a `llama-cpp-chat-completions` provider that talks to `/completion`
  and submits one Rosetta segment per raw completion request. The prompt
  uses the RWKV translate model's role-based format:
  `{SourceLang}: {text}\n\n{TargetLang}:`.
- Keep macOS on the existing MLX profile.
- Keep Windows RWKV Lightning as a secondary NVIDIA-only profile and warning
  copy source, not as the default.
- Replace user-facing mirror selection with internal source probing. Rosetta
  should probe known sources, sort available sources by observed response time,
  then download from the fastest source and silently fall back to the next one.
  UI should not expose ModelScope, HuggingFace, hf-mirror, or aifasthub as user
  choices.
- Validate Vulkan availability after runtime install by running
  `llama-server.exe --list-devices`. A successful Windows Vulkan runtime must
  report at least one Vulkan device.

## Public Interfaces

- Add provider id `llama-cpp-chat-completions`.
- Managed runtime status may expose multiple profile candidates on Windows:
  recommended llama.cpp Vulkan plus optional RWKV Lightning CUDA.
- PDF translation's OpenAI shim must accept the llama.cpp provider and route it
  through the same local chat-completions adapter used by document translation.

## Test Plan

- TypeScript:
  - `pnpm typecheck`
- Rust:
  - `cargo check`
  - `cargo test rosetta_jobs`
  - managed RWKV profile, lifecycle, install resolver, and provider tests
- Manual Windows smoke:
  - AMD or Intel GPU: clean install, model/runtime download, startup,
    Markdown translation, PDF translation, exit cleanup.
  - NVIDIA GPU: default path remains llama.cpp Vulkan; RWKV Lightning appears
    only as a secondary development-stage option.
  - Slow or blocked mirror: install continues without asking the user to pick a
    download source.

## Defaults

- Default Windows model: 0.4B Q8 GGUF.
- No user-facing model picker in this first release.
- No Flutter FFI integration from RWKV_APP; Rosetta keeps using an external
  managed sidecar process.
- RWKV Lightning remains available for NVIDIA users as a secondary runtime.
