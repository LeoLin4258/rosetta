# 2026-05-12 CSP Runtime Boundary

## Context

The code-quality follow-up identified `tauri.conf.json` using `csp: null` as a desktop security boundary risk. Rosetta renders local user documents inside a WebView, so the app should keep a minimal CSP even while PDF and richer preview formats are still planned.

## Changes

- Enabled a minimal Tauri CSP in `rosetta-app/src-tauri/tauri.conf.json`.
- Kept RWKV API traffic in Rust `reqwest`; the frontend still does not fetch model endpoints directly.
- Preserved the existing `ReactMarkdown` preview path without raw HTML/script execution.

## Current CSP

```text
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data: asset: http://asset.localhost;
font-src 'self' data:;
connect-src 'self' ipc: http://ipc.localhost
```

## Non-Goals

- Did not change the default RWKV API base URL.
- Did not add remote endpoint allow/deny policy.
- Did not run or require a production build.

## Validation

- `corepack pnpm typecheck`
- `cargo check`
- Runtime verification is still required with `corepack pnpm tauri dev` because CSP problems only show up inside the WebView.

## Rollback Boundary

If runtime verification shows a blocked dev-only channel such as Vite HMR, adjust the relevant CSP directive narrowly. Do not return to `csp: null` unless the main window is completely unusable and no narrower directive fixes it.
