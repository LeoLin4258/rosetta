# ADR 0046: PDF v3 Lazy Page Context and Source Stream Reads

Date: 2026-07-18

Status: Accepted

Amends ADR 0044 and ADR 0045.

## Context

ADR 0045 moved selected-page identity and top-level content references onto a
lazy `PdfPageIndex`, but replacement staging still returned to the complete
`lopdf::Document` for the page dictionary, inherited resources and every target
content stream. This duplicated source-object authority and made selected-page
work depend on a fully parsed source graph even when the page index had already
identified the exact objects needed.

Accumulated delta objects have a different responsibility. They define font
registry identity and object-number allocation, but must not replace immutable
source page or stream reads within the same export revision.

## Decision

TranslationPatch staging separates three immutable read contracts:

- a temporary `&lopdf::Document` used only for legacy cross-page ownership and
  Form invocation/copy-on-write traversal;
- a `&dyn PdfObjectView` for immutable source page, resource and stream reads;
- a `&dyn PdfObjectView` for accumulated source-plus-delta identity, registry
  lookup and allocation maximum.

`PdfPageObjectContext` resolves one `PdfIndexedPage` through the source view. It
clones the exact page dictionary and materializes effective `/Resources` by
walking the selected page followed by its indexed ancestors. Resource scopes
are merged farthest-to-nearest, so the closest definition wins. Direct and
indirect resource dictionaries and category dictionaries are supported.

Indirect reference chains are limited to 64 hops and reject cycles. Invalid
page types, non-dictionary resources and inconsistent resource categories are
typed failures. The result is an owned transient snapshot and is never durable
job data.

Replacement target identity, preflight decoding, staged source-stream reads
and non-copy-on-write page font resource updates now use the immutable source
view and page context. Compatibility APIs pass the same `Document` as all three
contracts. Final multi-page staging passes the mapped `PdfSourceObjectStore`
for source reads and a fresh source-plus-accumulated-delta overlay for identity
and allocation.

This decision does not move global stream/Form ownership discovery, Form
invocation validation or copy-on-write resource path resolution. Those remain
the explicit complete-document boundary.

## Evidence

Automated Windows AMD tests prove:

- nearest page/resource scope overrides farther ancestors;
- malformed selected-page resources fail without staging mutations;
- a real 30-page fixture materializes the same page resources as the legacy
  complete-document helper;
- the real two-page export performs 12 source-object loads, records 23 cache
  hits and retains 12 entries / 28,712 estimated bytes;
- source loads and resident entries remain below 32, and resident bytes remain
  below 16 MiB;
- output remains 1,617,258 bytes from a 1,590,242-byte source, with 27,016
  appended bytes and 10 delta objects.

Independent Poppler comparison confines changes to the two selected footer
rows and keeps page 3 pixel-exact. `pypdf` retains all 30 pages, metadata and
page 1-3 annotation arrays, and finds each translation only on its intended
page. Visual inspection found no clipping, overlap or unrelated movement.

## Consequences

### Positive

- Selected-page dictionary, inherited resources and target content streams no
  longer require complete-document object reads.
- Source objects and accumulated delta identity cannot be confused by one
  overloaded renderer parameter.
- Page resource precedence is explicit, bounded and independently testable.
- Ordinary non-Form page staging now has a measurable bounded lazy source
  working set.
- The remaining complete-document responsibilities are isolated to Form and
  global ownership traversal.

### Costs

- Staging APIs temporarily carry three read contracts until legacy traversal
  is fully migrated.
- Materialization clones resource dictionaries for the active selected page.
- One selected stream larger than the source-store cache ceiling remains a
  transient owned allocation.
- Renderer memory is not yet end-to-end bounded because Form traversal and
  global ownership discovery still enumerate the complete document.

## Rejected Alternatives

- Keep inherited-resource helpers on `lopdf::Document` until all renderer work
  can migrate in one change.
- Read selected-page source streams from the accumulated overlay.
- Cache materialized page contexts as durable job data.
- Flatten resources with string-based PDF object rewriting.
- Claim bounded-memory export before the remaining Form and ownership traversal
  is removed.
