# PDF v3 Runtime Manifest Binding

Date: 2026-07-18

## Summary

Added an immutable per-run PDF v3 runtime manifest and required the local page
processor to derive provider/model/font/render configuration from a validated
live binding.

## Implementation

- added deterministic component, model, renderer-policy and font asset
  identities;
- bound the manifest to the scheduler source, exact PageSet, language and
  schema identities;
- added a 64 KiB bounded strict JSON decoder and content-derived manifest ID;
- added immutable atomic commit with idempotent same-content reopen and explicit
  conflict rejection;
- excluded provider credentials, endpoints, file paths and document text;
- validated live provider kind, platform/architecture and exact font bytes;
- exposed stable provider IDs from the existing provider configuration;
- made page-processor configuration fields private and added construction from
  the bound runtime only.

## Validation

- `cargo test --locked pdf_v3 --lib` (`181 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs --lib` (`89 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The native run can now retain one reproducible translation runtime identity.
Tauri lifecycle assembly, verified component launch/model health, typed status
commands and final exporter orchestration remain pending.
