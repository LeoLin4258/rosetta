# PDF v3 Multi-Target Translated Replacement

Date: 2026-07-17

## Summary

Connected translated text-show replacement to the multi-target clone-tree
executor. One selected page can now atomically replace text across independent
content streams and Form invocation paths while sharing unified font subsets.

The implementation remains isolated from PDF v2, jobs, UI, durable
`TranslationPatch`, preview cache and export.

## Implementation

Added:

- typed page-level translated replacement batch requests and results;
- unchanged-source planning for every target before mutation;
- one-page and unique `stream + invocation path` batch gates;
- deterministic target ordering and batch-wide Regular/Bold face unioning;
- one staged font subset per required weight across all targets;
- clone-tree staging for the complete batch whenever any target requires COW;
- atomic rewiring of multiple page `/Contents` roots;
- one-target compatibility wrapping for the existing transaction API;
- text-free batch `/1` and batch-target `/1` diagnostics;
- mixed Form/top-level ownership and zero-mutation failure coverage.

## Windows AMD Results

Shared Form, two translated invocations:

- replacements: 2;
- cloned streams: 3 (one root plus two leaves);
- staged font objects: 6, reused by both targets;
- elapsed time: about 4 ms;
- source size: 13,129 bytes;
- output size: 17,044 bytes;
- growth: 3,915 bytes;
- PDFium and `pypdf` CJK text extraction: passed;
- Poppler visual review: no clipping, overlap or unrelated page changes.

Mixed Form and independent top-level target:

- replacements: 2;
- cloned streams: 3 (Form root, Form leaf and top-level root);
- both original page `/Contents` references replaced atomically;
- source root, Form and top-level streams unchanged;
- invalid second target left every object and `max_id` unchanged;
- unselected sibling Form source text remained present.

## Current Boundary

- all targets in one batch must belong to one selected page;
- each target remains one stream/path and one `BT`/`ET` text object;
- duplicate stream/path targets are rejected;
- unanchored consecutive shows and paragraph reflow remain preserved;
- durable patch persistence and bounded-memory export remain pending.

## Validation

- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- replacement tests: 12 passed, 2 ignored manual probes;
- `cargo test pdf_v3`: 73 passed, 0 failed, 10 ignored manual probes;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- Source Han PDFium/`pypdf` text extraction: passed;
- Poppler rendered-page visual review and pixel-bound check: passed.
