# PDF v3 Lazy Page Context and Source Stream Reads

Date: 2026-07-18

## Summary

Moved selected-page dictionaries, inherited resources and target content-stream
reads from the complete PDF document onto the immutable lazy source view.

## Implementation

- added an owned `PdfPageObjectContext` for exact page dictionaries and
  materialized inherited resources;
- resolved direct and indirect resource dictionaries with bounded reference
  traversal and nearest-scope precedence;
- separated renderer source-object reads from accumulated overlay identity;
- read replacement identity, preflight content and staged source streams from
  `PdfObjectView`;
- staged non-copy-on-write page font resources from the page context;
- retained `Document` only for cross-page ownership and Form COW traversal;
- added malformed-resource, precedence and real-fixture equivalence tests;
- added lazy source cache ceilings to the real two-page export proof.

## Current Boundary

- ordinary selected-page source reads no longer require a complete object
  graph;
- `content_stream_referencing_pages()` still uses complete
  `Document::get_pages()` for global ownership discovery;
- Form invocation validation, copy-on-write clone-tree resource resolution and
  global Form ownership still use the complete `Document`;
- renderer memory is therefore not yet end-to-end bounded by source object
  count;
- the next migration should move Form invocation and COW resource contexts onto
  owned lazy views, leaving global ownership discovery as the final major
  complete-document dependency.

## Validation

- PDF v3: 128 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo check`, Rust formatting and frontend typecheck passed;
- lazy two-page staging: 12 source loads, 23 cache hits, 12 resident entries and
  28,712 resident bytes;
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
