# ADR 0035: PDF v3 Bounded Render Cache

Date: 2026-07-17

Status: Accepted

Amends ADR 0015 and ADR 0034.

## Context

PDF v3 keeps the source PDF plus page `TranslationPatch` files as translation
authority. Preview PNGs and complete translated page PDFs are expensive to
regenerate, but retaining an unbounded copy for every page recreates the disk
growth and long-document failure mode that the rewrite is intended to remove.

The cache must remain useful for a document with hundreds or thousands of
pages without making startup, memory use or metadata rewrites proportional to
artifact payload size. Windows also requires explicit ownership while a file
is open and careful replacement semantics.

## Decision

PDF v3 uses an isolated, versioned render cache under `render-cache/v1`. It is
disposable derived state. A cache miss, corrupt entry or complete cache loss
must be recoverable from the source PDF and current `TranslationPatch`.

Every key binds:

- source fingerprint and 1-based page number;
- patch ID and positive translation revision;
- renderer version;
- output kind (`previewPng` or `translatedPagePdf`);
- raster width and/or fixed-point scale when the output is a preview.

The canonical JSON key is SHA-256 addressed. Artifact filenames contain only
the key hash, content hash and fixed extension, so source, language and model
identities never become path components. The artifact content SHA-256 and byte
count are stored in the index and revalidated when bytes are consumed.

The default policy is a hard 384 MiB artifact quota and at most 4,096 entries.
Both values are configurable when the cache is opened. Absolute implementation
guards cap a cache at 16 GiB and 16,384 entries. One process may open a root
with only one configuration at a time.

Metadata is split into 64 deterministic hash shards. This is not a page range,
PageSet, scheduler window or UI chunk. Each shard is independently capped at
1 MiB and carries a generation plus deterministic shard ID. A hit updates the
owning shard's logical access sequence; eviction orders entries by that
sequence and key ID, giving deterministic LRU behavior without loading
artifact bodies.

Insertion runs under a shared in-process coordinator. It rejects one artifact
larger than the whole quota, evicts inactive LRU entries before writing, writes
the new content through a unique temp plus `sync_all` and rename, then commits
only affected index shards through temp + backup + rename. Content-addressed
artifacts are never overwritten in place.

Opening an entry returns a lease. A leased key cannot be replaced or evicted.
This makes active file ownership explicit and avoids relying on platform file
deletion behavior. Reading a lease verifies the exact length, content hash and
basic output signature before returning bytes. A failed verification removes
only that entry.

First access performs repair using metadata and file sizes, not artifact
payload reads. Missing artifacts remove only their entries. A structurally
invalid shard removes only that shard. Orphan artifacts, interrupted artifact
temps and index sidecars are deleted. If the configured quota or entry count
shrinks, repair evicts oldest entries until both limits hold. Explicit repair
is rejected while leases are active.

The native orchestrator remains the single process owner. Cross-process cache
writers are not supported.

## Evidence

Automated tests cover:

- content-addressed round trips and key-dimension isolation;
- byte-quota and entry-count enforcement with true LRU touches;
- active lease protection and release;
- oversized and mislabeled artifact rejection;
- same-size checksum corruption and missing-file isolation;
- corrupt-shard isolation plus orphan/temp cleanup;
- concurrent insert, read and eviction through shared handles;
- 1,000 page inserts with bounded artifact count, quota and index bytes;
- absolute-root and same-process configuration ownership.

The 1,000-page Windows AMD test retains only the configured 128 newest entries,
keeps artifact bytes below 128 KiB and keeps all logical index bytes below
1 MiB. This synthetic test validates cache scaling and durability mechanics;
it is not a renderer throughput measurement.

## Consequences

### Positive

- Complete translated page files can no longer grow without a fixed bound.
- Long documents do not require loading cached PDF/PNG bodies during startup or
  eviction.
- One corrupt page artifact or shard does not invalidate unrelated pages.
- Active preview/export readers have explicit lifetime protection.
- A newer patch or renderer version cannot reuse stale render bytes.

### Costs

- Cache hits atomically update one small metadata shard.
- The 384 MiB quota covers artifact payloads; bounded manifests and shard files
  add a small amount of disk overhead.
- Startup repair lists cache-owned files once per process/root.
- The cache does not remove the need for streaming final export.

## Rejected Alternatives

- Keep one full translated PDF per page without a quota.
- Treat cached translated pages as the durable translation result.
- Put all entries in one manifest and rewrite it on every preview hit.
- Use filesystem modification times as the cache contract.
- Evict an artifact while a reader is using it and rely on Unix semantics.
- Share one writable cache across multiple Rosetta processes.
