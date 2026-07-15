# Beta.22 Linux PDF Component Pack Release

Date: 2026-07-15

## Summary

Published the Linux x64 PDF layout component required by Rosetta beta.22's
multi-document and durable PDF prepare caches.

## Artifact

- Release tag: `pdf-layout-pack-linux-x64-v2026.07.15.1`.
- File: `rosetta-pdf2zh-linux-x64.tar.gz`.
- Size: `510388352` bytes.
- SHA-256:
  `f6492939a7ea919d8d01923f59a78e2c5761abd5428264ca4a636da73dda2034`.
- PDFMathTranslate commit:
  `990bed055d372772f5cec8ef4a982a8f767d64a4`.

The Linux managed-component profile now pins the new mainland mirror and
GitHub fallback URLs. Existing beta.21 installations report the previous pack
as outdated and install this exact archive through the normal component update
flow.

## Validation

The Ubuntu 24.04 x64 release builder passed:

- 25 PDF pack patch tests.
- In-place and relocated real-PDF prepare and render smoke tests.
- Prepared-run reset followed by a second render.
- Durable layout-cache restore after disposing process-local prepared state.
- 28 packaged runtime module imports.
- Archive safety, executable mode, size, and SHA-256 checks.

GitHub's release asset API reported the same byte size and SHA-256 digest. The
mainland mirror returned HTTP 206 for a range request after publication.
