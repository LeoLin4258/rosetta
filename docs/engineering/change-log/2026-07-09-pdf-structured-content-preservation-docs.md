# PDF Structured Content Preservation Docs

Date: 2026-07-09

## Context

The July 2026 PDF optimization pass fixed several related failures across the
SCRWKV 18-page paper and the QianFSD 10-page paper:

- dense tables were translated and redrawn as paragraphs;
- formulas and algorithm boxes were translated into unreadable text;
- colored table highlights were erased by source-text masks;
- duplicate text layers caused overlap or placeholder mismatch failures;
- render replay order drift produced `ValueError` on macOS;
- a failed first PDF window made later pages look unprocessed.

The individual fixes were already recorded in targeted change-log entries, but
future PDF agents also need the higher-level product rule: structured visual
content that cannot be safely reflowed should remain in the original PDF layout
instead of being forced through paragraph translation.

## Documentation Changes

- Updated `docs/engineering/pdf-pipeline.md` with a structured content
  preservation section.
- Documented the two-layer preservation boundary:
  - converter-side visual text gating;
  - engine-side non-translatable unit classification.
- Documented why non-required units remain in the render stream and why only
  duplicate text layers are blanked.
- Added guidance for conservative table/formula/algorithm markers and nearby
  prose counterexamples.
- Added a checklist for future layout regressions:
  `prepareRun`, `collectUnits`, identity `renderPages`, full-window identity
  render, then UI verification.
- Added current dogfood expectations for the SCRWKV 18-page PDF and QianFSD
  10-page PDF before publishing a new PDF component pack.

## Validation

This was a documentation-only change. The recorded guidance is based on the
validated Windows and macOS smoke results from the preceding PDF patch work:

- SCRWKV 18-page identity render: 18/18 pages translated, no bad pages.
- SCRWKV page 4 no longer fails render replay with `ValueError`.
- QianFSD page 4 formula blocks remain `kind=formula`,
  `requiresTranslation=false`.
- QianFSD page 6 dataset split table is not collected as a required body unit.
