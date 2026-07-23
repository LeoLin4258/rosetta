# ADR 0029: PDF v3 Invocation-Local Translated Replacement

Date: 2026-07-17

Status: Accepted

Amends ADR 0019, ADR 0022, ADR 0027 and ADR 0028.

ADR 0030 adds multi-target clone-tree merging to the low-level executor. ADR
0031 adopts that executor for page-level translated replacement batches. The
single-target boundary described here remains the invariant inside each batch
target, not the page-level renderer limit.

## Context

PDF v3 had separately proven two required capabilities:

- anchored text-show transactions could embed unified Regular/Bold subsets and
  replace translated text atomically, but only in a unique top-level page
  content stream;
- the low-level patch executor could isolate one shared page/Form target with
  invocation-local copy-on-write, but only identity bytes used that path.

Keeping those paths disconnected meant valid text inside a Form or a page
content stream shared across pages still had to remain original. Reconciliation
also treated `SharedContentStream` as an unconditional fallback, which could
mask a more specific decoder, font or atom-coverage failure.

Translated replacement introduces one additional resource constraint. The
Rosetta Type0 font must be visible from the rewritten stream without modifying
a source resource dictionary that is still shared by sibling invocations or
unselected pages.

## Decision

Shared ownership is a renderer capability, not a reconciliation failure.
Mapping applies decode, source-font identity and atom-coverage gates first. Only
an otherwise valid mapping may receive `SharedContentStream`, and reconciliation
retains its complete structured invocation provenance.

One replacement transaction must target the same 1-based page, underlying
stream, complete `FormInvocationStep[]` path and `BT`/`ET` text object. Existing
operand hash, source text state, paint/style, geometry, fit, face-selection and
position-anchor gates remain unchanged.

The renderer uses three ownership paths:

1. A unique top-level page content stream is rewritten in place. All required
   unified font resources are attached to the selected page dictionary.
2. A top-level stream referenced by another page is cloned. Only the selected
   page `/Contents` entry is rewired, and its page resources receive the fonts.
3. Every Form target, including a currently unique Form, uses invocation-local
   copy-on-write. The rewritten leaf Form materializes its effective inherited
   resources and receives the fonts; every ancestor is cloned and exactly one
   selected-page invocation is rewired.

Font object IDs are reserved first in deterministic weight order. Clone stream
IDs continue after the font reservation. Font objects, rewritten/cloned
streams, resource dictionaries, the selected page and `Document.max_id` are
committed once, only after all validation and staging succeeds.

The reusable copy-on-write stage accepts typed resource bindings rather than a
font-specific API. Identity operand patches pass an empty binding set and keep
their prior behavior.

Per-show diagnostics advance to
`rosetta-pdf-v3-text-show-replacement/6` and add
`formInvocationDepth`. Transaction diagnostics advance to
`rosetta-pdf-v3-text-show-replacement-transaction/3` and add
`formInvocationDepth`, `clonedStreamCount` and `pageContentRewired`. Diagnostics
must not include source or translated text.

## Evidence

Automated Windows tests prove:

- a decodable Form invoked twice on one page can translate only the second
  invocation;
- the original Form bytes and sibling invocation remain unchanged;
- the cloned leaf owns the Rosetta font resource and PDFium extracts the
  translated text only from the selected invocation;
- an invalid `Do` path leaves the complete document object table and `max_id`
  unchanged even after font staging;
- a top-level content stream shared by two pages is cloned and rewired only on
  page 1, while page 2 still references the source stream and extracts the
  source text.

The Source Han Form probe used fit scale 1.0, cloned two streams in about 4 ms
and produced a 16,564-byte searchable output. Poppler differences were confined
to the selected second line, with no clipping, overlap or later-text movement.

The shared-page probe used fit scale 1.0 and cloned one stream. Poppler changes
were confined to the selected line on page 1; page 2 was pixel-exact.

## Consequences

### Positive

- Shared Forms and cross-page page streams no longer force valid translated
  text to remain original.
- The selected visual invocation is isolated without duplicating a full page
  or document.
- Unified translation fonts are attached at the resource boundary that owns
  the rewritten stream.
- Specific mapping failures remain visible instead of being hidden by shared
  ownership.
- Identity and translated mutations reuse one atomic copy-on-write primitive.

### Costs

- Every Form translation clones its invocation chain, even when current
  ownership appears unique.
- One target transaction still handles only one invocation path, one underlying
  stream and one `BT`/`ET`; ADR 0031 groups multiple target transactions into an
  atomic selected-page batch.
- Repeated same-stream references in one page `/Contents` remain ambiguous.
- The current implementation still owns a whole in-memory `lopdf::Document`;
  bounded-memory export remains Phase 4 work.
- Paragraph reflow, protected spans and durable `TranslationPatch` persistence
  remain disconnected.

## Rejected Alternatives

- Treat every shared mapping as an unconditional reconciliation fallback.
- Attach Form translation fonts to the page and rely on inherited lookup.
- Mutate the source Form resource dictionary in place.
- Clone only Forms that currently appear shared.
- Allocate clone objects before the font object range is known.
- Commit staged fonts before validating the invocation path.
- Translate all invocations of a shared Form together.
