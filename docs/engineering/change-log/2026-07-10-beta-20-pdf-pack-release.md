# Beta.20 PDF Component Pack Release

Date: 2026-07-10

## Summary

Published the beta.20 PDF component packs to `LeoLin4258/rosetta-assets` and
updated Rosetta's managed PDF component profiles to install them by default.
The packs include the July 2026 structured-content preservation fixes validated
against the SCRWKV 18-page PDF and QianFSD 10-page PDF on Windows and macOS.

## Artifacts

Windows x64:

- Release tag: `pdf-layout-pack-windows-x64-v2026.07.10.1`.
- File: `rosetta-pdf2zh-windows-amd64.zip`.
- Size: `349529488` bytes.
- SHA256:
  `80680b6fd94fba53a256e323337790bfd997af03c4703db0f99680a9dc1b2246`.

macOS arm64:

- Release tag: `pdf-layout-pack-macos-arm64-v2026.07.10.1`.
- File: `rosetta-pdf2zh-macos-arm64.tar.gz`.
- Size: `406417600` bytes.
- SHA256:
  `6a43e390af9cc5c4518af960696e3bb6322c247177d619585edb719897090635`.

## Included PDF Fixes

- Duplicate text-layer filtering to avoid repeated translated text overlap.
- Conservative table, formula, algorithm, and diagram-label preservation for
  visual boxes that should not be translated as body paragraphs.
- Render replay order matching for cases where pdf2zh emits render requests in
  a different order from Rosetta's prepared translation units.
- Pack-local BabelDOC fonts through `ROSETTA_BABELDOC_CACHE_DIR`, avoiding
  runtime GitHub raw font downloads.
- TencentCloud TMT dependency pinning in pack build/stage scripts to avoid
  incompatible SDK releases.

## Installer Behavior

`managed_pdf2zh/layout.rs` now requires the installed pack manifest to match
the current profile's `profileId`, `packFilename`, `sha256`, and `sizeBytes`
before reporting the managed PDF component as ready.

This is intentional for beta.20 because older beta.19-era packs can still have
the expected executable, ONNX model, and fonts while missing the structured
content and render-order fixes. Without the manifest check, upgraded users
could silently keep using the old pack.

Development overrides through `ROSETTA_PDF2ZH_BIN` or
`ROSETTA_DOCLAYOUT_MODEL` remain available for local testing. Explicit
dogfood installs launched with `ROSETTA_PDF2ZH_PACK_URL` also bypass the
release-profile identity check while still requiring the expected pack files
and bundled fonts.
