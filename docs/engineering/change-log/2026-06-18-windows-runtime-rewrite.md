# Windows runtime rewrite from beta.13 baseline

Date: 2026-06-18

## Summary

Started Windows support again from stable commit
`964788d1cae237f929fc737bb5a292ee6183747e`, without inheriting the later
Windows implementation.

## Release plan

| Phase | Name | Status |
|-------|------|--------|
| 0 | Dev environment ready | **Done** |
| 1 | RWKV runtime ZIP | **Done** |
| 2 | PDF pack ZIP | **Done** |
| 3 | No-GPU smoke test | Superseded by NVIDIA validation |
| 4 | GPU end-to-end test | **Done** |
| 5 | Build & release | Local installer done; signing/publish pending |

### Phase 0: Dev environment ready

- [x] `fetch-pdfium-windows-x64.ps1` — pdfium.dll staged
- [x] `cargo check` passes (5 warnings, 0 errors)
- [x] Fixed `lib.rs:175` `window` → `_window` variable name bug
- [x] `pnpm dev` frontend and Tauri dev app start

### Phase 1: RWKV runtime ZIP

Goal: From upstream `.7z` produce a dev ZIP, pin profile metadata.

- [x] Download `.7z` from `Alic-Li/rwkv_lightning_cuda` V1.0.0 Release
- [x] Build deterministic patched runtime ZIP (404,318,341 bytes, SHA256 `b2a4a08c...`)
- [x] Fill `profile.rs` `WINDOWS_AMD64_CUDA`: `runtime_archive_size_bytes`, `runtime_archive_sha256`
- [x] Publish verified ZIP to `rosetta-assets` tag `rwkv-runtime-windows-x64-v2026.06.18.21`
- [x] Configure GitHub and githubdog download URLs

### Phase 2: PDF pack ZIP

Goal: Build Windows pdf2zh embedded Python environment.

- [x] Build complete Windows PDF pack ZIP (388,779,668 bytes) + manifest
- [x] Fill `profile.rs` `WINDOWS_AMD64_PDF2ZH`: `pack_size_bytes`, `pack_sha256`
- [x] Upload to `LeoLin4258/rosetta-assets` Release (`pdf-layout-pack-windows-x64-v2026.06.18.2`)
- [x] Fill `pack_download_urls` (GitHub + githubdog mirror)

### Phase 3: No-GPU smoke test (on dev machine without NVIDIA)

- [x] App launches (`cargo tauri dev` compiles, 5 warnings 0 errors)
- [x] Main window shows (MainWindowTitle "Rosetta", Responding=True)
- [x] Onboarding window shows after deleting onboarding.json ("Welcome to Rosetta")
- [x] No orphan child processes on exit
- [x] Fixed `nvidia-smi` PATH detection bug (always-true guard)
- [ ] Hardware detection reports "未检测到 NVIDIA GPU" (need interactive GUI check)
- [ ] "使用自己的翻译 API" flow completes (need interactive GUI check)
- [ ] PDF rendering works (need interactive GUI check)
- [ ] Main window file import/export works (need interactive GUI check)

### Phase 4: GPU end-to-end test (on NVIDIA Windows machine)

- [x] Hardware detection identifies NVIDIA GPU model + compute capability
- [x] RWKV runtime ZIP download, verification, extraction, and manifest
- [x] Model download + SHA256 verify — HuggingFace mirror works
- [x] RWKV sidecar starts on IPv6 loopback and passes health probe
- [x] Text translation works through `/v1/batch/completions`
- [x] PDF translation works through the local OpenAI shim and Lightning CUDA
- [x] Persistent PDF worker prewarms and remains reusable between jobs
- [x] Stop runtime + process tree cleanup
- [x] Local data reset stops both workers and removes managed artifacts
- [x] Clean first-run onboarding downloads runtime, model, and PDF component

**Test run 1 (build without logging):** RWKV and PDF both failed silently.
No log file existed. No diagnostics.

**Test run 2 (build with app_log + diagnostic eprintln):**
- RWKV: tokenizer missing from runtime dir → sidecar never started.
  Root cause: `is_runtime_installed()` only checked exe existence.
  Fix applied: now checks exe + tokenizer + lib dir.
- PDF worker: spawned (pid 25276) but no stderr/timeout logged.
  Root cause: error paths in `spawn_worker` didn't `eprintln!`.
  Fix applied: added logging.

**Test run 3 (build with all fixes above):**
- RWKV: `runtime pack incomplete, extracting ZIP` — re-extraction triggered correctly.
  Model download in progress. RWKV sidecar start NOT YET VERIFIED (log ends during install).
- PDF worker: spawned (pid 31320), exited immediately with no stderr output.
  Error: `worker 在就绪前退出。` — Python process dies before printing anything.
  Root cause: **UNKNOWN — see Known Issues #2.**

