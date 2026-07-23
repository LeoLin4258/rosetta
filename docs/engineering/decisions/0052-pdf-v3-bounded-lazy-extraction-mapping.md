# ADR 0052: PDF v3 Bounded Lazy Extraction Mapping

Date: 2026-07-18

Status: Accepted

Refines ADR 0016, ADR 0042, ADR 0045, ADR 0046 and ADR 0051.

Multi-page page-tree index reuse is refined by ADR 0053.

## Context

PDFium extraction was selected-page addressable, and production replacement
and export already used `PdfSourceObjectStore`, but `DocumentHandle` still
retained a complete `lopdf::Document` for content operand mapping. Its object
graph and decoded source structures grew with the complete PDF even when the
caller requested one page. This was the largest remaining source-owned memory
boundary before real 500/1,000-page validation.

Mapping also depended on borrowed lopdf page resources, Form streams and font
encodings. Removing only the document field without migrating those lifetimes
would either weaken ToUnicode/font decoding or recreate unbounded page state.

## Decision

`DocumentHandle` now owns:

- the streamed source fingerprint and byte count;
- one bounded `PdfSourceObjectStore` over a read-only memory map;
- one PDFium document over the same immutable source path;
- the verified cross-engine page count.

It no longer loads or retains a complete `lopdf::Document` or an all-page object
ID vector. Each exact page mapping resolves a transient one-page
`PdfPageIndex`, then materializes one owned `PdfPageObjectContext`. Content
streams, inherited resources, font dictionaries, ToUnicode streams and Form
XObjects are fetched through `PdfObjectView` only when that selected page
reaches them.

The existing page-local parsed-content cache remains the only parsed operation
cache. It is dropped after one page. The source-object LRU retains its existing
hard limits of 16 MiB, 512 objects and 4 MiB per cached object.

Font decoding preserves the existing order:

1. Rosetta's source-only ToUnicode decoder;
2. the conservative lopdf fallback for the already-approved encoding kinds.

The fallback owns only a materialized font dictionary, direct ToUnicode object
when present and an empty lopdf adapter document. It never receives the source
object graph. Translation font selection and encoding remain separate.

Top-level `/Contents` references that resolve to an indirect array are expanded
lazily in source order. Direct content streams without stable indirect identity
remain an explicit preservation boundary.

`PdfSourceObjectStore` now initializes the `pdf-rs` file resolver before it
reads raw trailer semantics. A second read-only memory map exists only during
trailer parsing so xref streams with an indirect `/Length` can be resolved.
Both mappings refer to the same OS-backed file pages; the transient map is
dropped before `open()` returns.

## Evidence

The complete PDF v3 suite passes with 147 tests and 14 intentional manual
ignores. The fixture corpus includes both an indirect `/Contents` array and
`pdflatex-image.pdf`, whose xref stream has an indirect length.

Three Windows AMD debug runs over pages 1-10 of `2305.13048v2.pdf` produced
39,783 atoms:

- total: 742-807 ms, median 761 ms;
- content mapping: 404-434 ms, median 416 ms;
- one-page index/context lookup: 4.6-5.1 ms aggregate;
- source loads/cache hits: 167 / 998;
- final source cache: 167 objects and 524,541 estimated bytes;
- page-local parsed-stream cache hits: 219.

The previous complete-document runs were 717-797 ms total and 373-415 ms for
mapping. The bounded lazy path is therefore in the same debug performance
range while removing source-object retention proportional to document size.

## Consequences

### Positive

- Selected-page extraction and reconciliation no longer require a complete
  lopdf source object graph.
- Source object retention has explicit byte and entry ceilings independent of
  document page count.
- Page resources, Form provenance, font inspection and source decoding share
  the same lazy object authority already used by production rendering.
- Exact page selection remains caller-controlled and one-based.

### Costs

- The compatibility single-page mapping API walks the selected page-tree path;
  multi-page callers use the explicit reusable `PageSet` index from ADR 0053.
- Active large streams can still create transient allocations while they are
  decompressed and parsed; the cache ceiling does not cap one active operation.
- ADR 0053 records synthetic 100/500/1,000-page process-memory and throughput
  runs; complex real-world corpus validation remains required.
- `pdf-rs` compatibility remains an adapter risk and must stay covered by the
  fixture corpus and typed failures.

## Rejected Alternatives

- Retain the complete lopdf document only for font decoding.
- Build and retain an all-page object ID index during document open.
- Add a document-wide parsed content or font-context cache without a measured
  hard retained-memory budget.
- Drop the lopdf fallback and accept lower source-font decoding coverage.
- Reimplement page resources and Form inheritance separately from the existing
  lazy page-context boundary.
