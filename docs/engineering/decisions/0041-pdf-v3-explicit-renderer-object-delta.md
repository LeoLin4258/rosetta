# ADR 0041: PDF v3 Explicit Renderer Object Delta

Date: 2026-07-18

Status: Accepted

Amends ADR 0039 and ADR 0040.

## Context

ADR 0040 established a bounded incremental writer, but its real-paper proof
still derived changed objects by comparing every object in a fully rendered
`lopdf::Document` with the complete source object graph. That comparison made
mutation ownership implicit, required both graphs to remain available, and
could hide an unrelated renderer mutation inside the final export delta.

The low-level replacement implementation already staged font objects, rewritten
content streams, copy-on-write Form chains and the selected page dictionary
before committing any mutation. PDF v3 needed to expose that existing atomic
stage as the final export contract instead of reconstructing it afterward.

## Decision

PDF v3 introduces `PdfObjectDelta` as the only indirect-object mutation set
accepted by the incremental export writer.

A delta contains:

- a sorted map keyed by exact object number and generation;
- the maximum object number after its allocations.

Construction rejects object zero, generation 65535, multiple generations for
one object number and a maximum below any contained object. Merge preflights the
complete incoming delta before changing the accumulator. Equal values for the
same exact object ID are idempotent; different values are a typed conflict.

Document font preparation now has an immutable staging entry point that returns
`DocumentTranslationFontRegistry` plus its six-object-per-face delta. The
TranslationPatch renderer has a corresponding immutable staging entry point
that returns the resolved render result plus the page delta. Its existing
mutation APIs remain wrappers: stage the complete transaction, then explicitly
apply the returned delta to their owned working document.

A multi-page export session:

1. stages and applies the document font delta;
2. stages one page against the current read view;
3. applies that page delta to the temporary read view needed by later pages;
4. atomically merges the page delta into the export accumulator;
5. passes the accumulator directly to the incremental writer.

The writer no longer accepts an arbitrary raw object map. It accepts
`PdfObjectDelta`, so object validation and allocation identity cannot be skipped
by a final-export caller.

`PdfObjectDelta` is transient sensitive process memory. It is not patch-store
authority, is not serialized independently, and must not enter ordinary logs.

This change removes full-graph comparison, not full-graph loading. The current
temporary read view remains a complete `lopdf::Document`. A lazy source-object
reader and bounded overlay are still required for end-to-end bounded memory.

## Evidence

Automated tests prove:

- delta construction and application update only explicit objects and `max_id`;
- idempotent object values merge, while conflicting values fail before mutation;
- existing replacement atomicity and failure tests remain unchanged;
- a 30-page export stages one six-object font delta and two page deltas;
- the merged export delta contains exactly 10 objects without source comparison;
- the output reopens with `lopdf` and PDFium and contains both translations.

The real-paper output remains byte-for-byte sized as before this ownership
change: 1,617,258 bytes from a 1,590,242-byte source, with a 27,016-byte appended
incremental section.

Poppler comparison at 150 DPI found 2,559 changed pixels on page 1 and 2,059
on page 2, confined to the two translated footer rows. Page 3 was pixel-exact.
Independent `pypdf` inspection retained all 30 pages, source metadata and page
1-3 annotation counts of 26, 31 and 7. Each translation was extractable only
from its intended page. Visual inspection found no clipping, overlap or
unrelated movement.

## Consequences

### Positive

- Final export can enumerate every mutation without scanning the source graph.
- Renderer preflight is immutable by type and commit remains explicit.
- Unrelated or conflicting mutations cannot silently enter a multi-page export.
- Font, page and writer ownership now compose through one narrow contract.
- The next lazy-reader implementation can replace the temporary read view
  without changing delta or writer identity.

### Costs

- Applying a delta to the current full working document clones its changed
  objects; this remains bounded by delta size but is temporary duplication.
- Sequential pages still need an overlay read view for earlier allocations.
- `lopdf::Object` remains an internal implementation type at this native layer;
  it is not exposed to persistence or frontend APIs.

## Rejected Alternatives

- Continue comparing complete source and rendered object graphs.
- Let the incremental writer accept any caller-provided `BTreeMap<ObjectId,
  Object>`.
- Mutate the document during preflight and try to reconstruct failures.
- Persist the low-level object delta as translation authority.
