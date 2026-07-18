# ADR 0047: PDF v3 Lazy Form Invocation and Copy-on-Write

Date: 2026-07-18

Status: Accepted

Amends ADR 0046.

## Context

ADR 0046 moved selected-page dictionaries, inherited resources and source
stream reads onto `PdfObjectView`, but Form invocation validation and
copy-on-write clone-tree staging still re-entered the complete
`lopdf::Document`. The renderer used `Document` to decode every invocation
parent, resolve Form XObjects, materialize effective resources, clone source
streams, rewrite page `/Contents` and allocate clone IDs.

Those operations are local to one selected page and one explicit structured
invocation path. They do not require a global page or Form ownership scan. The
complete-document dependency therefore obscured a bounded local operation and
mixed immutable source reads with accumulated allocation identity.

## Decision

PDF v3 extends the owned page context with `PdfResourceContext`. It starts from
the selected page's materialized resources and derives each invoked Form's
effective resources by overlaying the Form's own `/Resources` on its parent
context. The closest scope wins while missing category entries inherit from the
parent.

Direct and indirect resource dictionaries, category dictionaries and XObject
streams resolve through immutable `PdfObjectView`. Reference chains are limited
to 64 hops and reject cycles. Invalid resource shapes and unresolved invocation
targets fail before clone allocation.

Production Form COW staging accepts only:

- immutable source objects;
- accumulated source-plus-delta identity;
- the selected `PdfIndexedPage`;
- its `PdfPageObjectContext`;
- explicit targets and the already reserved object-number boundary.

Root `/Contents` identity comes from the page index. Invocation parent and Form
streams come from the source view. The selected page dictionary and effective
page resources come from the page context. Clone IDs allocate consecutively
above the greater of the accumulated view maximum and reserved boundary. The
stage returns cloned streams and one rewritten page dictionary without mutating
either view.

The legacy `Document` API remains a compatibility wrapper. It builds a one-page
index and page context, then calls the same narrow staging function with the
document as both object views.

Global cross-page stream/Form ownership discovery is not changed by this ADR.
It remains the final major production renderer dependency on a complete
`lopdf::Document`.

## Evidence

Automated Windows AMD tests prove:

- Form resource scope overrides the parent while unspecified font entries are
  inherited;
- indirect Form resources, indirect category dictionaries and indirect Form
  XObjects resolve through the object view;
- cyclic Form resource references are rejected;
- a nested repeated-Form source stages the same page dictionary and four cloned
  streams through the lazy source store and the `Document` adapter;
- clone IDs start exactly above the source/accumulated maximum;
- mismatched page number/index/context identity is rejected before target work;
- exhausted object-number space returns a typed failure before staging output;
- the nested lazy stage performs 8 source loads, 11 cache hits and retains 8
  entries / 11,272 estimated bytes;
- source loads and resident entries remain below 32, and resident bytes remain
  below 16 MiB;
- existing shared Form, nested Form, mixed Form/top-level and atomic failure
  regressions remain unchanged.

A translated repeated-Form fixture renders one page through PDFium and Poppler.
The 13,129-byte source becomes 24,244 bytes with one shared font and three COW
clones. Poppler confines 12,747 changed pixels (0.585607%) to the two translated
rows at `[118, 1057) x [125, 229)`. `pypdf` retains the page count and metadata
and extracts only `ALPHA` and `BETA` from the translated output. Visual review
found no clipping, overlap or unrelated movement.

## Consequences

### Positive

- Form invocation and clone-tree staging memory is bounded by the selected
  paths, resource dictionaries and staged clones rather than source object
  count.
- Form source reads cannot accidentally observe accumulated page deltas.
- Clone allocation cannot collide with unapplied font or earlier page deltas.
- Page and Form resource precedence share one owned, typed implementation.
- Production replacement staging no longer needs `Document` for any local page
  or Form traversal.
- The remaining complete-document boundary is isolated to global ownership
  discovery.

### Costs

- Effective resource dictionaries are cloned for each validated invocation
  depth in the active target paths.
- Direct Form XObjects remain unsupported for COW because they have no stable
  indirect object identity.
- Compatibility operand-patch APIs still build their page index/context from a
  complete `Document`.
- End-to-end export memory is not yet bounded while global ownership discovery
  enumerates the complete document.

## Rejected Alternatives

- Keep Form staging on `Document` until global ownership discovery also moves.
- Resolve Form streams from the accumulated overlay instead of immutable source
  identity.
- Allocate clone IDs by probing candidate object numbers one at a time.
- Cache effective Form resource contexts as durable job data.
- Treat direct Form XObjects as if they had an indirect COW identity.
