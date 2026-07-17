# ADR 0034: PDF v3 Sharded Atomic TranslationPatch Store

Date: 2026-07-17

Status: Accepted

Amends ADR 0015 and ADR 0033.

## Context

ADR 0033 fixed the durable page-patch schema but deliberately left file
ownership and crash recovery open. PDF v3 needs to commit translated pages
independently, reject stale retries, recover interrupted writes and keep only
current patch authority without recreating full page PDFs.

A first implementation stored every page pointer in one generation-numbered
language manifest. It was correct, but every page commit re-serialized, hashed
and atomically replaced an index that grew with the whole document. A Windows
AMD debug probe committing 1,000 tiny page patches took 51.54 seconds. The final
manifest was only 319,983 bytes, showing that repeated whole-index writes, not
payload size, caused the scaling problem.

## Decision

Each source document and target language uses one isolated patch store. The
target-language directory name is a SHA-256 of the exact language identity, so
untrusted language text cannot become a filesystem path.

The store contains:

```text
translations/
  language-<sha256>/
    manifest.json
    shard-00000000.json
    shard-00000001.json
    page-0000000001-revision-00000000000000000001-patch-<sha256>.patch.json
```

`manifest.json` is a small stable identity record containing schema version,
source fingerprint, exact target language, deterministic manifest ID and the
fixed shard width. It does not contain every page.

Page authority is indexed in deterministic 64-page shards. Each shard contains
its own schema, source/language identity, shard index, generation, deterministic
shard ID and source-ordered page entries. A page entry records page/source hash,
translation revision, patch ID, immutable patch filename and exact byte count.

The 64-page width is only an internal index bound. It is not a translation
batch, scheduler window, PageSet restriction or user-visible chunk. Arbitrary
pages remain independently addressable and commits in one shard do not require
loading patch payloads from other pages.

Patch files are immutable and revision/content addressed. A commit:

1. validates the patch against its current PageGraph;
2. rejects lower revisions and same-revision content conflicts;
3. writes and `sync_all`s the immutable patch through a unique temp file;
4. updates only the owning shard using temp + backup + rename;
5. removes the replaced patch after the new shard is durable.

Store operations for the same absolute language directory are serialized by a
shared in-process coordinator. Different store handles therefore cannot lose a
page update. Cross-process writers are not supported; the native PDF
orchestrator remains the single store owner.

At first access after process start, repair scans manifest/shard canonical,
temp and backup candidates. It selects the highest valid shard generation,
preferring canonical state on ties. A newer complete temp can supersede an old
canonical shard. Missing or corrupt patch files remove only their own page
entries; other pages remain loadable. Structurally corrupt shards are dropped
as derived state and their pages become eligible for translation again.
Unreferenced revisions, incomplete patch temps and orphan patch files are
garbage-collected. Later normal commits read only the root manifest and owning
shard; they do not re-read every historical page patch.

## Evidence

Automated tests cover:

- compact commit/load without ordinary source text in store files;
- stale revision, same-revision conflict and idempotent commit behavior;
- repair of a corrupt current patch from a valid idempotent commit;
- highest-generation temp promotion over an older canonical shard;
- page-local recovery when one patch is missing;
- filename/internal shard identity mismatch rejection;
- concurrent same-shard commits without lost updates;
- target-language path traversal containment and absolute-root enforcement.

Two revised Windows AMD debug probes committed 1,000 independently synced page
patches in 15.54-16.40 seconds, about 3.1-3.3 times faster than the rejected
central manifest design. The final probe produced 16 shards, 323,244 logical
index bytes and 615,572 patch payload bytes. This is a persistence stress
measurement, not extraction or translation latency.

## Consequences

### Positive

- Per-page commit cost is bounded by one patch and one at-most-64-entry shard,
  rather than total document page count.
- A crash or corrupt page does not invalidate translation authority for the
  rest of a long document.
- Old revisions and failed commits do not accumulate indefinitely.
- Patch storage remains text-scale and does not duplicate PDF fonts, images or
  page resources.
- The store is page-addressable without exposing fixed chunk semantics.

### Costs

- A language store has several small shard files instead of one page index.
- Startup repair scans shard and patch filenames once per process/store.
- The current coordinator assumes one Rosetta process owns a job store.
- Atomic replace uses a backup sidecar on Windows because standard rename does
  not replace an existing destination there.
- Render cache, patch-to-renderer integration and streaming export remain
  pending Phase 4 work.

## Rejected Alternatives

- Rewrite one whole-document manifest after every page commit.
- Keep one mutable `page-0001.patch` and let newer writers overwrite it.
- Use fixed 10-page translation chunks as the persistence boundary.
- Retain every patch revision indefinitely.
- Treat one missing patch as corruption of the entire target-language store.
