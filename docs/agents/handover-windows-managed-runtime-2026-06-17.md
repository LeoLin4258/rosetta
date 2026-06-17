# Handover: Windows Managed RWKV + PDF Runtime

Date: 2026-06-17

This handover is for the next agent continuing the Windows NVIDIA version work.
The user considers commit `964788d1cae237f929fc737bb5a292ee6183747e` the stable
macOS baseline. Keep that baseline conceptually sacred: Windows should add only
platform-specific artifacts/runtime profiles, not fork Rosetta's proven macOS
logic.

## Primary Goal

Ship a Windows release whose user experience matches the current stable macOS
release.

This is the central product goal. The Windows version should feel like the same
Rosetta app with Windows-specific RWKV/PDF artifacts underneath, not a new
implementation with different onboarding, lifecycle behavior, PDF translation
flow, or error model. Treat the stable macOS build at
`964788d1cae237f929fc737bb5a292ee6183747e` as the behavioral reference for:

- onboarding and install guidance
- local RWKV model/runtime startup and stop behavior
- PDF component install/import/status UX
- PDF worker prewarm and translation flow
- text/PDF translation task lifecycle
- failure messages that guide repair instead of exposing implementation details

Success means a real Windows NVIDIA user can install Rosetta, prepare the local
RWKV model/runtime and PDF component, translate text documents and PDFs, stop or
restart the runtime, and recover from missing artifacts with the same level of
clarity and reliability that the existing stable macOS version provides.

## Current Implementation State

Implemented in the working tree:

- RWKV lifecycle now has a profile-derived `LaunchSpec` in
  `rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs`.
  - macOS keeps the old `--model --tokenizer --backend --host --port --model-name`
    invocation shape.
  - Windows CUDA uses `--model-path <model> --vocab-path <vocab> --port <port>`.
  - Start, stop, app-exit cleanup, and stale-process matching now share the same
    launch signature instead of duplicating platform-specific checks.
- Windows RWKV runtime profile now expects a `.zip` pack name:
  `RWKV_lightning_CUDA_sm75+_Win_MSVC.zip`.
  - Installer can extract zip runtime packs using Rust `zip` support.
  - The old `.7z` path should not be used as the release contract.
- `managed_pdf2zh` now has a Windows profile:
  - `id = "windows-amd64-pdf2zh"`
  - `pack_directory_name = "windows-amd64"`
  - Python path is `python/python.exe`
  - CLI fallback uses `python -m pdf2zh.pdf2zh`
- PDF worker and PDF CLI fallback now read Python/CLI behavior from
  `Pdf2zhProfile`; macOS still uses its existing `bin/pdf2zh` path.
- PDF pack installer supports `.zip` in addition to the existing macOS `.tar.gz`
  path, with zip-slip-safe extraction.
- Frontend PDF local import now supports `packPath`, so Windows native paths like
  `C:\...` no longer need broken `file://` construction.
- Local provider selection no longer silently uses `http://127.0.0.1:8765`.
  If the managed runtime is not ready or has no live base URL, it throws a clear
  error.
- Onboarding now exposes `runtimeReady` and bases local onboarding on the full
  RWKV install plan, not only model-file presence. This prevents Windows from
  skipping onboarding when the `.pth` exists but the runtime pack is missing.
- Added a Windows PDF pack helper:
  `rosetta-app/src-tauri/scripts/build-pdf2zh-pack-windows-amd64.ps1`.

## Important Caveats

- Windows RWKV runtime zip metadata is intentionally not finalized.
  `runtime_archive_size_bytes` and `runtime_archive_sha256` are currently `None`
  in `managed_rwkv/profile.rs` because the previous values belonged to the old
  `.7z` artifact. Do not paste the old values back.
  - Before Windows runtime install can be release-ready, build the final zip and
    fill in exact size, SHA256, and download URLs.
  - As of this handover, a Windows runtime install that needs to install the
    runtime pack will still require those profile values to be completed.
- Windows PDF profile also has no final pack size/SHA/download URL yet. It can
  be exercised through local import / env override paths, but release metadata
  must be pinned before shipping.
- `temp.md` is an untracked file in the repo root. It was present during this
  work and was not touched.
- `rosetta-app/src-tauri/Cargo.toml` appears as modified in `git status` due to
  Windows line-ending metadata, but `git diff -- rosetta-app/src-tauri/Cargo.toml`
  showed no content diff.

