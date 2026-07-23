# PDF v3 Atomic Content Patch Executor

Date: 2026-07-17

## Summary

Added the first provenance-addressed PDF v3 content patch executor. It applies
validated byte-range replacements to selected page or Form text operands while
preserving all-stream atomicity and rejecting shared-stream mutations that need
copy-on-write.

The implementation remains isolated under `pdf_v3`. It is not connected to PDF
v2, jobs, UI, translation, persistence, preview cache or export.

## Implementation

Added:

- page, stream, operation, operand, `TJ` item and byte-range addressing;
- expected complete operand byte count and SHA-256 preconditions;
- bounds, overlap and conflicting-source-identity validation;
- cloned stream staging and one commit after all streams succeed;
- direct page content sharing detection;
- exact selected-page Form invocation sharing detection;
- conservative cross-page Form resource-reachability detection;
- typed incomplete-ownership failures for direct Forms, cycles and depth;
- safe result metrics without source or replacement payloads.

The first cross-page ownership implementation parsed content recursively for
every page. It was replaced with a `/Resources/XObject` graph walk that does not
decompress unselected page content streams.

## Windows AMD Results

Fixture: page 1 of `2305.13048v2.pdf`, unique Form stream `24 0`.

- patches: 1;
- modified streams: 1;
- replaced source bytes: 1;
- replacement bytes: 1;
- previous executor time: about 1,418 ms;
- optimized executor time: 28-30 ms;
- PDFium text: exact;
- PDFium changed pixels: 0.

The optimization is about 47-51 times faster for this targeted patch. A prior
independent Poppler validation rendered source and output PNGs with identical
SHA-256 hashes.

## Safety Coverage

Automated tests verify:

- unique Form identity patching preserves text and pixels;
- selected-page shared Forms require copy-on-write;
- cross-page resource-reachable Forms require copy-on-write;
- hash mismatch, overlap and out-of-bounds ranges leave the document unchanged;
- a failure in the second stream leaves the first stream unchanged.

## Remaining Boundaries

- shared-stream copy-on-write is not implemented;
- translated Unicode encoding, font embedding and fitting are not implemented;
- resource reachability is conservative and can reject unused declarations;
- the executor still uses a whole in-memory `lopdf::Document`;
- bounded-memory incremental export remains a production gate.

## Validation

- targeted patch executor tests: passed;
- targeted unique Form identity timing: passed at 28-30 ms;
- PDFium text and pixel identity: passed;
- independent Poppler source/output PNG SHA-256: exact match;
- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 42 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed.
