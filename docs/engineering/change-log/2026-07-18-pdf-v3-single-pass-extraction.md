# PDF v3 Single-Pass Extraction

Date: 2026-07-18

## Summary

Removed two dominant PDFium extraction costs while preserving exact
character-to-object provenance and reducing source-sized memory duplication.

## Implementation

- reused the already-open page text handle for text-object extraction;
- added a narrow vendored `pdfium-render` identity adapter;
- replaced per-object full-page character scans with one page-character pass;
- retained exact recursive Form object IDs, text, style and character ranges;
- streamed SHA-256 fingerprinting through a 64 KiB buffer;
- opened lopdf and PDFium from the immutable source path instead of retaining a
  complete source byte vector;
- added extraction substage timings and an ignored ten-page diagnostic;
- added legacy-API object mapping and direct-character style equivalence tests.

## Validation Evidence

- first ten real-paper pages: 39,783 atoms before and after;
- previous stable debug total: 4,692-4,791 ms;
- final three-run debug total: 784-874 ms;
- exact object mapping equivalence: passed;
- exact character style equivalence: passed.

The timings are Windows AMD debug diagnostics on one fixture, not release or
end-to-end translation measurements. See the dedicated benchmark note and ADR
0050 for method, boundaries and maintenance costs.
