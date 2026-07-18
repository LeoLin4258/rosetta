# PDF v3 Typed Run Control Plane

Date: 2026-07-18

## Summary

Connected bounded PDF v3 scheduler status and owner-gated pause, resume and
cancellation to narrow Tauri commands.

## Implementation

- added one scheduler status snapshot without exposing shard manifests;
- projected exact PageSet, summary, cancellation and paginated page records;
- validated and returned immutable component/provider/model/font identities;
- capped status windows at 256 records with a 64-record default;
- resolved job paths beneath Tauri app data and kept owner identity native;
- projected page leases without serializing owner session or internal lease IDs;
- excluded credentials, endpoints, font paths and document text;
- completed idle cancellation immediately while preserving `cancelling` for
  runs with active page leases, with idempotent retry after leases settle;
- registered typed status, pause, resume and cancel commands in Tauri.

## Validation

- `cargo test --locked v3_control --lib` (`3 passed`);
- `cargo test --locked pdf_v3 --lib` (`182 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`93 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The durable run is now inspectable and controllable through native commands.
Frontend polling/status UI, run creation and recovery/takeover orchestration,
verified component launch/model health and real complex 500/1,000-page
end-to-end validation remain pending.
