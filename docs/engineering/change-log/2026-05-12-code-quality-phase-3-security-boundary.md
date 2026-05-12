# 2026-05-12 Code Quality Phase 3 Security Boundary

## Summary

Rosetta completed the third code-quality cleanup stage focused on desktop security boundaries and paused runtime exposure.

This pass intentionally avoids changing the current TXT/Markdown import, external RWKV API translation flow, job cache format, preview windows, updater behavior, or export behavior.

## Changes

- Removed paused managed RWKV runtime commands from the Tauri invoke handler.
- Kept `rwkv_runtime.rs` in the Rust codebase as parked experimental context, matching ADR 0002. It is no longer callable from the frontend command surface.
- Changed the parked frontend `rwkvRuntime.ts` adapter to reject with an explicit paused-runtime error instead of invoking unregistered commands.
- Removed initialization of the unused `tauri-plugin-opener` plugin.
- Removed `opener:default` from the default desktop capability.
- Removed unused opener package dependencies from frontend and Rust manifests.
- Kept `core:webview:allow-create-webview-window` because source and translation preview windows use Tauri `WebviewWindow`.
- Kept `process:default` because the Settings updater flow uses `relaunch()`.
- Kept `updater:default` because manual update checks remain part of Settings.

## Behavior Boundary

- The current translation backend remains the configured external RWKV translation API. This is the development-stage path and also a future optional backend.
- The long-term preference for an app-managed local model runtime is unchanged, but it remains paused until a runtime choice ADR is created.
- PDF and Word support remain planning-stage file format targets. Current implemented imports remain TXT and Markdown.
- No persistent Rosetta job data format changed.
- No user document contents, paths, segment text, credentials, or translation results were added to logs.

## Deferred

- CSP tightening is deferred to a runtime verification pass. The app currently uses custom Tauri webview windows, manual updater integration, and WebView IPC; changing CSP without running the desktop app can create silent runtime regressions that TypeScript and `cargo check` do not catch.
- The default external RWKV API endpoint remains unchanged during development. It is expected to be revisited when app-managed local RWKV runtime work resumes.

## Validation

Validate with:

```powershell
cd rosetta-app
corepack pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Expected result:

- TypeScript typecheck passes.
- Rust check passes.
- `rosetta_jobs` tests pass.
