# 2026-05-20 PDF Page-Level Translation

## Summary

Added page-level visual PDF translation so Rosetta can show translated PDF pages as soon as each page completes. Users can select pages by range or per-page checkbox, skip already translated pages by default, force retranslation, and export a complete PDF where untranslated pages are preserved from the source.

## Scope

- Added `pdf_page_translations.json` as PDF-specific page state.
- Added page-level PDF caches under `pdf-pages/page-000N.pdf`.
- Added Tauri commands for page state, page translation, and page-level translated-page rasterization.
- Extended the `pdf2zh` invocation to pass `--pages`.
- Updated the PDF preview UI with synchronized page range input and per-page checkboxes.
- Updated PDF export to assemble a complete output from translated page PDFs plus original source pages.
- Added `lopdf` as a narrow dependency for PDF page assembly. pdfium remains the renderer for preview PNGs.

## Validation

Relevant checks:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

## Notes

`pdfium-render` is still used for import pre-flight page counts and rasterized preview. It did not expose a suitable high-level page-copy/save API in the currently used crate version, so `lopdf` is used only to assemble exported PDFs.