### Phase 5: Build & release

- [x] Windows bundle configuration verified
- [x] Release executable and NSIS installer build successfully
- [x] NSIS installer first-run installation and complete user flow verified
- [x] Runtime and PDF component profiles use pinned published artifacts
- [ ] Configure `TAURI_SIGNING_PRIVATE_KEY` for updater artifacts
- [ ] Installer uninstall test
- [ ] GitHub Release

## Changes (code)

- Added the Windows NVIDIA SM75+ RWKV profile and hardware detection.
- Added native ZIP installation for the separate Windows CUDA runtime.
- Added hidden Windows child processes and process-tree termination.
- Added a Windows pdf2zh profile, native ZIP extraction, Windows Python
  invocation, and PDF worker process-tree cancellation.
- Restored onboarding order:
  `RWKV install/start/probe → PDF install/prewarm → ready`.
- External API selection now skips RWKV only and continues to PDF setup.
- PDF setup can be temporarily skipped through a visually secondary action.
- Added reproducible PowerShell scripts for the temporary RWKV development ZIP
  and the release-oriented Windows PDF pack.
- Added a Windows Tauri bundle override for PDFium resources.
- Fixed `lib.rs` `window` → `_window` variable in macOS close handler.
- Fixed `nvidia-smi` PATH detection: use `where.exe` fallback instead of
  always returning a bare filename that bypasses the existence check.
- Fixed all three Windows PowerShell scripts to reset PATH to system-only,
  avoiding Git Bash's `/usr/bin/tar` shadowing Windows `tar.exe`.
- Pinned final Windows PDF pack metadata: 388,779,668 bytes, SHA256
  `d3cad5c7...d529`.

## Artifact state

- Windows RWKV runtime ZIP: **published and verified**
  `RWKV_lightning_CUDA_sm75+_Win_MSVC.zip`
  Size: `404318341`
  SHA256: `b2a4a08cc3c1e6caa836850acd6ba86e3d03f9b2dde4fa1b65278aa00f870499`
  Release: `rwkv-runtime-windows-x64-v2026.06.18.21`
- Windows PDF pack: **published and verified**
  `rosetta-pdf2zh-windows-amd64.zip`
  Size: `388779668`
  SHA256: `d3cad5c7a5d0faf9a06d746c9a0e0343dcb969fada0c5702c96a1a5efe93d529`
  Release: `pdf-layout-pack-windows-x64-v2026.06.18.2`
- Both assets return HTTP 200 from the primary GitHub URL and the githubdog
  fallback, with server-reported sizes matching the pinned profiles.

## Historical blockers (resolved)

### Issue #1: RWKV install/start fails — no failure reason in log

**Symptom:** Test run 2: tokenizer file missing from runtime dir, sidecar
never starts. Test run 3: log shows `runtime pack incomplete, extracting ZIP`
but then **nothing** — no extraction result, no error, no sidecar start
attempt. RWKV is non-functional on both runs.

**Partial fix applied:** `layout.rs::is_runtime_installed()` now checks
exe + tokenizer + lib dir (previously only exe). This correctly triggers
re-extraction in test run 3. However:

**Remaining problems:**
1. The entire `install_runtime_pack` flow (extract → validate → write
   manifest) has NO `eprintln!` logging. If extraction fails, the error
   goes to the frontend only — invisible in the log file.
2. The log jumps from `[rwkv-install] runtime pack incomplete` directly to
   `[pdf2zh-worker]` — the RWKV extraction result and all subsequent steps
   (model verify, sidecar start) are completely absent.
3. It is unknown whether the ZIP extraction itself works correctly on
   Windows. The ZIP was created with Windows `tar -a -cf` and entries are
   prefixed with `./` — the Rust `zip` crate's handling of this has not
   been verified on the actual machine.

**To diagnose on the NVIDIA machine:**
```powershell
# Check if tokenizer was extracted after test run 3:
dir "C:\Users\rwkv\AppData\Local\com.rosetta.desktop\managed-rwkv\runtimes\rwkv-lightning-cuda-sm75-msvc\"

# Try manual sidecar start (if tokenizer exists now):
cd C:\Users\rwkv\AppData\Local\com.rosetta.desktop\managed-rwkv\runtimes\rwkv-lightning-cuda-sm75-msvc
.\rwkv_lighting_cuda.exe --model-path "C:\Users\rwkv\AppData\Local\com.rosetta.desktop\managed-rwkv\models\rwkv7-0.4b-translate-windows-pth\RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth" --vocab-path .\rwkv_vocab_v20230424.txt --port 28888
```

**Resolution:** The runtime archive is now built deterministically, contains
the required tokenizer and DLL set, and is validated after extraction. The
unsupported `--host` argument was removed, the pinned runtime was patched to
bind to IPv6 loopback, and Rosetta now uses the runtime's actual
`/v1/batch/completions` contract. Installation, startup, health probing, and
translation were verified on the NVIDIA Windows machine.

