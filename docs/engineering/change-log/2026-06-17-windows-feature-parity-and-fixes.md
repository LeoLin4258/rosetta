# 2026-06-17 Windows Feature Parity and Production Fixes

## Context

beta.13 shipped a Windows NVIDIA CUDA dogfood path with hand-staged model
import. Bringing the Windows experience to macOS parity (online model +
runtime + PDF component install, mainland mirrors, PDF translation, full
onboarding flow) surfaced several production blockers — most of them
Windows-only — that prevented the freshly installed app from being usable.

This change-log covers (a) the parity work, (b) the audit fixes that came
out of a code review of the dogfood patch, and (c) the production blockers
found during the first end-to-end test on a real NVIDIA Windows machine.

## Scope

### Windows online install parity

- `WINDOWS_AMD64_CUDA` model download list now includes the ModelScope
  mirror in addition to HuggingFace and `hf-mirror.com`.
- `WINDOWS_AMD64_CUDA.runtime_download_urls` is populated with the upstream
  `rwkv_lightning_cuda` GitHub release + `githubdog.com` mirror. The
  managed runtime can now fetch the runtime pack end-to-end without a
  local archive.
- Added the `WINDOWS_AMD64_PDF2ZH` profile and made it visible through
  `Pdf2zh` profile resolution. Windows machines now have a real PDF
  component pack instead of `unsupported`.
- `managed_pdf2zh::worker::spawn_worker` picks the Python interpreter path
  with `cfg!(target_os = "windows")`: macOS uses `python/bin/python` (POSIX
  PBS layout), Windows uses `python/python.exe` (PBS install_only layout).
- `Pdf2zhStaticStatus::unsupported` no longer hard-codes the macOS profile
  + "v1 仅支持 macOS Apple Silicon" copy.
- Onboarding restores the full **model → PDF** two-step flow that beta.11
  shipped on macOS: after the model installs, the user is shown the
  `PdfSetupStep` panel and can choose to install the PDF component now or
  skip it to Settings. This was previously moved entirely to Settings; the
  Windows release brings the in-onboarding choice back.
- Added `src-tauri/scripts/build-pdf2zh-pack-windows-x64.ps1` — a Windows
  build script for the PDF component (PowerShell instead of bash so it
  runs on dev machines without WSL). The script:
  - Auto-detects a local PBS tarball in `~/Downloads` before falling back
    to a network download.
  - Uses the Tsinghua PyPI mirror to install `pdf2zh` for mainland network
    conditions.
  - Stages `models/doclayout_yolo_docstructbench_imgsz1024.pt` (with the
    HuggingFace + `hf-mirror.com` fallback, or a local copy from
    `~/Downloads`) and patches `pdf2zh.py` to prefer
    `ROSETTA_DOCLAYOUT_MODEL`, matching the macOS pack.
  - Cleans `__pycache__` / `*.pyc` after the smoke test and verifies no
    stale bytecode remains.

### Removed dogfood scaffolding

The "导入 .pth 模型" Settings button was a dogfood-only path for when the
hosted model URL did not exist yet. With Windows online install fully
working, this is dead weight:

- Removed `InstallOptions.model_file_path`, `resolve_model_file_source`,
  and `install_local_model_file` (Rust).
- Removed `ManagedRuntimeInstallOptions.modelFilePath` (TypeScript).
- Removed `importModelFromFile` from `useManagedRwkvRuntime`.
- Removed the two "导入 .pth 模型" buttons from `LocalRwkvPanel`.

### Code-audit fixes (post-dogfood review)

- `request_translations` probe now selects `stop_token_mode` via
  `stop_token_mode_for_endpoint(endpoint)` instead of hard-coding
  `StopTokenMode::TextBoundary`, so the probe path matches the actual
  translation path for `/v1/batch/completions`.
- `debug_timestamp` outputs `YYYYMMDD-HHMMSS` UTC instead of Unix seconds.
  Implemented with manual date arithmetic to avoid pulling in a date
  crate.
- Useless SHA computation + blocking `std::fs` calls were eliminated as a
  side-effect of removing `install_local_model_file`.
- Audit findings #5b (no DocLayout model in the Windows pack) and #5c
  (CLI fallback running `python.exe <pdf>` without `-m pdf2zh.pdf2zh`)
  were caught before shipping — see "Production blockers" below.

### Production blockers (Windows-only)

Found during end-to-end test on a real NVIDIA Windows 11 machine. All
three issues only manifest in the packaged release, not in `tauri dev`.

- **Console windows popping up everywhere.** Every console-subsystem
  child process (the RWKV sidecar, `tar`, `taskkill`, `tasklist`,
  `powershell`, the pdf2zh Python worker, the CLI fallback) was being
  spawned without the `CREATE_NO_WINDOW` flag, so Windows allocated a
  fresh `conhost.exe` window for each one. Added
  `src-tauri/src/windows_process.rs` with a `HideConsole` trait
  implemented on both `std::process::Command` and
  `tokio::process::Command`, and applied `.hide_console_on_windows()` at
  every spawn site.
