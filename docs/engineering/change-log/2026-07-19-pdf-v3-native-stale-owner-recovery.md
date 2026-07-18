# PDF v3 Native Stale-Owner Recovery

Date: 2026-07-19

## Summary

Connected validated PDF v3 stale-owner recovery to the typed Tauri control
plane without exposing owner, path or cutoff authority to the frontend.

## Implementation

- added a five-minute native owner-lease takeover threshold;
- exposed the derived recovery-eligible timestamp in bounded status schema v2;
- rejected self-recovery while the current session has active page leases;
- rechecked same-owner active leases inside the scheduler coordinator lock;
- rejected different-session takeover until the old lease is stale;
- validated runtime identity before opening recovery stores;
- rebuilt recovery inventory from validated PageGraph and TranslationPatch
  authorities at the run's immutable translation revision;
- released stale leases and promoted or invalidated page authority through the
  durable scheduler;
- converged recovered cancelling runs after their final active lease cleared;
- moved complete inventory validation into a blocking Tauri worker;
- added a narrow `recover_rosetta_pdf_v3_run` command and typed recovery report.

## Validation

- `cargo test --locked v3_control --lib` (`4 passed`);
- `cargo test --locked pdf_v3 --lib` (`182 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`94 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

Native PDF v3 state can now be inspected, controlled and safely taken over
after a stale lease. Run creation, native heartbeat/process verification,
verified component launch/model health, frontend integration and real complex
500/1,000-page end-to-end validation remain pending.
