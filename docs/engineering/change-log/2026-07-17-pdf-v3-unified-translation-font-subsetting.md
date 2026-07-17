# PDF v3 Unified Translation Font Subsetting

Date: 2026-07-17

## Summary

Added the first PDF v3 translated-font layer. It selects one controlled font
family, validates embedding rights and coverage, creates a deterministic
document-wide subset, writes a Type0/CIDFont with ToUnicode and reuses the same
font object across pages.

The font layer remains isolated from PDF v2, jobs, UI, translation, persistence
and source-region replacement.

## Implementation

Added `pdf_v3/font.rs` with:

- target-language font family selection;
- Source Han Sans CN Regular/Bold policy for Simplified Chinese;
- GoNotoKurrent candidate policy for other target languages;
- immutable asset loading and process-owned cache support;
- OS/2 embedding and subsetting permission validation;
- TrueType outline and complete glyph coverage validation;
- deterministic Unicode-sorted glyph planning;
- PDF-specific font subsetting through `subsetter`;
- source-to-subset GID remapping and independent CID allocation;
- CID-to-GID map, widths, descriptor, descendant font and ToUnicode generation;
- six-object atomic staging before document commit;
- page resource attachment that reuses one Type0 object;
- encoded translated-text insertion probe;
- typed missing-glyph and unsupported-outline failures.

Added `subsetter 0.2.6` and `ttf-parser 0.25.1`. Both are MIT OR Apache-2.0.

## Windows AMD Results

Source Han Sans CN Regular:

- source asset: 10,397,552 bytes;
- 30-glyph subset: 7,064 bytes;
- source fixture PDF: 12,609 bytes;
- output PDF: 19,290 bytes;
- PDFium Chinese/Latin extraction: exact;
- Poppler CJK render: correct;
- visual review: no missing glyphs or layout defects.

1000-CJK-character stress subset:

- glyphs including `.notdef`: 1,001;
- subset: 255,624 bytes;
- subset time: about 14 ms;
- full-font ratio: about 2.46%.

One cold font load and validation measured about 304 ms. The asset cache shares
one immutable font byte buffer across subsequent plans and documents.

## Current Boundary

- Simplified Chinese and direct-cmap Latin are proven;
- complex-script shaping is not implemented;
- CFF/CFF2 embedding is not implemented;
- font subset collection is not connected to durable TranslationPatch data;
- actual source-text erasure, translated placement and fitting remain pending;
- production v3 component packaging must expose font hashes and licenses.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- automated font tests: 2 passed, 1 manual probe ignored;
- manual Source Han CJK subset/render probe: passed;
- `cargo test pdf_v3`: 48 passed, 0 failed, 8 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- PDFium CJK extraction: exact;
- Poppler rendering and visual review: passed.
