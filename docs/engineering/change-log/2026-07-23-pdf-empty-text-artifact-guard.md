# PDF Empty-Text Artifact Regression Guard

Date: 2026-07-23

## Summary

Fixed a clean-staging regression that reported translated PDF pages with
non-empty translation statistics while emitting artifacts with no text drawing
operators. Added renderer and background-compression checks so the same failure
cannot be committed as a successful translated page again.

## Root Cause

The pdf2zh converter patch inserted the centered single-line alignment helper
inside that helper's own body. The paragraph rendering statements followed its
`return`, so they were unreachable. Incrementally patched Windows packs kept an
older correct converter and masked the defect; clean Windows and macOS staging
both reproduced it.

## Changes

- Place the centered-alignment helper before the paragraph loop on clean packs.
- Detect and repair packs produced by the broken patch while preserving patch
  idempotency and compatibility with older converter fixtures.
- Reject a renderer result when translated characters were recorded but the
  generated content stream has no `Tf` plus `Tj`/`TJ` text operations.
- Reopen each saved single-page artifact and reject/delete it if those text
  drawing operations are absent.
- Refuse to replace a valid page artifact if background font subsetting removes
  its text drawing operations.

These checks do not change translation requests, layout analysis, PDF page
selection, persistent state schemas, or artifact compression settings.

## Validation

- Rosetta pdf2zh patch suite: 33 passed.
- PDFMathTranslate Rosetta engine suite: 10 passed.
- `pnpm typecheck`: passed.
- `cargo check`: passed.
- `cargo test rosetta_jobs`: 132 passed.
- A clean Windows staged component reproduced the pre-fix defect with
  `translatedChars=16`, zero extracted characters, and zero `Tf`/`TJ`
  operations.
- The same clean staging flow after the fix produced 16 extractable characters
  (`Translated text.`) with `Tf=4` and `TJ=4`.
- Running the production background-compression script on that artifact kept
  all 16 characters and the same `Tf=4` and `TJ=4` operations; raster inspection
  confirmed the text remained visible.

Final acceptance remains a clean macOS arm64 staging run followed by a manual
single-page App translation. Existing jobs and previously staged packs are not
valid substitutes for that check.

## Acceptance Follow-Up: Split Figure Panel Labels

Windows App acceptance exposed a separate unit-count mismatch on page 1 of
`2605.14926v2.pdf`. Collection classified three adjacent figure-panel labels
as required translation units because pdf2zh emitted them separately, while
the render replay correctly preserved the same labels as visual content. The
engine therefore reported 12 required units but only 9 translated units, and
Rosetta's page commit guard rejected the result.

The staging patch now groups consecutive units that each begin with an
`(a)`-style panel marker and applies the existing combined panel-label rule to
the group. It does not classify a single parenthesized prose unit as visual
content. On the real page, the required count changed from 12 to 9, rendering
completed with 9 translated units, all three panel labels remained visible,
and the artifact retained 147 `Tf` and 147 `TJ` operations.

The completeness guard remains unchanged: genuine missing translated units
must still fail the page instead of being silently committed.

## Acceptance Follow-Up: Split Diagram Labels

The full 18-page Windows acceptance run exposed the same collection/render
boundary in two additional layouts. Page 13 collected a standalone model name
inside a visual comparison grid, and page 16 collected 12 short labels from a
deployment flow diagram. The renderer preserved those labels in the original
visual layer, so the pages were correctly rejected as `3/2` and `19/7` unit
count mismatches.

The staging patch now recognizes a diagram-label cluster only when at least
three adjacent short units contain a strong diagram-label anchor. Expansion
stops at captions, references, formulas, tables, page numbers, long titles, or
sentence-like text. This preserves the visual labels without broadly treating
short headings as nontranslatable.

On the real document, pages 13 and 16 now complete as `2/2` and `7/7`. A full
18-page identity-render check completed every page with equal required and
translated unit counts. Raster inspection confirmed that the comparison-grid
model names and deployment-flow labels remained visible while titles, captions,
and body text stayed in the translation path.
