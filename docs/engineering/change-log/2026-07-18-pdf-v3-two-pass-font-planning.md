# PDF v3 Two-Pass Font Planning

Date: 2026-07-18

## Summary

Removed the document-font dependency cycle from the async page processor and
added bounded streaming font planning for final export.

## Implementation

- changed the processor configuration from prepared subsets to immutable
  Regular and optional Bold font assets;
- prepared deterministic page-local subsets only after provider translation;
- discarded temporary font and replacement object deltas after renderer
  decisions, keeping resolved TranslationPatch as the only durable authority;
- added transactional per-weight character plans with the current 65,535 CID
  limit;
- added a PageSet-driven helper that loads one PageGraph and resolved patch at
  a time and collects only fitted translation characters;
- proved that preserved entries do not enter final font plans and that glyph
  advances are invariant between page-sized and document-sized subsets.

## Validation

- focused font-plan and durable-store tests;
- Windows/PDFium async processor tests;
- `cargo test --locked pdf_v3 --lib` (`177 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`87 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The processor and export font character planning now have bounded ownership.
Final multi-page resolved-patch replay with the document-wide prepared fonts,
runtime manifest identity binding, Tauri lifecycle APIs, and complex 500/1,000
page end-to-end validation remain pending.
