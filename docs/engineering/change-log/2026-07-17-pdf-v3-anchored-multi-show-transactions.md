# PDF v3 Anchored Multi-Show Transactions

Date: 2026-07-17

## Summary

Replaced the absolute final-show restriction with a validated text-position
dependency gate and added an atomic multi-show transaction for one `BT`/`ET`
text object.

The work remains isolated from PDF v2, jobs, UI, model translation, persistence
and export.

## Implementation

- Recognizes finite, correctly shaped `Tm`, `Td`, `TD`, `T*` and quote operators
  as anchors that make the next show independent of previous glyph advance.
- Continues to reject consecutive unanchored `Tj`/`TJ` operations.
- Requires one page, one unique top-level content stream, one text object,
  distinct operation indices and one prepared translation face per transaction.
- Plans every hash, source state, PageGraph geometry, style, paint state, glyph
  coverage and fit against the unchanged source stream.
- Applies planned replacements in descending source operation order.
- Commits one rewritten stream, one page resource dictionary and one reusable
  font subset only after every request validates.
- Advances per-show diagnostics to `/4` and adds transaction diagnostics `/1`.

## Windows AMD Results

The automated real-paper test replaces two anchored Bold shows with one Arial
Bold transaction and proves a stale second hash leaves all PDF objects and
`max_id` unchanged.

The Source Han Sans CN Bold visual probe replaced two author-line shows:

- replacement count: 2;
- fit scales: 1.0 and 1.0;
- transaction stage: about 15 ms;
- source PDF: 1,590,242 bytes;
- output PDF: 1,508,982 bytes;
- PDFium searchable text, Bold face and color: passed;
- Poppler page-1 changes: confined to the two target glyph regions;
- Poppler page 2: pixel-exact.

## Current Boundary

- Only unique top-level page streams are connected to translation replacement.
- A transaction cannot cross `BT/ET`, stream or page. The single-face boundary
  in this stage is superseded by ADR 0028.
- Unanchored consecutive shows, unsupported faces, Form targets and shared
  streams remain typed preservation fallbacks.
- Per-show fitting remains local; paragraph reflow is not implemented.
- Model translation and durable TranslationPatch remain disconnected.

## Validation

- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 65 passed, 0 failed, 10 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- focused replacement tests: 7 passed, 0 failed, 2 ignored;
- manual Source Han multi-show probe: passed;
- PDFium translated text/font/color validation: passed;
- Poppler selected-page and unselected-page pixel validation: passed;
