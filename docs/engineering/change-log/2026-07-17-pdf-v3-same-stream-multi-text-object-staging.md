# PDF v3 Same-Stream Multi-Text-Object Staging

Date: 2026-07-17

## Summary

Separated logical `BT`/`ET` validation from physical stream staging so one PDF
v3 page batch can translate multiple text objects in the same content stream or
Form invocation without duplicate writes or clones.

## Implementation

Added:

- logical target identity using stream, invocation path and source text-object
  bounds;
- same-stream targets with distinct `BT`/`ET` bounds;
- physical grouping by stream and invocation path after unchanged-source
  validation;
- one descending operation splice and one encode/compress per physical group;
- defensive duplicate operation-index rejection;
- top-level and Form copy-on-write regression fixtures;
- zero-mutation stale-target and duplicate-text-object coverage.

The existing diagnostic schemas remain unchanged and text-free.

## Windows AMD Results

Unique top-level stream:

- logical targets: 2;
- physical stream rewrites: 1;
- cloned streams: 0;
- staged font objects: 6;
- Source Han elapsed time: about 4 ms;
- source size: 13,473 bytes;
- output size: 16,488 bytes;
- growth: 3,015 bytes;
- PDFium/`pypdf` text extraction and Poppler visual review: passed.

Selected Form invocation:

- logical targets: 2;
- physical Form leaf rewrites: 1;
- cloned streams: 2 (one leaf plus one root);
- sibling invocation source text: preserved;
- source root and Form objects: unchanged.

## Current Boundary

- every logical target remains inside one `BT`/`ET`;
- unanchored consecutive shows remain preserved;
- validation currently decodes a shared source stream per logical target;
- paragraph reflow, durable patches and bounded-memory export remain pending.

## Validation

- replacement tests: 14 passed, 2 ignored manual probes;
- `cargo test pdf_v3`: 75 passed, 0 failed, 10 ignored manual probes;
- Source Han searchable text extraction: passed;
- Poppler visual and pixel-bound review: passed.
