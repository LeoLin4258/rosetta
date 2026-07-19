# ADR 0074: PDF v3 Visible-Page-First Scheduling

Date: 2026-07-19

Status: Accepted

Refines ADR 0049, ADR 0066, ADR 0070 and ADR 0072.

## Context

The native PDF v3 scheduler deliberately keeps only bounded page windows and
walks an exact PageSet through persistent extraction and translation cursors.
The workbench already knew the first page actually inside the virtualized
viewport, but run creation discarded that information. A user viewing page 200
therefore waited for lower requested pages even though all pages had identical
durability and recovery requirements.

Adding a mutable frontend priority queue would duplicate scheduler authority,
grow with long documents and make restart ordering ambiguous. Reprioritizing on
every scroll event would also turn ordinary preview navigation into durable
manifest write traffic.

## Decision

Extend the trusted creation command with one optional `preferredPageNumber`.
The workbench supplies the first page actually intersecting the viewport only
when it is part of the selected translation PageSet; otherwise it supplies the
first selected page. Native creation rejects zero, out-of-range or unrequested
preferred pages before exposing a run.

During hidden staged creation, before any worker can claim a lease, the
scheduler rotates its existing extraction and translation cursors to the page
immediately before the preferred page. The ordinary circular PageSet traversal
then claims the preferred page first and continues in canonical order with
wraparound. This operation is allowed only while every requested page remains
pending and no page has a lease, artifact, patch, preservation or failure.

The preferred page is a one-time scheduling hint, not durable page authority.
No new queue, manifest field, status field or recovery rule is added. Once work
starts, pause, resume, cancellation, retry and stale-owner recovery continue to
use the ordinary persisted cursors and page shards. Scrolling during an active
run does not rewrite scheduling state.

## Consequences

- A newly started run produces the page the user is looking at before lower
  requested pages, reducing perceived first-translation latency without doing
  less validation or rendering work.
- Sparse and single-page PageSets use exactly the same bounded traversal.
- Long-document memory and disk complexity remain unchanged because priority
  reuses two existing cursor values.
- Priority follows the viewport only at run creation. Dynamic reprioritization
  remains intentionally excluded until measured user behavior justifies the
  additional durable control semantics.

## Rejected Alternatives

- Persist a frontend-owned ordered page array or priority queue.
- Add a second scheduler queue alongside the canonical PageSet and cursors.
- Rewrite scheduler priority continuously while the user scrolls.
- Prioritize a visible page that the user did not select for translation.
