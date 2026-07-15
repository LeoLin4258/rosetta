# Linux PDF2zh Release Pack

Date: 2026-07-14

## Summary

Published the first reproducible Linux x64 PDF layout component and enabled
Rosetta to install it through the existing managed PDF component flow.

## Change

- Added a Linux x64 release builder around a relocatable
  python-build-standalone runtime.
- Pinned PDFMathTranslate commit
  `990bed055d372772f5cec8ef4a982a8f767d64a4`, Python `3.12.13`, the
  `20260602` python-build-standalone release, and runtime-only Python
  dependencies.
- Bundled the DocLayout ONNX model and BabelDOC fonts required for offline PDF
  prepare and render after installation.
- Added in-place and relocated real-PDF smoke tests, runtime import checks, and
  archive safety checks to the release builder.
- Published `rosetta-pdf2zh-linux-x64.tar.gz` in the `rosetta-assets` release
  `pdf-layout-pack-linux-x64-v2026.07.14.1`.
- Enabled the Linux x64 managed PDF profile with checksum-pinned mainland mirror
  and GitHub fallback URLs.

No persistent data format or PDF translation contract changed.

## Published Artifact

- Size: `510384173` bytes
- SHA-256:
  `4f71a0ea881f899d2c10a8a76874f453b4829840f8a1f36efcc19fde9bfd3f5d`
- DocLayout model SHA-256:
  `fece9af02f618b603ff7921ccec6861d13e7e1f9830e091dfb7e8ad9311e5b21`

## Validation

Release builder validation on Ubuntu Linux x64:

- In-place real PDF prepare and render: passed.
- Relocated-pack real PDF prepare and render: passed.
- Runtime imports: 28 passed.
- Archive SHA-256, safe paths, and executable launcher mode: passed.
- GitHub release size and server digest matched the local artifact.
- Mainland mirror and GitHub fallback both returned HTTP 200.

Repository validation:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo fmt -- --check
cargo check
cargo test managed_pdf2zh::profile
cargo test rosetta_jobs
```

Release acceptance on Ubuntu Linux x64:

- Installed the component from the public mainland mirror through the Rosetta
  UI using the existing managed component flow.
- Installed manifest URL, size, and SHA-256 matched the published artifact.
- Installed launcher reported `pdf2zh v1.9.11` and retained executable mode
  `0755`.
- Forced a real one-page PDF retranslation through the downloaded pack. The run
  completed with no failed pages, and the generated PDF passed `pdfinfo`.
- Runtime logs confirmed the worker used
  `pdf2zh-sidecar/pack/linux-x64/python/bin/python` and the bundled DocLayout
  model.
