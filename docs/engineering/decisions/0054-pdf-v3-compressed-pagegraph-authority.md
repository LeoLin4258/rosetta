# ADR 0054: PDF v3 Compressed PageGraph Authority

Date: 2026-07-18

Status: Accepted

Refines ADR 0034, ADR 0049 and ADR 0053.

## Context

The durable scheduler could represent an extracted page only through an exact
artifact ID and source-page hash, but no PageGraph artifact store existed. Its
recovery inventory therefore used test-only placeholder authorities. Connecting
an extraction worker directly to that placeholder would allow scheduler state
to claim that extraction was durable when no independently validated PageGraph
could be loaded.

Raw PageGraph JSON is also not an acceptable long-document format. The first
ten pages of the real-paper fixture serialize to 37,855,795 bytes because exact
per-atom geometry, style and operand provenance are intentionally retained.
Writing that representation directly would recreate the disk-growth problem
that PDF v3 is intended to remove.

## Decision

Add a source-owned `PageGraphStore` with one immutable compressed artifact per
page. Its manifest binds:

- source fingerprint and page count;
- native engine version;
- PageGraph schema version;
- the fixed 64-page index-shard width.

PageGraphs serialize directly through a 64 KiB buffered, byte-limited JSON
writer into deterministic gzip with zero timestamp and the fast compression
level. Both compressed and uncompressed representations have independent
64 MiB per-page limits. The artifact ID is SHA-256 over the complete compressed
bytes, and its filename contains the exact one-based page number and digest.

Loading validates, in order:

1. compressed byte limit and artifact digest;
2. gzip integrity and decompressed byte limit;
3. PageGraph JSON/schema/page identity;
4. source-page hash derived from the store source fingerprint;
5. reconciled status, unique atom identity/order and style references;
6. exact manifest-shard metadata and byte counts.

Artifact files are the extraction authority. The 64-page JSON shards are
bounded indexes and can be rebuilt by scanning and validating one artifact at
a time. Invalid artifacts are removed and cannot enter a recovery inventory.
Index rebuilds do not retain PageGraphs across pages.

Add a narrow `PdfV3ExtractionWorker` above the scheduler, document handle and
store. Construction verifies the scheduler, handle and store source, page
count, engine and schema identities, then resolves one reusable index for the
scheduler's exact `PageSet`.

The sequential worker claims one page lease at a time. It reconciles that page,
commits and validates the PageGraph artifact, then commits the resulting
authority to the scheduler before claiming another page. A deterministic
reconciliation failure becomes non-retryable; a store failure is retryable.
If scheduler commit fails after artifact commit, recovery can promote the
validated artifact without repeating extraction.

No Tauri command or frontend protocol is added in this slice.

## Evidence

The automatic tests cover:

- deterministic compressed commit/load and idempotent commit;
- three sparse pages across three index shards;
- source identity rejection;
- corrupt artifact removal and recommit;
- I/O failure propagation during idempotent recommit;
- a real one-page worker flow where durable artifact commit precedes scheduler
  extraction authority;
- reconstruction of a validated scheduler extraction inventory.

A Windows AMD debug probe ran the complete durable extraction worker over pages
1-10 of `2305.13048v2.pdf`:

| Measurement | Result |
| --- | ---: |
| Total worker batch | 3,432 ms |
| Scheduler claims | 161,345 us |
| Native extraction/mapping/reconciliation | 722,923 us |
| PageGraph store total | 2,357,095 us |
| Streaming JSON + gzip | 2,116,120 us |
| Scheduler commits | 159,069 us |
| Logical PageGraph JSON | 37,855,795 B |
| Compressed artifacts | 3,323,703 B |
| Store directory | 3,327,910 B |
| Scheduler directory | 4,034 B |

The artifacts use 8.78% of the raw JSON size. Gzip level 6 produced smaller
2,010,386-byte artifacts but increased the batch to 5,314 ms. The fast level is
selected because the additional 1.31 MB for ten complex pages is preferable to
about 1.88 seconds of extra debug processing.

These are unoptimized diagnostics with synchronous file durability. They are
not release-profile or complete translation/export measurements.

## Consequences

### Positive

- Scheduler `extracted` state now refers to a real, reloadable content
  authority.
- Source, engine, schema, page and artifact identities are checked at every
  boundary.
- PageGraph disk usage is reduced substantially without dropping provenance.
- Recovery validates one page at a time and remains bounded in memory.
- Extraction no longer needs a fixed ten-page durable chunk.
- A crash after artifact commit but before scheduler commit does not require
  re-extraction.

### Costs

- Durable storage still adds about 2.36 seconds to the measured ten-page debug
  path; raw PageGraph serialization remains the dominant cost.
- The current JSON schema is verbose before compression. A compact versioned
  PageGraph disk schema remains a valid future optimization if corpus evidence
  shows that release performance is insufficient.
- A complete recovery inventory must read and validate every stored artifact.
- The store has per-page limits but no independent document-wide quota; its
  expected durable size grows with the explicitly selected pages.
- Translation workers, TranslationPatch inventory assembly, Tauri lifecycle
  commands and UI status are still separate Phase 6 work.

## Rejected Alternatives

- Commit scheduler extraction state without a durable PageGraph artifact.
- Persist raw uncompressed PageGraph JSON.
- Use gzip level 6 after measuring its synchronous latency.
- Hash the 37.86 MB uncompressed JSON after compression; deterministic
  compressed bytes provide the same content-addressed integrity with far less
  hashing work.
- Make rendered page PDF or PNG files extraction authority.
- Expose an external command before the store and worker contracts stabilize.
