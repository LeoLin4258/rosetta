# 2026-07-06 PDF Status Translated Character Count

## Summary

Restored the live translated-character count in the PDF translation status bar after the PDF engine contract v2 refactor.

## Changes

- PDF page progress events now carry cumulative translated characters from successfully committed `PageResult.translatedChars` values.
- Initial PDF run progress initializes the character count to `0` when a run has pages.
- Final render progress no longer derives status-bar character count from provider output metrics, avoiding a mismatch between model output and pages that actually committed.

## Rationale

The PDF v2 pipeline treats `PageResult` as the business contract for page commit. The status bar should therefore count translated characters only after a page artifact is accepted, rather than using RWKV/provider metrics that may include pages later rejected by render or commit validation.

## Validation

- `cargo fmt -- --check`
- `cargo test completed_page_progress`
- `cargo test pdf2zh_invoke`
- `cargo test unit_translation`
- `cargo test managed_pdf2zh`
- `cargo test rosetta_jobs`
- `cargo check`
- `pnpm typecheck`
