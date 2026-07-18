# ADR 0051: PDF v3 Page-Local Content Mapping Cache

Date: 2026-07-18

Status: Accepted

## Context

After single-pass PDFium object extraction, low-level content operand mapping
became the largest measured part of PDF v3 page inspection. Recursive Form
invocations can reference the same physical content stream many times. The
mapping traversal previously decompressed and parsed that immutable stream for
every invocation, even though only invocation path, inherited state and
resource context vary.

The same path also called `lopdf::Document::get_pages()` for every selected
page. That is small on the 30-page fixture but repeats a complete page-tree walk
and scales poorly when hundreds of exact pages are requested.

## Decision

`DocumentHandle` captures the ordered lopdf page object IDs once when the
document opens. Exact one-based page lookup then indexes that immutable vector
in constant time. This index is transient and contains only object IDs.

Each `collect_text_shows()` call keeps a page-local map from physical stream
object ID to one immutable parsed `Content`. Recursive invocations reuse the
same parsed operations while independently replaying:

- graphics and text operator state;
- parent and Form resource precedence;
- structured invocation paths;
- text-show IDs and shared-stream classification.

The parsed-content cache is dropped when that page mapping finishes. It is not
stored in `DocumentHandle`, shared across pages or persisted. Cross-page cache
retention requires a separately measured hard memory budget before it can be
accepted.

SHA-256 identifiers now use preallocated lowercase hexadecimal encoding rather
than one formatting allocation per digest byte. Digest inputs and all emitted
IDs remain byte-for-byte unchanged.

## Evidence

On the first ten pages of `2305.13048v2.pdf`, the page-local cache recorded 219
reused stream decodes. Three Windows AMD debug runs completed in 717-797 ms;
content mapping took 373-415 ms. Page lookup fell from about 1,300 microseconds
in aggregate to 29-30 microseconds. The previous committed single-pass runs
were 784-874 ms total with 432-498 ms in mapping.

Recursive Form provenance, repeated mapping IDs, serialized privacy checks and
the fixture reconciliation corpus remain automated tests. A canonical `abc`
SHA-256 assertion locks the hexadecimal output format.

## Consequences

### Positive

- Repeated Form streams are decompressed and parsed once per selected page.
- Invocation-specific provenance and inherited state remain independent.
- Exact random page lookup does not repeatedly traverse the page tree.
- Cache memory is released after each page and cannot grow with document page
  count.

### Costs

- A heavily shared stream used on many different pages is parsed once per page.
- The page ID vector uses memory proportional to page count, at two integer
  fields per page object ID.
- Current evidence is debug-profile data from one fixture, not a full corpus or
  release-profile guarantee.

## Rejected Alternatives

- Reuse a Form's already-collected text shows without replaying invocation
  state and resources.
- Keep an unbounded document-wide parsed `Content` cache.
- Persist lopdf object IDs or parsed operations in PageGraph.
- Continue walking the complete page tree for every selected page.
