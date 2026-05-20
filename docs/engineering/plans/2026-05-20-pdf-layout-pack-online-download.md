# Rosetta PDF Layout Component Online Download Plan

> Status: draft, created 2026-05-20. This plan turns the current dogfood-only `file://` PDF layout component install path into a real online download path for end users.

## Context

PDF Phase 3 now uses a local PDF layout processing component built from PDFMathTranslate (`pdf2zh`) to preserve PDF layout and generate translated PDFs. The app already has:

- a managed `pdf2zh` installer backend;
- a settings panel for PDF layout processing status/install/repair;
- onboarding flow that prompts users to install the PDF layout component after the local translation model;
- dogfood staging/archive scripts that produce a local `.tar.gz`;
- `ROSETTA_PDF2ZH_PACK_URL=file://...` support for testing.

What is missing: a real downloadable pack URL, published artifact, pinned SHA256/size, and release process.

## Product Goal

Real users should be able to install Rosetta, finish onboarding, and download the PDF layout component from the internet without developer-only environment variables.

The UI should continue to describe this as “PDF 版面处理”, not as a second translation model. The local RWKV model remains the translation engine; the PDF component is responsible for PDF parsing/layout/reconstruction.

## Non-Goals

- Do not make PDF setup mandatory forever. Users may skip PDF setup in onboarding and install later from Settings.
- Do not add account/login/cloud sync.
- Do not upload user PDFs.
- Do not broaden Tauri filesystem permissions.
- Do not solve Windows/Linux pack distribution in this step. v1 remains macOS Apple Silicon first.

## Target User Flow

### First Launch

1. User installs Rosetta.
2. Onboarding installs/validates local RWKV model.
3. Onboarding shows the PDF layout setup screen.
4. User clicks “安装 PDF 版面处理组件”.
5. App downloads the official macOS arm64 PDF component archive.
6. App verifies SHA256 and size.
7. App extracts to:

```txt
~/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar/pack/macos-arm64/
```

8. Completion page says translation model and PDF layout processing are ready.

### Skip Path

1. User clicks the small “稍后再装” action.
2. Onboarding completes.
3. Settings page shows PDF layout processing as not installed.
4. First PDF translation or Settings can install the component later.

### Repair Path

1. User opens Settings → PDF 版面处理.
2. User clicks “重新安装 / 修复”.
3. App removes the old pack and downloads/extracts the pinned official pack again.

## Pack Artifact

### Filename

```txt
rosetta-pdf2zh-macos-arm64.tar.gz
```

### Archive Root

The archive must contain a single top-level directory:

```txt
macos-arm64/
  bin/pdf2zh
  python/
  manifest.json        # optional inside pack, installer writes its own install manifest too
```

The installer already accepts archives whose root is either:

- `macos-arm64/`, or
- the pack root directly.

Keep the single top-level directory for predictable release artifacts.

### Runtime Requirements

The archive must be self-contained:

- bundled Python environment;
- `pdf2zh==1.7.9` or explicitly chosen replacement version;
- NumPy compatibility fixed, either by pinning a compatible NumPy/Python set or applying the `fromstring -> frombuffer` patch;
- wrapper at `bin/pdf2zh`;
- `PYTHONDONTWRITEBYTECODE=1` in the wrapper;
- no stale `__pycache__` / `.pyc`;
- no runtime HuggingFace model download on first user translation if possible.

Current dogfood pack is created by:

```bash
cd rosetta-app
bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
bash src-tauri/scripts/archive-pdf2zh-pack-local.sh
```

For release, this should become a deterministic build script rather than “whatever is on the developer machine”.

## Hosting

### Primary Host

Use a GitHub Release artifact under the Rosetta repository or a dedicated Rosetta release-assets repository.

Recommended release naming:

```txt
pdf-layout-pack-macos-arm64-vYYYY.MM.DD.N
```

Attach:

```txt
rosetta-pdf2zh-macos-arm64.tar.gz
rosetta-pdf2zh-macos-arm64.tar.gz.sha256
```

### Mirror Host

Optional but recommended later:

- Hugging Face dataset/repo mirror;
- another static download endpoint if GitHub is slow/unavailable in some regions.

The profile already supports multiple download URLs, so the installer can try the first URL now and later support fallback if needed.

## Backend Changes

### 1. Pin Official Artifact In Profile

