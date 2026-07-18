# ADR 0049: PDF v3 Durable Sharded Long-Document Scheduler

Date: 2026-07-18

Status: Accepted

## Context

The legacy PDF run model exposes a fixed ten-page chunk as durable state and
owns full-page translated PDF artifacts. That model cannot provide bounded
memory, page-exact control or patch-first recovery for PDF v3. A long run also
needs to survive app/process exits without treating the whole document as one
in-memory queue.

TranslationPatch storage already uses 64-page index shards, but those shards
are patch ownership, not scheduler state. Reusing the old run model or making
the patch manifest carry transient work would couple unrelated lifecycles.

## Decision

PDF v3 uses an independent durable scheduler under an orchestrator-owned
absolute run directory. No PDF v1/v2 run migration is provided while the app
is beta.

The scheduler stores one small `manifest.json` and page-state
`shard-XXXXXXXX.json` files. A shard contains at most 64 requested page
records. The width is an internal persistence bound only; it is not a
translation batch, queue window, PageSet restriction or UI chunk.

The manifest binds run/source/language identity, exact requested PageSet,
engine/PageGraph/TranslationPatch/renderer versions, capacity configuration,
typed run/cancellation state, owner session lease, fair claim cursors and a
rebuildable summary. Page shards are the durable state authority. Opening a
run streams all expected shards, verifies exact PageSet coverage and rebuilds
the manifest summary and completion state.

Each requested page has one durable state:

- `pending`;
- `extracted` with exact extraction artifact and source-page identity;
- `completed` with exact extraction and TranslationPatch authority;
- `preserved` with extraction authority and a stable reason code;
- `failed` with stage, retryability and the authority needed to resume.

An optional page lease records a unique lease ID, owner session, stage and
timestamp. Only pending pages can receive extraction leases and only extracted
pages can receive translation leases. Claim/commit/fail APIs validate the
owner, lease ID and stage before every transition. Pause and cancellation stop
new claims while allowing already leased work to settle.

Backpressure has independent hard limits for extracting pages, extracted pages
waiting for translation and translating pages. Claims never return more than
the caller limit or remaining capacity. Status reads use a maximum 256-page
window and never return the entire run by default.

Crash recovery takes complete, independently validated extraction and patch
inventories. It may promote work committed before its scheduler update,
retains `completed` only when exact patch authority is valid, demotes invalid
patches to valid extraction where possible, and returns pages without valid
extraction to `pending`. All stale page leases are released while a stale run
owner is replaced.

Manifest and shard writes use unique temp files, file `sync_all`, backup/rename
replacement and parent-directory sync where supported. Reads examine valid
canonical/temp/backup candidates, roll forward the highest durable generation
and remove sidecars. Initial creation writes the complete run into a unique
sibling staging directory and exposes it with one directory rename, so a
partially initialized run directory never becomes canonical. In-process
handles for one run share a coordinator lock.

## Evidence

Automated Windows AMD tests prove:

- a 1,000-page run creates 16 shards with at most 64 records each;
- extraction, extracted-backlog and translation capacities remain bounded when
  claim requests are much larger than capacity;
- the first claim is three pages under a capacity of three, demonstrating no
  fixed ten-page scheduling behavior;
- stale extraction leases recover to exact patch/extraction authority without
  re-requesting completed work;
- a restarted scheduler only claims pages missing valid authority;
- synced manifest and shard backup candidates are promoted after simulated
  interrupted replacement;
- completed and explicitly preserved pages finish a run;
- pause, retryability and cancellation transitions are typed and enforced.

## Consequences

### Positive

- Long-run active work is bounded independently from document page count.
- Any requested page can be extracted, translated, retried or inspected
  without exposing a chunk abstraction.
- PageGraph and TranslationPatch stores remain the content authorities; the
  scheduler stores only identity and lifecycle state.
- Crash recovery avoids repeating artifacts already committed durably.
- Manifest size and rewrite frequency do not grow with per-page state detail.

### Costs

- Capacity checks stream scheduler shards and trade bounded memory for bounded
  metadata I/O.
- A complete recovery inventory must be produced by the extraction and patch
  stores before stale-owner recovery.
- A transition updates one page shard and then the manifest summary; opening a
  run must reconcile the crash window between those writes.
- Tauri commands, worker integration and user-facing long-run status remain a
  separate Phase 6 slice.

## Rejected Alternatives

- Reuse the legacy ten-page `PdfTranslationRun` model.
- Persist one unbounded page array in the run manifest.
- Put transient scheduler state in the TranslationPatch store.
- Treat rendered page PDFs as completion authority.
- Load all page records for every status request.
- Resume a completed page without validating its patch authority.
