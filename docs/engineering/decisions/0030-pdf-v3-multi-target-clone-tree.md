# ADR 0030: PDF v3 Multi-Target Clone Tree

Date: 2026-07-17

Status: Accepted

Amends ADR 0022 and ADR 0029. ADR 0031 adopts this clone tree for translated
replacement batches.

## Context

Invocation-local copy-on-write originally accepted one logical target. Cloning
each additional target independently would be incorrect and wasteful:

- two targets can share the same page root and several nested Form ancestors;
- one target can patch an ancestor stream while another patches its descendant;
- a transaction can contain direct targets that fall under a root already
  being cloned;
- committing each path separately would overwrite page rewiring, duplicate
  ancestors or expose partial output after a later failure.

The renderer therefore needs one structural transaction model rather than a
loop around the single-path API.

## Decision

The low-level patch executor builds one clone forest for all copy-on-write
targets on a selected page.

Each node is identified by:

- the source top-level page content stream; and
- the exact structured `FormInvocationStep[]` prefix leading to the node.

The empty prefix identifies a page-content root. Two targets with the same
prefix share that node; two invocations of one stored Form with different `Do`
steps remain separate nodes even though their source object ID is identical.

All targets whose root is already entering copy-on-write are folded into that
root tree. This prevents a sibling or ancestor target from mutating source bytes
that the cloned tree is expected to isolate.

The executor validates every target path before document mutation, then stages
nodes in deterministic deepest-first order:

1. use the target's already validated patched stream when the node is a direct
   target, otherwise clone the source stream;
2. attach target-local resource bindings to the effective Form resources;
3. redirect every targeted child `Do` to its staged child ID;
4. allocate one object ID for the completed node;
5. merge all root replacements into one selected-page `/Contents` rewrite.

Root `Do` aliases and top-level resource bindings share one materialized page
resource dictionary. Nested aliases use the owning cloned Form resource
dictionary. Multiple roots are committed through the same cloned page object.

All cloned streams, the selected page and `Document.max_id` are committed only
after the complete forest succeeds. A failure in any target leaves every source
object and `max_id` unchanged.

The existing single-target staging function remains as a wrapper over the batch
API. Its callers and object-allocation semantics remain compatible.

## Evidence

Automated Windows tests prove:

- two invocations of one Form under one page root produce two leaf clones and
  one common root clone;
- two leaf invocations below a shared parent Form produce four clones total:
  root, parent and two leaves, rather than six independent-chain clones;
- source root, parent and leaf streams remain byte-unchanged;
- PDFium re-extraction preserves the complete page text;
- an invalid second path leaves every document object and `max_id` unchanged.

ADR 0031 additionally proves translated targets across two Form invocations and
across mixed Form/top-level roots. Both cases reuse one unified font subset and
commit all page rewiring atomically.

The nested identity fixture produced:

- source PDF: 13,987 bytes;
- output PDF: 16,597 bytes;
- growth: 2,610 bytes;
- Poppler source/output PNG SHA-256:
  `BE7F5890978B1653E060E029568FAEF5786BC363017D35D9C67F606EA232EBA2`;
- visual review: no clipping, overlap, missing text or layout change.

## Consequences

### Positive

- Clone cost follows the union of targeted paths rather than the sum of each
  path length.
- Multiple root rewrites cannot overwrite one another's page dictionary.
- Ancestor and descendant targets compose on the same staged stream.
- Invalid later targets cannot expose partial clones.
- The tree key preserves visual invocation identity without inventing new
  source operand identities.

### Costs

- Staging materializes the effective resources for every cloned Form node.
- Every target path is validated independently before prefix merging.
- The implementation still operates on one selected page and one in-memory
  `lopdf::Document`.
- Each translated target transaction still exposes one invocation path, while
  ADR 0031 now groups multiple targets through this batch staging API.

## Rejected Alternatives

- Call the single-target API repeatedly and keep the last page dictionary.
- Merge nodes only by source stream object ID.
- Clone every complete path independently.
- Mutate non-COW targets in place when their root is already being cloned.
- Commit each completed root before validating the remaining targets.
