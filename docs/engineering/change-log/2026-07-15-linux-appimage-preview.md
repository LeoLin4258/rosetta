# Linux AppImage Preview Baseline

Date: 2026-07-15

## Summary

Established Rosetta's first Linux x64 application packaging baseline and
validated a production AppImage through a real PDF translation workflow.

## Change

- Selected AppImage as the initial Linux x64 distribution and future Tauri
  updater format.
- Set the first supported build and runtime baseline to Ubuntu 24.04 x64.
- Added a Linux Tauri bundle override that produces AppImage artifacts and
  includes the staged Linux PDFium resource.
- Added `release-linux.sh` with version, host, architecture, PDFium, clean
  worktree, updater key, checksum, and artifact-name validation.
- Added explicit unsigned/dirty preview switches. Preview builds do not produce
  updater archives or signatures and must not be published.
- Documented the Linux release contract and recorded the packaging decision in
  ADR 0012.
- Fixed AppImage's `PYTHONHOME`, `PYTHONPATH`, and Linux `LD_LIBRARY_PATH`
  environment from leaking into the separately installed PDF Python worker.
- Made closing the Linux main window enter the same managed shutdown path as
  Windows, so hidden utility windows cannot keep the AppImage and its managed
  PDF/RWKV child processes alive.

No persistent data format changed.

## Preview Artifact

- File: `Rosetta-0.1.0-beta.20-linux-x64.AppImage`
- Size: `97450488` bytes
- SHA-256:
  `a31f463a4526dcf7f9c6171b61d41a573605fca17df7cbe0d337779bf947a6e5`
- Host: Ubuntu 24.04.4 x64, glibc 2.39

This artifact was built from a dirty development worktree through the explicit
preview path. It is for packaging validation only and must not be uploaded as a
public release.

## Validation

- `bash -n release-linux.sh`: passed on Ubuntu.
- Release safety guards rejected invalid flag combinations and dirty ordinary
  builds.
- Production frontend and Rust release compilation: passed.
- Tauri AppImage bundling: passed.
- AppImage SHA-256 verification: passed.
- AppImage extraction contained the x86_64 Rosetta binary and bundled Linux
  PDFium library.
- Normal FUSE AppImage launch: passed without
  `APPIMAGE_EXTRACT_AND_RUN`.
- PDF worker prewarm from the AppImage: passed after environment isolation.
- Managed RWKV Lightning CUDA launch from the AppImage: passed.
- Forced real one-page PDF retranslation: completed with 0 failed pages.
- Generated single-page PDF passed `pdfinfo`.
- Standard X11 `WM_DELETE_WINDOW` close with both managed services running:
  Rosetta, the AppImage runtime, PDF worker, Lightning CUDA, FUSE mount, and
  utility windows were all removed.

The signed `.AppImage.tar.gz` updater artifact, Supabase release row, updater
smoke test, and website download are intentionally left for later stages.
