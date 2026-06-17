# 0006. Windows Managed RWKV CUDA Runtime

Date: 2026-06-17
Status: accepted

## Context

Rosetta beta.13 has only shipped macOS support. The old Windows runtime
placeholder was not validated and should not constrain the Windows path.

RWKV engineering provided a Windows CUDA runtime package for
`rwkv_lightning_cuda` V1.0.0 and a verified run guide. The backend executable
is `rwkv_lighting_cuda.exe`; the package also includes `rwkv_launcher.exe`,
`rwkv_vocab_v20230424.txt`, and a `lib/` directory with CUDA and runtime DLLs.

The launcher opens its own HTTP control UI, but Rosetta should remain the
user-facing workbench and runtime manager.

## Decision

Add Windows support as a clean managed runtime profile:

- Profile id: `windows-amd64-rwkv-lightning-cuda`.
- Target platform: Windows x86_64 with NVIDIA CUDA.
- Runtime package: `RWKV_lightning_CUDA_sm75+_Win_MSVC.7z`.
- Rosetta installs the runtime package under app-local data:
  `managed-rwkv/runtimes/rwkv-lightning-cuda-sm75-msvc/`.
- Rosetta starts `rwkv_lighting_cuda.exe` directly, not `rwkv_launcher.exe`.
- Before spawn, Rosetta prepends the installed runtime `lib/` directory to
  `PATH`.
- Spawn args are:
  `--model-path <model> --vocab-path <vocab> --port <ephemeral>`.
- Health probe uses `GET /v1/models`.

The runtime package is separate from the model artifact. The Windows profile
must not report itself ready until both runtime and model are present. Until
the default model file is pinned by filename, size, SHA256, and source URL,
the app may install the runtime pack but must show the model as missing.

## Consequences

- The old Windows libtorch placeholder is superseded and should not be
  extended.
- macOS keeps its existing bundled sidecar path.
- Windows process cleanup must use Windows-native process listing and
  termination, not Unix `ps` / `kill`.
- The translation provider layer may reuse existing OpenAI-compatible
  plumbing where possible, but local Windows runtime state must not be mixed
  with user-configured remote API credentials.
- Public Windows installer signing and updater work remain a later phase after
  dogfood validation.

## Validation

Relevant validation:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Manual dogfood must verify install, start, stop, app-exit cleanup, TXT/Markdown
translation, and runtime logs that do not contain document text or prompts.
