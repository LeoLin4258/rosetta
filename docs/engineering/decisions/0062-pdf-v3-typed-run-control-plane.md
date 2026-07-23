# ADR 0062: PDF v3 Typed Run Control Plane

Date: 2026-07-18

Status: Accepted

Refines ADR 0055 and ADR 0061.

Refined by ADR 0075.

## Context

The durable scheduler already supported exact PageSets, page-local leases,
bounded status windows, pause/resume/cancellation and recovery. Those controls
were not reachable through Tauri. The existing PDF v2 commands expose a
different run model and cannot represent PDF v3 shard authority, immutable
runtime identity or hundreds of pages without leaking legacy behavior.

A new control surface must remain bounded for long documents, prevent the
frontend from selecting arbitrary job paths or forging scheduler ownership,
and make the runtime identity inspectable without returning credentials,
document text or machine-local font paths.

## Decision

Add one PDF v3 run-control projection over the durable scheduler and immutable
runtime manifest. Its status response contains:

- run state, cancellation state, exact canonical PageSet and summary;
- source page count and language direction;
- whether the current native session owns the run, without exposing the owner
  session ID;
- component build, platform, provider/model and translation-font byte
  identities from the validated immutable runtime manifest;
- page-number ordered records with `nextStartAfter` and `hasMore` pagination.

Page lease projections contain only stage, timestamp and whether the current
native session owns the lease. Internal lease IDs and owner session IDs are not
serialized.

The default page window is 64 records and the hard limit is 256. Status first
validates the runtime manifest against the scheduler translation binding.
Missing or drifting identity is a hard error.

Expose four narrow Tauri commands for status, pause, resume and cancellation.
Commands accept only `jobId`, safe `runId` and status pagination. Tauri resolves
the job directory beneath its app-data jobs root and supplies the process-local
session identity and timestamp. The frontend cannot provide a filesystem path,
owner ID, runtime identity or cancellation reason.

Pause, resume and cancellation use the scheduler's owner and state gates.
Cancellation enters `cancelling`; an idle run can immediately finish as
`cancelled`, while a run with extraction or translation leases remains
`cancelling` until those leases explicitly settle. Cancellation is idempotent:
a later request finishes a settled cancelling run and an already cancelled run
returns its current status.

The status contract excludes source/translated text, provider response bodies,
endpoints, credentials and font paths. It is a projection of durable authority,
not another persisted state file.

## Evidence

Automated Windows tests cover:

- ordered multi-window status across a sparse exact PageSet;
- the 256-record hard limit and unsafe run-ID rejection;
- run/page current-session ownership without exposing owner or lease IDs;
- pause/resume owner enforcement;
- idle cancellation completing immediately;
- active leases keeping cancellation in `cancelling`, then converging through
  idempotent cancellation after the lease settles;
- component/provider/model/font identity from the validated runtime manifest.

The Tauri handler registration is compile-checked with the full Rust crate.

## Consequences

### Positive

- Native PDF v3 run state is now inspectable and controllable without legacy
  PDF state translation.
- Status cost and response size remain bounded for very long documents.
- Ownership and filesystem boundaries stay inside native code.
- Runtime drift is visible before a caller trusts progress or resumes work.

### Costs

- A run without a valid immutable runtime manifest cannot be queried through
  this control projection.
- A restarted process must use the scheduler recovery/takeover path before it
  can control a run owned by the previous session.
- Frontend polling, virtualized page status and lifecycle start/recovery
  commands remain separate work.

## Rejected Alternatives

- Reuse or extend the PDF v2 run-state commands.
- Return every requested page in one status response.
- Let the frontend pass job directories or scheduler owner identities.
- Return persisted provider connection settings or local font paths.
- Mark cancellation complete while page leases are still active.
