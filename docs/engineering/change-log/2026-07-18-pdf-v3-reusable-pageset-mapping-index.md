# PDF v3 Reusable PageSet Mapping Index

Date: 2026-07-18

## Summary

Removed repeated page-tree discovery from multi-page extraction/mapping and
completed the first native 100/500/1,000-page bounded-memory stress pass.

## Implementation

- added a source-fingerprint-bound `PageOperandMappingIndex` for an explicit
  `PageSet`;
- added indexed mapping and reconciliation entry points while preserving the
  existing single-page compatibility entry point;
- reject cross-document index and snapshot reuse with typed errors;
- added a valid flat-page-tree stress fixture with independent compressed page
  streams and exact text-show mappings;
- added automatic sparse-page reuse and cross-source rejection tests;
- added ignored Windows probes for 100, 500 and 1,000 pages;
- sampled current, maximum and process-peak working set plus private bytes with
  the existing Windows dependency;
- retained the 512-entry / 16 MiB source-object cache limits.

## Windows AMD Evidence

- 100 pages: 51 ms main loop, 20,086,784-byte peak working set;
- 500 pages: 286 ms main loop, 22,069,248-byte peak working set;
- 1,000 pages: 681 ms main loop, 23,728,128-byte peak working set;
- all pages reconciled as `Complete` with one exact mapped text show;
- the 1,000-page final source cache held 512 entries / 521,630 bytes;
- a 500-page reusable index took 16,914 us versus 756,893 us for repeated
  single-page construction, a 44.75x difference.

These debug measurements isolate extraction, source operand mapping and
PageGraph reconciliation on a synthetic text PDF. They do not include model
translation, patch persistence, final export, UI work or a complex real-world
long-document corpus.

## Remaining Boundary

The native core now has evidence that basic 1,000-page page processing is fast
and bounded without ten-page user-visible chunking. Worker/Tauri integration,
durable scheduler orchestration and real complex 500/1,000-page end-to-end
translation/export remain Phase 6 gates.
