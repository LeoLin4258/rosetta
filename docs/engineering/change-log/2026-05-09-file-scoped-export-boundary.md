# File-Scoped Export Boundary

## Summary

The task workbench now treats the selected file as the export unit. This fixes a mismatch where the UI looked file-focused but the export action still called project-level commands and could export every file in a multi-file project.

## Changes

- Added `export_rosetta_job_file` as the Tauri command used by the workbench.
- Removed project-level export commands from the active invoke handler so the current frontend path cannot accidentally export the whole project.
- Updated the Jobs page to export only the selected file.
- Disabled export buttons until the selected file is fully processed.
- Added Rust checks that reject file export when segments are pending, failed, translating, or have empty translations.
- Updated data model conventions to record that the task workbench is file-scoped and future project batch export needs its own explicit entry point.

## Validation

- Pending or failed current files should not expose enabled export actions.
- Completed current files export to a user-selected file path.
- Multi-file projects no longer export every file from the current file toolbar.
