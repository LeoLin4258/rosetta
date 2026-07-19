# ADR 0069: PDF v3 Bounded Run Enumeration

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0062, ADR 0064, ADR 0066, ADR 0067 and ADR 0068.

## Context

PDF v3 had typed commands for creating and controlling a known run, but the
frontend had no trustworthy way to discover committed runs after navigation or
app restart. Persisting a second frontend-owned run index would create another
state authority and could drift from the durable scheduler. Returning every run
or every page would also make a long-lived job progressively more expensive.

Enumeration must not expose owner session IDs, paths, provider configuration or
document content. It must also remain observational: reading history cannot
start a worker, attach a heartbeat or mutate scheduler state.

## Decision

Expose one asynchronous Tauri command whose public inputs are `jobId`, optional
`targetLanguage`, optional `beforeRevision` and optional `limit`. The app-data
job root and current native session are resolved internally. Filesystem work is
performed in a blocking task.

The durable run directory remains the only index authority. Enumeration scans
visible committed run directories, skips hidden creation staging directories,
and validates every visible run by opening its scheduler, rebuilding its
summary and matching its immutable runtime manifest to the scheduler binding.
A malformed visible run fails the request instead of disappearing from history.

Results use schema `rosetta-pdf-v3-run-list/1`. They are ordered by descending
positive translation revision and paginated with an exclusive
`beforeRevision` cursor. The default page size is 16 and the hard maximum is 64.
The implementation retains only the highest requested number of eligible
revisions while scanning, so response working memory does not grow with run
history. `nextBeforeRevision` is the last returned revision only when more
eligible runs exist.

Target-language filtering uses the same case-insensitive primary-language
normalization as trusted run creation, so values such as `zh-CN`, `zh_CN` and
`zh` select the same direction. Returned items preserve the exact language
identity stored in the scheduler.

Each item contains only run ID, translation revision, run state, source page
count, exact requested PageSet, source/target language, rebuilt summary,
`ownedByCurrentSession` and the fixed native owner-recovery eligibility time.
It does not return page records, owner IDs, lease IDs, source fingerprint,
runtime/component details, paths, endpoints, credentials, text or raw errors.

Enumeration does not synchronize lifecycle state, start or stop supervisors,
attach heartbeats, recover ownership or update leases. A selected run must use
the existing bounded run-control status command for page-window details and
explicit control actions.

## Evidence

Automated Windows tests cover revision-descending order, strict cursor
pagination, primary-language filtering, hidden staging exclusion, malformed
committed-run rejection, input bounds and the exact serialized privacy field
set. The TypeScript client exposes the same schema through a typed invoke
wrapper.

## Consequences

### Positive

- App restart and workspace navigation can discover durable runs without a
  second index authority.
- Listing cost has fixed response and retained-result memory bounds.
- Corrupt committed state remains visible as a repairable error instead of
  being silently omitted.
- List polling cannot take ownership or create execution side effects.

### Costs

- Enumeration validates each committed scheduler/runtime pair and therefore
  performs work proportional to run history, although retained response memory
  remains bounded.
- Revision allocation and enumeration still scan run directories; a durable
  native sequence/index may be justified if real jobs accumulate very large run
  histories.
- The list intentionally omits page records and runtime diagnostics; the UI
  needs a second bounded status call after selecting one run.

## Rejected Alternatives

- Persist a frontend-maintained run index.
- Return all runs or complete page arrays in one command.
- Treat malformed committed runs as absent.
- Use directory timestamps or run IDs as pagination authority.
- Let list polling start workers, renew ownership or perform stale recovery.
