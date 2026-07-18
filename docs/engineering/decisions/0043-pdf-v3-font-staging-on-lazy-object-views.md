# ADR 0043: PDF v3 Font Staging on Lazy Object Views

Date: 2026-07-18

Status: Accepted

Amends ADR 0039, ADR 0041 and ADR 0042.

## Context

The lazy source store and explicit object delta established the read and write
contracts needed for bounded export, but every renderer staging API still
accepted a complete `lopdf::Document`. Document-wide translation-font staging
is a narrow first migration target because it needs only the source maximum
object number. Registry reuse validation needs one exact Type0 object lookup.

Keeping those operations tied to `Document` would force a complete source
object graph to exist before any page render, even though all six font objects
are new delta objects.

## Decision

Document-wide translation-font staging, prepared-font staging and registry
binding accept `&dyn PdfObjectView`.

Object allocation starts strictly above the greater of the source view maximum
and the caller's reserved maximum. Because the view maximum is authoritative,
allocation only needs to reject collisions inside the staged object map. It
does not enumerate or probe source objects.

Registry binding first validates weight, asset fingerprint and deterministic
subset identity, then resolves the registered Type0 object through the view and
checks `/Subtype` and `/BaseFont`. During export that view is a
`PdfObjectOverlay`, so a newly staged Type0 object resolves from the delta
without loading any source object.

`lopdf::Document` implements `PdfObjectView` only as a migration adapter. The
existing mutation helpers remain compatibility wrappers that stage against the
adapter and apply the resulting `PdfObjectDelta` after successful preflight.

The real two-page incremental export now opens one `PdfSourceObjectStore`
before font staging and reuses it for `IncrementalExportBase`. Page patch
staging still uses a complete temporary `Document`; this ADR does not claim
end-to-end bounded memory.

## Evidence

Automated Windows AMD tests prove:

- one Arial face stages exactly six objects against the lazy source;
- staging leaves the source maximum object number unchanged;
- the delta maximum is exactly source maximum plus six;
- registry binding succeeds through `PdfObjectOverlay`;
- source object loads remain zero through staging and overlay validation;
- a different deterministic subset is rejected before object lookup;
- the existing `Document` compatibility path remains atomic and identity
  checked;
- the two-page incremental export remains 1,617,258 bytes from a 1,590,242-byte
  source, with 27,016 appended bytes and 10 delta objects.

Independent Poppler comparison confines all changes to the two selected footer
rows and keeps page 3 pixel-exact. `pypdf` retains all 30 pages, metadata and
annotation arrays, and finds each translation only on its intended page.

## Consequences

### Positive

- Font preparation no longer requires loading the complete source object graph.
- Font allocation performs no source object reads.
- Registry identity validation composes directly with source-plus-delta
  overlays.
- The compatibility wrapper preserves existing page-cache and renderer tests
  while new export code can depend on the narrow view contract.
- The migration establishes the allocation and error pattern for later page,
  resource and content traversal work.

### Costs

- `PdfObjectView::object()` returns an owned `lopdf::Object`, so every lookup
  clones either the cached source value or delta value.
- Page patch staging and sequential page overlay still depend on a complete
  temporary `lopdf::Document`.
- Source object failures now propagate through the font error boundary and must
  remain typed through future command APIs.

## Rejected Alternatives

- Keep font staging on `Document` until the entire renderer can migrate at once.
- Probe every candidate object ID through the source view during allocation.
- Apply font objects directly to a mutable overlay during staging.
- Treat the `Document` adapter as the permanent source contract.
