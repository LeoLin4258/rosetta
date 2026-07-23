# PDF v3 Lazy Translated-Page Preview

Date: 2026-07-19

## Summary

Added the production PDF v3 translated-page preview path that renders one exact
completed page without loading the complete source PDF or requiring the local
translation model to remain active.

## Implementation

- added a bounded selected-page PDF materializer over the lazy source-object
  store, including inherited resources and page geometry;
- rejected cross-page page-tree traversal and bounded reachable objects and
  recursion depth;
- added a Tauri binary command for exact run/page/width translated PNG preview;
- required scheduler extraction/patch authority to match durable PageGraph,
  TranslationPatch and runtime-manifest identity;
- returned cached PNG immediately, reused cached page PDF after source identity
  verification, and loaded unified font assets only on complete cache miss;
- separated render-font resolution from managed provider/model process health;
- added a 32-entry file-stamp source fingerprint cache so page navigation does
  not rehash an unchanged long PDF on every request;
- exposed a typed frontend `Uint8Array` wrapper without returning paths or text.

## Validation

- `cargo test --locked page_pdf` (`4 passed`);
- `cargo test --locked v3_preview` (`6 passed`);
- `cargo test --locked pdf_v3` (`183 passed`, `19 ignored`);
- `cargo test --locked rosetta_jobs` (`123 passed`);
- `cargo test --locked managed_rwkv` (`53 passed`);
- `cargo check --locked`;
- `cargo fmt --all -- --check`;
- `pnpm typecheck`.

## Current Boundary

Completed translated pages now have a production lazy preview path. Preserved
pages intentionally reuse source preview, complete workbench UI wiring remains
pending, and real complex 500/1,000-page end-to-end translation/export stress
validation is still required.