### Issue #2: PDF worker Python process exits immediately with no output

**Symptom:** Worker Python process spawns (pid visible in log) then exits
before printing anything to stdout or stderr. Error: "worker 在就绪前退出。"
with empty stderr tail.

**Root cause:** UNKNOWN. The embedded Python interpreter at
`pack/windows-amd64/python/python.exe` starts but crashes before any import
output. Possible causes (not yet investigated):
1. Missing Visual C++ runtime DLLs (vcruntime140.dll etc.) on the test machine
2. Embedded Python can't find its stdlib (wrong PYTHONHOME / ._pth configuration)
3. torch DLL load failure (CUDA / cuDNN mismatch or missing)
4. Python crashes before the unbuffered stderr flag takes effect
5. The worker script has a top-level syntax/import error that crashes silently

**To diagnose on the NVIDIA machine, run manually:**
```powershell
cd C:\Users\rwkv\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\worker
set PYTHONDONTWRITEBYTECODE=1
set PYTHONUNBUFFERED=1
set PYTHONNOUSERSITE=1
set PYTHONPATH=
set ROSETTA_DOCLAYOUT_MODEL=C:\Users\rwkv\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\models\doclayout_yolo_docstructbench_imgsz1024.pt
C:\Users\rwkv\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\python\python.exe rosetta_pdf2zh_worker.py
```
If that crashes, try just:
```powershell
C:\Users\rwkv\AppData\Local\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\python\python.exe -c "print('hello')"
```
to verify the Python interpreter itself works.

**Resolution:** The embedded Python environment itself was healthy. The pack
was incomplete because native `pip` failures did not stop the PowerShell
script. All native build steps now check `$LASTEXITCODE`; the pack includes
the modules imported unconditionally by pdf2zh and smoke-tests
`pdf2zh.converter.TextConverter`. The persistent worker now reaches ready and
successfully translates PDFs.

### Historical checkpoint

The section above records the state before takeover on the NVIDIA Windows
machine. Those failures are retained because they explain the runtime, pack,
API-contract, and process-lifecycle fixes. They are no longer current release
blockers.

## Diagnostic infrastructure added

- `app_log.rs`: Redirects stderr to `%APPDATA%/com.rosetta.desktop/logs/rosetta.log`
  with rotation (current → `.prev.log`). All `eprintln!` across all modules is persisted.
- RWKV lifecycle: logs command, cwd, port, PATH, pid before spawn; checks
  `child.try_wait()` during health poll; reads sidecar log tail on failure.
- RWKV start: logs full `build_static_status` snapshot (install plan items,
  paths, state) before attempting sidecar launch.
- PDF worker: logs python/script/cwd/model/pid at spawn; logs error message
  + stderr tail on failure and timeout.
- Install flow: logs whether runtime pack extraction was skipped or triggered.

## NVIDIA Windows takeover findings and fixes

Validated on an NVIDIA GeForce RTX 5070 (driver 596.21, CUDA 13.2):

- The published PDF pack was incomplete. Its embedded Python worked, but
  `doclayout_yolo` failed with `ModuleNotFoundError: No module named 'tqdm'`.
  The PowerShell pack script had continued after failed native `pip` and
  Python smoke-test commands because `$ErrorActionPreference = "Stop"` does
  not convert a native executable's non-zero exit code into an exception.
  Every Python build step now checks `$LASTEXITCODE`, and the pruned pack is
  import-tested again immediately before archive creation.
- The color-preservation patch also relied on the Windows process locale and
  failed on a Chinese GBK locale while reading UTF-8 Python source. It now
  reads and writes UTF-8 explicitly.
- The rebuilt PDF worker completed all four warmup phases and reached ready.
  That intermediate replacement pack was 386,074,457 bytes with SHA256
  `408690d6b04ea3ed2066dce1b3b4a33b50aaadd546f1c1b8bd9a8669603d4910`.
  It was later superseded by the final dependency-complete `.2` pack recorded
  in Artifact state.
- RWKV failed because Rosetta passed `--host 127.0.0.1`, which upstream
  V1.0.0 does not support. Its uncaught unknown-argument exception terminates
  as Windows BEX64 / `0xc0000409` in `ucrtbase.dll`.
- Starting the same runtime without `--host` loads the 0.4B model and serves
  `/v1/models`, but upstream hard-codes `0.0.0.0`, which violates Rosetta's
  local-only boundary.
- The staging script now verifies the upstream `.7z` SHA256, uses a static
  runtime-DLL allowlist, patches the two pinned `0.0.0.0` literals to IPv6
  loopback `::1`, and verifies the patched executable SHA256. Rosetta omits
  the unsupported `--host` argument and connects to `[::1]`.
