# PDF v3 Recursive Form Identity Renderer

Date: 2026-07-17

## Summary

Extended the isolated PDF v3 content-stream identity renderer from top-level
page streams to recursive Form XObject streams.

The implementation still performs identity byte rewrites only. It does not
encode translated Unicode or connect to PDF v2, jobs, UI, persistence, render
cache or export.

## Implementation

Added:

- a read-only recursive Form discovery pass;
- deterministic unique-stream rewrite order;
- invocation path collection for shared Forms;
- one identity rewrite per unique stream;
- Form invocation, unique-stream and shared-stream metrics;
- Form-owned resource lookup with parent fallback;
- direct Form, cycle and 32-level depth fallback reporting;
- recursive real-page identity tests.

Content-stream diagnostics continue to serialize hashes, byte counts and
provenance without source text or encoded byte payloads.

## Windows AMD Results

Page 1 of `2305.13048v2.pdf`:

- content streams: 7;
- Form invocations: 27;
- unique Form streams: 5;
- shared Form streams: 4;
- operations: 1,360;
- text-show operators: 258;
- text operands: 800;
- rewritten operands: 800;
- malformed text shows: 0;
- PDFium text: exact, 3,909 / 3,909;
- PDFium changed pixels: 0;
- recursive parse/rewrite: about 74 ms;
- total probe: about 725 ms;
- output: 1,505,764 bytes, 94.69% of source.

Independent Poppler rendered the source and identity output at 144 DPI. The two
PNG files had identical SHA-256 hashes. Visual inspection showed no clipping,
overlap, font, chart or layout changes.

## Remaining Boundaries

- shared Form invocation-local patches still require copy-on-write;
- translated Unicode encoding and document-level font embedding are not
  implemented;
- Form sharing across unselected pages is not fully indexed;
- `lopdf` still loads and saves the complete document;
- streaming long-document export is not implemented.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 35 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- recursive Form identity test: passed;
- fixture corpus text/pixel identity matrix: passed;
- PDFium real-page identity probe: passed;
- Poppler source/output PNG SHA-256: exact match;
- rendered page visual review: passed.
