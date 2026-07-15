# Linux PDFium Bootstrap

Date: 2026-07-13

## Summary

Made the existing PDFium bootstrap flow reproducible on Linux x64.

## Change

- Pinned the SHA-256 checksum for the `chromium/7834` Linux x64 PDFium
  archive.
- Included staged PDFium resources in the Linux Tauri bundle configuration.

No PDF pipeline behavior or persistent data format changed.

## Validation

Run on Ubuntu Linux x64:

```bash
cd rosetta-app
bash src-tauri/scripts/fetch-pdfium.sh --platform linux-x64
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Results:

- PDFium `chromium/7834` downloaded, checksum-verified, and installed as an
  x86-64 ELF shared library.
- `pnpm typecheck`: passed.
- `cargo check`: passed with 9 existing Linux dead-code warnings.
- `cargo test rosetta_jobs`: 70 passed, 0 failed.
