# File Translation Revisions

## Summary

Completed files can now be translated again without losing the previous translation. Rosetta now stores user-facing history as file-level translation revisions, so the preview can switch between the current translation and past complete file translations.

## Changes

- Added `TranslationRevision` and `translation_revisions.json` under each job directory.
- `RosettaJobBundle` now returns `translationRevisions` alongside the document and segments.
- Full-file retranslation saves the current file's complete translated snapshot before clearing current translations.
- Selection retranslation also saves a complete current-file snapshot, then resets only the selected blocks' segments.
- Language direction changes save file-level snapshots before clearing stale translations.
- The task workbench primary action changes from a disabled completed state to `重新翻译全文` when the selected file has completed translatable segments.
- The document preview replaces the old history sheet with a translation-side version selector:
  - `当前译文`
  - past complete file translation versions
- Segment-level `translationHistory` is kept as a compatibility field for older caches, but it is no longer the default UI history source.
- Active translation runs are tracked in frontend state so retranslation progress starts from `0 / N` and the status stays `翻译中` between batches.

## Notes

- History is file-scoped because the user expectation is to compare complete past translations, not inspect individual segment records.
- Current file export still uses only the active translation, not historical translations.
- Future editing UI can add restore-from-history or export-history as separate commands without changing the current export boundary.

## Validation

- `corepack pnpm typecheck`
- `cargo test rosetta_jobs`
