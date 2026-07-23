# PDF v3 Bounded Lazy Stream Ownership Index

Date: 2026-07-18

## Summary

Removed the final complete-document dependency from the PDF v3 production
replacement renderer by adding a reusable, bounded lazy stream ownership index.

## Implementation

- added three-state ownership for an explicit stream target set;
- streamed the full page tree through `PdfObjectView` without retaining page
  dictionaries or content bytes;
- indexed direct `/Contents` references without loading unrelated streams;
- added conservative page and nested Form resource reachability for targets
  that declare `/Subtype /Form`;
- bounded page-tree depth, Form depth and per-page Form visits;
- rejected malformed page counts, resource cycles and direct Forms with typed
  failures;
- exposed selected content roots from `PdfPageIndex` for one-time export index
  construction;
- removed `Document` from production replacement and TranslationPatch staging
  signatures;
- reused one ownership index across consecutive page stages;
- kept complete-document scans only in the legacy operand-patch compatibility
  API.

## Validation

- PDF v3: 136 passed, 13 ignored manual probes;
- synthetic 1,000-page scan retains two requested target states;
- real 30-page proof under a 12-entry / 2 MiB cache: 51 source loads, 30 hits,
  12 resident entries and 29,181 resident bytes;
- incremental two-page output remains 1,617,258 bytes with 27,016 appended
  bytes and 10 delta objects;
- Poppler visual diff: 6,383 changed pixels, 0.318252%, confined to the replaced
  first-line region;
- `pypdf`: page count and metadata retained, translated text searchable;
- temporary PDF and PNG artifacts cleaned after inspection.

## Current Boundary

Production PDF v3 font, page, stream, resource, Form COW, ownership and
incremental export staging now consume lazy source/overlay views with bounded
resident object cache. The next major boundary is the durable long-document
scheduler: backpressure, leases, cancellation, crash recovery and 500/1,000
page end-to-end stress.
