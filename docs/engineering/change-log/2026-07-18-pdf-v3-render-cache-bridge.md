# PDF v3 Render Cache Bridge

Date: 2026-07-18

## Summary

Connected the PDF v3 TranslationPatch renderer to the bounded render cache
without making cached page PDFs translation authority. Resolved patches can now
reproduce cache misses, and cached `translatedPagePdf` artifacts contain only
the selected page and its reachable objects.

## Implementation

Added:

- renderer contract identity
  `rosetta-pdf-v3-translation-patch-renderer/1`;
- exact contract-version checks for render and cache addressing;
- all-resolved deterministic renderer replay;
- stored-decision drift rejection before PDF mutation;
- consumed working-document ownership for page artifact generation;
- unselected-page and document-navigation removal;
- unreachable-object pruning, renumbering and stream compression;
- PDF signature/reload/exact-one-page artifact validation;
- source fingerprint binding inside the rendered page artifact;
- resolved-patch-only translated-page cache key construction;
- bounded cache insert and lease-validated read helpers;
- corrupt cached body conversion to a rebuildable cache miss;
- multi-page fixture coverage for miss/insert/hit/rebuild byte identity.

Cache insertion remains a separate call after render. A cache quota or I/O
failure does not erase the resolved patch and does not block patch-store
authority.

## Current Boundary

- pending patches must render before their resolved cache identity exists;
- resolved cache misses repeat renderer preflight;
- the caller must provide one owned working document per active render task;
- preview PNG cache population is not connected yet;
- streaming final export with source outlines/navigation remains pending;
- v3 remains isolated from legacy PDF v1/v2 page artifacts.

## Visual Verification

The Windows AMD probe used page 1 of the 30-page real-paper fixture.

- source: 1,590,242 bytes and 30 pages;
- cached page artifact: 104,857 bytes and exactly one A4 page;
- Poppler render: 1241x1754 at 150 DPI;
- changed pixels: 2,718 (0.1249%);
- change bounds: `(245, 1592)` through `(551, 1611)`, confined to the target row;
- annotations: 26 before and after, including the RWKV external link;
- independent `pypdf` extraction found `Bounded cached page`;
- visual inspection found no clipping, overlap or unrelated page changes.

## Validation

- patch renderer tests: 8 passed, 2 ignored manual probes;
- manual cache bridge Poppler probe: passed;
- complete PDF v3 suite: 113 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo fmt --all -- --check`, `cargo check`, `pnpm typecheck` and
  `git diff --check`: passed.
