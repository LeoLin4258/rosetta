# PDF visual table unit regression fix

Date: 2026-07-08

## Context

The PDF converter patch that allowed prose-like text in visual regions fixed
some overlap cases, but it also widened extraction too far for academic tables
and formulas. Dense table text was collected as normal Rosetta translation
units, sent to the local model, and then redrawn as paragraph text during PDF
rendering. On the SCRWKV PDF this caused pages 7 and 8 to translate tables that
should have stayed in their original layout, increased unit counts, slowed the
run, and contributed to page failures.

Page 1 of the same PDF also exposed a render-count mismatch: figure panel
labels were collected as a required unit, but the replay render did not request
that unit, so the page returned fewer rendered translation units than collected
source units.

The full 18-page run then exposed the same mismatch on diagram and visual
legend labels. Pages 3, 4, 6, 13, 16, and 18 had source-only labels collected
from figures or visual comparisons even though render replay preserved those
regions through the original visual path.

Page 17 exposed the next quality boundary: a dense inference-speed table and
an algorithm/pseudocode box were valid visual content but were promoted into
translation units. The table could lose text or be redrawn as a paragraph, and
the algorithm box could be translated into unreadable text. These structured
boxes are now treated as "preserve original" content rather than translated
prose.

## Changes

- Added a page-level visual text gate to the pdf2zh converter patch. Visual
  regions that look like dense tables or formulas now remain on the original
  visual preservation path instead of being promoted into prose translation
  units.
- Kept the visual prose relaxation for non-table visual text so the earlier
  overlap fix is not fully reverted.
- Added conservative engine-side filtering for page numbers, formula-like
  fragments, table-like fallbacks, figure panel label units, and diagram or
  visual legend labels.
- Added an algorithm/code-like visual gate so pseudocode boxes such as
  `Algorithm 1` stay in the original PDF layout instead of being translated and
  redrawn as prose.
- Made non-required duplicate text layers render as blank while other
  non-required fallback units can pass through source text if they still reach
  replay.
- Hardened render unit matching so replay order drift can match units by
  page-local source text before failing.
- Preserved page-specific commit rejection errors during chunk cleanup so a
  later chunk-level failure does not overwrite every failed page with the first
  page's message.

## Validation Notes

- Patch tests cover fresh patching and upgrades from already-patched installed
  packs.
- Windows installed pack smoke on `2605.14926v2.pdf`:
  - All 18 pages pass identity render smoke with
    `sourceUnitCount == translatedUnitCount` and
    `placeholderMismatchCount=0`.
  - Page 1 renders `sourceUnitCount=8`, `translatedUnitCount=8`, and
    `placeholderMismatchCount=0`.
  - Page 3 renders `sourceUnitCount=10`, `translatedUnitCount=10`, with
    network diagram labels excluded from required translation units.
  - Page 4 renders `sourceUnitCount=23`, `translatedUnitCount=23`, with
    GBST schematic labels excluded from required translation units.
  - Page 6 renders `sourceUnitCount=8`, `translatedUnitCount=8`, with visual
    comparison legend labels excluded from required translation units.
  - Page 7 renders `sourceUnitCount=12`, `translatedUnitCount=12`, with the
    top-left complexity table preserved in original layout.
  - Page 8 renders `sourceUnitCount=11`, `translatedUnitCount=11`, with the
    ablation tables preserved in original layout.
  - Page 17 renders `sourceUnitCount=5`, `translatedUnitCount=5`, with the
    inference-speed table and `Algorithm 1` pseudocode box preserved in
    original layout.
