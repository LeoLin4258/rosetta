# 2026-07-02 Windows Release PDF Component Handover

## Purpose

This handover captures the current state after preparing the first Windows x64
release using the locally forked PDFMathTranslate component.

The next agent should continue from here with Windows release packaging and
release regression. Do not rediscover or rebuild the PDF component unless a
new fork commit or package problem is found.

Primary related plan:

```txt
docs/engineering/plans/2026-07-02-windows-nvidia-lightning-performance-tuning.md
```

Latest explicit user intent:

```txt
Ship one Windows version first. The PDF component has been packaged, uploaded
to rosetta-assets, and the app should now use the real online resource.
```

## Current Status

Completed:

- Built a Windows x64 pdf2zh sidecar ZIP from the local PDFMathTranslate fork.
- User uploaded that ZIP to `LeoLin4258/rosetta-assets`.
- Updated the Windows pdf2zh profile to point at the new online asset.
- Verified both the GitHub release URL and the githubdog mirror return `200 OK`.
- Verified the online `Content-Length` matches the profile `pack_size_bytes`.
- Ran the narrow Rust test suite for managed pdf2zh.

Not completed yet:

- A real fresh-download install through the app UI.
- Windows release packaging.
- Full release regression.
- Final user-facing release notes.

## PDF Fork And Pack

Local fork:

```txt
C:\Users\Leo\Documents\GitHub\PDFMathTranslate
```

Fork commit used for this pack:

```txt
8013b071c78bdf5ce514502d931f7e5234abcd58
```

The fork was observed clean before the pack build.

Build script:

```txt
rosetta-app/src-tauri/scripts/build-pdf2zh-pack-windows-amd64.ps1
```

Build command used:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri

.\scripts\build-pdf2zh-pack-windows-amd64.ps1 `
  -Pdf2zhSourcePath "C:\Users\Leo\Documents\GitHub\PDFMathTranslate"
```

Local build output:

```txt
rosetta-app/dist/pdf-layout/rosetta-pdf2zh-windows-amd64.zip
rosetta-app/dist/pdf-layout/windows-amd64-manifest.json
```

Pack metadata:

```txt
pack filename: rosetta-pdf2zh-windows-amd64.zip
sizeBytes: 337414999
sha256: 7a8a00f9acab81561cbdd4848e8e42b1e64f845ec211d10e66562ecfdfb820b4
pdf2zh version: 1.9.11
python version: 3.12.13
layout model: doclayout_yolo_docstructbench_imgsz1024.onnx
layout model sha256: fece9af02f618b603ff7921ccec6861d13e7e1f9830e091dfb7e8ad9311e5b21
```

The build smoke test reported:

```txt
pdf-pack-imports-ok pdf2zh=1.9.11 providers=AzureExecutionProvider,CPUExecutionProvider
```

Important pack entries were checked after build:

```txt
windows-amd64/python/python.exe
windows-amd64/models/doclayout_yolo_docstructbench_imgsz1024.onnx
```

## Published Asset

Release tag:

```txt
pdf-layout-pack-windows-x64-v2026.07.02.1
```

Primary mirror URL in the app profile:

```txt
https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.02.1/rosetta-pdf2zh-windows-amd64.zip
```

GitHub fallback URL in the app profile:

```txt
https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.02.1/rosetta-pdf2zh-windows-amd64.zip
```

Both URLs were verified with:

```powershell
curl.exe -I -L --max-time 30 "<url>"
```

Observed result for both:

```txt
HTTP 200
Content-Length: 337414999
Content-Type: application/octet-stream
```

Do not use PowerShell `Invoke-WebRequest -Method Head` as the only check here.
It threw a local `System.NullReferenceException` on the GitHub release redirect,
while `curl.exe` succeeded and showed the correct final response.

## App Profile State

Profile file:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs
```

The Windows profile should contain:

```rust
pub const WINDOWS_AMD64_PDF2ZH: Pdf2zhProfile = Pdf2zhProfile {
    id: "windows-amd64-pdf2zh",
    platform_os: "windows",
    platform_arch: "x86_64",
    enabled: true,
    pack_directory_name: "windows-amd64",
    bin_relative_path: "python/python.exe",
    pack_filename: "rosetta-pdf2zh-windows-amd64.zip",
    pack_size_bytes: Some(337_414_999),
    pack_sha256: Some("7a8a00f9acab81561cbdd4848e8e42b1e64f845ec211d10e66562ecfdfb820b4"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.02.1/rosetta-pdf2zh-windows-amd64.zip",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.02.1/rosetta-pdf2zh-windows-amd64.zip",
    ],
};
```

The `githubdog` mirror intentionally remains first for mainland download
reliability. The GitHub URL remains as fallback.

## Validation Already Run

This passed after updating the profile:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo test managed_pdf2zh
```

