# PDF No-Text Page Preview Fallback

Date: 2026-07-13

## Summary

Fixed completed PDF pages with no extractable text appearing as blank
"untranslated" placeholders in the bilingual preview.

The reported `The Wealth Ladder` run correctly committed its first two scanned
cover pages as `resultKind="no_text"`. A later artifact repair incorrectly
changed them from `translated` to `pending` because they had no translated PDF
artifact. That artifact absence is valid for `no_text` pages.

## Change

- Artifact repair now preserves `translated/no_text` pages without requiring a
  translated page PDF.
- Existing `pending/no_text` state is normalized back to `translated` when the
  page state is read, so affected jobs repair without retranslation.
- Continue-mode scheduling treats `no_text` pages as completed and does not
  send them through the PDF engine again.
- The bilingual translated pane reuses the already-rendered source-page PNG for
  `no_text` pages instead of showing an untranslated placeholder.
- The source raster is shared with the translated pane, avoiding duplicate PDF
  rasterization.
- Added accessible image text explaining that the translated pane is showing
  the original page because no translatable text was found.

No page-state schema or artifact path changed.

## Validation

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo fmt --check
cargo check
cargo test rosetta_jobs
```

Results:

- `pnpm typecheck`: passed.
- `cargo fmt --check`: passed.
- `cargo check`: passed.
- `cargo test rosetta_jobs`: 69 passed, 0 failed.
- Added regression coverage for restoring `pending/no_text` state and
  preserving `translated/no_text` through artifact repair.
