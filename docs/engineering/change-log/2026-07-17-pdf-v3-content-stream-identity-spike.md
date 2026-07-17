# PDF v3 Content-Stream Identity Spike

Date: 2026-07-17

## Summary

Added the first Rust-native content-stream renderer spike for PDF v3. It proves
that Rosetta can address and rewrite the original encoded PDF text operands
without reconstructing page text from Unicode spans.

The implementation remains isolated under `pdf_v3` and is not connected to the
old PDF worker, jobs, UI, translation scheduler, persistence, preview cache, or
export path.

## Implementation

Added `pdf_v3/content_stream.rs` with:

- `save-only` and `rewrite-text-operands` control modes;
- exact selected-page content stream discovery;
- parsing of `Tj`, `TJ`, `'`, and `"` text-show operators;
- discovery of every string inside `TJ` arrays while preserving numeric
  advances;
- stable provenance from page, content-stream object/generation, operation,
  operand, and array item;
- encoded byte counts and SHA-256 hashes without diagnostic text payloads;
- font resource and text-object context tracking;
- malformed operator and cross-page shared-stream reporting;
- same-byte operand replacement, stream encoding, compression, document save,
  text re-extraction, and pixel comparison.

The diagnostic result contains no source or output text. Manual PDF and PNG
outputs are opt-in test artifacts and were removed after visual inspection.

## Windows AMD Results

Environment:

- Windows 11 build `26200`;
- AMD Ryzen 7 8745HS, 8 cores / 16 logical processors;
- Rust test/dev profile;
- PDFium for text and pixel validation;
- Poppler 26.05.0 for independent raster validation.

### Simple LibreOffice page

The simple page exposes stable operand IDs across repeated inspection. Every
text operand is rewritten, extracted text remains exact, and the changed pixel
count is zero.

The same exact text and zero-pixel identity checks pass for the first pages of
`simple-one-page.pdf`, `pdflatex-image.pdf`, `multicolumn.pdf`,
`google-doc-document.pdf`, and `GeoTopo.pdf`.

### Real paper page

Fixture: page 1 of `2305.13048v2.pdf`.

Content boundary:

- 2 content streams;
- 752 total operations;
- 242 text-show operations;
- 779 string operands;
- 0 malformed text-show operations;
- 0 shared page content streams.

Save-only control:

- text exact: yes, 3,909 / 3,909 characters;
- changed pixels: `0`;
- output size: `1,506,056` bytes, `94.7061%` of source;
- content inspection: about `17ms`;
- complete save and validation: about `442ms`.

Same-byte operator rewrite:

- rewritten operands: 779 / 779;
- text exact: yes, 3,909 / 3,909 characters;
- first text difference: none;
- changed pixels in PDFium: `0`;
- mean and maximum channel difference: `0`;
- output size: `1,506,085` bytes, `94.7079%` of source;
- content parse and rewrite: about `21ms`;
- complete save and PDFium validation: about `462ms`.

Independent Poppler rendering produced source and output PNGs with identical
SHA-256 hashes and zero pixel differences. Manual inspection found no changes
in titles, authors, superscripts, citation colors, body spacing, columns,
figures, captions, footnotes, or the rotated page stamp.

## Decision

The content-stream operand boundary passes the first identity-render gate and
is accepted as the renderer direction. PDFium remains the extraction, preview,
and validation engine. High-level PDFium and PyMuPDF text replacement remain
rejected.

The next slice must map PDFium PageGraph atoms to encoded content operands and
classify font encodings. It must not insert translated Unicode yet. Pages with
unmapped operands, nested forms, shared streams, unsupported fonts, or
ambiguous mappings must preserve source content.

The current `lopdf` implementation loads the entire document and therefore is
not accepted as the final long-PDF writer. Bounded-memory and incremental export
remain explicit production gates.

## Validation

- `cargo fmt`;
- focused content-stream tests: 4 passed, 1 manual probe ignored;
- explicit real-page manual probe;
- PDFium text and pixel comparison;
- independent Poppler rendering and pixel comparison;
- manual source/output raster inspection;
- source/output page-count identity checks;
- `cargo fmt -- --check`;
- `cargo check`;
- `cargo test pdf_v3` (`16` passed, `4` manual probes ignored);
- `cargo test rosetta_jobs` (`78` passed).

`cargo clippy --lib -- -D warnings` was also attempted, but the repository has
25 existing lint failures outside `pdf_v3`. None of the reported failures point
to the new content-stream module. Unrelated lint cleanup is not part of this
slice.
