# PDF v3 Recursive Form XObjects

Date: 2026-07-17

## Summary

Added recursive PDFium and source content-stream traversal for Form XObjects.
Form existence is no longer treated as an automatic PageGraph fallback.

The implementation remains isolated from PDF v2, jobs, UI, translation,
persistence, render cache and export.

## Implementation

Added:

- depth-first PDFium Form child-object snapshots;
- stable nested source object IDs;
- recursive `Do` traversal in content operation order;
- Form-owned resource lookup with parent-context fallback;
- invocation-qualified text-show IDs;
- unique and shared Form stream accounting;
- source preservation for text in shared Form streams;
- reference-cycle, direct-stream and 32-level depth guards;
- typed Form and decoder fallback reasons;
- recursive object/show alignment and provenance tests.

Mapping diagnostics advanced to schema
`rosetta-pdf-v3-page-operand-mapping/2`. They still contain no source text or
encoded byte payloads.

## Windows AMD Results

Page 1 of `2305.13048v2.pdf`:

- PDFium/source text objects: 258 / 258;
- Form invocations: 27;
- unique Form streams: 5;
- shared Form streams: 4;
- total inspected content streams: 7;
- mapped objects: 242;
- preserved Form Type3 objects: 16;
- verified atoms: 3,238;
- ToUnicode-corrected atoms: 15;
- synthetic whitespace atoms: 601;
- preserved atoms: 57;
- fallback: `text-show-decode-unavailable`;
- page status: `partial`.

The previous generic `form-xobject-requires-recursive-mapping` fallback is gone.

On the reused `DocumentHandle`, three debug runs measured:

- page 1: 462-529 ms for 258 text shows;
- page 3: 742-882 ms for 481 text shows;
- handle open: 96-119 ms.

The page 3 cost reflects newly inspected Form content that the previous path did
not process. These are directional debug-build timings, not release benchmarks.

## Remaining Boundaries

- Type3 source decoding is not implemented.
- Shared Form text requires renderer copy-on-write or whole-stream validation.
- The identity content-stream renderer does not yet recurse into Forms.
- `lopdf` still parses the complete source into memory.
- No translated Unicode is encoded or rendered.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 34 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- real-page recursive Form mapping test: passed;
- real-page reconciliation probe: passed;
- fixture corpus reconciliation matrix: passed;
- existing identity and fixture pixel tests: passed.
