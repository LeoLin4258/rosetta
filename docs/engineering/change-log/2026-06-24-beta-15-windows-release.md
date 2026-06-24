# 2026-06-24 beta.15 Windows release prep

## Summary

Prepared Rosetta `0.1.0-beta.15` for a Windows-first beta release from the
current Windows release machine.

## Changes

- Bumped the app version to `0.1.0-beta.15` in the npm package, Tauri config,
  Cargo manifest, and Cargo lockfile.
- Added user-facing beta.15 release notes focused on the Windows llama.cpp
  Vulkan runtime.
- Added in-app Settings release highlights for beta.15.

## Release focus

beta.15 makes llama.cpp Vulkan the recommended Windows managed runtime. It
expands the Windows local translation path beyond NVIDIA CUDA-only hardware to
supported AMD, Intel, and NVIDIA Vulkan devices, while keeping RWKV Lightning
CUDA as a secondary NVIDIA profile.

## Validation

- `tsc --noEmit`
- PowerShell parser check for `release-windows.ps1`
- PowerShell parser check for `publish-windows-updater.ps1`
