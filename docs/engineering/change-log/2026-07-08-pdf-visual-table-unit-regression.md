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

Page 12 exposed a related formula-and-table boundary: a compact visual region
containing Dice/BCE formulas and an ODS/OIS/F1/mIoU sensitivity table used
enough punctuation to evade the dense numeric visual gate. The region was then
partly redrawn instead of preserved, producing missing table text and broken
formula layout. The formula introduction paragraph also showed that the
diagram-label fallback was too broad for normal prose with several formula
placeholders.

Pages 8 and 13 exposed one more structured-table boundary. Their compact table
text arrived from pdfminer without word separators, for example
`LayerNumODSOISPRF1mIoU...`, so the previous metric detector missed it. Page 13
then collected Table 7 as a non-required `table-like` unit and could render it
as one flattened paragraph. Even after the table text was preserved, the render
mask still erased original green and blue table highlights because formula-only
or visual-only paragraphs were receiving the same white backing rectangle as
translated prose.

A later QianFSD PDF exposed the same issue for dataset-statistics tables. Its
right-bottom `Dataset / Category / Train / Val / Test` table was inside a
visual class that also contained formulas, but the compact text did not include
the metric-table markers used by the SCRWKV regressions. The table was promoted
to a translatable body unit and redrawn as prose even though the original table
layout should be preserved.

The same QianFSD PDF also exposed an engine-side formula-unit gap on page 4.
The right-column saliency window partition equations `(6)-(8)` contained enough
operator words (`Partition`, `TopK`, `EM`) to evade the short-form formula
filter, so they were translated as a body unit while the following `(9)-(11)`
formula block was already preserved.

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
- Added a math/table signal gate for compact visual regions with dense numeric
  content and Dice/BCE/mIoU/ODS/OIS-style markers, preserving page 12's formulas
  and sensitivity table in their original layout.
- Added compact no-space table markers such as `LayerNumODSOIS`, `HeadODSOIS`,
  `AMCMGBSTDSCD`, `F1mIoU`, and `ModelSize` so ablation and layer-number tables
  remain on the original-layout path.
- Added dataset-statistics table markers such as `DatasetCategoryTrainValTest`,
  `FarmInsects`, `IP102`, `QianFSD`, and `AgriInsect` so dataset split tables
  remain on the original-layout path instead of being translated as body prose.
- Added a formula-operator fallback for compact equation blocks with many
  placeholders and operators such as `Partition`, `TopK`, `Gumbel`, `Flatten`,
  and `EM`, preserving QianFSD page 4's `(6)-(8)` equation block.
- Narrowed the diagram-label placeholder fallback so normal prose that contains
  several formula placeholders is still translated.
- Limited white source-text masking to paragraphs that are actually translated.
  Formula-only and visual-only table redraws no longer erase the original green
  or blue table highlight rectangles underneath them.
- Made non-required render requests pass through their source text unless they
  are duplicate text layers, so app-provided empty passthrough translations do
  not blank preserved structured units.
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
    ablation tables preserved in original layout and green/blue highlights
    retained.
  - Page 12 renders `sourceUnitCount=11`, `translatedUnitCount=11`, with the
    Dice/BCE formulas and sensitivity table preserved in original layout, while
    the formula introduction remains a translatable body unit.
  - Page 13 renders `sourceUnitCount=2`, `translatedUnitCount=2`, with the
    visual comparison grid and layer-number table preserved in original layout,
    including green/blue table highlights. Table 7 is no longer collected as a
    `table-like` render unit.
  - Page 17 renders `sourceUnitCount=5`, `translatedUnitCount=5`, with the
    inference-speed table and `Algorithm 1` pseudocode box preserved in
    original layout.
- Windows installed pack smoke on the 10-page QianFSD PDF:
  - Page 4 marks the right-column `Partition` / `TopK` equation block as
    `kind=formula`, reducing required translation units from 8 to 7; render
    smoke passes with `sourceUnitCount=7`, `translatedUnitCount=7`, and
    `placeholderMismatchCount=0`.
  - Page 4 visual PNG check preserves both equation blocks `(6)-(8)` and
    `(9)-(11)` in the original formula layout.
  - Page 6 no longer collects the right-bottom `Dataset / Category / Train /
    Val / Test` table as a required body unit; the page now renders
    `sourceUnitCount=13`, `translatedUnitCount=13`, and
    `placeholderMismatchCount=0`.
  - Page 6 visual PNG check preserves Table 1 in the original English table
    layout instead of translating/redrawing it as paragraph text.
  - Page 7 smoke keeps the quantitative comparison table out of required
    translation units; only the table caption and normal prose are collected.
