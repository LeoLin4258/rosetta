# Cross-platform onboarding and runtime lifecycle handoff

Date: 2026-06-22

## Purpose

This document records the platform impact and merge requirements for the
onboarding, first-translation, PDF setup, theme, and process-lifecycle work
completed on `codex/windows-runtime-rewrite`.

This branch will eventually replace or merge into `main`, and both the macOS
and Windows release packages are built from `main`. These changes must
therefore be treated as shared release behavior with platform-specific
branches, not as a disposable Windows-only implementation.

## Shared behavior that benefits both platforms

The following frontend and managed-runtime behavior is shared by Windows and
macOS:

- Onboarding prevents duplicate RWKV installation commands caused by rapid
  repeated clicks.
- Every RWKV installation error leaves the install registry in a terminal
  `failed` state instead of an active phase, so the user can retry without
  restarting Rosetta.
- The onboarding error screen displays the concrete backend error rather than
  replacing it with a generic installation failure.
- PDF setup no longer appears frozen after download reaches 100%:
  - verifying the archive;
  - extracting and installing the component;
  - writing installation metadata;
  - starting the persistent PDF worker.
- PDF worker startup displays its live warmup stage:
  - loading PyTorch;
  - loading the document-layout model library;
  - loading pdf2zh;
  - completing preparation.
- The onboarding window also displays elapsed warmup time.
- When onboarding completes, the pre-created main window immediately refreshes
  the real managed-RWKV status. It no longer continues using the stale
  boot-time `not-installed` or `installed` snapshot.
- If onboarding has already started the runtime, the main window treats an
  “already running” start result as a reason to refresh status rather than as
  a terminal startup failure.
- When the selected provider is the managed local runtime, translation remains
  disabled until the runtime state is actually `ready`.
- The workspace shows a startup state instead of allowing the user to submit a
  translation request to a runtime that has not finished loading.
- The default theme for a new installation is `system`.
- Tauri windows no longer force an initial dark theme.
- The React system-theme snapshot is initialized directly from
  `prefers-color-scheme`, avoiding a dark first-frame flash on light systems.
- Existing users who explicitly selected light or dark mode retain that
  persisted preference.

These shared changes should ship in both the Windows x64 and macOS Apple
Silicon beta.14 packages.

## Windows-specific behavior

The following fixes are intentionally Windows-only:

- Before replacing an incomplete Windows RWKV runtime pack, Rosetta stops the
  registered sidecar and matching stale sidecar processes.
- Managed sidecar command-line paths are compared case-insensitively, matching
  Windows filesystem semantics.
- Runtime-directory deletion retries for a bounded period when Windows reports
  `PermissionDenied` while a process or antivirus scanner is releasing a file
  lock.
- Closing the primary Windows window through the custom title-bar close button
  requests an application-level exit.
- A Windows `CloseRequested` event for the `main` window also requests an
  application-level exit, covering Alt+F4 and other native close paths.
- Application exit runs the existing cleanup sequence before termination:
  - stop and reap the managed RWKV process tree;
  - stop the persistent PDF worker;
  - then exit Tauri.

The Windows close behavior is required because Rosetta pre-creates hidden
windows and may also have preview windows. Destroying only `main` does not
necessarily produce `ExitRequested`, leaving Tauri, WebView2, RWKV, and Python
processes alive.

## macOS-specific behavior

macOS keeps its existing native lifecycle semantics:

- Closing the main window hides it instead of exiting Rosetta.
- The app remains available from the Dock and restores the hidden window on
  reopen.
- The managed RWKV runtime and PDF worker may remain warm while the app is
  running with no visible windows.
- `Cmd+Q` or the Rosetta application menu’s Quit command requests a real
  application exit.
- A real application exit must run the same RWKV and PDF worker cleanup used by
  Windows.

Do not apply the Windows “close main window means quit” rule to macOS. It would
conflict with the existing macOS window and Dock behavior.

## Merge requirements for `main`

When promoting `codex/windows-runtime-rewrite` to `main`:

1. Preserve the shared onboarding and workspace changes. Do not resolve
   conflicts by reverting to the beta.13 frontend files solely because the
   branch is named for Windows.
2. Preserve the `cfg(target_os = "windows")` and
   `cfg(target_os = "macos")` window-event boundaries in `src-tauri/src/lib.rs`.
3. Preserve the macOS platform override in `tauri.macos.conf.json`, including
   native decorations, overlay title bar, traffic lights, transparency, and
   sidebar effect.
4. Do not restore hard-coded `"theme": "Dark"` values in either base or macOS
   Tauri configuration.
5. Keep the Zustand default theme as `system`.
6. Keep the onboarding-completed runtime status refresh and local-runtime
   readiness gate in the main workspace.
7. Keep the existing updater public key. These changes do not authorize
   generating a new updater signing key.
8. Include the engineering change log
   `docs/engineering/change-log/2026-06-22-windows-onboarding-install-retry.md`
   in the merged history.

## Dual-platform release validation

### Windows x64

- Complete onboarding from clean local data.
- Verify RWKV and PDF downloads, post-download phases, and PDF warmup feedback.
- Enter the workspace and immediately translate a Markdown document without
  visiting Settings or restarting Rosetta.
- Verify the translate button stays disabled with clear startup feedback until
  RWKV is ready.
- Close through the custom title-bar button and through Alt+F4.
- Confirm that Rosetta, WebView2 children, `rwkv_lighting_cuda.exe`, and the
  managed PDF Python worker terminate.
- Relaunch and confirm immediate translation works with the installed
  artifacts.
- Verify first-run theme follows Windows light/dark mode.

### macOS Apple Silicon

- Complete onboarding from clean local data.
- Verify shared RWKV/PDF phase feedback and worker warmup labels.
- Enter the workspace and immediately translate a Markdown document without
  visiting Settings or restarting Rosetta.
- Verify the translate button remains disabled until the MLX runtime is ready.
- Close the main window and confirm the app stays available from the Dock.
- Reopen from the Dock and confirm the workspace returns.
- Quit with `Cmd+Q` and confirm the RWKV sidecar and PDF worker terminate.
- Verify the first-run theme follows macOS appearance and still responds to
  live system appearance changes.
- Recheck native traffic lights, overlay title bar, transparent sidebar, and
  macOS drag regions after the merge.

## Validation already completed on the branch

- `pnpm typecheck`
- `cargo check`
- managed RWKV installation tests
- managed RWKV lifecycle tests
- `cargo test rosetta_jobs`

These checks do not replace the Windows and Apple Silicon packaged-app smoke
tests above.
