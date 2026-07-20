# PDF v3 Patch Store Isolation and Failed Terminal State

Date: 2026-07-20

## Summary

Fixed UI-triggered PDF v3 translation failures caused by an incompatible beta
patch store and stopped fully failed runs from polling and rewriting manifests
forever.

## Implementation

- versioned the target-language patch-store directory as
  `language-v2-<sha256>` and left legacy derived stores untouched;
- added the run-level `failed` terminal state and deterministic shard-summary
  reconciliation on commit, recovery and open;
- stopped workers, owner heartbeat and active frontend polling for failed runs;
- unlocked page selection and allowed a new revision after failure;
- retained owner-gated exact-page retry, including takeover after an expired
  owner lease and transition from failed back to running;
- preserved the concrete preview render/raster cause in command errors instead
  of collapsing it into a generic failure;
- normalized indirect references to free or missing xref entries to PDF `null`,
  matching ISO 32000 semantics for otherwise valid page resources;
- kept native export restricted to completed runs.

## Validation

- `pnpm typecheck`;
- `cargo fmt --all -- --check`;
- `cargo check --locked`;
- `cargo test --locked pdf_v3 --lib` (`190 passed`, `22 ignored`);
- `cargo test --locked rosetta_jobs` (`128 passed`);
- focused tests for store namespace isolation, failed-run transitions, preview
  diagnostics and free/missing indirect object handling;
- managed-runtime UI validation completed a 3-page run with 3 completed pages,
  0 failed pages and compressed `*.patch.json.gz` output.

## Current Boundary

Legacy beta patch directories are not migrated or proactively deleted. They
remain inert until normal job deletion.
