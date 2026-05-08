# 2026-05-08 Rosetta Job Flow MVP

## Summary

Implemented the first end-to-end Rosetta user flow:

```txt
import TXT/Markdown
  -> JSON job cache
  -> segment preview
  -> RWKV API batch translation
  -> save translated segments
  -> export translation or bilingual output
  -> delete project cache
```

Managed in-app RWKV runtime remains paused. Translation still uses the confirmed external `/v1/chat/completions` batch `contents[]` connector.

## Changes

- Added a Rust/Tauri job store for app data JSON persistence.
- Added narrow Tauri commands for:
  - picking import/export paths through system dialogs
  - importing TXT/Markdown files
  - listing/loading/deleting jobs
  - saving translated segments
  - exporting pure translation or bilingual output
- Added minimal TXT and Markdown parsing:
  - TXT paragraphs split on blank lines
  - Markdown headings, paragraphs, list items, blockquotes, blank lines, and fenced code blocks
  - code blocks, plain URL lines, and blank lines are skipped
- Updated the frontend store to use real job summaries and active job bundles instead of demo-only state.
- Updated `/new`, `/jobs`, `/jobs/:jobId`, the sidebar, and preview flow for real projects.
- Added a batch translation scheduler in the Jobs page:
  - pending/failed segments only
  - 16 segments per batch
  - each successful batch is saved immediately
  - failed batches mark segments failed and stop later batches
- Added export actions for translation and bilingual output.

## Boundaries

- No DOCX/PDF support in this step.
- No streaming translation parser.
- No background queue, pause/resume, or SQLite.
- Markdown export preserves basic structure only; it is not a full CommonMark roundtrip.
- API credentials remain in local settings and are not written into job JSON.
- Source files are copied into Rosetta's job cache, but user original files are not modified or deleted.

## Validation

Executed:

```txt
cargo fmt
cargo test rosetta_jobs
cargo test rwkv_api
cargo test
cargo check
corepack pnpm typecheck
```

Per project instruction, no dev server or build command was run.
