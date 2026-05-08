# 2026-05-08 Preview Sidebar Export Polish

## Summary

Polished the first document-style task workflow after manual testing.

## Changes

- Reduced bilingual preview scroll jank:
  - synchronized scroll updates now run through `requestAnimationFrame`
  - hover highlight state is paused while scrolling
  - preview block color transitions were removed from the hot scroll path
- Fixed preview panel sizing:
  - task page now uses a full-height grid
  - preview card and panes fill the remaining task page height instead of using a fixed 520px pane height
- Changed untranslated translation preview behavior:
  - translatable blocks now render blank on the translation side until translated
  - skipped blocks such as code remain visible
- Reduced excessive Markdown export blank lines:
  - Markdown export now joins blocks with structure-aware separators
  - consecutive list items no longer get paragraph blank lines between them
  - blank metadata blocks are normalized instead of multiplying empty paragraphs
- Updated sidebar project structure:
  - project rows can expand to show imported files
  - job summaries now store `sourceFiles` for lightweight project tree rendering
  - projects can be renamed from the sidebar; default names still come from the imported file or folder
- Added single-file selection for multi-file projects:
  - sidebar file clicks update the frontend `activeFileId`
  - document preview renders only the selected file
  - project row clicks default to the project's first file

## Boundaries

- File entries in the sidebar select the current preview file and navigate to the project.
- Project rename changes the displayed project name, not the original source filename.
- No dev server or build command was run.

## Validation

Executed:

```txt
cargo fmt
cargo test rosetta_jobs
cargo check
corepack pnpm typecheck
```