## Files Touched

Backend RWKV:

- `rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs`
- `rosetta-app/src-tauri/src/managed_rwkv/install.rs`
- `rosetta-app/src-tauri/src/managed_rwkv/mod.rs`
- `rosetta-app/src-tauri/src/managed_rwkv/profile.rs`
- `rosetta-app/src-tauri/src/onboarding.rs`

Backend PDF:

- `rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/layout.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/status.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/install.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/worker.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`
- `rosetta-app/src-tauri/scripts/build-pdf2zh-pack-windows-amd64.ps1`

Frontend:

- `rosetta-app/src/lib/providers/index.ts`
- `rosetta-app/src/lib/pdf2zhRuntime.ts`
- `rosetta-app/src/lib/useManagedPdf2zhRuntime.ts`
- `rosetta-app/src/types/rosetta.ts`

## Validation Already Run

Use absolute tool paths in this environment because `cargo`, `pnpm`, and `node`
were not all on PATH by default.

Passed:

```powershell
& C:\Users\Leo\.cargo\bin\cargo.exe check
```

Result: passed. Existing warnings remain in PDF-related dead code
(`Encrypted`, `count_pdf_pages_lopdf`, `extract_pages_pdf`, `timeout_ms`).

```powershell
$env:PATH='C:\Users\Leo\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin;C:\Users\Leo\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin;' + $env:PATH
& C:\Users\Leo\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin\pnpm.cmd typecheck
```

Result: passed.

```powershell
& C:\Users\Leo\.cargo\bin\cargo.exe test rosetta_jobs
```

Result: passed, 36 tests.

```powershell
& C:\Users\Leo\.cargo\bin\cargo.exe test managed_rwkv
```

Result: passed, 35 tests.

```powershell
& C:\Users\Leo\.cargo\bin\cargo.exe test managed_pdf2zh
```

Result: passed, 9 tests.

## Recommended Next Steps

1. Build final Windows RWKV runtime zip.
   - Root should contain `rwkv_lighting_cuda.exe`, `rwkv_vocab_v20230424.txt`,
     and `lib/`.
   - Compute exact byte size and SHA256.
   - Fill `runtime_archive_size_bytes`, `runtime_archive_sha256`, and
     `runtime_download_urls` in `managed_rwkv/profile.rs`.

2. Build final Windows PDF zip.
   - Root directory should be `windows-amd64/`.
   - It must contain `python/python.exe` and
     `models/doclayout_yolo_docstructbench_imgsz1024.pt`.
   - Confirm `python/python.exe -m pdf2zh.pdf2zh --version` works after
     extraction.
   - Fill `pack_size_bytes`, `pack_sha256`, and `pack_download_urls` in
     `managed_pdf2zh/profile.rs`.

3. Test on an actual Windows NVIDIA machine.
   - Fresh app data.
   - RWKV runtime install/import.
   - `.pth` model install/download.
   - Start/stop/restart runtime; verify no stale `rwkv_lighting_cuda.exe`
     remains.
   - Text translation through local provider.
   - PDF component install/import.
   - PDF worker prewarm and PDF translation.
   - Cancellation kills PDF child processes.

4. Re-check macOS regression risk.
   - Fresh macOS onboarding still reaches the same PDF-ready / PDF-skipped
     states as the stable baseline.
   - Already-installed macOS users are not sent back to onboarding unless the
     full RWKV install plan is actually incomplete.

5. Consider adding one small helper to reduce future mistakes:
   - A shared artifact extraction helper for zip/tar.gz could serve both RWKV
     runtime and PDF packs.
   - Keep this narrow; do not refactor the translation pipeline while finishing
     Windows support.

## Design Constraints To Preserve

- Do not turn Rosetta into a generic AI assistant or cloud workflow.
- Do not fork PDF translation logic for Windows. The platform difference should
  stay inside `Pdf2zhProfile` and pack contents.
- Do not add a default local URL fallback. Managed runtime translation should
  only use a live `baseUrl` from runtime status.
- Do not reintroduce `.7z` as the app-facing Windows runtime artifact unless
  a robust in-app 7z extractor is deliberately added and tested.
- Do not change macOS stable behavior unless required by a shared bug fix.
