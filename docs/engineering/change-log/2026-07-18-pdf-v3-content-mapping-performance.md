# PDF v3 Content Mapping Performance

Date: 2026-07-18

## Summary

Reduced the remaining PDF v3 content mapping cost without weakening recursive
Form provenance or extending cache lifetime across pages.

## Implementation

- cached immutable decoded lopdf `Content` by physical stream ID for one page
  mapping lifetime;
- continued replaying operator state, inherited resources and invocation paths
  independently for every Form invocation;
- indexed lopdf page object IDs once in `DocumentHandle` for O(1) exact page
  access;
- replaced allocation-heavy digest formatting while preserving canonical
  lowercase SHA-256 IDs;
- added non-serialized mapping substage timings and cache-hit diagnostics;
- added cache exercise, diagnostic serialization and hash compatibility tests.

## Performance Evidence

The 10-page Windows AMD debug diagnostic recorded:

- total: 717-797 ms across three runs;
- content mapping: 373-415 ms;
- aggregate page lookup: 29-30 microseconds;
- page-local parsed stream cache hits: 219;
- atom count: unchanged at 39,783.

These results exclude translation, preview and export, and Windows filesystem
caching was uncontrolled.
