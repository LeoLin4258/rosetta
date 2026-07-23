# PDF v3 Lazy Form Invocation and Copy-on-Write

Date: 2026-07-18

## Summary

Moved Form invocation validation and copy-on-write clone-tree staging from the
complete PDF document onto owned lazy resource contexts.

## Implementation

- extended `PdfPageObjectContext` with an owned `PdfResourceContext`;
- overlaid Form-local resources on parent effective resources with nearest-scope
  precedence;
- resolved indirect resource dictionaries, category dictionaries and XObject
  streams through `PdfObjectView`;
- bounded indirect reference traversal and rejected resource cycles;
- split production COW staging from the `Document` compatibility wrapper;
- sourced page content identity from `PdfIndexedPage` and page rewrites from
  `PdfPageObjectContext`;
- sourced invocation/root/Form streams from the immutable source view;
- allocated clone IDs above the accumulated view maximum and reserved font
  range;
- kept the complete `Document` only for global cross-page stream/Form ownership
  discovery in production replacement staging.

## Current Boundary

- selected-page, source-stream, Form invocation, effective-resource and COW
  clone-tree staging now use bounded lazy views;
- global cross-page top-level stream ownership still calls complete
  `Document::get_pages()`;
- the conservative compatibility Form ownership scan also still enumerates the
  complete document;
- global ownership discovery is therefore the final major complete-document
  dependency before renderer memory can be claimed end-to-end bounded.

## Validation

- PDF v3: 132 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo check`, Rust formatting and frontend typecheck passed;
- lazy nested Form staging: 8 source loads, 11 cache hits, 8 resident entries
  and 11,272 resident bytes;
- lazy and `Document` adapter stages produce identical page dictionaries, four
  cloned streams and maximum object numbers;
- repeated-Form source/output: 13,129 / 24,244 bytes;
- Poppler: 12,747 changed pixels, 0.585607%, confined to
  `[118, 1057) x [125, 229)`;
- `pypdf`: one page and identical source metadata retained; output text is
  exactly `ALPHA` and `BETA`;
- PDFium and visual inspection found no clipping, overlap or unrelated movement.
