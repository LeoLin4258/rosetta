# PDF v3 Single Text-Show Replacement

Date: 2026-07-17

## Summary

Added the first real unified-font source text replacement for PDF v3. It
rewrites one validated top-level text-show in place, preserves surrounding
graphics and text state, and atomically commits the translated font, page
resources and content stream.

The implementation remains isolated from PDF v2, jobs, UI, model translation,
persistence and paragraph layout.

## Implementation

Added:

- PageGraph schema v4 source font resource, `Tf`, `Tz`, stream and operation
  provenance;
- mapping state tracking across `q`, `Q`, `Tf`, `Tz`, Form entry and text shows;
- source operator, operand hash and font-state validation;
- unique selected-page content-stream ownership validation;
- final-show-in-`BT`/`ET` safety validation;
- unified-font `Tf`/`Tz` insertion and source-state restoration;
- translated CID replacement for `Tj`, `TJ`, `'` and `"`;
- minimum-fit-scale overflow rejection;
- combined font/page/content staging and commit;
- zero-mutation overflow tests;
- automated Latin and manual Source Han Chinese replacement probes.

## Windows AMD Results

Source Han Sans CN replaced the first line of the simple LibreOffice fixture:

- translated text: 8 Chinese characters;
- fit scale: 1.0;
- natural/fitted advance: 80;
- replacement stage: about 3 ms;
- source PDF: 12,609 bytes;
- output PDF: 16,483 bytes;
- PDFium translated text extraction: exact;
- Poppler render: correct;
- following source lines: unchanged positions;
- visual defects: none.

## Current Boundary

- one unique top-level text-show is supported;
- shared streams and Form invocation replacement are not connected;
- a later show in the same text object forces preservation;
- PageGraph-to-text-space fit conversion was subsequently implemented by ADR
  0025 and the PageGraph-derived fit-bounds change;
- paragraph reflow, protected spans, mixed styles and bold selection are not
  connected;
- model translation and durable TranslationPatch remain disconnected.

## Validation

- focused replacement tests: 2 passed, 1 manual probe ignored;
- manual Source Han replacement probe: passed;
- PDFium translated extraction: passed;
- Poppler rendering and visual review: passed;
- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 50 passed, 0 failed, 9 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed.
