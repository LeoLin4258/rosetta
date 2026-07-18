# ADR 0044: PDF v3 Overlay-Backed Page Delta Staging

Date: 2026-07-18

Status: Accepted

Amends ADR 0041, ADR 0042 and ADR 0043.

## Context

ADR 0043 moved document-font staging to `PdfObjectView`, but the real
multi-page proof still applied every font and page delta to a complete temporary
`lopdf::Document`. The next page used that mutated document both for source
traversal and for accumulated object identity.

This retained duplicate font stream bytes in the complete document, made object
allocation depend on mutation order, and prevented the export accumulator from
being the sole authority for staged changes.

## Decision

TranslationPatch and text-show replacement staging separate two read contracts:

- an immutable `&lopdf::Document` used temporarily for page tree, resource and
  content-stream traversal;
- an immutable `&dyn PdfObjectView` used for accumulated object identity,
  registry binding and maximum object-number allocation.

Final multi-page export constructs the second contract as
`PdfObjectOverlay(source_store, accumulated_delta)`. The font delta is never
applied to the source traversal document. After each successful page stage, its
delta is merged into the accumulator and the next page receives a fresh overlay
over that merged value. Earlier page deltas are likewise not applied to the
source traversal document. The multi-page proof therefore needs one read-only
complete source document, not a second cloned working document.

All new font and copy-on-write object IDs allocate strictly above the
accumulated view maximum. An all-preserved page returns an empty delta carrying
that same maximum, so it cannot move allocation identity backward.

Compatibility mutation APIs pass the same `Document` as both contracts and
continue to apply the staged delta only after complete preflight.

The accumulated view does not yet replace source traversal. Each page in one
export revision may be staged once. A repeated mutation of the same page or
source object must fail through `PdfObjectDelta` merge conflict rather than
implicitly reading an earlier page delta.

## Evidence

Automated Windows AMD tests prove:

- the real 30-page/two-page export leaves the source traversal document object
  map and maximum object number unchanged;
- the real export does not clone the complete source document into a mutable
  working graph;
- both page renders stage zero font objects while resolving the Type0 font from
  the accumulated overlay;
- the output remains 1,617,258 bytes from a 1,590,242-byte source, with 27,016
  appended bytes and 10 delta objects;
- a two-target Form copy-on-write stage keeps six font objects only in the
  overlay, then allocates three cloned streams above those six IDs;
- the Form page delta contains four objects, ends at source maximum plus nine,
  and merges with the font delta into 10 non-conflicting objects;
- overlay-backed Form staging leaves the complete source document unchanged.
- an all-preserved page returns zero objects while retaining the accumulated
  font-delta maximum object number.

Independent Poppler comparison confines page 1 and 2 changes to their selected
footer rows and keeps page 3 pixel-exact. `pypdf` retains all 30 pages, metadata
and annotation arrays, and finds each translation only on its intended page.

## Consequences

### Positive

- Large subset font streams no longer have a second copy in the complete
  temporary document during final export.
- Final multi-page staging removes one complete cloned working object graph.
- The accumulated delta is the sole authority for staged object identity.
- Copy-on-write allocation cannot collide with unapplied font or earlier page
  allocations.
- Multi-page staging no longer mutates its source traversal view.
- The remaining `Document` dependency is now isolated to source navigation and
  content interpretation.

### Costs

- Final export still pays the complete `lopdf::Document` source graph cost.
- Staging functions temporarily accept two views with different responsibilities.
- Repeated staging of one page cannot consume its earlier delta and is rejected
  during accumulator merge.
- A future lazy page index must replace `Document::get_pages()` and inherited
  resource helpers before the complete document can be removed.

## Rejected Alternatives

- Keep applying every accumulated delta to a temporary complete document.
- Use the source document maximum while font objects remain only in the delta.
- Let allocation probe object IDs reactively after collisions occur.
- Make the overlay pretend to provide page traversal before page indexing and
  inherited resource semantics are implemented.
