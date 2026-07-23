# PDF v3 Reusable DocumentHandle

Date: 2026-07-17

## Summary

Added the first reusable PDF v3 `DocumentHandle` and changed the combined
PageGraph reconciliation path from repeated whole-document loads to one shared,
read-only engine lifetime.

The implementation remains isolated from PDF v2, jobs, UI, translation,
persistence, render cache and export.

## Implementation

The handle now:

- reads and fingerprints one immutable source byte snapshot;
- parses one read-only `lopdf::Document`;
- transfers the same owned byte snapshot into one read-only PDFium document;
- rejects encrypted input before page work;
- verifies that PDFium and `lopdf` report the same page count;
- exposes source identity, byte size, page count and open timing;
- supports repeated exact-page extraction, mapping and reconciliation without
  reopening the document.

Path-based extraction and mapping functions remain as single-operation
convenience wrappers. Combined reconciliation opens one handle and passes it to
both stages. Mutable identity/content-stream probes keep independent documents
because they intentionally rewrite or save state and must not mutate the
read-only handle.

## Windows AMD Results

On `2305.13048v2.pdf` (1,590,242 bytes, 30 pages), one debug probe reported:

- handle open: about 101 ms;
- page 1 reconciliation using the open handle: about 658 ms;
- page 3 reconciliation using the same handle: about 691 ms;
- page 1 atoms/status: 3,911 / `partial`;
- page 3 atoms/status: 3,391 / `partial`.

The page results and conservative fallback states are unchanged. Per-page debug
timings varied between runs. This slice
removes repeated document initialization, but it does not make per-page
reconciliation cheap: PDFium character/object traversal and source mapping are
still the dominant debug-build costs. Timings are directional and not release
benchmarks.

## Remaining Boundaries

- `lopdf` still parses the complete source into memory. This is not the final
  bounded-memory architecture for 500/1000-page export.
- Extraction and mapping still traverse PDFium page text/objects separately.
- Form XObject content and inherited resources are not recursively reconciled.
- No translated Unicode is encoded or rendered.
- No PageGraph or patch persistence is connected.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 32 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- reused-handle sparse page test: passed for pages 1 and 3;
- explicit Windows reused-handle timing probe: passed;
- existing PDFium/Poppler identity and fixture visual tests: passed.
