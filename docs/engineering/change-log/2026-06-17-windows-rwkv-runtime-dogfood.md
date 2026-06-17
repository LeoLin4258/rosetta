# 2026-06-17 Windows RWKV Runtime Dogfood

## Scope

- Added a clean Windows managed RWKV runtime profile for the
  `rwkv_lightning_cuda` NVIDIA CUDA package (`sm75+`).
- Added app-data runtime pack layout under `managed-rwkv/runtimes/`.
- Added Windows runtime pack installation from the local dogfood archive
  `RWKV_lightning_CUDA_sm75+_Win_MSVC.7z`.
- Added local `.pth` model import plumbing (`modelFilePath`,
  `ROSETTA_RWKV_MODEL_FILE`, or Downloads fallback) so dogfood can proceed
  before a hosted model URL exists.
- Added a Settings action to choose and import a local RWKV `.pth` model file
  through the native file picker.
- Added Windows backend launch args and `PATH` setup for the runtime `lib/`
  directory.
- Added Windows process listing / termination for managed runtime cleanup.
- Added runtime label and hardware requirement fields so Settings identifies
  this profile as NVIDIA CUDA instead of generic Windows GPU support.
- Updated frontend provider selection and settings copy so local runtime
  behavior comes from the active managed profile instead of hardcoded macOS
  assumptions.
- Pointed the Windows local `rwkv-lightning-contents` provider at
  `/v1/batch/completions`, matching the `rwkv_lightning_cuda` batch API.
- Fixed Windows CUDA translation requests to serialize `stop_tokens` as token
  ids (`[0]`) for `/v1/batch/completions`. The runtime reads this field with
  `asInt64()`, so the previous string stop token caused
  `Value is not convertible to Int64` when translation started.
- Pinned and staged Windows x64 PDFium (`chromium/7834`) under
  `resources/pdf-sidecar/pdfium/win-x64/`.
- Fixed PDF test helper path resolution so Windows tests bind
  `win-x64/pdfium.dll` instead of the macOS dylib path.

## Validation

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test managed_rwkv
cargo test rosetta_jobs
```

Results on Windows x86_64:

- `pnpm typecheck`: passed.
- `cargo check`: passed, with existing PDF dead-code warnings.
- `cargo test managed_rwkv`: passed, 32 tests.
- `cargo test rosetta_jobs`: passed, 36 tests.

## Notes

The Windows runtime package is now pinned by local dogfood archive size and
SHA256. It targets NVIDIA CUDA only; AMD / Intel support needs a separate
runtime package or backend.

Windows cannot reuse the macOS 0.4B MLX zip directly. The CUDA runtime expects
a `.pth` model path, so the Windows default now pins the engineer-confirmed
0.4B translation artifact:

- `RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth`
- Size: `901775740` bytes
- SHA256: `b9a1b013c3a938515f8b9bc23c28d815fa6f839eef77a943e92e7e70d35a0527`
- Sources: Hugging Face plus `hf-mirror.com`

A local 1.5B translation `.pth` is present on this machine and can still be
used for fallback dogfood via explicit `modelFilePath`, but it is not the
desired default.
