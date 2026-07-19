# ADR 0066: PDF v3 Trusted Atomic Run Creation

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0061, ADR 0064 and ADR 0065.

## Context

PDF v3 already had a durable scheduler, immutable runtime manifest, native
owner heartbeat and trusted component resolver. There was no production path
that created those authorities together. A caller could otherwise choose a run
ID, revision or component identity, or a crash between scheduler and runtime
manifest creation could expose a run that could never be resumed safely.

Run creation must also remain cheap for a document with hundreds of pages. It
must not load all document blocks, segments or translations merely to resolve
language settings.

## Decision

Expose one native asynchronous run-creation command. Its public inputs are
limited to `jobId`, optional exact `requestedPageSet` and `targetLanguage`.
Omitting the page set selects every source page; an empty or out-of-range set is
rejected. The frontend cannot provide a run ID, source path, source
fingerprint, page count, source language, revision, scheduler capacity,
renderer policy, component/provider/model identity, font identity, owner
session or timestamp.

The native preparation path derives the cached source path below the app-data
job root, requires PDF source metadata, hashes the actual immutable source and
matches its fingerprint. It reads only format and language fields from
`document.json` through a streaming deserializer, skipping blocks without
materializing them. Persisted source language wins; when it is absent, the
native path chooses the opposite supported language for the English/Chinese
translation profile. The trusted component resolver and source preparation run
concurrently, then the selected direction must match the live profile's
supported directions.

Run ID and translation revision are native-owned. Under one process-wide
creation lock, the creator scans committed immutable runtime manifests and
allocates `max(revision) + 1`, then generates a collision-resistant safe run
ID from native time, revision and a process counter. Existing committed runs
with invalid runtime identity stop creation rather than allowing revision
reuse.

Scheduler and runtime manifest are exposed atomically through a nested staging
transaction:

1. Create the complete scheduler manifest and bounded page shards under a
   hidden sibling run directory.
2. Derive the scheduler translation binding from that staged authority.
3. Build and durably commit the immutable runtime manifest from the trusted
   live component and unified font bytes.
4. Build the bounded typed status from the staged scheduler/runtime pair,
   proving their binding before exposure.
5. Rename the complete staged directory once to the final native run ID and
   sync the parent directory where supported.

Any pre-rename error removes the staging directory. The default scheduler
capacities are independently bounded at two extracting pages, four extracted
pages waiting for translation and one translating page. They are native policy,
not frontend batch or chunk semantics. The default renderer fit policy is
bound into the runtime manifest.

After commit, the Tauri layer attaches the current native lifecycle heartbeat
and refreshes the ordinary bounded run-control status. Creation does not yet
start extraction or translation workers; worker ownership is a following
phase.

## Evidence

Automated Windows tests cover:

- exact sparse PageSet creation and all-page selection;
- scheduler and runtime manifest appearing together under one final run;
- a second run receiving a distinct native ID and monotonic revision 2;
- invalid runtime identity removing both visible and staged run state;
- compilation of the narrow asynchronous Tauri command and lifecycle hookup.

## Consequences

### Positive

- The frontend can no longer forge PDF v3 runtime or scheduling identity.
- A visible run always has both scheduler and immutable runtime authorities.
- Exact single-page and sparse-page runs use the same long-document scheduler.
- Run creation does not deserialize document blocks, segments or translations.
- A newly created nonterminal run immediately has a native owner heartbeat.

### Costs

- Source bytes are hashed during creation to reject source drift.
- Revision allocation currently scans one small immutable manifest per
  committed run; a dedicated sequence authority can replace this if run counts
  become large.
- A process crash during hidden staging can leave an unreachable staging
  directory for later repair cleanup, but cannot expose partial run authority.
- Worker startup, run enumeration and frontend workflow integration remain
  pending.

## Rejected Alternatives

- Let React provide run, revision, source, component or scheduler identity.
- Commit the scheduler directly to its final directory before the runtime
  manifest exists.
- Use wall-clock milliseconds alone as the translation revision.
- Load the complete Rosetta job bundle to resolve PDF language metadata.
- Treat an empty page selection as a successful zero-page run.
