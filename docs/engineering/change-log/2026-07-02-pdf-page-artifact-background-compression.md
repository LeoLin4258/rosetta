# PDF Page Artifact Background Compression

Date: 2026-07-02

## Summary

Rosetta's Windows PDF Lightning path now keeps the optimized speed-first
single-page artifact write on the translation hot path and moves PDF page
artifact compression into a background Rust-owned maintenance task.

The motivation is the Phase 7 performance result: after cross-page batching
and layout replay reuse, single-page PyMuPDF compression on the hot path became
too expensive, but uncompressed page artifacts were about 14-17 MB per page on
the benchmark PDFs. Keeping those artifacts indefinitely would create
unacceptable job cache growth.

## Changes

- Added optional artifact metadata to `pdf_pages.<targetLang>.json`:
  `artifactCompression`, `artifactBytes`, and `artifactCompressionError`.
- Mark newly committed page artifacts as `fast` with their byte size.
- Added Windows background compression using the installed pdf2zh sidecar
  Python/PyMuPDF runtime. The task writes `.compressing.tmp.pdf`, validates the
  result as one page, and replaces the canonical page artifact only when the
  compressed file is meaningfully smaller.
- Added run/version/path guards so stale compression tasks skip pages changed
  by force retranslation or a newer run.
- Added backup/repair handling for interrupted replacement:
  `.precompress.bak` is restored if the canonical file is missing and removed
  if the canonical file already exists.
- Repair/load schedules compression for translated pages that are still in the
  fast format, so app exit during background compression does not permanently
  strand large artifacts.
- Added `ROSETTA_PDF_PAGE_ARTIFACT_COMPRESSION=off` as a local diagnostic
  escape hatch.

## Safety Notes

Compression is not part of translation correctness. A compression failure keeps
the page translated and records diagnostic state only. Job deletion and force
retranslation races are handled as skipped maintenance whenever the page state
no longer matches the candidate run/version/path.

## Validation

- `cargo check`
- `cargo test rosetta_jobs`

