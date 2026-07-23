# 2026-07-21 PDF Table And Reference Fill-Back

## Summary

Improved two narrow pdf2zh fill-back cases without changing layout inference,
translation batching, page scheduling, cache behavior, or renderer ownership.

## Changes

- Preserve compact model-summary tables when the visual region contains a
  `Model` header, at least two `Pred`/`Obs` labels, and at least six percentage
  cells. The converter keeps the table at its original PDF coordinates, while
  the engine applies the same signature as a non-translatable fallback.
- Preserve probe-summary tables that combine `Probe` and `Truncation` headers
  with at least twelve numeric cells.
- Preserve benchmark result tables that combine `Target`, `Realized`, and
  `Ref.` headers with at least two recognized model columns and at least eight
  percentage cells.
- Preserve compact numbered-row tables when a leading row-family header is
  followed by at least three rows from that same family, four numeric cells,
  and little sentence punctuation. The structural rule supports labels such
  as `Exp1`/`Exp 1`, `Run1`, and `Model1` without depending on a PDF filename
  or page number. Requiring the matching leading header keeps ordinary prose
  that compares several experiments translatable.
- Detect visual numeric tables from PDF character geometry before applying
  textual signatures: characters are grouped into source baselines and a
  table is preserved when large column gaps align across at least three rows.
  This handles vocabulary-free tables and compact PDF text extraction without
  rerunning DocLayout or changing translation batching.
- Preserve table summary fragments split out of the visual grid when they
  contain repeated parenthesized-label/value rows. Forced source-row breaks
  keep those non-translatable summaries from collapsing into one line.
- Detect consecutive bibliography entries that begin with `[n]` at recorded
  source line boundaries. Entry boundaries become forced render breaks and
  automatically wrapped continuation lines receive a conservative hanging
  indent.
- Keep the rules limited to structured signatures. Numeric prose, inline
  citations, non-consecutive bracketed labels, and ordinary soft line wraps do
  not trigger the new behavior.
- Support both fresh component patching and upgrades of an already-patched
  development component.

## Fixture Verification

Fixture:

```text
C:\Users\Leo\Desktop\pdf-set\07_Task_Budget_Displacement.pdf
```

Command-line `prepareRun`, `collectUnits`, and identity `renderPages` checks on
pages 5 and 8 confirmed:

- the page 5 table is absent from translation units and retains its source
  columns, rows, and rules;
- the page 5 caption remains translatable;
- the page 6 probe-summary table is absent from translation units while its
  caption remains translatable;
- the page 8 reference unit remains non-translatable and contains forced
  breaks before `[7]` through `[20]`;
- the rendered page 8 restores separate reference entries and hanging
  continuation indentation;
- the page 9 benchmark table is absent from translation units while its
  caption remains translatable;
- all four checked pages render successfully with zero empty translations and zero
  placeholder mismatches.

Additional fixture:

```text
C:\Users\Leo\Desktop\pdf-set\15_Omnilingual_ASR_Speech_LLM.pdf
```

Command-line verification on page 4 confirmed that the
`Exp / System / tcpMER` table is absent from translation units, its `Table 3`
caption remains translatable, and identity rendering completes with zero empty
translations and zero placeholder mismatches. At 150 DPI, the table-only crop
is pixel-identical to the source (`0 / 67,900` changed pixels).

Page 3 verification confirmed that both per-language table bodies are absent
from required translation units, their two summary fragments are
`table-like / requiresTranslation=false`, and the `Table 1` / `Table 2`
captions remain translatable. Identity rendering of pages 3 and 4 completed
with zero empty translations and zero placeholder mismatches. The page 3 grid
text is pixel-identical to the source in both table bodies; the split summary
rows retain separate source-language lines.

## Validation

```text
python rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py -q
33 passed
```

## Follow-Up: Diagram Preservation

The Omnilingual ASR fixture exposed a separate visual-region failure on page 1:
sixteen centered workflow-label lines and a footer were grouped into one
translation unit, so the normal source-erasure rectangle covered the workflow
figure. The converter now detects a geometry-only diagram-label cluster when
at least six visual baselines share a horizontal center, include several short
label lines, and span a meaningful vertical range. Those characters remain in
the original PDF content stream and coordinates. The rule does not inspect PDF
names, page numbers, or diagram vocabulary, and runs in the existing character
scan without another layout or render pass.

Command-line identity rendering confirmed the page 1 workflow crop is
pixel-identical to the source in the diagram body, while the full four-page
fixture and the Task Budget pages 5, 6, 8, and 9 all completed with zero empty
translations and zero placeholder mismatches.
