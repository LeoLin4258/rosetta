# ADR 0021: PDF v3 Provenance-Addressed Atomic Content Patching

Date: 2026-07-17

Status: Accepted

Shared-stream rejection boundary partially superseded by ADR 0022.

## Context

ADR 0018 established atom-to-operand byte-range provenance, and ADR 0020 proved
recursive identity rewriting. Neither decision defined the mutation boundary
for a real patch. Directly editing a located byte range would still allow:

- stale provenance to modify a different source operand;
- overlapping patches to produce order-dependent output;
- a later stream failure to leave earlier streams committed;
- one Form invocation patch to alter every invocation of a shared stream;
- a Form used by another page to change outside the requested PageSet.

The first ownership implementation avoided the last case by recursively parsing
all page content streams. On the 30-page paper fixture, one one-byte identity
patch spent about 1,418 ms in the executor, mostly proving that the Form was not
used by another page.

## Decision

PDF v3 applies low-level text changes through a provenance-addressed,
all-stream atomic operand-range executor.

Each patch identifies:

- the 1-based page;
- stream object number and generation;
- operation and operand indexes;
- optional `TJ` array index;
- encoded byte start and length;
- expected complete operand byte count and SHA-256;
- replacement bytes.

The executor groups patches by stream and operand. It validates page identity,
stream reachability, source length, source hash, bounds, source-identity
agreement and non-overlap before committing anything. Ranges are applied in
descending byte order to keep earlier offsets stable.

Every affected stream is decoded into a clone, patched, encoded and compressed
in memory. The document object table is updated only after every affected
stream succeeds. Any error leaves all source streams unchanged.

Replacement payloads are transient native data. Results and diagnostics expose
counts, hashes, stream IDs and elapsed time, never replacement bytes or source
operand bytes.

### Ownership policy

- A direct page content stream referenced by multiple pages requires
  copy-on-write.
- Form invocation count on the selected page is exact because the selected
  page content graph is decoded. More than one invocation requires
  copy-on-write.
- A selected-page-unique Form is then checked against every page's
  `/Resources/XObject` graph without decoding unselected page content streams.
- If the Form is resource-reachable from more than one page, it conservatively
  requires copy-on-write even when another page may not execute its `Do`.
- Direct Form streams, reference cycles and graphs deeper than 32 levels make
  ownership incomplete and reject the patch.

The resource walk expands each page resource context and each indirect Form's
own resource dictionary once per page. Parent resources are already covered by
the parent walk, so inherited lookup remains conservatively represented without
recursively re-expanding all sibling Forms.

## Evidence

On page 1 of `2305.13048v2.pdf`, an identity patch against unique Form stream
`24 0` modifies one source byte in one stream:

- previous all-page content parsing: about 1,418 ms;
- resource-reachability ownership check: 28-30 ms;
- executor speedup: about 47-51 times;
- PDFium extracted text: exact;
- PDFium changed pixels: 0.

Targeted tests also prove:

- a shared selected-page Form requires copy-on-write;
- an otherwise unique Form listed in another page resource graph requires
  copy-on-write;
- hash mismatch, overlap and out-of-bounds ranges do not mutate the document;
- failure in a later stream leaves every earlier stream unchanged.

An independent Poppler identity render of the targeted Form patch produced the
same PNG SHA-256 as the source render.

## Consequences

### Positive

- PageGraph provenance now has a guarded native mutation boundary.
- Multi-stream patches have transaction-like commit semantics.
- Unselected pages no longer have their content streams decompressed merely to
  prove Form ownership.
- Shared-object hazards are explicit typed outcomes, not silent side effects.
- Patch diagnostics remain safe for privacy-sensitive documents.

### Costs

- Resource reachability can reject a Form that another page declares but never
  invokes.
- Shared page streams and shared Forms still require a future copy-on-write
  implementation.
- The current executor still operates on an in-memory `lopdf::Document`; it is
  not the bounded-memory long-document exporter.
- Identity replacement does not prove translated Unicode encoding, font
  embedding, fitting or protected-span restoration.

## Rejected Alternatives

- Parse every page content stream for exact global invocation counts.
- Commit each stream immediately after it is patched.
- Trust byte offsets without a complete operand length and hash.
- Resolve overlapping ranges by caller order.
- Mutate shared Form streams and accept changes to other invocations.
- Treat resource reachability as proof that a Form is safe to mutate.
