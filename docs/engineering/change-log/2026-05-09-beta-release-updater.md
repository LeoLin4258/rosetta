# 2026-05-09 Beta Release Updater

## Summary

Added the internal beta release skeleton and manual in-app update flow for Rosetta.

## Changes

- Added Tauri updater and process plugins on both frontend and Rust sides.
- Registered updater/process plugins in the Tauri builder.
- Added updater/process permissions to the default desktop capability.
- Enabled updater artifacts in `tauri.conf.json`.
- Added a Tauri updater public key and GitHub Release `latest.json` endpoint.
- Updated app version fields to `0.1.0-beta.1`.
- Added a Settings page “应用更新” section:
  - current app version
  - manual update check
  - available update details
  - download/install action
  - restart action after installation
  - clear failure state
- Added the beta release and updater procedure plan.

## Security Notes

- Windows code signing is still deferred for the first internal beta.
- Tauri updater signing is required and configured.
- The updater private key is stored outside the repository at:

```txt
C:\Users\Leo\.rosetta-release\rosetta-beta.key
```

- The private key must not be committed or copied into docs, fixtures, tests, or source files.

## Validation

Planned:

- `corepack pnpm typecheck`
- `cargo check`
- `cargo test rosetta_jobs`

`corepack pnpm build`, `corepack pnpm build:tauri`, `tauri dev`, and `tauri build` are intentionally not part of normal local validation unless a release build is being prepared.