Result:

```txt
37 passed
```

No dev server or production app build was run during this handover step.

## Local Sidecar Layout

The managed pdf2zh installer resolves the sidecar under Tauri's local app data
directory:

```txt
%LOCALAPPDATA%\com.rosetta.desktop\pdf2zh-sidecar
```

Windows pack directory:

```txt
%LOCALAPPDATA%\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64
```

Manifest path:

```txt
%LOCALAPPDATA%\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\manifest.json
```

Downloads directory:

```txt
%LOCALAPPDATA%\com.rosetta.desktop\pdf2zh-sidecar\downloads
```

For a real fresh-download test, prefer moving the existing `windows-amd64`
directory aside instead of deleting it immediately:

```powershell
$pack = "$env:LOCALAPPDATA\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64"
$backup = "$env:LOCALAPPDATA\com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64.backup-$(Get-Date -Format yyyyMMdd-HHmmss)"
Move-Item -LiteralPath $pack -Destination $backup
```

Only do that when the app and any pdf2zh worker are stopped. This avoids mixing
old locally imported PDF packs with the new online release asset.

## Recommended Next Steps

### 1. Verify real online PDF component install

Goal: prove the packaged app path can download the online resource, verify
size/hash, unpack it, and start the warm pdf2zh worker.

Suggested approach:

- Stop the app and any lingering pdf2zh worker.
- Move the existing local `windows-amd64` pack aside, not user jobs.
- Start the app in the mode explicitly required for release verification.
- Use the app's PDF component install flow.
- Confirm the installed manifest has the expected `sha256` and `sizeBytes`.
- Confirm the PDF worker warms up without needing manual local ZIP import.

Do not delete user job data under:

```txt
%APPDATA%\com.rosetta.desktop\jobs
```

### 2. Run release validation

Per `AGENTS.md`, when relevant run:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
pnpm typecheck

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo check
cargo test rosetta_jobs
```

Also rerun the pdf2zh narrow tests if the profile or installer changes again:

```powershell
cargo test managed_pdf2zh
```

### 3. Manual Windows smoke tests

At minimum:

- clean or fresh PDF component install from the online asset;
- PDF worker prewarm completes noticeably faster than the old package;
- forced PDF translation works through Lightning;
- Markdown translation works through Lightning;
- PDF export works after background page artifact compression;
- switching Lightning -> llama.cpp -> Lightning still selects the expected
  provider;
- app restart can reopen the latest translated PDF job without visible repair
  warnings.

### 4. Package the Windows app

Only run production packaging when the user explicitly asks to proceed with the
Windows release build. The repo rule says not to run dev servers or production
builds unless explicitly requested.

Before packaging, decide whether to include a short change-log entry for this
release pass. The performance tuning and PDF component replacement are
release-relevant, but the current handover did not create that final release
note.

## Important Context For The Next Agent

The PDF fork changes are not Windows-only conceptually. The fork-side work
should benefit other platforms once corresponding macOS / AMD / Linux packs are
built from the same fork and their profiles are updated. However, this handover
only prepared and validated the Windows x64 asset.

Do not overwrite the existing rosetta-assets release if another PDF pack is
needed. Create a new release tag, upload the new asset, then update:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs
```

with the new URL, size, and sha256.

Keep PDF component assets out of the main Rosetta repo. The app should download
them from `rosetta-assets`; the local `dist/pdf-layout` ZIP is only a build
artifact.

The performance pass is otherwise considered close enough to stop active tuning.
The remaining work is release validation, not another large optimization round,
unless fresh smoke tests reveal a regression.

## Privacy Reminder

PDF diagnostics and benchmark summaries may include:

- job id;
- run id;
- page counts;
- timings;
- request counts;
- batch sizes;
- file sizes;
- hashes.

They must not include:

- source document text;
- translated text;
- prompts;
- raw model responses containing document content;
- document structure content.

## Quick Checklist

- [x] Windows PDF sidecar pack built.
- [x] User uploaded pack to `rosetta-assets`.
- [x] Windows profile points to the online release asset.
- [x] GitHub URL and githubdog URL verified by `curl.exe -I -L`.
- [x] `cargo test managed_pdf2zh` passed.
- [ ] Fresh online install verified through the app.
- [ ] Full validation commands completed.
- [ ] Windows release package built.
- [ ] Final release smoke test completed.
- [ ] Release notes / change-log finalized if needed.
