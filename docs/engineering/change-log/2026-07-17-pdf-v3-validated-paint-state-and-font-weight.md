# PDF v3 Validated Paint State and Font Weight

Date: 2026-07-17

The final-show boundary recorded here is superseded by the anchored transaction
rules in ADR 0027.

## Summary

Added a fail-closed style and paint-state gate to native single text-show
replacement, plus explicit Regular/Bold translation-font assets and prepared
subsets. Translated text still uses Rosetta's unified font family; it now selects
the Bold face only when PageGraph carries validated bold intent.

The work remains isolated from PDF v2, jobs, UI, model translation, persistence
and export.

## Implementation

- Added typed `Regular` and `Bold` face intent throughout font asset loading,
  caching, subsetting, preparation and staging.
- Required every replacement source object to resolve to one PageGraph style.
- Added a style planner for source weight, explicit bold-name classification,
  fill color, opacity, render mode and italic preservation.
- Replayed `q`/`Q`, `Tf`, `Tz`, `Tr`, DeviceGray, DeviceRGB and DeviceCMYK state
  before the target show.
- Required effective stream fill/render state to match PageGraph before commit.
- Preserved unsupported color-space, arbitrary-color and external graphics-state
  operators instead of approximating them.
- Rejected a prepared font whose face intent differs from the style plan with
  zero document mutation.
- Advanced replacement diagnostics to
  `rosetta-pdf-v3-text-show-replacement/3`.

## Windows AMD Results

Source Han Sans CN Bold replaced one independent show in the LibreOffice
fixture:

- translated text: PDFium exact and searchable;
- embedded face: Source Han Sans CN Bold;
- source/translated fill: black;
- fit scale: 1.0;
- replacement stage: about 4 ms;
- output PDF: 16,101 bytes;
- Poppler review: baseline and following lines retained, with no clipping,
  overlap or missing glyphs.

Fixture inspection also confirmed why font names participate in classification:
Arial Bold may report weight 380 and CMBX12 Bold may report 545.

## Boundary At This Stage

- Only one unique top-level show that is last in its `BT`/`ET` can be replaced.
- Real-paper and Google Docs Bold regions commonly contain later shows in the
  same text object and therefore remain original.
- Mixed styles, italic, clipping/stroking, external graphics-state opacity and
  non-device color spaces remain typed preservation fallbacks.
- The next renderer stage is a transaction-level plan for multiple shows in one
  text object, not a relaxation of single-show validation.
- Model translation and durable TranslationPatch remain disconnected.

## Validation

- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 63 passed, 0 failed, 10 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- manual Source Han Bold replacement probe: passed;
- PDFium translated text/font/color validation: passed;
- Poppler source/output rendering and visual review: passed;
- Poppler source/output pixel difference was confined to the replaced first-line
  rectangle; all later-line pixels were identical.
