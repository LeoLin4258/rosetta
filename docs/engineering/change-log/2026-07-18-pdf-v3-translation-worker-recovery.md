# PDF v3 Translation Worker and Recovery Inventory

Date: 2026-07-18

## Summary

Connected scheduler translation leases to validated PageGraph and
TranslationPatch durable authorities through a bounded provider-neutral worker.

## Implementation

- added an exact scheduler translation binding for source, PageSet, language,
  engine, schema and renderer identity;
- added a sequential translation worker that loads and retains one PageGraph at
  a time;
- required resolved patch validation and durable patch commit before scheduler
  completion authority;
- added explicit source-preservation results without placeholder patches;
- added stable retryable/non-retryable translation failure transitions;
- added claim, PageGraph load, processor, patch-store and scheduler timing data;
- added bounded assembly of complete validated extraction/patch recovery
  inventories;
- hardened TranslationPatch repair so missing/corrupt content remains page-local
  while real filesystem I/O errors propagate.

## Validation

- focused translation worker, recovery, invalid-binding, preservation and patch
  I/O tests;
- `cargo test --locked pdf_v3 --lib`;
- `cargo test --locked rosetta_jobs --lib`;
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

The durable translation lifecycle and crash inventory are connected inside the
isolated PDF v3 module. The concrete async local-provider planner/renderer
adapter, Tauri lifecycle API, frontend status and real complex 500/1,000-page
translation/export validation remain pending.
