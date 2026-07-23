# ADR 0067: PDF v3 Native Worker Supervisor

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0061, ADR 0064, ADR 0065 and ADR 0066.

Refined by ADR 0075.

## Context

Trusted PDF v3 runs could be created atomically and kept alive by a native
owner heartbeat, while the extraction and translation workers remained
isolated library APIs. A `running` run therefore did not perform work. Driving
those APIs from React would make window timers own durable progress and would
not solve bounded memory, cancellation during provider I/O or app-exit order.

PDFium documents and the reusable extraction mapping index also need a stable
blocking-thread lifetime. Reopening the document for every page would preserve
thread safety but reintroduce avoidable long-document setup cost.

## Decision

Add one process-local `PdfV3RunWorkerState`. It registers at most one supervisor
per canonical run directory. Each supervisor owns shared level-triggered stop
and provider-cancel flags, bounded health and completion notification. Its two
stage loops are implementation details of that one run registration:

- extraction stays in one blocking task, reuses one PDFium `DocumentHandle`
  and mapping index, and requests at most two pages per worker batch;
- translation stays in one blocking task, reuses one lazy source-object view
  and renderer ownership index, and processes one PageGraph/provider page at a
  time through the existing Tokio runtime.

The durable scheduler remains the sole work authority. Its independent
`2 / 4 / 1` extraction, extracted-waiting and translation capacities provide
backpressure. Empty claims use bounded polling sleeps. Pause prevents new
claims without abandoning current leases. Cancellation sets the provider flag
immediately; both loops stop claiming, current leases commit or fail, and the
supervisor finishes durable cancellation only after both active lease counts
reach zero.

All process-local PDFium open, extraction, preview rendering and document-drop
operations share one operation lock. Extraction holds it only for one bounded
worker batch, allowing preview and other runs to interleave; translation does
not use PDFium and remains independent.

Before either loop starts, a blocking preflight consumes the native
`VerifiedDocumentIdentity` produced by run creation or stale recovery and
matches it to the scheduler binding. PDFium opening consumes that same identity
instead of hashing the complete source again. The preflight loads the immutable
runtime manifest, requires exact equality with the newly resolved live
component identity, and constructs the bound provider/font runtime. Creation
starts the supervisor only after the final run-directory rename. Stale recovery
resolves the current live component and verifies the source before ownership
changes, then starts a supervisor only for a successfully recovered
nonterminal run.

Run-control schema `rosetta-pdf-v3-run-control-status/4` adds a bounded worker
projection containing only `active`, `stage`, `lastProgressAtMs` and
`consecutiveFailures`. It contains no path, owner identity, endpoint,
credential, raw error or text. Status polling does not start workers. A worker
that is absent does not retain a heartbeat merely because the frontend reads
status.

Terminal state, owner loss and explicit stop unload the registration. App exit
and local-data reset signal every worker, wait for completion, stop heartbeats,
and only then shut down the managed translation runtime or remove job/model
directories. Deleting one job similarly waits only for supervisors below that
job's native run root and does not stop unrelated runs.

## Evidence

Automated tests cover:

- one registered task identity per run;
- level-triggered cancellation;
- terminal auto-unload;
- shutdown waiting for registered tasks;
- strict bounded worker-health serialization;
- run-control schema and existing pause/cancel/recovery behavior.

The existing extraction, translation, scheduler, processor and long-document
tests continue to validate page-bounded commit ordering and backpressure.

## Consequences

### Positive

- Newly created trusted runs now execute without frontend timer ownership.
- PDFium and lazy renderer state are reused across pages without loading the
  complete document into memory.
- Hundreds of pages remain governed by durable page authority and fixed
  backpressure rather than visible ten-page chunks.
- Cancellation reaches an in-flight local provider request and does not abandon
  scheduler leases.
- Worker state is inspectable without exposing sensitive process or document
  details.

### Costs

- Each active run uses one supervisor plus two blocking stage tasks.
- Polling is currently bounded but filesystem-event wakeups could reduce idle
  scheduler reads later.
- Retryable failed pages remain durable and require the explicit page-retry
  control surface planned for frontend integration.
- Real managed-runtime 500/1,000-page translation and export validation remains
  a separate beta gate.

## Rejected Alternatives

- Let React polling claim and execute page work.
- Reopen PDFium and rebuild the complete mapping index for every page.
- Keep a heartbeat alive when no native worker owns execution.
- Cancel by dropping tasks and leaving active durable leases for stale recovery.
- Return paths, endpoints, owner IDs, raw errors or text in worker status.
