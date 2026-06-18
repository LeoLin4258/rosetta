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
| 3 | No-GPU smoke test | In progress |
| 4 | GPU end-to-end test | Blocked on 3 |
| 5 | Build & release | Blocked on 4 |

### Phase 0: Dev environment ready

- [x] `fetch-pdfium-windows-x64.ps1` — pdfium.dll staged
- [x] `cargo check` passes (5 warnings, 0 errors)
- [x] Fixed `lib.rs:175` `window` → `_window` variable name bug
- [ ] `pnpm dev` frontend starts (not yet verified)

### Phase 1: RWKV runtime ZIP

Goal: From upstream `.7z` produce a dev ZIP, pin profile metadata.

- [x] Download `.7z` from `Alic-Li/rwkv_lightning_cuda` V1.0.0 Release
- [x] Run `build-rwkv-lightning-windows-dev-zip.ps1` → ZIP (404,232,358 bytes, SHA256 `2370dcf5...`)
- [x] Fill `profile.rs` `WINDOWS_AMD64_CUDA`: `runtime_archive_size_bytes`, `runtime_archive_sha256`
- [ ] After upstream publishes official ZIP tonight: swap URL + re-pin

### Phase 2: PDF pack ZIP

Goal: Build Windows pdf2zh embedded Python environment.

- [x] Run `build-pdf2zh-pack-windows-amd64.ps1` → ZIP (355,011,264 bytes) + manifest
- [x] Fill `profile.rs` `WINDOWS_AMD64_PDF2ZH`: `pack_size_bytes`, `pack_sha256`
- [x] Upload to `LeoLin4258/rosetta-assets` Release (`pdf-layout-pack-windows-x64-v2026.06.18.1`)
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

- [ ] Hardware detection identifies GPU model + compute capability
- [x] RWKV runtime ZIP install — fixed, re-extraction now triggered (see Known Issues #1)
- [x] Model download + SHA256 verify — HuggingFace mirror works
- [ ] RWKV sidecar start + /v1/models health probe — blocked by #1 (first run), should work after fix
- [ ] Text translation (Markdown) works
- [ ] PDF translation (OpenAI shim → Lightning CUDA) works
- [ ] Stop runtime + process tree cleanup

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

- [ ] `tauri.windows.conf.json` config verified
- [ ] `tauri build --target x86_64-pc-windows-msvc` succeeds
- [ ] Installer install/uninstall test
- [ ] If upstream published official RWKV ZIP: swap profile URL + re-pin
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
- Pinned Windows PDF pack metadata: 355,011,264 bytes, SHA256 `9d64d03a...`.

## Artifact state

- Temporary RWKV ZIP: **built**
  `RWKV_lightning_CUDA_sm75+_Win_MSVC.zip`
  Size: `404232358`
  SHA256: `2370dcf5578f480be4721100bad8ff44b1de83b4cb98d17a38ba6955cd6faddf`
- Windows PDF pack: **built**
  `rosetta-pdf2zh-windows-amd64.zip`
  Size: `355011264`
  SHA256: `9d64d03abf505d67df396f8560aebd4d47478b465149dff9c295c667efd59825`
- The runtime URL, size, and SHA must be replaced with the official upstream
  ZIP metadata before release.
- The Windows PDF pack download URLs remain empty — fill after uploading to
  `LeoLin4258/rosetta-assets`.

## Known Issues (Phase 4 blockers)

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

**Status:** UNRESOLVED. Detection improved, but extraction and startup not
verified. Logging still insufficient.

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

**Status:** Unresolved. Needs manual diagnosis on the NVIDIA test machine.

### Summary: both RWKV and PDF are non-functional on Windows

Three test builds, three failures. The core problem is that the Codex-written
code was never tested on an actual Windows machine — it compiled but the
runtime behavior is unvalidated. The diagnostic logging added across these
test iterations has improved visibility but not fixed the underlying issues.

**Next steps require manual work on the NVIDIA machine** — run the diagnostic
commands above to determine whether the components work at all outside the
Tauri app wrapper. If they don't, the problems are in the artifacts (ZIP
contents, Python pack structure), not the Rust orchestration code.

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
  The verified replacement pack is 386,074,457 bytes with SHA256
  `408690d6b04ea3ed2066dce1b3b4a33b50aaadd546f1c1b8bd9a8669603d4910`.
  The broken `.1` download URLs are disabled until this replacement is
  uploaded under a new release tag.
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
- Repair installation now stops and reaps the managed sidecar before deleting
  the Windows runtime directory. This avoids Windows error 5 when the running
  executable and DLLs are still locked.
