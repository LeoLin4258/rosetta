# PDF v3 Trusted Atomic Run Creation

Date: 2026-07-19

## Summary

Added the native PDF v3 run-creation transaction that binds exact source,
PageSet, language, scheduler, component, model and unified-font identity
without accepting those identities from the frontend.

## Implementation

- exposed a narrow command accepting only job, optional page selection and
  target language;
- verified the actual cached source fingerprint against native PDF source
  metadata;
- streamed only document format/language metadata without loading blocks,
  segments or translation history;
- validated language direction against the live trusted component profile;
- allocated native run IDs and monotonic translation revisions;
- committed scheduler shards and immutable runtime manifest in a hidden
  staging directory before one final directory rename;
- bound native engine/schema/renderer versions, default fit policy and bounded
  `2 / 4 / 1` scheduler capacities;
- attached the process-native owner heartbeat immediately after commit;
- kept creation errors free of paths, endpoints, owner IDs and credentials.

## Validation

- `cargo test --locked v3_run_creation --lib` (`2 passed`);
- `cargo test --locked pdf_v3 --lib` (`182 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`104 passed`);
- `cargo test --locked managed_rwkv --lib` (`53 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

PDF v3 runs can now be created as complete trusted durable authorities for an
exact PageSet. The native extraction/translation worker supervisor, run
enumeration and frontend workflow integration remain pending.
