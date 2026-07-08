# 2026-07-07 PDF Artifact Compression Cross-Platform

## Context

PDF page artifacts can become very large because each translated single-page
PDF may retain a full embedded CJK font copy. The background PyMuPDF
compression task was originally wired through the Windows PDF pack path because
the Windows profile exposed `python/python.exe` as its runnable binary, but the
disk-pressure problem is not Windows-specific.

## Changes

- Added a platform-aware `Pdf2zhLayout::python_path` helper for the installed
  PDF component pack.
- Switched PDF page artifact background compression from the profile binary
  path to the pack Python path, so macOS uses `python/bin/python` and Windows
  uses `python/python.exe`.
- Removed the Windows-only guard from background page artifact compression.
  Compression now runs on any supported platform with an installed PDF
  component pack and valid translated-page candidates.
- Kept `ROSETTA_PDF_PAGE_ARTIFACT_COMPRESSION=off` as the cross-platform local
  diagnostic switch.

## Validation

```powershell
cd rosetta-app/src-tauri
cargo test managed_pdf2zh::layout::tests::pdf_pack_python_path_matches_platform_layout
cargo test rosetta_jobs::tests::pdf_cleanup_restores_missing_canonical_page_from_compression_backup
```
