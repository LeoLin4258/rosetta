# Beta.22 Windows PDF Component Pack Release

Date: 2026-07-16

## Summary

Published the Windows x64 PDF layout component required by Rosetta beta.22's
multi-document and durable PDF prepare caches.

## Artifact

- Release tag: `pdf-layout-pack-windows-x64-v2026.07.16.1`.
- File: `rosetta-pdf2zh-windows-amd64.zip`.
- Size: `349587199` bytes.
- SHA-256:
  `1ecfe406fb9e583f38e6ec644ff969aa50c8c86b9d1c87d9f057328454a7d494`.
- PDFMathTranslate commit:
  `990bed055d372772f5cec8ef4a982a8f767d64a4`.

The Windows managed-component profile now pins the new mainland mirror and
GitHub fallback URLs. The archive contains the reusable prepared-run and
durable layout-cache engine used by beta.22.

## Validation

The Windows x64 pack builder passed:

- Packaged runtime import smoke with ONNX providers.
- Pruned-pack runtime import smoke.
- PDF engine contract, reusable reset, and durable cache capability checks.
- Archive creation and local SHA-256 verification.

GitHub's release asset API reported the same byte size and SHA-256 digest.
