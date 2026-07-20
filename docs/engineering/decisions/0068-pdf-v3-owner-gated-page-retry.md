# ADR 0068: PDF v3 Owner-Gated Failed-Page Retry

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0063, ADR 0065 and ADR 0067.

Refined by ADR 0075.

## Context

The durable scheduler retained retryable extraction and translation failures,
but PDF v3 had no public command that could restore one exact failed page. A
run could therefore remain `running` with durable failed pages and no supported
way for the frontend to resume them. An inactive process-local worker also
needed a trustworthy restart path; registering it from status polling or from
frontend-supplied runtime identity would weaken the native ownership model.

Stale recovery already resolved the current source and live component before
takeover, but their exact equality with the immutable run binding was checked
only by the worker's asynchronous startup preflight after ownership changed.

## Decision

Expose one asynchronous Tauri command for retrying an exact failed page. Its
only public identity inputs are `jobId`, safe `runId` and `pageNumber`. The
native lifecycle supplies the owner session and timestamp.

The command first verifies that:

- the current native session owns the run;
- the run is `running`, `paused` or terminal `failed`;
- the page belongs to the run's exact PageSet; and
- its durable state is `failed` with `retryable=true`.

The scheduler remains the state authority. An extraction failure returns to
`pending`; a translation failure retaining valid extraction authority returns
to `extracted`. Attempts and durable artifacts are not erased. Cancelled and
completed runs, foreign owners, non-requested pages and non-retryable failures
are rejected. Retrying from `failed` changes the run to `running` before worker
registration.

If the run supervisor is inactive, the command resolves the current trusted
component and hashes the cached source before changing the page state. A
shared blocking validator requires exact equality with the scheduler binding,
immutable runtime manifest, provider/model identity and unified font bytes.
Only after that validation succeeds may retry mutate the shard and the
idempotent worker registry attach one supervisor. The same validator now runs
before stale recovery changes ownership, closing the previous validation
ordering gap.

Retry returns the existing bounded run-control schema 4. Its page window starts
at the retried page and remains capped at the default 64 records, so an exact
page retry in a hundreds-page document is immediately observable without
returning the complete run. Retrying while paused is allowed; a restarted
worker remains quiescent under scheduler state until resume.

## Evidence

Automated Windows tests cover running and paused retries, foreign-owner
rejection, non-retryable failure rejection, terminal-state rejection, exact
page-window placement and the existing one-worker-per-run registry invariant.

## Consequences

### Positive

- Durable retryable failures now have an explicit native recovery surface.
- Restart cannot trust stale or frontend-provided runtime identity.
- Retrying page 500 does not materialize status for pages 1 through 499.
- Pause, retry and worker execution retain one scheduler-owned state machine.

### Costs

- Restarting an inactive run re-verifies source and component bytes before the
  retry becomes visible.
- Retry remains page-granular; unit-level selective retranslation is a later
  workflow layered above stable page authority.
- Run enumeration and frontend integration are still required before this
  command is user-accessible.

## Rejected Alternatives

- Retry all failed pages as an unbounded bulk operation.
- Let React change shard state or supply owner/component identity.
- Restart workers from ordinary status polling.
- Change page state first and discover source/component drift asynchronously.
- Return the complete page array after one retry.
