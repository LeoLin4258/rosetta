# 2026-05-11 Workbench Preview Window

## Scope

- Changed the main job workbench from an embedded bilingual preview into a source/translation file list.
- Source files are listed in the left pane; translation files for the selected source file are listed in the right pane.
- The source and translation panes use shadcn `Resizable` so users can adjust the two list widths.
- Workbench actions are separated by scope: row-level actions for existing translation files and source-file-only batch creation/translation from the left pane.
- Batch translation now creates/reuses all requested translation files first and marks them as queued in the list before translating them sequentially.
- Translation file switching is now lightweight and does not load translation segment bodies in the main window.
- Double-clicking a source file opens an independent source-only preview window.
- Double-clicking a translation file opens an independent preview window for side-by-side source and translation reading.
- The independent preview window supports synchronized source/translation hover highlighting and block-level selection for partial retranslation.
- Added the minimal Tauri webview window permission needed for the main window to open translation preview windows.

## Rationale

Embedding the long-document bilingual reader inside the workbench made source file switching expensive because each selection could trigger large preview rendering, virtualizer measurement, and translation body loading. The product model is clearer if the main window acts like a project file manager and the heavy bilingual reader is opened only when the user explicitly opens a translation file.

## Validation

- `corepack pnpm typecheck`
- `cargo check`
