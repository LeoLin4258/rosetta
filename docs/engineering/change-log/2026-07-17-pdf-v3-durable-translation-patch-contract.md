# PDF v3 Durable TranslationPatch Contract

Date: 2026-07-17

## Summary

Implemented the first durable PDF v3 page-patch contract so translated state
can be stored independently from complete rendered page PDFs and safely
revalidated against the current PageGraph.

## Implementation

Added:

- deterministic patch and entry IDs;
- source page and per-atom SHA-256 identity;
- translation revision plus provider/model identity;
- canonical PageGraph-order entry and atom serialization;
- exact protected span values with translated UTF-8 byte ranges;
- typed pending, fitted and source-preserved renderer decisions;
- canonical rebuild validation for stale, reordered or modified patches;
- source protected-span integrity checks;
- compact JSON encode/decode with a 16 MiB durable patch limit;
- typed failures for stale pages/atoms, duplicate atoms, partial/missing/
  overlapping protected spans, invalid fit state and non-canonical content.

Ordinary source text is not copied into a patch. Only the exact source values
that must remain protected, such as citations, are retained.

## Current Boundary

- the logical schema and validation layer are implemented in the isolated
  native PDF v3 module;
- the patch has not yet been connected to translation scheduling or the
  low-level renderer;
- atomic revisioned file storage, manifest ownership, compression, bounded
  render cache and streaming export remain pending Phase 4 work;
- this stage does not alter PDF rendering bytes, so the preceding Poppler
  visual baseline remains applicable.

## Validation

- `cargo check`: passed;
- TranslationPatch tests: 6 passed, 0 failed;
- deterministic compact round trip, stale page/atom/ID rejection, protected
  span placement, invalid renderer state and encode size-limit coverage: passed.