Update:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs
```

Set:

```rust
pack_size_bytes: Some(...),
pack_sha256: Some("..."),
pack_download_urls: &[
    "https://github.com/.../releases/download/.../rosetta-pdf2zh-macos-arm64.tar.gz",
],
```

Keep environment overrides for development:

- `ROSETTA_PDF2ZH_PACK_URL`
- `ROSETTA_PDF2ZH_PACK_SHA256`
- `ROSETTA_PDF2ZH_PACK_SIZE_BYTES`
- `ROSETTA_PDF2ZH_BIN`

### 2. Improve Download Robustness

The installer currently supports HTTP download, `file://`, proxy, SHA256, size check, extraction, progress, and cancel.

Before shipping online download, add:

- `.part` temp file writes, then atomic rename after success;
- delete corrupt partial file on checksum mismatch;
- user-facing failure message that does not expose raw internal terms;
- retry button in onboarding/settings already calls install again;
- optional fallback URL attempt if primary URL fails.

### 3. Keep Local-Only Privacy Boundary

The downloaded artifact is executable code, but user PDFs never leave the machine. Document this in UI copy if needed:

```txt
PDF 版面处理组件只在本机运行，用于读取 PDF 排版并生成译文 PDF。
```

## Build Script Plan

Add a release-oriented script separate from the dogfood staging helper:

```txt
rosetta-app/src-tauri/scripts/build-pdf2zh-pack-macos-arm64.sh
```

Responsibilities:

1. Create a clean temporary build root.
2. Create a Python environment.
3. Install pinned dependencies.
4. Apply compatibility patches.
5. Remove caches and unnecessary files.
6. Write `bin/pdf2zh`.
7. Smoke test:

```bash
bin/pdf2zh --version
```

8. Archive to:

```txt
dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz
```

9. Emit:

```txt
dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz.sha256
dist/pdf-layout/manifest.json
```

The existing `stage-pdf2zh-pack-local.sh` can remain a fast dogfood helper.

## Release Checklist

1. Build pack from a clean machine or clean CI runner.
2. Confirm `bin/pdf2zh --version`.
3. Confirm no stale bytecode:

```bash
find macos-arm64 -name '__pycache__' -o -name '*.pyc'
```

4. Archive and compute SHA256/size.
5. Upload artifact and checksum to GitHub Release.
6. Update `managed_pdf2zh/profile.rs` with URL/SHA/size.
7. Run:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test managed_pdf2zh
cargo test rosetta_jobs
```

8. Manual dogfood:

```bash
rm -rf "$HOME/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar"
rm -f "$HOME/Library/Application Support/com.rosetta.desktop/onboarding.json"
pnpm tauri dev
```

Expected: onboarding downloads from the pinned online URL without `ROSETTA_PDF2ZH_PACK_URL`.

## Test Matrix

### Happy Path

- Fresh app data.
- No `ROSETTA_PDF2ZH_PACK_URL`.
- Onboarding installs RWKV.
- Onboarding prompts PDF layout setup.
- User clicks install.
- Download progresses.
- SHA/size verification passes.
- Settings shows “PDF 版面处理已就绪”.
- PDF import/translation works.

### Skip Path

- Fresh app data.
- User skips PDF layout setup.
- Onboarding completes.
- Settings shows PDF layout component missing.
- First PDF translation prompts/installs.

### Network Failure

- Disable network or use an invalid URL in profile/dev override.
- Install fails with product-level message.
- Retry is available.
- App can still enter workspace if user skips PDF setup.

### Proxy

- Configure download proxy in onboarding/settings.
- Confirm RWKV and PDF downloads both use the stored proxy.

### Corrupt Download

- Serve a modified archive with wrong SHA.
- Installer rejects it.
- Old working pack remains if this was a repair install.

### Cancel

- Start PDF component download.
- Cancel before completion.
- Onboarding stays in PDF setup/error path.
- Retry can resume/restart cleanly.

## Open Decisions

1. Hosting location:
   - same app release;
   - separate `rosetta-assets` release;
   - Hugging Face mirror.

2. Version policy:
   - stay on `pdf2zh==1.7.9` for first real pack;
   - move to newer PDFMathTranslate/BabelDOC CLI before publishing.

3. Pack size target:
   - dogfood pack is currently larger than ideal;
   - release pack should remove optional web/server dependencies where safe.

4. Whether to pre-bundle layout models to avoid runtime HuggingFace downloads.

## Recommended Implementation Order

1. Create release build script.
2. Build first clean macOS arm64 pack locally.
3. Upload to a private/draft GitHub Release.
4. Pin URL/SHA/size in `profile.rs`.
5. Test fresh onboarding with no environment variables.
6. Add fallback URL support if primary download reliability is poor.
7. Publish official pack release.

