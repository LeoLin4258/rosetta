# ADR 0064: PDF v3 Native Owner Heartbeat Lifecycle

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0062 and ADR 0063.

## Context

PDF v3 durable runs use an owner lease to prevent two native sessions from
claiming or committing the same page work. The typed control plane and stale
recovery path initially borrowed the legacy PDF cancellation session and did
not renew the lease. A healthy long-running app therefore looked stale after
five minutes, while frontend polling could not safely own process liveness.

The lifecycle must remain bounded for hundreds of pages, survive frontend
navigation, avoid blocking the async runtime with durable filesystem work and
never expose an owner credential or machine-local path.

## Decision

Add one process-native `PdfV3RunLifecycleState`, independent from the legacy
PDF cancellation state. It creates a unique native session identity and owns
at most one Tokio heartbeat task per active run directory.

Each task renews its scheduler owner lease every 10 seconds. Recurring durable
scheduler I/O runs in a blocking worker. Renewal verifies owner identity and determines
under the scheduler coordinator lock whether the run is nonterminal. A
`cancelled` or `completed` run is not renewed and unloads its task. An owner
mismatch also unloads the task; it never adopts or takes over another session.
Only the validated stale-owner recovery operation may change ownership.

Tauri status, pause, resume, cancellation and recovery commands all use this
dedicated PDF v3 session. A nonterminal result owned by the current session
ensures one heartbeat and refreshes the returned durable status. A terminal or
foreign-owned result stops any local task. App exit stops every registered
heartbeat before managed provider shutdown begins.

Transient renewal failures retain the task and increment a bounded consecutive
failure counter. A successful renewal resets the counter and records its
timestamp. Run-control status schema `rosetta-pdf-v3-run-control-status/3`
projects only:

- `active`;
- `intervalMs`;
- `lastSuccessAtMs`;
- `consecutiveFailures`.

It does not expose the owner session ID, run path, raw error, provider
configuration, credentials or document content. The conservative five-minute
stale-owner threshold remains unchanged; heartbeat reduces false eligibility
but does not prove that another native process has exited.

## Evidence

Automated Windows tests cover:

- periodic renewal and terminal auto-unload;
- refusal to adopt a run owned by another session;
- a strict serialized heartbeat health field set;
- atomic terminal renewal rejection in the durable scheduler;
- existing bounded status, owner control and stale recovery behavior.

The Tauri state registration, command injection and exit cleanup are checked
by the full Rust crate.

## Consequences

### Positive

- Healthy PDF v3 runs retain ownership without frontend timers or polling.
- Legacy PDF cancellation no longer supplies PDF v3 process identity.
- Heartbeat memory is proportional to active runs, not page count.
- Terminal runs and app exit release native lifecycle tasks explicitly.
- Operators can inspect heartbeat health without receiving replayable or
  sensitive internal state.

### Costs

- Each active run performs one small durable manifest replacement every 10
  seconds.
- A stalled filesystem can accumulate heartbeat failures until it recovers or
  ownership changes.
- The five-minute takeover delay remains until native process verification is
  designed and validated.
- Run creation must explicitly register its new nonterminal run once trusted
  component, model and font identity are available.

## Rejected Alternatives

- Renew ownership from React polling or window lifecycle callbacks.
- Reuse the legacy single-run cancellation state for PDF v3.
- Automatically take over a run when renewal reports an owner mismatch.
- Persist heartbeat health as a second run-state authority.
- Return owner IDs, paths or raw renewal errors in typed status.
