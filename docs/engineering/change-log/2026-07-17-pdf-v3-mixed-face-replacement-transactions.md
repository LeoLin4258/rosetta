# PDF v3 Mixed-Face Replacement Transactions

Date: 2026-07-17

## Summary

Extended anchored text-show transactions from one translation face to an
atomic Regular/Bold face set. This removes the single-face restriction without
connecting PDF v3 to PDF v2, jobs, UI, model translation, persistence or
export.

## Implementation

- Selects each translation face from the reconciled PageGraph style instead of
  accepting a caller-selected face per request.
- Rejects missing or duplicate prepared face weights before document mutation.
- Reserves non-overlapping object IDs for multiple six-object font subsets.
- Materializes one page resource dictionary containing every used face.
- Stages only the Regular/Bold faces actually required by the transaction.
- Commits both subsets, the rewritten stream, page resources and `max_id`
  together after all source identity, style, glyph and fit checks pass.
- Advances per-show diagnostics to `/5` and transaction diagnostics to `/2`,
  with an ordered `translationFontWeights` field.

## Windows AMD Results

The automated real-paper test uses one anchored Regular show and one anchored
Bold show:

- replacement count: 2;
- staged font objects: 12 with distinct page resource references;
- `max_id` increase: exactly 12;
- PDFium searchable text and distinct Regular/Bold font identity: passed;
- missing Bold face rollback: all document objects and `max_id` unchanged.

The Source Han Sans CN production-font probe reported:

- fit scales: 1.0 and 1.0;
- transaction stage: about 13 ms;
- source PDF: 1,590,242 bytes;
- output PDF: 1,511,382 bytes, 78,860 bytes smaller;
- Poppler page-1 changed pixels: 1,373 / 2,005,644, confined to both targets;
- Poppler page 2: pixel-exact.

## Current Boundary

- Regular and Bold may share one anchored top-level transaction.
- Italic, one-show mixed styles, Form targets, shared streams and unanchored
  consecutive shows remain typed preservation fallbacks.
- Font subsets are reusable within the transaction; cross-page document-wide
  resource reuse remains a Phase 4 responsibility.
- Paragraph reflow, protected spans, model translation and durable
  TranslationPatch remain disconnected.

## Validation

- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 66 passed, 0 failed, 10 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- focused replacement tests: 8 passed, 0 failed, 2 ignored;
- Source Han mixed-face production-font probe: passed;
- PDFium translated text/font validation: passed;
- Poppler selected-page visual confinement and unselected-page pixel identity:
  passed.
