# PDF v3 Native Owner Heartbeat

Date: 2026-07-19

## Summary

Added a native PDF v3 run lifecycle that keeps healthy durable owner leases
current without relying on frontend polling or the legacy PDF cancellation
session.

## Implementation

- added a unique process-native PDF v3 session identity;
- registered one bounded heartbeat task per active run;
- renewed durable owner leases every 10 seconds through blocking workers;
- made terminal renewal rejection atomic under the scheduler coordinator lock;
- stopped heartbeats on terminal state, owner loss and app exit;
- switched all PDF v3 Tauri control and recovery commands to the dedicated
  lifecycle state;
- attached heartbeat after current-owner status/control/recovery and refreshed
  the durable response;
- kept foreign-owner inspection read-only and takeover restricted to validated
  stale recovery;
- added typed health fields without paths, owner IDs or raw errors;
- advanced the run-control status schema from v2 to v3.

## Validation

- `cargo test --locked v3_lifecycle --lib` (`4 passed`);
- `cargo test --locked completed_and_preserved_pages_finish_the_run --lib`
  (`1 passed`);
- `cargo test --locked v3_control --lib` (`4 passed`);
- `cargo test --locked pdf_v3 --lib` (`182 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`98 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

Native PDF v3 status, controls and stale recovery now share a real native
owner lifecycle. Trusted run creation, verified component launch/model health,
frontend integration and real complex 500/1,000-page end-to-end validation
remain pending.
