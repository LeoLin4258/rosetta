# ADR 0048: PDF v3 Bounded Lazy Stream Ownership Index

Date: 2026-07-18

Status: Accepted

Amends ADR 0047.

## Context

ADR 0047 moved selected-page and Form copy-on-write staging onto lazy object
views, but replacement planning still accepted a complete `lopdf::Document`.
For every top-level target it called `Document::get_pages()` and
`get_page_contents()` to determine whether mutating the source stream would
affect another page.

That boundary made renderer memory depend on the complete source object graph.
Building the same ownership answer separately for every page would also make a
long export repeat the full page-tree scan and approach quadratic work.

## Decision

PDF v3 introduces a transient `PdfStreamOwnershipIndex` over
`PdfObjectView`. The caller provides the exact stream object IDs relevant to an
export. The index retains one of three states per target:

- `Unreferenced`;
- `UniqueToPage(pageNumber)`;
- `SharedAcrossPages`.

It does not retain page dictionaries, resource dictionaries, content bytes or
one page-number set per target. State memory therefore grows with the explicit
target set, not with the number of pages that reference each target.

The index walks the page tree from trailer `/Root` and catalog `/Pages`, checks
declared and actual page counts, and reads direct page `/Contents` references.
It rejects malformed node types, kids, counts, cycles, repeated `/Pages`
ownership, excessive depth and page-number overflow.

Ordinary content targets do not cause any unselected content stream to be
loaded or decompressed. A target whose own stream dictionary declares
`/Subtype /Form` also receives conservative resource reachability analysis:

- page effective `/Resources/XObject` names are inspected;
- nested traversal follows only each Form's locally declared XObject names;
- source streams are inspected as dictionaries and are never content-decoded;
- direct Forms, explicit resource cycles, more than 32 nested levels and more
  than 4,096 Form visits on one page return typed ownership failures.

Multi-page export builds one index from the selected `PdfPageIndex` content
roots and reuses it for every page stage. Compatibility mutation APIs may build
a temporary index over `Document` through its `PdfObjectView` adapter. The
production replacement and TranslationPatch staging interfaces no longer
accept `Document`.

Legacy operand-range patch compatibility helpers keep their old complete
document ownership scan. They are not part of the PDF v3 TranslationPatch
production renderer and must not be used by the long-document scheduler.

## Evidence

Automated Windows AMD tests prove:

- direct content streams distinguish unique and cross-page ownership;
- direct and nested Form resource reachability distinguishes unique and
  cross-page ownership;
- ordinary targets do not load unrelated page content or Form streams;
- malformed page counts and explicit Form resource cycles return typed errors;
- a 1,000-page synthetic tree retains exactly two requested ownership states;
- existing shared top-level and shared/nested Form COW regressions are
  unchanged;
- a real 30-page, two-page staging proof reuses one ownership index with a
  12-entry / 2 MiB source cache, performs 51 source loads and 30 hits, and ends
  with 12 resident objects / 29,181 estimated bytes.

The complete PDF v3 suite passes 136 tests with 13 manual probes ignored. A
Poppler 144 DPI render of the translated probe changes 6,383 pixels
(0.318252%), confined to `(115, 119)-(1015, 140)`. `pypdf` retains one page and
identical metadata and extracts the translated first line plus all preserved
source text.

## Consequences

### Positive

- Production page staging no longer needs a complete PDF object graph.
- Cross-page ownership state is bounded by explicit export targets.
- Multi-page export can pay for one global page-tree scan instead of one scan
  per page.
- Unselected page content streams are not decompressed for ownership.
- Shared top-level streams still use page-local copy-on-write.
- Malformed ownership remains a typed failure instead of risking destructive
  in-place mutation.

### Costs

- Unique ownership requires reading every page dictionary once for the export.
- Form resource ownership is deliberately conservative and may require COW for
  a resource that page content never invokes.
- The source object LRU may reload page-tree objects under a very small cache,
  but resident entries and bytes remain hard-bounded.
- Export planning must collect target content roots before page staging.
- Scheduler queues, leases, recovery and 500/1,000-page end-to-end job stress
  remain Phase 6 work.

## Rejected Alternatives

- Keep `Document::get_pages()` only for ownership discovery.
- Build a new ownership map independently for every rendered page.
- Retain every page dictionary or every referencing page number in the index.
- Decompress every page content stream to prove actual Form invocation.
- Mutate a stream in place when ownership analysis is incomplete.
