# PDF v3 Shared PDFium Page Snapshot

Date: 2026-07-17

## Summary

Removed the duplicate PDFium page-text and page-object traversal from the
combined PDF v3 PageGraph reconciliation path.

Extraction now creates one short-lived `PdfiumPageSnapshot`. Mapping consumes
the same snapshot instead of opening and inspecting the PDFium page again. The
implementation remains isolated from PDF v2, jobs, UI, translation,
persistence, render cache and export.

## Implementation

The in-memory snapshot contains:

- the extracted PageGraph;
- page object and Form XObject counts;
- stable text-object indexes and source object IDs;
- object text, font name, mapped atom count and Unicode atom count;
- first and last mapped atom order.

Snapshot source text is required only for local reconciliation. The snapshot is
not serializable, is not a persistent model and is not emitted through mapping
diagnostics or telemetry. Mapping diagnostics continue to serialize hashes,
counts and provenance without object text payloads.

Path-based standalone mapping still creates its own snapshot. The combined
reconciliation path creates one snapshot, uses it for content-operand mapping,
and then moves its PageGraph into atomic reconciliation.

## Windows AMD Results

The test source was `2305.13048v2.pdf` (1,590,242 bytes, 30 pages). Before this
slice, one debug run reported about 658 ms for page 1 and 691 ms for page 3 with
an already-open `DocumentHandle`.

After sharing the page snapshot, three repeated debug runs reported:

- page 1: 429-477 ms;
- page 3: 452-555 ms;
- handle open: 89-102 ms.

These numbers are directional debug-build measurements, not release
benchmarks. PageGraph and reconciliation results remained unchanged:

- page 1 atoms: 3,911;
- verified: 3,238;
- ToUnicode-corrected: 15;
- synthetic whitespace: 602;
- preserved Form XObject atoms: 56;
- mapped top-level objects: 242 / 242;
- status: `partial`.

## Remaining Boundaries

- Form XObject content and inherited resources are not recursively inspected or
  reconciled.
- `lopdf` still parses the complete source into memory.
- Per-page ToUnicode/resource inspection is not cached across pages.
- No translated Unicode is encoded or rendered.
- No PageGraph or patch persistence is connected.

## Validation

- `cargo fmt -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 33 passed, 0 failed, 7 ignored;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- page snapshot object/atom coverage test: passed;
- explicit real-page reconciliation probe: passed;
- repeated Windows reused-handle timing probe: passed;
- existing identity and fixture pixel tests: passed.
