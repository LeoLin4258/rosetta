# PDF v3 Contract and PDFium Extraction Spike

Date: 2026-07-16

## Summary

Started the first implementation slice of the PDF v3 native rewrite without
connecting it to the existing PDF v2 worker, UI, translation scheduler, page
state, or export path.

The slice establishes the versioned page-addressing and extraction contracts,
then verifies that the already-bundled PDFium library can extract exact selected
pages with character-level geometry and style information on the Windows AMD
development platform.

## Added Contracts

- `PageSet` provides strict 1-based page parsing, range validation,
  deduplication, sorting, containment, all-pages construction, and canonical
  serialization.
- `PageGraph` stores selected-page dimensions, rotation, text atoms, styles,
  groups, protected spans, warnings, and a conservative source-page hash.
- `PageAtom` records Unicode text, character order, tight/loose bounds, origin,
  text matrix, angle, generated/hyphen state, translation eligibility, and an
  optional source object id.
- `PageStyle` records font name, unscaled/scaled size, weight, italic/serif
  flags, fill/stroke color, opacity, and render mode.
- `TranslationPatch` and `PageResult` establish the patch-first and
  translated/preserved/failed result boundaries before rendering work begins.

## PDFium Probe

The probe opens the source once, hashes it with a streaming SHA-256 reader, and
loads only the pages requested by `PageSet`. It does not run ONNX layout,
pdfminer, translation, rendering, PNG generation, or PDF artifact generation.

Confirmed PDFium character capabilities:

- Unicode value;
- generated-character and hyphen flags;
- tight and loose bounds;
- character origin and transform matrix;
- character rotation;
- font name, size, scaled size, weight, italic and serif flags;
- fill and stroke RGBA color;
- text render mode.

The current public high-level PDFium binding does not expose a stable source PDF
object number for each character. The probe therefore leaves
`sourceObjectId=null` and emits the page warning
`pdfium-stable-object-id-unavailable`. Stable object provenance remains a hard
requirement for the identity-render engine comparison.

## Windows AMD Measurement

Environment:

- Windows 11 Home, version `10.0.26200`, build `26200`;
- AMD Ryzen 7 8745HS with Radeon 780M Graphics;
- 8 CPU cores / 16 logical processors;
- Rust test/dev profile;
- fixture `2305.13048v2.pdf`, 30 pages, about 1.59 MB.

Sparse random access for pages `1,5,10`:

- total: `276ms`;
- page 1: `71ms`, 3,911 characters, 20 page-local styles;
- page 5: `73ms`, 4,579 characters, 16 page-local styles;
- page 10: `91ms`, 5,027 characters, 6 page-local styles.

First ten pages:

- total: `730ms`;
- characters: `39,783`;
- sum of page-local style counts: `165`.

These numbers are not yet comparable to the current 13.77-second PDF prepare
measurement because v3 does not yet perform reading-order grouping, table/formula
classification, protected-span construction, layout inference, identity
rendering, or artifact validation. They do confirm that selected-page native
character extraction itself is not the dominant cost and can avoid processing
unselected pages.

## Validation

- `cargo fmt -- --check`
- `cargo test pdf_v3`
- manual sparse-page PDFium probe with ignored tests enabled
- manual ten-page PDFium probe with ignored tests enabled

Automated result at this slice: 9 passed, 1 manual probe ignored during the
normal suite. Both manual Windows probes pass when explicitly enabled.

## Next Step

Build the identity-render spike and compare whether PDFium can provide a stable
enough object replacement boundary. If stable object provenance or
object-preserving replacement is insufficient, run the same fixture and
contract against MuPDF before selecting the production native engine.
