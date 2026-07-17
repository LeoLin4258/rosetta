# PDF v3 PDFium Identity Replacement Spike

Date: 2026-07-16

## Summary

Extended the PDF v3 Windows/AMD spike from read-only character extraction to
page-object provenance and identity text replacement.

PDFium remains a strong candidate for exact selected-page extraction. It is not
accepted as the PDF v3 object replacement renderer because rewriting a text
object with its own original text changes rendered pixels and, on a real paper
page, changes the re-extracted text.

## Object Provenance

The extraction probe now enumerates PDFium page objects in page content order
and maps each extracted character to a deterministic page-local id:

```text
page-0001-object-000042
```

Repeated extraction of the same source and page produces the same character to
object mapping in the current PDFium build. This is stable enough for the
extraction spike when combined with the source fingerprint and page hash. It is
not a raw PDF indirect object number and must not be treated as one across
different engines or source revisions.

## Identity Method

The probe performs two paths against the same source page:

1. `save-only`: load, render, save without modifying page objects, reload, and
   compare text and pixels;
2. `replace-text`: enumerate every text object, call `set_text()` with the
   object's own original text, regenerate page content, save, reload, and
   compare text and pixels.

Comparison records text hashes/counts and pixel statistics without storing
source or output text in the diagnostic result.

## Results

Environment remains:

- Windows 11 build `26200`;
- AMD Ryzen 7 8745HS, 8 cores / 16 logical processors;
- bundled PDFium;
- Rust test/dev profile.

### Simple LibreOffice page

Save-only round trip:

- text exact: yes;
- changed pixels: `0`.

Same-text replacement of 7 text objects:

- text exact: yes, 603 / 603 characters;
- changed pixels: `29,097`;
- changed pixel ratio: `2.5397%`;
- mean absolute channel difference: `1.1854`;
- maximum channel difference: `253`;
- elapsed: about `177ms`.

### Real paper page

Fixture: page 1 of `2305.13048v2.pdf`.

Save-only round trip:

- text exact: yes, 3,909 / 3,909 characters;
- changed pixels: `0`;
- output size: `1,527,904` bytes;
- elapsed: about `411ms`.

Same-text replacement of 242 text objects:

- text exact: no;
- source/output characters: 3,909 / 3,927;
- first text difference: character index `1332`;
- changed pixels: `178,975`;
- changed pixel ratio: `8.7888%`;
- mean absolute channel difference: `5.8522`;
- maximum channel difference: `255`;
- output size: `1,528,562` bytes;
- elapsed: about `738ms`.

## Decision

The control group proves that PDFium document saving is not the source of the
drift. The drift appears only after `FPDFText_SetText` and content regeneration.
The same-text operation therefore does not preserve the original text encoding,
glyph placement, or extraction semantics reliably enough for Rosetta's visual
fidelity contract.

PDFium remains eligible for:

- page count and random page access;
- fast character extraction;
- character geometry and style inspection;
- preview rasterization;
- source-page validation.

PDFium is rejected as the sole PDF v3 object replacement renderer. The next
engine spike must test MuPDF or a lower-level content-stream patching approach
against the same identity contract.

## Validation

- `cargo fmt -- --check`
- `cargo test pdf_v3`
- explicit Windows real-page identity probe with ignored tests enabled
