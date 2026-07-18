# ADR 0053: PDF v3 Reusable PageSet Mapping Index

Date: 2026-07-18

Status: Accepted

Refines ADR 0045 and ADR 0052.

## Context

ADR 0052 removed the complete `lopdf::Document` from extraction and mapping,
but its compatibility path rebuilt a transient one-page page-tree index for
every mapped page. On a valid flat `/Pages/Kids` tree, resolving page N must
inspect every preceding kid to establish page order. Sequentially processing N
pages through that path therefore repeats the same traversal and approaches
O(N squared) source-object work.

Rosetta needs exact sparse-page control and bounded long-document processing.
It must avoid hidden document-wide parsed-content caches, but repeated page-tree
discovery is immutable structural work and can be shared safely for the exact
`PageSet` already selected by the caller.

## Decision

Add a `PageOperandMappingIndex` resolved once from a `DocumentHandle` and an
explicit `PageSet`. The index owns:

- the source fingerprint it was built against;
- one `PdfPageIndex` containing only selected page records, their page-tree
  ancestors and direct content-stream references.

Long-document reconciliation passes this index into each page mapping. Mapping
still materializes and drops the page resource context, parsed content streams,
PDFium snapshot and reconciled PageGraph one page at a time. No font context,
decoded stream or PageGraph becomes document-wide state.

The index rejects a different `DocumentHandle` with the typed
`IndexSourceMismatch` error before it resolves page context. Requests for pages
outside its original `PageSet` retain the existing typed `PageNotSelected`
failure. Mapping also verifies the snapshot's source-page hash against the
handle and returns `SnapshotSourceMismatch` for a cross-source snapshot. The
original single-page mapping API remains as a convenience wrapper and accounts
its one-page index construction in existing lookup timing.

## Evidence

A generated PDF fixture uses a valid worst-case flat `/Pages/Kids` array,
independent compressed page streams and one exact text show per page. A
500-page Windows AMD debug comparison, with the source cache already warm for
the repeated path, measured:

- one reusable all-page index: 16,914 us;
- 500 repeated single-page indexes: 756,893 us;
- repeated construction was 44.75 times slower.

Separate process runs then extracted, mapped and reconciled every page while
retaining no page snapshots or PageGraphs:

| Pages | Main loop | Extraction | Mapping | Reconciliation | Peak working set | Final source cache |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 51 ms | 18,119 us | 27,541 us | 5,041 us | 20,086,784 B | 204 entries / 171,334 B |
| 500 | 286 ms | 81,560 us | 172,404 us | 26,194 us | 22,069,248 B | 512 entries / 461,630 B |
| 1,000 | 681 ms | 181,747 us | 425,263 us | 60,096 us | 23,728,128 B | 512 entries / 521,630 B |

All pages produced exact one-to-one operand mappings and `Complete`
reconciliation. The 1,000-page run performed 3,005 source loads and 3,998 cache
hits while remaining under the fixed 512-entry / 16 MiB source-cache limits.

These are unoptimized synthetic-fixture diagnostics. They establish bounded
retained state and linear structural traversal for this class of input, not a
release-profile or complex-corpus throughput guarantee.

## Consequences

### Positive

- Sequential selected-page processing no longer rebuilds overlapping page-tree
  prefixes.
- The caller retains exact control over which one-based pages are indexed.
- Index identity cannot be confused across source documents.
- Page-local extraction, mapping and reconciliation state remains disposable.
- Retained index state scales with the explicit selection, not decoded document
  content.

### Costs

- Selecting every page retains one compact structural record per page for the
  handle lifetime chosen by the caller.
- Flat page trees still require one linear scan because their page kids have no
  subtree `/Count` values to skip.
- Callers that keep using the convenience single-page API do not receive the
  multi-page reuse benefit.
- Complex real-world 500/1,000-page corpus and end-to-end translation/export
  validation remain required.

## Rejected Alternatives

- Restore a complete lopdf document or all-page parsed object graph.
- Keep an implicit index for every source page regardless of the requested
  `PageSet`.
- Add a document-wide parsed-content or font-context cache.
- Accept repeated one-page tree traversal as negligible based on the 30-page
  fixture.
