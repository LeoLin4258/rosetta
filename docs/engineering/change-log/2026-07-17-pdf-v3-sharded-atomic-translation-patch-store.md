# PDF v3 Sharded Atomic TranslationPatch Store

Date: 2026-07-17

## Summary

Added the first PDF v3 disk authority for page translations: immutable
revisioned patch files, bounded page-index shards, atomic Windows-compatible
replacement, crash recovery and orphan cleanup.

## Implementation

Added:

- source-fingerprint and target-language-bound store manifests;
- SHA-256 language directory identities that cannot traverse paths;
- deterministic 64-page index shards with independent generations and IDs;
- immutable page/revision/patch-ID-addressed patch filenames;
- stale revision, same-revision conflict and source-page conflict rejection;
- idempotent commits, including repair of a corrupted current patch;
- temp file `sync_all`, backup/rename replacement and interrupted-write repair;
- highest-generation candidate selection across canonical/temp/backup shards;
- page-local invalid-patch removal without losing unaffected pages;
- startup cleanup of incomplete temps, orphan patches and superseded revisions;
- shared in-process store coordination for concurrent page commits;
- compact snapshots for scheduler/status consumers without loading patch bodies.

The 64-page shard width is an internal index implementation detail. It does not
constrain PageSet, scheduling, translation batches or user-visible page ranges.

## Performance Decision

The rejected whole-manifest prototype took 51.54 seconds to commit 1,000 tiny
page patches and ended with a 319,983-byte manifest.

The accepted sharded store took 15.54-16.40 seconds across two Windows AMD
debug probes. The final run produced 16 shards, 323,244 logical index bytes and
615,572 patch payload bytes. Each page patch and owning shard was independently
synced. The improvement is about 3.1-3.3x while retaining page-level crash
durability.

## Current Boundary

- the store is isolated in the native PDF v3 module and is not connected to
  legacy PDF job state;
- patch-to-renderer orchestration, bounded render cache and streaming export
  remain pending;
- this stage does not alter PDF rendering bytes, so the existing Poppler visual
  baseline remains applicable.

## Validation

- patch-store tests: 8 passed, 1 ignored manual probe;
- TranslationPatch tests: 7 passed, 0 failed;
- explicit 1,000-page Windows AMD probe: passed;
- complete PDF v3 suite: 90 passed, 11 ignored manual probes;
- `rosetta_jobs`: 78 passed, 0 failed;
- `cargo check`, Rust formatting and whitespace checks: passed.
