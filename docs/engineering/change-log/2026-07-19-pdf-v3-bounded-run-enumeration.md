# PDF v3 Bounded Run Enumeration

Date: 2026-07-19

## Summary

Added a privacy-safe, bounded native run-history query for PDF v3 and its typed
frontend API contract.

## Implementation

- added `list_rosetta_pdf_v3_runs(jobId, targetLanguage?, beforeRevision?, limit?)`;
- moved committed-run directory scanning and scheduler/runtime validation to a
  blocking worker;
- ordered results by descending native translation revision with an exclusive
  revision cursor;
- fixed the default result window at 16 and the hard maximum at 64 while
  retaining only top-K candidates in memory;
- normalized language filters to the same primary-language identity used by
  trusted creation while preserving exact returned language tags;
- skipped hidden atomic-creation staging directories and failed closed on any
  malformed visible committed run;
- returned only summary-level ownership, recovery timing, PageSet and progress
  fields, with no paths, owner IDs, credentials, endpoints, text or raw errors;
- added shared TypeScript list/run/summary types and a typed invoke wrapper;
- kept enumeration free of lifecycle, heartbeat, worker and recovery side
  effects.

## Validation

- focused Windows tests for ordering, cursor pagination, language filtering,
  staging exclusion, malformed committed state, bounds and serialized privacy;
- full PDF v3, Rosetta jobs and managed RWKV suites;
- Rust check/format, TypeScript typecheck and diff checks.

## Current Boundary

The frontend can now discover and select native v3 runs without loading an
unbounded history or duplicating scheduler state. Visible workspace migration
still waits for lazy page-bounded translated preview raster support; complete
PDF generation is not used as a preview workaround.