- **Translation hangs on first launch; cancel does nothing.** Same root
  cause as the console windows. The RWKV sidecar's stdout/stderr were
  rerouted to a log file, but Windows still attached the process to a
  newly-created console — and the sidecar prints a steady stream of
  per-token diagnostics during translation. Once the inherited console's
  pipe buffer filled, the sidecar's `write()` calls blocked, freezing
  translation mid-request and making `/cancel` HTTP calls never get
  serviced. Killing the console window (or restarting the app, which
  re-spawned without the leftover console attached) cleared the freeze
  — which matches what we saw in QA. With `CREATE_NO_WINDOW` the sidecar
  no longer has a console attached and `stdin/stdout/stderr` go where we
  redirect them, end of story.
- **PDF import always fails with "无法导入这个文件".** `pdfium.dll` was
  never bundled into the Windows release. The macOS bundle declares
  `bundle.resources = ["resources/pdf-sidecar/pdfium/*/*"]` in
  `tauri.macos.conf.json`, but the shared `tauri.conf.json` and the
  (absent) Windows override did not. Added
  `src-tauri/tauri.windows.conf.json` with the matching `resources` glob
  so the NSIS installer carries `pdfium.dll`. Also fixed
  `WorkspaceEmpty.tsx` to surface the real Rust error string instead of
  swallowing it when `err` is a plain string (Tauri's `invoke` rejects
  with the Rust `Err(String)` payload, not an `Error` instance) — without
  this, the generic "无法导入这个文件" hid the actual "找不到 pdfium 库
  文件…" diagnostic.

### Other Windows-specific correctness fixes

- `kill_process_tree` in `managed_pdf2zh::worker` now has a Windows
  branch using `taskkill /T /F /PID <pid>`. Previously only Unix had a
  process-tree kill, so cancelling a PDF translation on Windows could
  leave `pdf2zh` / Python multiprocessing workers running.
- The CLI fallback path in `pdf2zh_invoke.rs` now prepends `-m
  pdf2zh.pdf2zh` when `bin` is `python.exe` (Windows). On macOS `bin` is
  a `bin/pdf2zh` bash shim that already does this internally; on Windows
  the worker uses `python.exe` directly, so without the explicit module
  args the fallback would have executed `python.exe <source>.pdf …`.
- `useManagedPdf2zhRuntime` now accepts both POSIX (`/…`) and Windows
  (`C:\…`) absolute paths when importing a local PDF pack archive.
  Previously the `/`-prefix check rejected every Windows file-picker
  result.

## Release assets

### Windows RWKV runtime pack

Profile already points at:

```
https://github.com/Alic-Li/rwkv_lightning_cuda/releases/download/V1.0.0/RWKV_lightning_CUDA_sm75+_Win_MSVC.7z
https://githubdog.com/https://github.com/Alic-Li/rwkv_lightning_cuda/releases/download/V1.0.0/RWKV_lightning_CUDA_sm75+_Win_MSVC.7z
```

### Windows PDF component pack (rebuilt with DocLayout model)

- URL: `https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.06.17.2/rosetta-pdf2zh-windows-x64.tar.gz`
- Mainland mirror: `https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.06.17.2/rosetta-pdf2zh-windows-x64.tar.gz`
- Size: `394076995`
- SHA256: `fd5c2811980e1d6340f8a2f9a94da08a57bfcd2717050c8b7508547cd3a25138`

Tag `v2026.06.17.1` exists but is missing the DocLayout-YOLO model and is
superseded by `.2`. `managed_pdf2zh::profile::WINDOWS_AMD64_PDF2ZH` is
pinned to `.2`.

## Validation

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test
```

End-to-end testing on a Windows 11 NVIDIA machine (after rebuild):

1. Fresh install. Onboarding downloads model → prompts for PDF install →
   downloads PDF pack → enters workspace.
2. No `conhost.exe` window flashes for `tar.exe`, the sidecar, or
   `python.exe` at any point.
3. Translate a Markdown document immediately after onboarding (no
   restart). No hang, no console window.
4. Cancel a translation mid-stream — request stops, sidecar stays
   healthy.
5. Import a PDF, translate, export.

## Notes

- `tauri.windows.conf.json` is a new file. Tauri 2's config-merge picks
  it up automatically based on the build target — no CLI flag change is
  needed.
- The legacy `rwkv_runtime.rs` (`#[allow(dead_code)]` in `lib.rs`)
  already used `CREATE_NO_WINDOW` via `configure_runtime_command`. The
  new managed-runtime stack did not inherit that pattern; this change
  adds parity through a shared `HideConsole` trait so future spawn sites
  cannot regress silently.
