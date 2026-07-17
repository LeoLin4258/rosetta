# ADR 0036: PDF v3 TranslationPatch Renderer and Resolved Store Boundary

Date: 2026-07-18

Status: Accepted

Amends ADR 0033 and ADR 0034.

## Context

ADR 0033 made renderer decisions part of `patchId`, while ADR 0034 correctly
rejects different patch content at the same translation revision. Persisting a
pending patch before rendering would therefore require either weakening the
same-revision conflict rule or inventing a second durable draft protocol.

The low-level replacement renderer also accepted requests containing stream,
operation, source operand hash and geometry fields. Letting an orchestrator
reconstruct those requests independently would duplicate safety logic and risk
falling back to text search. A durable patch needs one conservative bridge to
that renderer, with page-level atomicity and explicit source preservation.

## Decision

`TranslationPatch` has two lifecycle roles:

- a fully pending patch is an ephemeral, in-process renderer draft;
- a fully fitted/preserved patch is the only durable translation authority.

The patch store rejects pending entries during commit, load and repair. It does
not implement a pending-to-resolved same-revision update and does not persist a
separate draft manifest. If the process stops before the resolved patch is
committed, that page revision is planned and rendered again from PageGraph and
translation inputs.

PageGraph schema v5 adds the exact text-show operator and full operand SHA-256
to atom source provenance. The `TranslationPatch` renderer derives low-level
requests only from this reconciled provenance. An entry currently renders only
when it completely covers one source text object and resolves to one stream,
Form invocation path, text show and PageGraph style.

Rendering follows one transaction boundary:

1. validate the pending patch against the current PageGraph;
2. classify incomplete entries as preserved;
3. group eligible entries by stream/path and source `BT`/`ET` object;
4. preflight source identity, anchors, geometry, style, font coverage and fit
   against the unchanged PDF;
5. resolve every entry and recompute `patchId` in memory;
6. apply all fitted targets through one atomic page-level replacement batch;
7. return the resolved patch only when the batch succeeds.

Stale source operator, operand hash or state is fatal and must leave document
objects and `max_id` unchanged. Unsupported structure, anchor, style, font or
fit is a stable entry/group preservation decision so unrelated safe entries can
still render. The default minimum fit scale is 0.9.

## Evidence

Automated Windows tests prove:

- a pending patch becomes a searchable PDF and receives a new resolved ID;
- one safe entry renders while an incomplete sibling preserves source text;
- an unsupported text-object boundary resolves to preservation with zero PDF
  mutation;
- a stale operand hash fails with every document object and `max_id` unchanged;
- patch-store commit rejects pending drafts and stores resolved patches.

A LibreOffice fixture probe replaced one row with `Unified patch renderer`.
Independent Poppler rendering at 150 DPI changed 6,846 pixels, 0.3145% of the
1241x1754 page, all inside the original first-row band. Visual inspection found
no clipping, overlap or later-line movement. Independent `pypdf` extraction
found the replacement text in the output.

## Consequences

### Positive

- Durable patch identity is immutable within one revision.
- The patch store remains simple and crash recovery has one authority format.
- Renderer safety fields come from PageGraph provenance, not string matching.
- Unsupported regions preserve source without blocking safe siblings.
- No resolved patch can be committed for a failed or partially applied batch.

### Costs

- A crash before commit repeats page planning and renderer preflight.
- Translation drafts are not independently resumable; durable page progress
  advances only after rendering decisions are complete.
- Current renderable entries must cover one complete source text object.
- Paragraph reflow, mixed-style shows and arbitrary-angle text remain preserved.

## Rejected Alternatives

- Persist pending patches and allow same-revision content replacement.
- Add a second durable draft store before the scheduler requires one.
- Exclude renderer decisions from `patchId`.
- Build low-level replacement requests by searching Unicode source text.
- Commit safe targets incrementally and update decisions after PDF mutation.
