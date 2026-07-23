# 2026-07-22 PDF Table-of-Contents Fill-Back

## Summary

Improved translated PDF table-of-contents fill-back without adding document-
specific keywords, page rules, layout inference, or additional page passes.

## Changes

- Detect dotted TOC entries from repeated leader geometry and a trailing
  one-to-four-digit page number on each physical source line.
- Detect isolated TOC section headings from a large extracted whitespace gap
  before a trailing page number, with a stricter threshold for non-bold text.
- Replace each leader/page-number suffix with a unique internal placeholder
  before translation, keeping page numbers out of model reordering.
- Render translated labels one entry per source line, rebuild dot leaders, and
  right-align the preserved page numbers at the source paragraph edge.
- Exclude internal layout placeholders from source/translated character
  accounting while retaining placeholder-presence validation.
- Upgrade already-patched development components that contain the earlier
  reference-only structural-break implementation.

## Performance

The detector scans each extracted paragraph and its existing physical line
break metadata once. It does not change DocLayout inference, translation
batching, prepared-run caching, page rendering passes, or artifact storage.

## Validation

- Rosetta pdf2zh patch suite: 33 passed.
- Added positive coverage for multi-line dotted entries and isolated bold TOC
  headings, plus negative coverage for prose ellipses, decimal-heavy text, and
  ordinary sentences ending in a number.
- Patched and compiled the installed Windows component.
- Prepared, collected, and identity/Chinese-rendered pages 3-4 of
  `Refactoring-ui.pdf` through the engine API without using the app UI.
- Both translated pages completed with zero empty translations and zero
  placeholder mismatches. Poppler raster inspection confirmed one entry per
  line, rebuilt dot leaders, preserved indentation, and a stable right-aligned
  page-number column.
- Re-rendered Task Budget Displacement pages 5, 6, 8, and 9 and Omnilingual
  ASR Speech LLM pages 1-4. All eight artifacts completed with zero empty
  translations and zero placeholder mismatches.
