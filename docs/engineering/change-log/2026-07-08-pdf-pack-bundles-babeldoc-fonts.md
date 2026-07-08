# PDF Pack Bundles BabelDOC Fonts

Date: 2026-07-08

## Context

macOS validation showed that the current PDF component patch applies and
compiles, but the installed macOS pack can still fail before `prepareRun`
returns JSON. BabelDOC attempted to download missing fonts such as
`SourceHanSansCN-Bold.ttf` and `GoNotoKurrent-Regular.ttf` into the user-level
`~/.cache/babeldoc/fonts` directory, then hit GitHub raw `429 Too Many
Requests`.

That made the PDF component pack non-self-contained: even with the bundled ONNX
layout model, a clean machine could still need network access for fonts during
translation.

## Changes

- Added `stage-pdf2zh-font-assets.py` to patch BabelDOC so
  `ROSETTA_BABELDOC_CACHE_DIR` can redirect its cache into the PDF component
  pack.
- The macOS release builder, macOS local staging helper, and Windows release
  builder now stage the required BabelDOC fonts into
  `assets/babeldoc/fonts`.
- Pack smoke tests now verify that `SourceHanSansCN-Regular.ttf`,
  `SourceHanSansCN-Bold.ttf`, and `GoNotoKurrent-Regular.ttf` are served from
  the pack-local BabelDOC cache.
- The runtime worker now sets `ROSETTA_BABELDOC_CACHE_DIR` to the installed
  pack's `assets/babeldoc` directory.
- PDF pack readiness now requires the bundled ONNX layout model and the three
  required BabelDOC fonts.
- Fixed `archive-pdf2zh-pack-local.sh` to check the current `.onnx` layout
  model filename instead of the old `.pt` path, and to reject staged packs that
  are missing required fonts.

## Validation Notes

The macOS pack should not be published until it is rebuilt with these asset
changes and the macOS agent reruns the `prepareRun` and render smoke tests on a
clean BabelDOC user cache. A successful smoke should not make outbound font
requests during PDF preparation.
