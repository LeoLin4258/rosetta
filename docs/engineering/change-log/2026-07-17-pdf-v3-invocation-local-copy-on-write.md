# PDF v3 Invocation-Local Copy-on-Write

Date: 2026-07-17

The single-target boundary recorded here is amended by ADR 0030 and the
multi-target clone-tree change-log.

## Summary

Added invocation-local copy-on-write for one shared PDF v3 patch target. Shared
page streams and Form streams no longer require unconditional preservation when
the patch carries a validated structured invocation path.

The implementation remains isolated from PDF v2, jobs, UI, translation,
persistence, preview cache and export.

## Implementation

Added:

- PageGraph schema v3 structured `FormInvocationStep` provenance;
- structured path propagation through mapping, reconciliation and patches;
- copy-on-write triggers for cross-page page streams, selected-page shared
  Forms and cross-page resource-reachable Forms;
- validation of every parent stream, `Do` operation and resolved child Form;
- leaf-to-root stream cloning with collision-free XObject aliases;
- effective resource materialization for cloned Form boundaries;
- selected-page resource and `/Contents` rewiring;
- staged object IDs and atomic object-table/page commit;
- typed rejection for missing paths, invalid paths, repeated page content
  references and multi-target COW batches;
- support for valid uncompressed content streams through
  `get_plain_content()`.

## Windows AMD Results

Real 30-page fixture: `2305.13048v2.pdf`, page 1 Form stream `24 0`, with an
additional unused page 2 resource reference to force cross-page ownership.

- source size: 1,506,372 bytes;
- COW output size: 1,514,133 bytes;
- growth: 7,761 bytes, about 0.52%;
- PDFium page 1 text and pixels: exact;
- PDFium page 2 text and pixels: exact;
- Poppler page 1 PNG SHA-256: source/output identical;
- Poppler page 2 PNG SHA-256: source/output identical;
- visual review: no defects.

The synthetic shared-Form fixture confirms that two invocations become one
source invocation and one cloned invocation while the original Form bytes stay
unchanged. A shared page content fixture confirms that only the selected page
receives a new `/Contents` reference.

## Current Boundary

- one logical COW target is supported per atomic patch batch;
- multi-target clone-tree merging is not implemented;
- repeated same-stream page content references remain ambiguous;
- translated Unicode encoding, font embedding and fitting remain disconnected;
- bounded-memory incremental export is still pending.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- patch executor tests: 11 passed;
- `cargo test pdf_v3`: 46 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- PDFium selected/unselected page text and pixel identity: passed;
- Poppler selected/unselected page PNG SHA-256: exact;
- rendered page visual review: passed.
