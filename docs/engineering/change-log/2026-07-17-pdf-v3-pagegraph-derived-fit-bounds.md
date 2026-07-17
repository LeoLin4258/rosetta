# PDF v3 PageGraph-Derived Fit Bounds

Date: 2026-07-17

## Summary

Removed the unverified caller-provided width from PDF v3 single text-show
replacement. The renderer now derives the available text advance directly from
reconciled PageGraph geometry and transforms before mutating the PDF.

The change remains isolated from PDF v2, jobs, UI, model translation,
persistence and paragraph layout.

## Implementation

Added `pdf_v3/layout.rs` with:

- typed text-show geometry identity;
- PageGraph schema/page/provenance validation;
- one-source-object resolution;
- synthetic whitespace inclusion;
- finite bounds, origin and matrix validation;
- page-axis baseline projection;
- page-space to text-space scale conversion;
- typed arbitrary-angle and stale-geometry fallback;
- horizontal, vertical, reverse-direction and scale tests.

Updated single text-show replacement to:

- require a reconciled PageGraph;
- remove `max_advance` from the request;
- derive fit bounds inside the renderer;
- report derived advance, matrix scale and atom count;
- keep stale PageGraph and overflow failures zero-mutation;
- advance its diagnostic result schema to
  `rosetta-pdf-v3-text-show-replacement/2`.

## Windows AMD Results

Source Han Sans CN replaced the first line of the simple LibreOffice fixture:

- derived page advance: 453.68;
- baseline matrix scale: 1.0;
- derived text-space maximum advance: 453.68;
- translated natural/fitted advance: 80.0;
- fit scale: 1.0;
- replacement stage: about 4 ms;
- output PDF: 16,483 bytes;
- PDFium translated text extraction: exact;
- Poppler source/output comparison: original baseline and later lines retained;
- clipping, overlap, missing glyphs and background defects: none.

## Current Boundary

- one unique top-level text-show is supported;
- source-object geometry is the fit region; paragraph/column spare width is not
  consumed;
- page-axis-aligned baselines support scale and orthogonal rotation;
- arbitrary-angle text is preserved until exact glyph quads or source advances
  are available;
- shared streams, Form replacement, multi-show paragraphs, color validation,
  bold selection and protected spans remain disconnected;
- model translation and durable TranslationPatch remain disconnected.

## Validation

- `cargo test pdf_v3`: 56 passed, 0 failed, 9 ignored;
- manual Source Han replacement probe: passed;
- PDFium translated extraction: passed;
- Poppler source/output rendering and visual review: passed;
- stale PageGraph and overflow zero-mutation tests: passed.
