# PDF v3 Durable Translation Export

Date: 2026-07-18

## Summary

Connected durable resolved TranslationPatch authorities to shared-font,
multi-page incremental PDF export.

## Implementation

- added a source/store/PageSet-bound final export request and typed result;
- added cancellable first-pass document font planning;
- prepared one document-wide subset for each required Regular/Bold face;
- replayed one resolved page patch at a time against the accumulated lazy
  object overlay;
- separated page-local decision fonts from fitted-only document output fonts so
  overflow preservation can be reproduced without embedding unused glyphs;
- rejected renderer-decision drift before destination replacement;
- merged only explicit page and font object deltas;
- added a SHA-256-verified atomic source-copy path for all-preserved exports;
- normalized cancellation from planning and commit phases into one top-level
  export cancellation outcome;
- reported fitted/preserved counts, font characters/subset bytes, object counts
  and source/appended/output bytes without logging document text.

## Validation

- focused font-plan and incremental-export tests;
- real Windows/PDFium 30-page source with two durable translated pages;
- PDFium text extraction and pixel comparison for changed and untouched pages;
- all-preserved byte-exact source-copy export;
- Poppler rendering and visual inspection of translated and untouched pages;
- `cargo test --locked pdf_v3 --lib` (`180 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`87 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The isolated native pipeline can now export durable resolved translations into
a complete PDF without page-PDF merge artifacts. Runtime manifest binding,
Tauri/job lifecycle integration, and real complex 500/1,000-page
translation/export stress validation remain pending.
