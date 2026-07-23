# ADR 0019: PDF v3 Recursive Form XObject Provenance

Date: 2026-07-17

Status: Accepted

Shared-stream preservation in this ADR is amended by ADR 0022 and ADR 0029.
Validated shared mappings now retain invocation provenance and can enter
invocation-local translated replacement.

## Context

ADR 0018 intentionally preserved all Form XObject text because PageGraph only
paired top-level PDFium text objects with top-level page content streams. That
boundary was safe but incomplete:

- Form XObjects can contain text-show operations and nested Forms;
- a Form may define its own resources or use fonts from its invocation context;
- one Form stream can be invoked multiple times with different geometry;
- PDFium exposes invocation-specific child objects, while the PDF contains one
  shared encoded operand;
- treating a visual invocation as an independent operand would make a local
  patch silently modify every use of the shared Form.

## Decision

PDF v3 recursively traverses Form XObjects on both sides of reconciliation.

### PDFium traversal

The page snapshot walks top-level objects and Form child objects depth-first.
Nested text-object IDs append each stable Form child index to the root page
object ID. PDFium character indexes continue to bind PageGraph atoms to the
resulting source object IDs.

### Content-stream traversal

The source mapper follows `Do` operators in operation order. An indirect Form
stream is entered at the exact parent stream and operation index that invoked
it. A Form resource dictionary has priority; missing categories fall back to
the parent invocation context. Text-show IDs include a stable hash of the full
invocation path.

Operand IDs remain tied to the underlying stream object, operation and operand.
They do not pretend that repeated visual invocations contain independent source
bytes.

### Shared streams

Each Form invocation is counted separately. If a Form content stream containing
text is invoked more than once, its text shows are marked as shared and remain
source-preserved until the renderer implements graph-aware copy-on-write or a
validated whole-stream patch policy.

Repeated Forms without text do not block unrelated text mapping.

### Safety limits

- recursive traversal is limited to 32 Form levels;
- reference cycles are detected and preserved;
- direct Form streams without an indirect object identity are preserved;
- PDFium/content invocation count mismatches remain typed fallbacks;
- Form existence alone is no longer a fallback reason.

## Evidence

Page 1 of `2305.13048v2.pdf` now reports:

- 258 PDFium text objects and 258 source text shows;
- 27 Form invocations;
- 5 unique Form streams;
- 4 shared nested Form streams;
- 16 text objects inside the one non-shared top-level Form;
- 242 mapped objects and 16 preserved Type3 objects;
- no ordinal mismatch and no generic Form fallback.

The 16 Form text objects use a Type3 font without a safe source decoder. They
remain preserved with `text-show-decode-unavailable`. Existing top-level
coverage is unchanged.

`GeoTopo.pdf` has one Form with no text-show operators. Its three top-level text
shows remain preserved because their fonts cannot be decoded; Form traversal is
not the cause of the fallback.

## Consequences

### Positive

- Form text is visible to diagnostics and reconciliation in true invocation
  order.
- Nested resource ownership and inherited font lookup are explicit.
- Repeated visual instances cannot be mistaken for independent patch targets.
- Unsupported Form details no longer force unrelated top-level text to be
  preserved.
- Typed fallback reasons identify decoder, shared-stream, cycle, direct-stream
  and depth failures separately.

### Costs

- Pages with substantial Form content perform more real parsing than the old
  top-level-only probe.
- Font/resource inspection is scoped per text-bearing stream.
- The renderer still needs copy-on-write before it can safely patch one
  invocation of a shared Form.
- No current fixture contains a decodable, non-shared Form text run that proves
  successful translated-text insertion; identity rendering remains the next
  gate.

## Rejected Alternatives

- Keep every page with a Form permanently partial.
- Flatten Form text into top-level ordinal order without invocation provenance.
- Give each invocation a fake independent operand ID.
- Patch shared Form bytes and accept changes to all visual instances.
- Recurse without cycle and depth limits.