- ZIP creation now uses sorted .NET `ZipArchive` entries with a fixed
  timestamp. Two consecutive builds produced the same 404,318,341-byte
  archive and SHA256
  `b2a4a08cc3c1e6caa836850acd6ba86e3d03f9b2dde4fa1b65278aa00f870499`.
- The first application translation attempt exposed a separate API-contract
  mismatch. Rosetta sent its Lightning `contents[]` batch body to
  `/v1/chat/completions`, while the CUDA runtime implements that body at
  `/v1/batch/completions`; the chat route is intended for `messages[]`.
  Rosetta also serialized `stop_tokens` as strings, but the runtime parses
  them as integer token IDs, producing HTTP 500 before inference.
- Managed runtime profile summaries now expose their translation endpoint.
  The frontend dispatches macOS profiles through `rwkv-mobile-batch-chat` and
  the Windows CUDA profile through `rwkv-lightning-contents` using
  `/v1/batch/completions`. The invalid string `stop_tokens` override was
  removed so each runtime uses its model-specific defaults.
- Live verification against the managed Windows process at IPv6 loopback
  returned HTTP 200 for a two-item streaming batch and produced the expected
  Chinese translations in choice-index order.
- The CUDA runtime emits a final SSE chunk with `delta: {}` and
  `finish_reason: "stop"`. Rosetta previously required every delta object to
  contain `content`, so it discarded otherwise successful batches at the
  finish frame. Streaming message content is now optional at deserialization,
  while the completed aggregate is still required to be non-empty.
- Rosetta requests Lightning batch translations with `stream: false`, matching
  the managed macOS translation behavior. Streaming response parsing remains
  only as compatibility handling for external APIs.
- The persistent PDF worker no longer changes its process working directory to
  a job's `pdf2zh-output` folder. Windows keeps a process working directory
  locked, so the old behavior made the next translation retry partially delete
  the folder, lose its diagnostics, and fail before the worker could start.
- Clearing Rosetta's local data now suspends and stops the persistent PDF
  worker before deleting `pdf2zh-sidecar`. This avoids Windows file-lock
  failures and prevents a cancelled translation from immediately prewarming a
  replacement worker during reset. Reset errors now show the concrete backend
  message instead of a generic failure.
- The Windows PDF pack now includes `deepl`, `ollama`, and
  `azure-ai-translation-text`. `pdf2zh.translator` imports all three modules
  unconditionally even when Rosetta selects only the local OpenAI-compatible
  shim. Pack smoke tests now import `pdf2zh.converter.TextConverter`, which
  exercises this real translation import path instead of only `import pdf2zh`.
- Published the verified Windows artifacts to `LeoLin4258/rosetta-assets`:
  - RWKV runtime tag `rwkv-runtime-windows-x64-v2026.06.18.21`
  - PDF component tag `pdf-layout-pack-windows-x64-v2026.06.18.2`
  Both profiles now use the GitHub Release URL with a githubdog fallback.
- First-run onboarding reports the remaining combined download size for the
  runtime, translation model, and PDF component, and labels the two real setup
  stages. Returning users do not count artifacts already present on disk.
- Windows runtime download now tries every pinned remote URL before falling
  back to a matching ZIP in the user's Downloads directory. Explicit
  `runtimePackPath` and environment overrides still take precedence. User
  cancellation stops mirror retries and the Downloads fallback immediately.
- Repair installation now stops and reaps the managed sidecar before deleting
  the Windows runtime directory. This avoids Windows error 5 when the running
  executable and DLLs are still locked.

## Final acceptance validation

Validated on 2026-06-19 using a clean local-data state and the generated NSIS
installer:

- Onboarding opened for a first-time user.
- The UI reported the combined runtime, model, and PDF component download.
- The Windows runtime, translation model, and PDF component downloaded from
  their published profiles and installed successfully.
- The managed RWKV process started and translated documents.
- The PDF worker prewarmed and completed PDF translation.
- Clearing local data stopped managed processes and removed installed
  components, allowing the first-run flow to be repeated.
- The complete user installation and usage flow passed.

Local NSIS artifact:

- File: `Rosetta_0.1.0-beta.13_x64-setup.exe`
- Size: `13,924,576` bytes
- SHA256:
  `405D559B53C4AD1B081C27E1BBCC22306B9EFCDD7E87CCB4C8712BF5075E80E0`

The first NSIS tool download was truncated and failed with
`unexpected end of file`; retrying downloaded and validated NSIS 3.11
successfully. The standard release build then stopped after installer
generation because updater artifacts are enabled but
`TAURI_SIGNING_PRIVATE_KEY` was not present. A local-test build with updater
artifacts disabled completed successfully. Production publishing still
requires the updater signing private key.
