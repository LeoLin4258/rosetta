# 2026-06-17 Windows RWKV Runtime Dogfood

## Scope

- Added a clean Windows managed RWKV runtime profile for the
  `rwkv_lightning_cuda` CUDA package.
- Added app-data runtime pack layout under `managed-rwkv/runtimes/`.
- Added Windows runtime pack installation from the local dogfood archive
  `RWKV_lightning_CUDA_sm75+_Win_MSVC.7z`.
- Added Windows backend launch args and `PATH` setup for the runtime `lib/`
  directory.
- Added Windows process listing / termination for managed runtime cleanup.
- Updated frontend provider selection and settings copy so local runtime
  behavior comes from the active managed profile instead of hardcoded macOS
  assumptions.

## Validation

Planned:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

## Notes

The Windows runtime package is now pinned by local dogfood archive size and
SHA256. The default `.pth` model artifact is still pending; until it is pinned,
Windows can install the runtime package but cannot start local inference unless
a matching model file is present in the managed model directory.
