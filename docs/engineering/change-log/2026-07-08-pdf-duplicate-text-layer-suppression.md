# 2026-07-08 PDF Duplicate Text Layer Suppression

## Summary

Fixed a long-standing translated PDF readability issue where some source PDFs
with duplicate embedded text layers produced translated pages with repeated
overlapping text.

The observed case was the SpaceX prospectus PDF. Page 1 extracted 24 PDF
translation units, but units 13-24 were a second copy of units 1-12 from a
duplicate source text layer. The English source looked mostly tolerable because
the two original layers were nearly identical; after translation, the two
Chinese render passes overlapped and made the page look smeared.

## Change

- The PDF pack patch now injects duplicate text-layer suppression into
  `pdf2zh.rosetta_engine`.
- During unit collection, Rosetta detects page-level repeated text-layer
  sequences using canonicalized text similarity.
- The canonicalization keeps Unicode letters and numbers, so the same guard can
  catch non-ASCII duplicate source layers instead of only ASCII-heavy PDFs.
- Duplicate layer units are kept in the render order but marked
  `requiresTranslation=false`.
- Rust emits empty passthrough translations for non-required PDF units so the
  renderer can consider those units handled without drawing duplicate text.
- Rendering now ignores formula placeholder checks for non-required duplicate
  units; otherwise duplicate layers that contained `{vN}` placeholders could
  make a page fail with `formula placeholder mismatch` even though the layer was
  intentionally skipped.
- The pdf2zh converter patch now draws a white paragraph mask before translated
  text and keeps CJK line spacing above a legible floor, shrinking translated
  text when needed instead of squeezing line height until glyphs overlap.
- Formula detection was narrowed so italic prose and ordinary alphanumeric text
  in layout regions classified as visual/table are translated as text instead
  of being preserved as pseudo-formulas.
- Page result `sourceUnitCount` and `sourceChars` count only required,
  non-duplicate units, preserving the existing translated-unit completeness
  guard.

This does not remove legitimate source content. It targets broad repeated text
layers, not isolated repeated words or normal repeated legal phrases.

## Local Verification

The local Windows PDF component pack was patched in place:

```powershell
$py = Join-Path $env:LOCALAPPDATA 'com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\python\python.exe'
& $py rosetta-app\src-tauri\scripts\patch-pdf2zh-color-preservation.py
```

The SpaceX page 1 collect smoke showed:

```txt
before: 24 total units, 24 required
after:  24 total units, 12 required, 12 duplicate-layer skipped
```

A render smoke using fixed test translations returned:

```txt
sourceUnitCount: 12
translatedUnitCount: 12
near duplicate spans: 0
```

After the follow-up placeholder/layout fix, a page 1 render smoke using only
required-unit translations returned:

```txt
status: translated
placeholderMismatchCount: 0
sourceUnitCount: 12
translatedUnitCount: 12
```

A page 2 long-Chinese render smoke confirmed ordinary visual-region prose is
collected as text (`body` units increased from 16 to 21) and source English no
longer remains under the translated layer.

## Validation

```powershell
python rosetta-app\src-tauri\scripts\test-pdf2zh-patches.py
cd rosetta-app\src-tauri
cargo test unit_translation
cargo test rosetta_jobs
cargo check
cd ..
node --check scripts\check-pdf-translation-run.mjs
pnpm typecheck
```

Results:

```txt
patch tests: 9 passed
unit_translation: 11 passed
rosetta_jobs: 66 passed
cargo check: passed
node --check: passed
pnpm typecheck: passed
```

`cargo fmt -- --check` still reports an existing rustfmt diff in
`src/managed_pdf2zh/layout.rs`; that file was not changed for this fix.
