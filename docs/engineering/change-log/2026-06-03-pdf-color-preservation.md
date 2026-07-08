# PDF Color And Bold Preservation Patch

## Context

Visual PDF translation delegates layout restoration to the managed `pdf2zh` pack. Dogfood PDFs showed that translated text kept approximate layout but lost original font colors, usually rendering translated text as black.

The issue was inside the `pdf2zh 1.7.9` converter path used by Rosetta's pack scripts: parsed `LTChar` objects already carry `graphicstate.ncolor`, but the translated text output path emitted `TJ` text operations without restoring the original non-stroking color.

## Changes

- Added `src-tauri/scripts/patch-pdf2zh-color-preservation.py` to patch installed `pdf2zh` converter code during pack construction.
- Updated local staging and release pack scripts to apply the color preservation patch after installing `pdf2zh`.
- The patch preserves paragraph text color from the source paragraph's chosen text run, formula glyph color from each original formula glyph, and stroke color for preserved formula/global lines.
- The patch previously detected paragraph-level bold/medium font names and applied a conservative PDF text rendering mode stroke to simulate bold for translated paragraph text. This faux-bold stroke was removed on 2026-07-07 because Windows output rendered too heavy compared with source text and macOS output.
- The replacement bold path uses a real `SourceHanSansCN-Bold.ttf` font resource named `notobold` for simplified Chinese paragraphs whose source text contains a bold/medium font run. Normal simplified Chinese text uses `SourceHanSansCN-Regular.ttf`.
- The bold detection is computed inline during the first character scan because the PDF converter defines later output helpers only after that scan. It is cumulative across the paragraph so a later regular run cannot erase an earlier bold/medium source run.
- The pack patch also maps simplified Chinese PDF output (`zh`, `zh-CN`, `zh-Hans`) from pdf2zh's default Source Han Serif font to Source Han Sans, using the same BabelDOC assets on macOS and Windows.
- The patch is idempotent and removes Python bytecode caches after editing the installed package.

## Validation

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

The local `.venv-pdf2zh` copy was also patched to verify the patch matches `pdf2zh 1.7.9` and `converter.py` compiles after modification. `src-tauri/scripts/test-pdf2zh-patches.py` covers the pack patch against a temporary fake `pdf2zh` package.
