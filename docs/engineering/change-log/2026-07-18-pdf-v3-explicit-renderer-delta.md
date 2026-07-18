# PDF v3 Explicit Renderer Delta

Date: 2026-07-18

## Summary

Replaced complete-object-graph comparison in final PDF v3 export with an
explicit, merge-checked renderer object delta.

## Implementation

Added:

- `PdfObjectDelta` construction, validation, merge and apply behavior;
- immutable document-font registry staging with a font object delta;
- immutable TranslationPatch staging with a page object delta;
- compatibility wrappers that stage before applying to owned documents;
- direct `PdfObjectDelta` input for incremental export;
- conflict and explicit-application unit tests;
- real 30-page accumulation of one font delta and two page deltas.

Removed from the export proof:

- iteration across the complete rendered object graph;
- source-versus-rendered object comparison;
- arbitrary raw object-map input to the incremental writer.

## Current Boundary

- renderer mutation ownership is now explicit and page-addressed;
- final writer input is bounded by actual changed objects;
- the temporary renderer read view is still a complete `lopdf::Document`;
- lazy source-object access and a bounded overlay remain the next Phase 4 step.

## Validation

- PDF v3: 119 passed, 13 ignored manual probes;
- real-paper delta: 10 objects;
- source: 1,590,242 bytes;
- output: 1,617,258 bytes;
- appended bytes: 27,016;
- output pages: 30;
- matching translation Type0 subsets: one;
- both translations re-extracted on their intended pages;
- Poppler page 1: 2,559 changed pixels, 0.1176%, confined to
  `[245, 551) x [1592, 1611)`;
- Poppler page 2: 2,059 changed pixels, 0.0946%, confined to
  `[671, 899) x [1592, 1611)`;
- Poppler page 3: pixel-exact;
- page 1-3 annotations retained at 26, 31 and 7;
- source metadata retained and both translations occurred only on their
  intended pages;
- visual inspection found no clipping, overlap or unrelated movement.
