# PDF v3 Bounded Render Cache

Date: 2026-07-17

## Summary

Added the isolated PDF v3 render-cache contract and implementation. Preview
PNGs and translated page PDFs are now modeled as disposable, content-addressed
artifacts with hard quota, deterministic LRU eviction, integrity checks and
active-use protection.

## Implementation

Added:

- source/page/patch/revision/renderer/options-bound cache keys;
- fixed output kinds for preview PNG and translated page PDF;
- SHA-256 key/content addressing without user-controlled path components;
- a 384 MiB and 4,096-entry default policy with absolute safety bounds;
- 64 fixed hash-index shards with generations and deterministic IDs;
- atomic artifact writes and Windows-compatible atomic index replacement;
- in-process shared coordination across cache handles;
- explicit leases that prevent replacement and eviction while in use;
- logical LRU touches without loading artifact bodies;
- page-local length, checksum and output-signature validation;
- first-access repair, quota shrink enforcement and orphan/temp cleanup;
- concurrent and 1,000-page bounded-growth tests.

The 64 index shards distribute key metadata. They do not represent pages or
constrain PageSet, translation scheduling or UI ranges.

## Current Boundary

- the cache is isolated inside PDF v3 and is not connected to legacy PDF state;
- source PDF plus `TranslationPatch` remain the only translation authority;
- no renderer or preview command writes cache artifacts yet;
- patch-to-renderer orchestration and streaming final export remain pending;
- no PDF bytes changed, so the existing Poppler visual baseline remains valid.

## Validation

- render-cache tests: 13 passed, 0 failed;
- 1,000-page bounded metadata/disk stress test: passed;
- complete PDF v3 suite: 103 passed, 11 ignored manual probes;
- `rosetta_jobs`: 78 passed, 0 failed;
- `cargo check`, Rust formatting and whitespace checks: passed.
