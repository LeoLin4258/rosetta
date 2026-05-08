# 2026-05-08 Directory Import And Document Preview

## Summary

Improved the short-term Rosetta user flow after the first TXT/Markdown MVP test:

```txt
folder or file import
  -> multi-file JSON job
  -> RWKV batch translation
  -> document-style bilingual preview
  -> single-file or directory export
```

The managed in-app RWKV runtime remains paused. Translation still uses the external RWKV `/v1/chat/completions` connector.

## Changes

- Replaced blocking Windows file/save dialogs with non-blocking Tauri dialog commands:
  - `pick_rosetta_import_path`
  - `pick_rosetta_import_directory`
  - `pick_rosetta_export_path`
  - `pick_rosetta_export_directory`
- Added directory project import:
  - recursively collects TXT/Markdown files
  - stores per-file metadata in `RosettaDocument.files`
  - assigns `fileId` to blocks and segments
  - keeps each source file's project-relative path
- Added directory export:
  - multi-file projects export to a selected folder
  - output files preserve source relative paths
  - output names use `.zh` or `.bilingual` suffixes
- Updated `/new`:
  - removed the technical “current flow” card
  - made folder import the primary action
  - kept single-file import as a secondary action
- Replaced the default segment-list preview with a document-style bilingual preview:
  - left side renders source structure
  - right side renders translated structure
  - panes use synchronized scrolling
  - hovering a block highlights the corresponding block on both sides
  - Markdown preview uses `react-markdown` and `remark-gfm`

## Boundaries

- Markdown import is still a lightweight block parser, not a full CommonMark AST roundtrip.
- Document-style preview currently renders by block. Very large projects may need virtualized document blocks in a later pass.
- Directory import is limited to TXT/Markdown and capped to avoid loading huge projects into the prototype UI.
- No dev server or production build was run.

## Validation

Executed during implementation:

```txt
cargo fmt
cargo test rosetta_jobs
cargo check
corepack pnpm typecheck
```

Full validation status is recorded in the final task response.
