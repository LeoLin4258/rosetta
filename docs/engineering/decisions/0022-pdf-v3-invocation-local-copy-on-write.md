# ADR 0022: PDF v3 Invocation-Local Copy-on-Write

Date: 2026-07-17

Status: Accepted

ADR 0029 connects this copy-on-write primitive to translated text-show
replacement and defines font-resource staging at cloned boundaries.
ADR 0030 replaces the single-target batch restriction with deterministic
multi-target clone-tree merging.

## Context

ADR 0021 introduced atomic operand-range patching but rejected page content
streams shared across pages and Form streams shared across invocations or page
resource graphs. That protected source ownership, but it also prevented a
translation patch from targeting one visual invocation of shared content.

The old provenance was insufficient to solve this safely. A stream object and
operand location identify stored bytes, not the visual invocation that should
change. `textShowId` included a hash of the invocation path, while the explicit
path was serialized as display strings. Neither representation could validate
and rewire each parent `Do` operation without parsing an internal string format.

Copying only the leaf Form is also insufficient. Every shared ancestor must be
isolated up to one selected page content stream, and the resource alias used by
the root `Do` belongs to the page resource dictionary rather than the page
content stream.

## Decision

PDF v3 adopts PageGraph schema v3 and structured Form invocation provenance.

Each `FormInvocationStep` records:

- parent stream object number and generation;
- parent `Do` operation index;
- child Form stream object number and generation.

Mapping, text-show inspection, reconciled atom provenance and low-level content
patches carry the same ordered structure. Hash-based text-show IDs remain
stable identifiers, but renderer correctness no longer depends on decoding
those IDs or parsing display strings.

### Copy-on-write trigger

The atomic patch executor uses copy-on-write when:

- a top-level page content stream is referenced by another page;
- a Form has multiple actual invocations on the selected page; or
- a Form is resource-reachable from more than one page.

A Form that needs copy-on-write must provide a structured invocation path. The
executor validates every parent stream, operation index, `Do` operator,
resource name, resolved child object, Form subtype, chain continuity, selected
page root and 32-level depth limit before committing anything.

### Clone chain

For one logical shared invocation target, the executor:

1. patches a cloned leaf stream;
2. walks the invocation path from leaf to root;
3. clones every parent stream;
4. gives each cloned parent a collision-free Rosetta XObject alias pointing to
   the cloned child;
5. materializes effective inherited resources so existing font, graphics state,
   color, pattern and XObject lookup remains valid;
6. writes the root alias into the selected page resource dictionary;
7. replaces exactly one selected page `/Contents` reference with the cloned
   root stream.

The source leaf, source ancestors, other Form invocations and other pages remain
unchanged.

New object IDs, cloned streams and the selected page dictionary are staged in
memory. `Document.max_id` and the object table change only after operand
validation, path validation, stream encoding, compression and page rewiring all
succeed. An invalid path leaves the complete document object table unchanged.

The original executor accepted one logical copy-on-write target per patch
batch. ADR 0030 now defines transaction-wide clone-tree merging for multiple
low-level targets.

All PDF v3 content parsing now uses `get_plain_content()` so valid uncompressed
content streams and filtered streams share the same path.

## Evidence

Automated identity tests prove:

- two invocations of one Form become one original invocation and one cloned
  invocation;
- a page content stream shared across pages is rewired only on the selected
  page;
- a Form merely reachable from another page resource graph clones the selected
  invocation chain;
- source streams remain byte-unchanged;
- invalid invocation provenance leaves `max_id` and every document object
  unchanged;
- uncompressed synthetic page content is accepted.

On page 1 of `2305.13048v2.pdf`, Form stream `24 0` was also declared in page 2
resources to exercise cross-page ownership. The identity COW result:

- cloned one leaf plus its invocation ancestors;
- preserved PDFium text and pixels on pages 1 and 2 exactly;
- produced byte-identical Poppler PNG SHA-256 values for pages 1 and 2;
- increased the 30-page PDF from 1,506,372 to 1,514,133 bytes;
- added 7,761 bytes, about 0.52%, instead of duplicating a page or document.

Visual inspection found no clipping, overlap, font, citation, formula, table or
layout differences. Poppler emitted the same local display-font warnings for
source and output.

## Consequences

### Positive

- Shared stored bytes can support invocation-local translated replacement.
- PageGraph provenance directly describes the renderer ownership path.
- Unselected pages and sibling Form invocations remain isolated.
- Resource inheritance is materialized explicitly at cloned boundaries.
- COW remains part of the all-stream atomic transaction.
- Storage growth follows the cloned stream chain rather than full page PDFs.

### Costs

- PageGraph schema advances from 2 to 3; isolated beta-derived IR is rebuilt.
- One COW target can clone multiple small ancestor streams and resource
  dictionaries.
- Multi-target low-level patch batches use the transaction-wide clone tree
  defined by ADR 0030.
- Repeated references to one top-level page content stream on the same page are
  still ambiguous and rejected.
- The executor still uses a whole in-memory `lopdf::Document`.
- Identity COW does not prove translated Unicode encoding, font embedding or
  fitting.

## Rejected Alternatives

- Parse invocation identity back out of `textShowId` hashes.
- Persist Form paths as renderer-parsed display strings.
- Clone only the leaf Form while leaving shared ancestors unchanged.
- Put the root XObject alias on the page content stream instead of the page.
- Modify shared resource dictionaries in place.
- Allocate objects into the document before all validation succeeds.
- Implicitly merge multiple COW targets without an explicit clone-tree model.
