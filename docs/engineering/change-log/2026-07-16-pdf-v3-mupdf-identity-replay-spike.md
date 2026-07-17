# PDF v3 MuPDF Identity Replay Spike

Date: 2026-07-16

## Summary

Compared PyMuPDF/MuPDF high-level text replay against the PDF v3 identity
contract on the Windows AMD development platform. The probe uses an independent
Poppler renderer for pixel comparison and emits hashes and measurements only.
All mutated PDFs and page images are held in a temporary directory and removed
before the probe exits.

MuPDF remains useful for inspection and may still be viable through a lower-level
C API. Its high-level redact-and-reinsert path is rejected as the PDF v3
replacement renderer because it cannot reliably reuse every embedded font and
does not preserve visual or text extraction identity.

## Probe Boundary

Added `scripts/probe-pdf-v3-mupdf-identity.py` as an isolated diagnostic. It is
not imported by the app and does not create a production dependency on the old
pdf2zh Python component pack.

The probe performs three operations against one selected page:

1. save the source without page mutation;
2. overlay the extracted text using existing page font resource names;
3. remove only text for preflighted spans, then insert the same text at the
   extracted origin with the original size, color, opacity, and render mode.

Before deletion, each font resource is preflighted through the insertion API.
Spans using fonts that cannot be replayed are left unchanged, matching the v3
source-preservation policy. Rotated, vertical, unmapped, and unsupported render
modes are also left unchanged.

Text comparisons use character counts and SHA-256 hashes. The probe never emits
source text. Pixel comparisons use Poppler 26.05.0 at a fixed target width so
the engine under test does not render its own control result.

## Environment

- Windows 11 build `26200`;
- AMD Ryzen 7 8745HS, 8 cores / 16 logical processors;
- PyMuPDF `1.25.2` backed by MuPDF `1.25.2`;
- Poppler `26.05.0` for independent rasterization.

The PyMuPDF runtime was discovered in the installed beta pdf2zh component pack
and used only to run this spike. A future production engine must have its own
versioned component and license boundary.

## Results

### Simple LibreOffice page

Fixture: `002-trivial-libre-office-writer.pdf`, page 1, width 900.

Save-only:

- text exact: yes, 598 / 598 characters;
- changed pixels: `0`;
- output size: `12,503` bytes, `99.16%` of source.

Same-text replacement of 7 spans with the existing TrueType font resource:

- insert failures: `0`;
- new font xrefs: `0`;
- text exact: no, 598 to 596 characters;
- first text difference: character index `174`;
- changed pixels: `25,185`;
- changed pixel ratio: `2.1982%`;
- mean absolute channel difference: `2.2219`;
- output size: `17,450` bytes, `138.39%` of source.

### Real paper page

Fixture: `2305.13048v2.pdf`, page 1, width 1200.

Save-only:

- text exact: yes, 3,851 / 3,851 characters;
- changed pixels: `0`;
- output size: `1,509,688` bytes, `94.93%` of source.

Font and span coverage:

- 237 extracted text traces;
- 5 candidate page font resources;
- 4 resources replayable through the high-level API;
- 1 embedded Type1/PFA resource rejected with `unhandled font type`;
- 222 replayable spans and 15 preserved spans.

Same-text replacement of the 222 replayable spans:

- insert failures after preflight: `0`;
- new font xrefs: `0`;
- text exact: no, 3,851 to 3,308 characters;
- first text difference: character index `0`;
- changed pixels: `245,235`;
- changed pixel ratio: `12.0355%`;
- mean absolute channel difference: `10.6926`;
- output size: `1,660,084` bytes, `104.39%` of source.

The text order changes because removed source operators are replaced by new
overlay content at the end of the page stream. Reusing the same font resource
name does not preserve the original encoded strings, glyph positioning,
kerning, or extraction order.

Manual inspection of the independently rendered real page confirms visible
word-space loss, changed character advances, horizontal overflow across column
and figure boundaries, and damaged title/author spacing. Untargeted graphics
remain intact, so the failure is localized to reconstructed text rather than
page saving or rasterization.

## Decision

PyMuPDF's high-level text mutation APIs are rejected as the PDF v3 replacement
renderer. They are not a sufficient fidelity boundary for citations, mixed
styles, exact glyph placement, or stable text extraction.

This result does not reject MuPDF's lower-level C API, PDFium extraction, or a
dedicated content-stream patcher. PDFium remains the current extraction,
preview, and validation candidate because it already meets the selected-page
performance and character provenance requirements measured in the earlier
spike.

The next renderer spike will work at PDF content-stream operator level. Its
identity test must preserve the original text operators, matrices, advances,
resource references, graphics order, and untouched streams instead of
reconstructing page text from extracted Unicode spans.

## Validation

- Python bytecode compilation of the diagnostic script;
- simple fixture save, overlay, and replacement matrix;
- real-page save, overlay, and replacement matrix;
- independent Poppler pixel comparisons for every mode;
- manual inspection of the real-page source and replacement rasters;
- `cargo fmt -- --check`;
- `cargo check`;
- `cargo test pdf_v3` (`12` passed, `3` manual probes ignored);
- `cargo test rosetta_jobs` (`78` passed).
