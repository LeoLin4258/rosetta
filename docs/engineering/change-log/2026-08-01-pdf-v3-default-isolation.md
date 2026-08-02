# 2026-08-01 PDF v3 Isolation and Removal

## Summary

Removed the unused PDF v3 frontend control surface, then deleted its archived
native command, lifecycle, preview, export, worker, IR, and renderer
implementation after the Linux observation gate. Production PDF translation,
source preview, translated-page preview, export, and canonical SHA-256 source
identity remain enabled.

## Changes

- First isolated eleven unused PDF v3 Tauri commands behind the default-off
  `experimental-pdf-v3` Cargo feature.
- Removed the unused frontend PDF v3 hooks, command wrappers, and response
  types.
- Created and pushed the pre-removal snapshot tag
  `archive/pdf-v3-pre-removal-2026-08-02`.
- Deleted 48,199 lines of archived native v3 Rust, its eleven runtime modules,
  command/state/cleanup wiring, feature, and recovery build.
- Removed the now-unused `pdf`, `memmap2`, `subsetter`, and `ttf-parser`
  dependencies while retaining production PDF dependencies.
- Replaced the test-only `DocumentHandle` comparison with a direct canonical
  SHA-256 fixture contract. A deeper caller audit showed the proposed v3 DTO
  move was unnecessary because those plan/result types had no production
  consumer.
- Replaced the isolation check with a production-boundary check that rejects
  restored v3 modules, commands, frontend types, feature wiring, or unused
  dependencies while requiring the production wrappers and renderer crates.

The removal does not change persistent job formats, migrate existing data,
alter the production PDF translation workflow, rebuild PDF packs, or release a
new app version.

## Managed PDF Compatibility

The same integration restores the managed PDF runtime minimum to engine
revision 1. `resource-manager-reuse` remains a build-time AST verification for
new packs, not a runtime protocol requirement, so the frozen Windows, macOS,
and Linux release packs are not incorrectly treated as outdated. The managed
PDF test suite now runs in main application CI.

## Validation

- `pnpm typecheck`
- `pnpm check:pdf-production-boundary`
- `cargo fmt --all -- --check`
- `cargo check`
- `cargo test rosetta_jobs`
- `cargo test managed_pdf2zh`
- `python scripts/test-pdf2zh-patches.py -q`
- `git diff --check`

No development server or production build was run.
