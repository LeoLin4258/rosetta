# PDF v3 Overlay-Backed Page Delta Staging

Date: 2026-07-18

## Summary

Stopped applying accumulated font and page deltas to the complete PDF v3
source traversal document during final multi-page export.

## Implementation

- split renderer staging into immutable source traversal and accumulated object
  identity views;
- used the accumulated view maximum for font and copy-on-write allocation;
- resolved document-font registry objects from the accumulated overlay;
- preserved the accumulated maximum for all-preserved page deltas;
- kept compatibility mutation APIs on stage-then-apply behavior;
- changed the real two-page export proof to stage every page against one
  unchanged source document and a fresh accumulated overlay;
- removed the proof's complete mutable working-document clone;
- added Form copy-on-write coverage for unapplied font deltas and collision-free
  clone allocation.

## Current Boundary

- final export no longer duplicates font subset streams in a complete working
  document;
- multi-page staging retains one read-only complete source graph instead of
  source plus cloned working graphs;
- the accumulated delta is the only staged-object authority;
- page tree lookup, inherited resources, content decode and ownership discovery
  still use a complete immutable `lopdf::Document`;
- lazy page indexing, remaining source traversal migration and 500/1000-page
  stress validation remain pending.

## Validation

- PDF v3: 122 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo check`, Rust formatting and frontend typecheck passed;
- real two-page incremental export test: passed;
- overlay-backed multi-target Form allocation test: passed;
- overlay-backed all-preserved page maximum test: passed;
- lazy source open: 8ms in one Windows AMD debug run;
- three source object reads: less than 1ms in aggregate;
- post-read cache: 3 entries, estimated 10,303 bytes;
- source: 1,590,242 bytes, output: 1,617,258 bytes;
- appended section: 27,016 bytes, delta objects: 10;
- Poppler page 1: 2,559 changed pixels, 0.1176%, confined to
  `[245, 551) x [1592, 1611)`;
- Poppler page 2: 2,059 changed pixels, 0.0946%, confined to
  `[671, 899) x [1592, 1611)`;
- Poppler page 3: pixel-exact;
- `pypdf`: 30 pages, metadata and page 1-3 annotation counts of 26, 31 and 7
  retained;
- both translations were extractable only on their intended pages;
- visual inspection found no clipping, overlap or unrelated movement.
