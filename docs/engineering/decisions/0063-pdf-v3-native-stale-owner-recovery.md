# ADR 0063: PDF v3 Native Stale-Owner Recovery

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0061 and ADR 0062.

## Context

PDF v3 durable scheduler recovery could already reconcile page shards against a
validated PageGraph and TranslationPatch inventory. The Tauri control plane did
not assemble that recovery path, so a run owned by a crashed app session could
be inspected but not safely taken over.

Blindly replacing the owner would risk two live workers committing against the
same page leases. Trusting scheduler shard references would also preserve
patches or PageGraphs that were missing, corrupt or bound to a different source,
language or schema. Recovery can be much more expensive than bounded status
because it may validate every durable page artifact.

## Decision

Add one native PDF v3 recovery operation. Tauri accepts only `jobId` and safe
`runId`; it supplies the current native session, current timestamp and a fixed
stale cutoff. Job, run, extraction and translation-store paths are derived
beneath the app-data job directory.

The control layer applies these gates in order:

1. Open and structurally validate the scheduler and every requested shard.
2. Reject recovery when the current session still owns active extraction or
   translation leases.
3. Reject a different owner while its lease is not stale.
4. Load the immutable runtime manifest and validate its scheduler binding.
5. Open source-bound PageGraph and target-language TranslationPatch stores.
6. Build a complete validated recovery inventory, loading patches only through
   their matching PageGraph authority and only at the immutable runtime
   manifest's translation revision.
7. Atomically recheck owner staleness inside the scheduler, release old leases,
   promote valid disk authority and invalidate stale shard authority.

The scheduler coordinator lock also rechecks that the same owner did not claim
new page work during inventory validation. A same-owner recovery can never
release a concurrently acquired lease.

The temporary takeover threshold becomes eligible immediately after five full
minutes have elapsed since `ownerLeaseUpdatedAtMs`. Status exposes
`ownerRecoveryEligibleAtMs`, but the frontend cannot choose or shorten the
cutoff. This is a conservative lease
policy, not proof that the old process exited. A future native lifecycle manager
must own periodic heartbeat and process identity before shortening or bypassing
it.

Adding the recovery timestamp advances the run-control status schema from `1`
to `2`; recovery results use `rosetta-pdf-v3-run-recovery-result/1`.

Recovery runs in a blocking worker because complete inventory validation can
walk a long document. It retains only one PageGraph/patch body at a time through
the existing store validation path. It does not recover pending patches,
prepared fonts, render cache entries or PDF object deltas.

If recovery releases the final active lease from a cancelling run, the control
layer immediately finishes it as `cancelled`. The result returns typed recovery
counters and the ordinary bounded run status projection.

## Evidence

Automated Windows tests cover:

- refusing self-recovery while the current session owns an active page lease;
- rechecking the same-owner active-lease gate inside the scheduler lock;
- refusing takeover at the inclusive stale boundary;
- stale-owner takeover after the fixed gate;
- excluding a valid patch from another translation revision;
- releasing an extraction lease against an empty validated inventory;
- reconciling a cancelling run directly to `cancelled`;
- transferring status ownership without exposing owner or lease IDs.

The registered asynchronous Tauri command is compile-checked with the full
crate.

## Consequences

### Positive

- Crashed PDF v3 runs now have a safe native takeover path.
- Recovery never trusts scheduler completion references without disk authority.
- Long-document recovery work does not block the Tauri command thread.
- The frontend sees exactly when lease-based recovery becomes eligible without
  controlling the policy.

### Costs

- Recovery may read every durable PageGraph index entry and resolved patch.
- A restart can require waiting up to five minutes until native heartbeat and
  old-process verification are implemented.
- Run creation and actual worker lifecycle ownership remain pending.

## Rejected Alternatives

- Let the frontend supply `staleBeforeMs`, owner IDs or artifact inventories.
- Immediately take over whenever the app has a new session ID.
- Trust completed/extracted shard state without reopening durable artifacts.
- Perform complete inventory validation on the synchronous command thread.
- Persist pending renderer or provider work so it can be resumed mid-page.
