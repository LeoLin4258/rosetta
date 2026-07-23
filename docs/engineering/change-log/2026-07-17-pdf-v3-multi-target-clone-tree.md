# PDF v3 Multi-Target Clone Tree

Date: 2026-07-17

## Summary

Replaced the low-level single-target copy-on-write restriction with a
deterministic clone forest for multiple page/Form invocation targets.

The implementation remains isolated from PDF v2, jobs, UI, translated batch
planning, persistent `TranslationPatch`, preview cache and export.

## Implementation

Added:

- batch copy-on-write targets carrying stream, invocation path, staged bytes
  and typed resource bindings;
- clone node identity based on root page stream plus invocation path prefix;
- deepest-first staging with one clone per shared prefix;
- automatic folding of direct targets beneath a root already entering COW;
- composition of ancestor stream patches with descendant `Do` redirects;
- one materialized page resource dictionary for all root aliases;
- atomic replacement of multiple selected-page `/Contents` roots;
- zero-mutation rejection when any later target path is invalid;
- compatibility wrapper for existing single-target callers.

Removed the old parallel single-path staging implementation and the
`CopyOnWriteBatchUnsupported` error.

## Windows AMD Results

Two invocations directly below one root:

- cloned streams: 3 (one root plus two leaves);
- original root and Form bytes: unchanged;
- PDFium page text identity: passed.

Two invocations below a shared nested parent:

- cloned streams: 4 (root, parent and two leaves);
- independent-path equivalent: 6 streams;
- source size: 13,987 bytes;
- output size: 16,597 bytes;
- growth: 2,610 bytes;
- PDFium text identity: passed;
- Poppler source/output PNG SHA-256: identical;
- visual review: no defects.

An invalid second target left every object and `max_id` unchanged.

## Current Boundary

- targets must belong to one selected page;
- structured invocation paths remain mandatory for shared Forms;
- repeated same-stream references in one page `/Contents` remain ambiguous;
- translated replacement transactions still target one stream/path;
- durable patch persistence and bounded-memory export remain pending.

## Validation

- patch executor tests: 14 passed;
- replacement tests: 10 passed, 2 ignored manual probes;
- `cargo test pdf_v3`: 71 passed, 0 failed, 10 ignored manual probes;
- PDFium nested multi-target text identity: passed;
- Poppler PNG SHA-256 identity: passed;
- rendered page visual review: passed.
