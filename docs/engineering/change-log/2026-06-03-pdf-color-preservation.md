# PDF Color And Bold Preservation Patch

## Context

Visual PDF translation delegates layout restoration to the managed `pdf2zh` pack. Dogfood PDFs showed that translated text kept approximate layout but lost original font colors, usually rendering translated text as black.

The issue was inside the `pdf2zh 1.7.9` converter path used by Rosetta's pack scripts: parsed `LTChar` objects already carry `graphicstate.ncolor`, but the translated text output path emitted `TJ` text operations without restoring the original non-stroking color.

## Changes

- Added `src-tauri/scripts/patch-pdf2zh-color-preservation.py` to patch installed `pdf2zh` converter code during pack construction.
- Updated local staging and release pack scripts to apply the color preservation patch after installing `pdf2zh`.
- The patch preserves paragraph text color from the source paragraph's chosen text run, formula glyph color from each original formula glyph, and stroke color for preserved formula/global lines.
- The patch also detects paragraph-level bold/medium font names and applies a conservative PDF text rendering mode stroke to simulate bold for translated paragraph text.
- The bold detection is computed inline during the first character scan because the PDF converter defines later output helpers only after that scan. This avoids `UnboundLocalError` during page processing.
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
