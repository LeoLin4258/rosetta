# PDF v3 Font Staging on Lazy Object Views

Date: 2026-07-18

## Summary

Migrated document-wide translation-font allocation and registry validation from
a complete `lopdf::Document` to the bounded PDF v3 object-view boundary.

## Implementation

- implemented `PdfObjectView` for the temporary `lopdf::Document` compatibility
  path;
- changed prepared-font and document-font staging to allocate from a generic
  immutable object view;
- simplified allocation to avoid source object enumeration or lookup;
- changed registry binding to validate the live Type0 object through the view;
- propagated typed lazy source-object failures through `TranslationFontError`;
- staged the real export's document font delta directly against
  `PdfSourceObjectStore`;
- retained the old mutation helper as stage-then-apply compatibility behavior;
- added a lazy-source/delta-overlay identity test with zero source object loads.

## Current Boundary

- font allocation and registry binding no longer require a complete document;
- incremental export base and font staging share one mapped source store;
- page tree, resource inheritance, content parsing, replacement preflight and
  sequential page read views still use a complete temporary document;
- end-to-end bounded-memory export and 500/1000-page stress validation remain
  pending.

## Validation

- PDF v3: 122 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo check`, Rust formatting and frontend typecheck passed;
- font tests: 5 passed, 1 ignored manual probe;
- real two-page incremental export test: 1 passed;
- lazy source open: 9ms in one Windows AMD debug run;
- three source object reads: less than 1ms in aggregate;
- post-read cache: 3 entries, estimated 10,303 bytes;
- source: 1,590,242 bytes, output: 1,617,258 bytes;
- appended section: 27,016 bytes, delta objects: 10;
- Poppler page 1: 2,559 changed pixels, 0.1176%, confined to
  `[245, 551) x [1592, 1611)`;
- Poppler page 2: 2,059 changed pixels, 0.0946%, confined to
  `[671, 899) x [1592, 1611)`;
- Poppler page 3: pixel-exact;
- `pypdf`: 30 pages, metadata and annotation arrays retained;
- both translations were extractable only on their intended pages;
- visual inspection found no clipping, overlap or unrelated movement.
