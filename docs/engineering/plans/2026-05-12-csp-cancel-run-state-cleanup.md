# 2026-05-12 CSP, Cancelable Translation Run, and State Cleanup Plan

## Background

The code-quality follow-up left three larger items for a staged cleanup:

- `tauri.conf.json` still disables CSP.
- Frontend stop currently exits the UI loop, but it does not cancel the Rust `reqwest` request already submitted to the RWKV API.
- Rust command inputs still accept broad strings, and Jobs page selection is derived from route and store state in-line.

## Stages

1. Enable a minimal Tauri CSP and add small internal enums for command input boundaries.
2. Move document translation batch execution into a Rust in-memory run state with `start`, `cancel`, and `status` commands.
3. Add a Jobs selection helper so route-first selection logic is centralized without removing persisted store fields yet.

## Explicit Non-Goals

- Do not change the default RWKV API base URL.
- Do not add remote URL policy or remote endpoint restrictions.
- Do not introduce persistent run cache, SQLite, or a long-lived backend queue format.
- Do not run a production build unless explicitly requested.

## Validation

Use the normal static validation:

```powershell
cd rosetta-app
corepack pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
cargo test rwkv_api
```

Because this plan enables CSP, runtime validation is also required:

```powershell
cd rosetta-app
corepack pnpm tauri dev
```

Manual runtime checks:

- Main window starts without a blank screen.
- Settings page opens and theme sync works.
- TXT/Markdown import dialog works.
- Source and translation preview windows open.
- Translation can start, stop, and continue.
- Selected-block retranslation still creates a revision.
- Export and updater check entry points remain usable.

## Rollback Boundary

If CSP breaks runtime behavior, adjust to the narrowest working CSP. Do not return to `csp: null` unless the main window is unusable and no narrower source directive fixes the issue.
