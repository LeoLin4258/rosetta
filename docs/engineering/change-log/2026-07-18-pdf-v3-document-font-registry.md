# PDF v3 Document Font Registry

Date: 2026-07-18

## Summary

Added the document-level translation-font resource boundary required by final
PDF export. Consecutive page-patch renders can now reuse one deterministic
Type0 subset instead of embedding a font once per page.

## Implementation

Added:

- atomic `DocumentTranslationFontRegistry` staging;
- deterministic face ordering and duplicate-weight rejection;
- prepared font asset/fingerprint/subset identity validation;
- live Type0 object and `/BaseFont` validation;
- registry-aware text-show batch replacement;
- registry-aware `TranslationPatch` rendering;
- zero per-page font staging when the registry is active;
- explicit separation from self-contained single-page cache rendering;
- real two-page renderer reuse and searchable-text coverage.

## Current Boundary

- complete-document glyph collection must precede registry construction;
- the registry is not persistent authority and is rebuilt for each export;
- final atomic output-file commit and cancellation are not connected;
- the current working `lopdf::Document` still loads the complete source object
  graph and is not the final bounded-memory export architecture;
- patch compression remains pending.

## Visual Verification

The Windows AMD probe translated pages 1 and 2 of the 30-page real-paper
fixture through one shared 27,568-byte Arial subset.

- source: 1,590,242 bytes;
- complete output: 1,521,952 bytes;
- matching Type0 subsets in output: one;
- page 1 changed 2,559 pixels, confined to the target footer row;
- page 2 changed 2,059 pixels, confined to the target footer row;
- page 3 was Poppler pixel-exact;
- annotations, 30-page count and source metadata were retained;
- independent text extraction found both translations on the correct pages;
- visual inspection found no clipping, overlap or unrelated movement.

## Validation

- document font registry tests: 2 passed;
- targeted multi-page visual probe: passed;
- complete PDF v3 suite: 115 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `pnpm typecheck`: passed;
- `git diff --check`: passed.
