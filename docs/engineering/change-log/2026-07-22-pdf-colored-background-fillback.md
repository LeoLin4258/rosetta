# 2026-07-22 PDF Colored Background Fill-Back

## Summary

Hardened pdf2zh fill-back for designed documents with colored panels, page
furniture, and all-text tables. The change is geometry-based and does not add
layout inference, raster background sampling, translation requests, or page
passes.

## Changes

- Removed Rosetta's hard-coded white paragraph erasure rectangles. pdf2zh
  already emits a replacement text content stream; the additional rectangles
  destroyed non-white backgrounds and made light translated text invisible.
- Added an upgrade path that removes the old rectangle helper and call from an
  already-patched installed component as well as omitting them from fresh
  component patching.
- Extended visual table geometry detection to preserve all-text grids when at
  least three baselines contain two recurring next-column start coordinates.
  Column starts remain stable when cell text widths vary or adjacent cells
  overlap visually; existing numeric two-column table behavior is unchanged,
  while ordinary two-column prose remains translatable.
- Split paragraph grouping across large vertical discontinuities. Characters
  more than both 24 PDF points and three source line heights apart no longer
  merge merely because DocLayout assigned them the same class.
- Kept the existing CJK line fitting, source color preservation, centered
  alignment, translation batching, cache, and renderer ownership unchanged.

## Performance

The new checks reuse the existing `LTChar` scan and layout mask. They are
linear in page character count and add no model or render pass. Removing the
rectangle operators slightly reduces translated PDF content streams.

## Validation

- Rosetta pdf2zh patch suite: 33 passed.
- Added positive coverage for all-text three-column grids and large vertical
  discontinuities, including variable-width rows with one overlapping cell.
- Added negative coverage for ordinary two-column prose and normal line gaps.
- Patched and compiled the installed Windows component, then prepared and
  identity-rendered all five pages of
  `19_Ridge_County_Preparedness_Guide.pdf` through `prepareRun`, `collectUnits`,
  and `renderPages` without using the app UI.
- On page 2, the required unit count fell from seven to four: the all-text table,
  orange header, and footer are preserved visual content, while the heading,
  body paragraph, and bottom information panel remain translatable.
- Raster inspection of identity and explicit UTF-8 Chinese renders confirmed
  that page 1 keeps its orange background and pale corner graphic, page 2 keeps
  its table and orange header, and the beige bottom panel and footer fills remain
  visible. Both Chinese pages rendered with zero empty translations and zero
  placeholder mismatches.
- Re-ran command-line identity rendering for Task Budget Displacement pages 5,
  6, 8, and 9 and Omnilingual ASR pages 1-4. The known tables remained visual,
  Task Budget references `[6]` through `[20]` retained entry breaks and hanging
  indents, and all eight page artifacts completed successfully.
- The historical SpaceX duplicate-layer fixture was not present in the current
  fixture directory; duplicate-layer behavior remains covered by the patch
  suite.
