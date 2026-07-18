# PDF v3 Incremental Export Commit

Date: 2026-07-18

## Summary

Added the bounded-memory write side of PDF v3 final export. The new committer
streams the immutable source into a temporary file, appends only changed PDF
objects and atomically replaces the destination after source verification and
file sync.

## Implementation

Added:

- compact `IncrementalExportBase` metadata;
- fixed 64 KiB source copy and simultaneous SHA-256 verification;
- structured serialization for every `lopdf::Object` variant;
- sorted delta-object validation and contiguous classic xref sections;
- source trailer preservation with `/Prev` chaining;
- cancellation checks throughout copy, object emission and pre-commit;
- same-directory temporary output, `sync_all` and platform atomic replace;
- Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` integration;
- generic destination-preservation tests;
- a real 30-page shared-font renderer/export integration test.

## Current Boundary

- writer memory is bounded by a 64 KiB source buffer plus the caller-owned delta;
- the current renderer still creates that delta from a complete mutable
  `lopdf::Document` in tests;
- production export is not yet end-to-end bounded-memory;
- lazy source-object access, explicit renderer delta staging, resumable export
  orchestration and 500/1000-page stress validation remain pending;
- current classic xref offsets are limited to `u32::MAX`.

## Initial Evidence

On the 30-page real-paper fixture, two page translations and one shared Arial
subset produced:

- source: 1,590,242 bytes;
- output: 1,617,258 bytes;
- appended bytes: 27,016;
- delta objects: 10;
- output pages: 30;
- matching translation Type0 subsets: one.

Both translated strings were searchable through PDFium after incremental
reopen.

## Independent Verification

Poppler rendering at 150 DPI found:

- page 1: 2,559 changed pixels (0.1176%), bbox `(245, 1592, 551, 1611)`;
- page 2: 2,059 changed pixels (0.0946%), bbox `(671, 1592, 899, 1611)`;
- page 3: zero changed pixels.

Both changed regions are the intended footer replacements. Visual inspection
found no clipping, overlap or unrelated movement.

Independent `pypdf` checks confirmed:

- 30 source and output pages;
- each translation appears only on its target page;
- page 1-3 annotation counts remain `26, 31, 7`;
- metadata is unchanged;
- source and output both contain zero outlines.
