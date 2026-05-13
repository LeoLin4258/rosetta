# 2026-05-13 macOS Window Chrome

## Scope

- Added `rosetta-app/src-tauri/tauri.macos.conf.json` as the macOS-specific window override.
- macOS now uses native window decorations with `titleBarStyle: "Overlay"` and `hiddenTitle: true`, so the system red/yellow/green traffic lights are preserved instead of using the Windows custom titlebar buttons.
- Positioned the macOS traffic lights inside the left sidebar region and aligned them closer to the main header controls.
- Kept the Windows path on the existing undecorated window, custom React titlebar, and Mica configuration from `tauri.conf.json`.
- Extended the app sidebar into the macOS titlebar area and added top padding so the sidebar content does not collide with traffic lights.
- Added a macOS-only header drag region in the main content header. Interactive controls remain excluded from drag behavior.
- When the sidebar is collapsed on macOS, the header's sidebar toggle and page title shift right with a `duration-300 ease-out` transition to avoid the traffic lights.
- Added a macOS-only `rosetta-macos` styling scope for translucent sidebar tokens and CSS `backdrop-filter`.
- Enabled macOS transparent window rendering and the native `sidebar` window material for the main window.

## Rationale

Rosetta previously used a Windows-first window treatment on all desktop platforms: an undecorated main window plus a custom React titlebar. On macOS that made the app feel foreign and forced Rosetta to reimplement behavior that the OS already provides well.

The macOS path now follows a native desktop workbench shape: system traffic lights, content under an overlay titlebar, a translucent sidebar, and a draggable main header. The Windows path remains unchanged because it depends on Mica and custom window controls.

## Platform Boundary

- macOS window behavior lives in `tauri.macos.conf.json`, not in the shared `tauri.conf.json`.
- `macOSPrivateApi` is enabled for transparent window rendering. This is acceptable for direct distribution builds, but it is a known Mac App Store constraint.
- macOS-only CSS must stay under `.rosetta-macos` so Windows Mica tuning and default sidebar tokens are not accidentally changed.
- Do not add custom React traffic lights on macOS unless a future ADR explicitly reverses the native titlebar direction.

## Validation

- `pnpm typecheck`
- `cargo check`
- `pnpm tauri info` was used during the initial macOS config pass, but later runs were stopped when environment probing stalled after printing environment details.

## Runtime QA Needed

Before release packaging, verify in `pnpm tauri dev` on macOS:

- Traffic lights align with the header controls and do not overlap the sidebar toggle when the sidebar is expanded or collapsed.
- The collapsed sidebar header shift animates smoothly and preserves the drag region.
- Sidebar translucency is visible in both light and dark themes.
- Header drag and double-click maximize still work outside interactive controls.
