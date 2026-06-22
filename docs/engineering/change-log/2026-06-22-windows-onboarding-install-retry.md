# 2026-06-22 Windows onboarding install retry

## Problem

On a clean NVIDIA Windows installation, the first RWKV download could fail
and show the onboarding error screen. Clicking **重新下载** then returned
immediately with:

```txt
已有安装任务在进行中。
```

The original download failure was no longer recoverable without restarting
the application.

## Root cause

`install_model` marked the shared install progress as `preflight` before
filesystem preparation and the full Windows runtime/model installation.
Several fallible operations then used early `?` returns.

Those exits cleared neither the active progress phase nor, for the earliest
failures, the cancellation handle. The command returned an error to the
frontend while the registry still looked active, so every retry was rejected
as a concurrent installation.

The onboarding action also relied only on React render state to prevent
duplicate starts. A sufficiently fast double click could issue two install
commands before the disabled state rendered.

After the retry-state fix exposed the original backend error, a second issue
was found on the NVIDIA Windows test machine:

```txt
无法清理旧 Windows RWKV 运行包: 拒绝访问。 (os error 5)
```

Onboarding retries use `repair: false`, while the installer previously stopped
the managed sidecar only for explicit repair installs. An incomplete runtime
pack also causes the status layer to hide its sidecar path, so stale-process
cleanup could not derive the full process signature before deleting the
runtime directory.

## Changes

- Wrapped all post-registration installation work in one result boundary.
- On every error, convert an active install phase to `failed`, preserve the
  concrete error message, emit the final progress event, and clear the
  cancellation handle.
- Keep explicit `cancelled` state intact instead of rewriting it as a failure.
- Centralized the definition of active install phases so retry admission and
  failure cleanup cannot drift apart.
- Added a synchronous onboarding flow lock to prevent duplicate RWKV install
  commands from rapid clicks.
- Made the onboarding error state display the latest concrete backend error
  instead of replacing it with a generic installation failure.
- Before replacing an incomplete Windows runtime pack, stop both the
  registered sidecar and any Rosetta sidecar matching the exact runtime,
  tokenizer, and model paths derived directly from `RuntimeLayout`.
- Match managed sidecar command-line paths case-insensitively on Windows, in
  line with Windows filesystem semantics.
- Retry Windows runtime-directory deletion for up to five seconds when
  `PermissionDenied` indicates that process termination or antivirus scanning
  has not released a file lock yet.
- After the PDF download reaches 100%, replace the completed byte meter with
  explicit verification, extraction, installation, and worker-warmup states.
- During PDF worker startup, show the live warmup phase
  (`加载 PyTorch` / `加载文档版面模型库` / `加载 pdf2zh` / `完成准备`) and elapsed
  seconds so the cold start does not look frozen.
- Refresh the main window's managed-runtime snapshot when onboarding
  completes. The main window is pre-created and previously retained its
  boot-time `not-installed` state even though onboarding had already started
  and probed the runtime successfully.
- Disable local translation until the selected managed runtime is truly
  `ready`, showing a short startup state instead of allowing an immediate
  failed translation request.
- On Windows, closing the main window now requests an application exit rather
  than destroying only that window. This guarantees the existing
  `ExitRequested` cleanup stops the RWKV sidecar and PDF worker even while
  hidden onboarding or preview windows still exist.
- Changed the first-run theme default from dark to `system`. Removed the
  hard-coded dark Tauri window theme and initialize the React system-theme
  snapshot directly from `prefers-color-scheme`, avoiding a dark first-frame
  flash on light Windows and macOS systems. Existing explicit light/dark
  preferences remain persisted.
- Added regression tests covering retry-blocking phases, failed-state release,
  and cancelled-state preservation.

## Expected behavior

After a download, filesystem, verification, extraction, or manifest error:

- onboarding displays the concrete failure;
- the install registry is in `failed`, not an active phase;
- clicking **重新下载** starts a new attempt immediately;
- cancelling remains a cancellation and can also be retried.
