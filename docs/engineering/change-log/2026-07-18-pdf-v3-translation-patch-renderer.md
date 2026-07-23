# PDF v3 TranslationPatch Renderer

Date: 2026-07-18

## Summary

Connected the durable PDF v3 `TranslationPatch` contract to the low-level
object-preserving renderer. Pending page translations can now be preflighted,
resolved to fitted/preserved decisions and applied through one atomic page
batch without reconstructing targets by source-text search.

## Implementation

Added:

- PageGraph schema v5 text-show operator and operand-hash provenance;
- exact provenance propagation through content mapping and reconciliation;
- a conservative patch-to-renderer request builder;
- complete source-object coverage and single-text-show validation;
- grouping by stream, Form invocation path and source `BT`/`ET` object;
- read-only transaction preflight with stable text-show result identity;
- deterministic fitted/preserved decision resolution and `patchId` rebuild;
- safe sibling rendering when another entry is incomplete or unsupported;
- fatal stale-source handling with page-level zero-mutation guarantees;
- a 0.9 default minimum fit scale and stable preservation reason codes;
- resolved-only patch-store validation during commit, load and repair.

Pending patches are now explicitly in-process renderer drafts. Only patches in
which every entry is fitted or preserved may become durable store authority.
This keeps the existing same-revision conflict rule intact and avoids a second
draft persistence protocol.

## Current Boundary

- entries must currently cover one complete source text object;
- unsupported anchors, text-object boundaries, styles, fonts and overflow
  preserve source content;
- paragraph reflow, one-show mixed style and arbitrary-angle text remain
  outside the renderable set;
- render-cache population and streaming document export remain pending;
- the integration is still isolated from legacy PDF v1/v2 job state.

## Visual Verification

A Windows AMD manual probe replaced one LibreOffice row with `Unified patch
renderer` and saved a searchable one-page PDF.

- Poppler render: 1241x1754 at 150 DPI;
- changed pixels: 6,846 (0.3145%);
- change bounds: `(119, 125)` through `(1057, 146)`, confined to the source row;
- visual inspection: no clipping, overlap or later-line movement;
- independent `pypdf` extraction: replacement text found in output.

## Validation

- patch renderer tests: 4 passed, 1 ignored manual probe;
- patch store tests: 9 passed, 1 ignored manual probe;
- TranslationPatch tests: 8 passed;
- complete PDF v3 suite: 109 passed, 12 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo fmt --all -- --check`, `cargo check`, `pnpm typecheck` and
  `git diff --check`: passed.
