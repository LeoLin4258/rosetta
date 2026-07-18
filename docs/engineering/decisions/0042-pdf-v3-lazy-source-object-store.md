# ADR 0042: PDF v3 Lazy Source Object Store

Date: 2026-07-18

Status: Accepted

Amends ADR 0040 and ADR 0041.

## Context

The incremental writer and renderer delta are bounded by changed objects, but
the renderer still obtains its read view by loading every source object into a
`lopdf::Document`. `lopdf 0.34` reads the complete file into a heap `Vec` and
parses every normal and compressed object before returning. Its filtered loader
can discard objects after parsing, but cannot provide true random access.

The real-paper fixture uses xref streams and multiple object streams. A
classic-xref-only shortcut would therefore reject or misread exactly the PDFs
that the new pipeline must handle.

## Decision

PDF v3 adds `PdfSourceObjectStore` as an isolated random-access source layer.
It uses `pdf-rs 0.10` with no library cache and a read-only `memmap2` mapping to
parse xref tables/streams, object streams and requested indirect objects. The
library is not used for extraction, layout, translation, fitting, rendering or
writing.

Every requested `pdf-rs::Primitive` is converted immediately into the existing
internal `lopdf::Object`. No `pdf-rs` type crosses the module boundary. Rosetta
owns cache policy, object identity validation, error typing and the delta
overlay.

The default LRU limits are:

- 16 MiB total resident estimated object bytes;
- 512 resident objects;
- 4 MiB maximum for one cached object.

Larger streams remain transient for the active operation and are not cached.
Cache diagnostics expose only counts and estimated bytes.

`PdfObjectOverlay` resolves exact IDs from `PdfObjectDelta` first and falls back
to the immutable source store. The source store also retains raw trailer
semantics, latest xref offset, page count and maximum object number, allowing
`IncrementalExportBase` construction without a complete `lopdf::Document`.

The memory map is not identity authority. Final export continues to stream and
hash the source again before atomic destination replacement.

## Evidence

Automated Windows AMD tests prove:

- the 30-page real-paper xref/trailer opens through the mapped reader;
- a page dictionary, a normal content stream and an object stored inside an
  `/ObjStm` convert to objects equal to the established `lopdf` view;
- a repeated lookup hits the bounded LRU and resident bytes stay below policy;
- an overlay returns replaced and newly allocated delta objects without
  changing the source view;
- incremental export base construction from the lazy store preserves a valid
  one-page update;
- the existing two-page real-paper renderer/export proof remains 1,617,258
  bytes with a 27,016-byte append and 10 delta objects.

One debug test run opened the 1,590,242-byte, 30-page fixture in 7ms. Reading a
page dictionary, normal content stream and compressed object then took less
than 1ms in aggregate; after one repeated lookup, the cache held three entries
and an estimated 10,303 bytes. These timings cover only the source-object layer,
not PDFium layout extraction or translation.

## Consequences

### Positive

- Source bytes are demand-paged by the OS instead of copied into a second Rust
  heap buffer for the source-object layer.
- Object count no longer determines resident source-object cache size.
- Xref streams, object streams and classic xref tables share one tested path.
- Renderer and writer keep their existing `lopdf::Object` contract during
  migration.
- `pdf-rs` is confined to a replaceable adapter rather than becoming pipeline
  authority.

### Costs

- `pdf-rs` and `memmap2` add native dependencies and eventual release-size cost;
  this must be measured when release builds are authorized and weighed against
  removal of the legacy Python/pdf2zh component.
- Conversion creates one owned object for the active operation. Very large
  selected streams can still create a large transient allocation.
- The source must remain immutable during a session; final commit detects
  length or SHA-256 changes.
- The current production renderer still accepts `&lopdf::Document`; this ADR
  establishes the source/overlay layer but does not claim end-to-end bounded
  memory yet.

## Rejected Alternatives

- Keep `Document::load_mem` and only prune its object map after full parsing.
- Implement a classic-xref-only file reader.
- Vendor and maintain a complete `lopdf` fork solely to expose xref-only load.
- Let `pdf-rs` own layout, extraction, rendering or export behavior.
- Use an unbounded object cache.
