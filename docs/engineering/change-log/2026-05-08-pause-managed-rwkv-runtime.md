# 2026-05-08 Pause Managed RWKV Runtime

## Summary

Paused active development of Rosetta-managed local RWKV runtime launch.

The long-term product goal is unchanged: Rosetta should eventually let non-technical users run a local RWKV translation model from inside the app. The current implementation work is paused until the RWKV model engineer confirms the runtime scheme, model format, hardware backend, and API contract.

## Decision Record

Added:

```txt
docs/engineering/decisions/0002-pause-managed-rwkv-runtime.md
```

The ADR records:

- why managed runtime work is paused
- that current development should use the RWKV engineer-deployed translation API
- that user self-deployed RWKV API and user-configured remote/cloud RWKV API can be future opt-in backend options
- that existing runtime manager code is parked/experimental
- what must be confirmed before runtime work resumes

## Documentation Updates

Updated:

```txt
docs/engineering/plans/2026-05-07-rwkv-runtime-one-click.md
docs/engineering/plans/2026-05-08-rwkv-runtime-progress-snapshot.md
```

The runtime plan is now marked `Paused`.

The progress snapshot now states that later feature work should not treat existing runtime skeleton commands, Settings UI, artifact metadata, or CUDA/NVIDIA preflight code as the active product path.

## Current Development Direction

Until runtime work resumes, Rosetta should:

- connect to an already deployed RWKV translation API
- keep the connector independent from managed runtime status
- keep API backends configurable enough to later support local, LAN, self-hosted remote, or explicit cloud RWKV endpoints
- prioritize document import, segmenting, translation scheduling, progress, retry, preview, and export
- keep all local-first and privacy boundaries intact

Remote/cloud API use must remain explicit opt-in and must not become Rosetta's default behavior.

## Validation

Documentation-only change. No code validation was run.

Per project instruction, no dev server or build command was run.
