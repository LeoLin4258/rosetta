# PDF v3 Selected-Page Index

Date: 2026-07-18

## Summary

Moved selected-page identity in PDF v3 renderer staging onto a reusable lazy
page index.

## Implementation

- added a `PdfObjectView`-backed page-tree index for explicit `PageSet` values;
- skipped unselected subtrees by `/Count` and stopped after the maximum selected
  page;
- retained page object, ancestor page-tree and direct content-stream IDs;
- rejected malformed trees, cycles, repeated ownership, invalid contents and
  out-of-range selections;
- passed the index through TranslationPatch preflight and replacement staging;
- replaced renderer `Document::get_pages()` and selected-page
  `get_page_contents()` lookups;
- reused one lazy `[1, 2]` index across the real multi-page export proof;
- kept compatibility render paths on one-page indexes.

## Current Boundary

- page count, selected page identity and selected top-level content roots no
  longer require a complete source object graph;
- inherited resources, content-stream decoding and global cross-page ownership
  discovery still use a complete immutable `lopdf::Document`;
- renderer memory is therefore not yet end-to-end bounded by source object
  count;
- the next migration should move resource context and exact stream reads onto
  `PdfObjectView` without weakening source-preserving fallbacks.

## Validation

- PDF v3: 125 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo check`, Rust formatting and frontend typecheck passed;
- source: 1,590,242 bytes, output: 1,617,258 bytes;
- appended section: 27,016 bytes, delta objects: 10;
- Poppler page 1: 2,559 changed pixels, 0.1176%, confined to
  `[245, 551) x [1592, 1611)`;
- Poppler page 2: 2,059 changed pixels, 0.0946%, confined to
  `[671, 899) x [1592, 1611)`;
- Poppler page 3: pixel-exact;
- `pypdf`: 30 pages, identical metadata and page 1-3 annotation counts of 26,
  31 and 7 retained;
- both translations were extractable only on their intended pages;
- visual inspection found no clipping, overlap or unrelated movement.
