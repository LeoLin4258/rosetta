# PDF v3 Trusted Translation Component Resolver

Date: 2026-07-19

## Summary

Added the native trusted component boundary that resolves PDF v3 provider,
model and unified-font identity from live managed components instead of caller
input.

## Implementation

- strictly validated installed managed RWKV model/runtime manifests against
  the active compile-time platform profile;
- required a ready, healthy managed process whose profile, PID and loopback
  endpoint remain stable before and after artifact verification;
- hashed actual sidecar and model content in blocking workers, including
  deterministic symlink-rejecting directory hashing for extracted models;
- cached unchanged direct-file digests and immutable parsed translation fonts;
- verified exact Source Han Sans CN and Go Noto font bytes;
- derived a content-addressed PDF v3 component manifest identity;
- exposed a narrow typed component probe without paths, process details,
  endpoints, credentials or raw errors;
- reused only font assets and release identity from the legacy PDF pack, without
  requiring its Python worker or doclayout model.

## Validation

- `cargo test --locked v3_component --lib` (`4 passed`);
- `cargo test --locked installed_manifests_must_match_the_exact_runtime_profile --lib`
  (`1 passed`);
- `cargo test --locked pdf_v3 --lib` (`182 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`102 passed`);
- `cargo test --locked managed_rwkv --lib` (`53 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The native app can now prove which translation runtime, model and unified font
bytes are available for a PDF v3 run. Trusted run creation, signed standalone
component manifests, install/repair/update/remove commands, typed capability
events and full real-document stress validation remain pending.
