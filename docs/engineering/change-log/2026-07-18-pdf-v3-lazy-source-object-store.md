# PDF v3 Lazy Source Object Store

Date: 2026-07-18

## Summary

Added the bounded random-access source-object foundation for PDF v3 and removed
the complete `lopdf::Document` requirement from incremental export base
construction.

## Implementation

- opened source PDFs through a read-only memory map;
- parsed classic xref, xref stream and object stream entries with an uncached,
  isolated `pdf-rs` adapter;
- converted only requested primitives into existing `lopdf::Object` values;
- added a 16 MiB / 512-entry LRU with a 4 MiB per-object cache ceiling;
- added source-load, cache-hit and resident-size statistics without payloads;
- added an immutable `PdfObjectOverlay` that resolves explicit delta objects
  before source objects;
- retained raw trailer, latest xref offset, page count and maximum object number;
- constructed `IncrementalExportBase` directly from the lazy source store.

## Current Boundary

- source/xref/object access and the delta overlay now have bounded ownership;
- the final writer no longer needs a complete document to establish its base;
- production page/resource/content renderer traversal still accepts a complete
  `lopdf::Document`;
- renderer migration, large transient stream policy and 500/1000-page stress
  validation remain pending.

## Validation

- PDF v3: 121 passed, 13 ignored manual probes;
- source-object tests: 2 passed;
- real fixture: 1,590,242 bytes, 30 pages;
- lazy store open: 7ms in one Windows AMD debug run;
- three source object reads: less than 1ms in aggregate;
- post-read cache: 3 entries, estimated 10,303 bytes;
- page dictionary, normal content stream and compressed object matched the
  established `lopdf` object view;
- cache remained inside its configured byte and entry limits;
- incremental export tests: 2 passed;
- real two-page export: 1,617,258 bytes, 27,016 appended bytes, 10 delta objects;
- Poppler page 1: 2,559 changed pixels, 0.1176%, confined to
  `[245, 551) x [1592, 1611)`;
- Poppler page 2: 2,059 changed pixels, 0.0946%, confined to
  `[671, 899) x [1592, 1611)`;
- Poppler page 3: pixel-exact;
- all 30 pages, source metadata and page 1-3 annotation counts of 26, 31 and 7
  were retained;
- both translations were extractable only on their intended pages;
- visual inspection found no clipping, overlap or unrelated movement.
