# PDF Page-Level Translation Design

## Context

Rosetta currently treats visual PDF translation as an end-to-end `pdf2zh` run:

1. import caches the source PDF and creates a lightweight PDF job;
2. translation calls `pdf2zh` for the whole document;
3. only after `pdf2zh` finishes does Rosetta copy `translated.pdf`;
4. the preview rasterizes the completed translated PDF page by page.

This preserves PDF layout, but the user cannot preview any translated page until the whole file is done. On a local M4 Mac mini, a text-heavy 5-page PDF can take around ten minutes, which makes the app feel stuck even when progress events are visible.

The new experience keeps PDF layout as the primary PDF translation promise. It does not introduce chat, summarization, cloud upload, OCR, or a separate generic assistant flow.

## Goals

- Let users choose which PDF pages to translate.
- Show each translated page as soon as that page is complete.
- Keep the right pane as visual translated PDF pages, not a temporary text preview.
- Export a complete PDF with translated pages substituted in and untranslated pages preserved from the source PDF.
- Reuse the existing local `pdf2zh`, pdfium rasterization, Tauri command, and workbench patterns.

## Non-Goals

- OCR for scanned PDFs.
- Text-mode fallback preview for PDF page translation.
- Editing translated PDF text inside Rosetta.
- Bilingual PDF export.
- Perfect reconstruction for PDFs that `pdf2zh` itself cannot handle.
- Turning PDF jobs back into Rosetta block/segment text jobs.

## User Experience

PDF preview remains a two-pane workbench:

- left: source PDF pages;
- right: page-level translated output.

The preview header gains page-selection controls:

- a compact range input accepting values such as `1-3,5`;
- per-page checkboxes shown alongside page rows;
- the input and checkboxes stay synchronized;
- a "Translate selected pages" action;
- a "Retranslate selected pages" toggle.

Default behavior:

- selected pages that already have page translations are skipped;
- enabling retranslation overwrites selected translated pages;
- failed pages remain selectable for retry;
- stopping the run preserves completed page translations and returns the current page to retryable state.

During translation, the right pane shows page states:

- translated pages render their page-level translated PDF;
- the currently running page shows a translating placeholder;
- pending selected pages show queued placeholders;
- unselected or untranslated pages show that they will be preserved from the source in export.

Export always writes a complete PDF. If the user selected only pages 2-3 in a 5-page source PDF, the exported PDF has all 5 pages: pages 2-3 translated, pages 1, 4, and 5 preserved from the source.

## Architecture

The core change is to stop treating translated PDF output as a single all-or-nothing artifact.

```txt
source.pdf
  -> page selection
  -> pdf2zh --pages <page>
  -> page-level translated PDFs
  -> live per-page rasterized preview
  -> final full PDF assembly on export
```

### Backend

Add a page-level PDF translation layer under `src-tauri/src/rosetta_jobs/formats/pdf/`.

Suggested modules:

```txt
formats/pdf/
  page_state.rs
  page_translate.rs
  page_assemble.rs
```

Responsibilities:

- parse and validate page selections using the source PDF page count;
- persist page translation state in the job directory;
- call `pdf2zh --pages <page>` for each requested page;
- cache each completed translated page PDF;
- emit page-level progress events;
- assemble a complete export PDF from translated page PDFs and original source pages.

Keep `generate_rosetta_translated_pdf` as a compatibility wrapper that translates all pages through the new page-level path, so existing UI and export assumptions have a migration bridge while new commands support explicit page selections.

### Job Cache Layout

Add PDF-specific artifacts inside the existing job directory:

```txt
<jobId>/
  source.pdf
  pdf_page_translations.json
  pdf-pages/
    page-0001.pdf
    page-0002.pdf
  exports/
    translated.pdf
```

`pdf_page_translations.json` stores durable page state:

```json
{
  "schemaVersion": 1,
  "sourcePageCount": 5,
  "targetLang": "zh-CN",
  "pages": [
    {
      "pageNumber": 1,
      "status": "translated",
      "translatedPdfPath": "pdf-pages/page-0001.pdf",
      "updatedAt": "..."
    }
  ]
}
```

Supported statuses:

- `pending`
- `queued`
- `translating`
- `translated`
- `failed`

On app/job load, stale `translating` pages are restored to `pending` with a clear retry path, matching existing translation-file conventions for interrupted work.

### PDF Assembly

Export needs a narrow "page substitution" capability:

- for each source page number:
  - if a translated page PDF exists, take that translated page;
  - otherwise take the original source page;
- write the combined result to `exports/translated.pdf` or the user-selected export path.

Before implementation, verify whether `pdfium-render` can safely create the assembled PDF. If not, add a focused PDF manipulation dependency and document it in the engineering change log. If the dependency creates a durable architecture constraint, add an ADR.

### Frontend

Update the PDF preview components without replacing the workbench model:

- `PdfDocumentPreview` owns the page-selection input and run actions.
- `PdfPane` gains optional per-page controls and a page rendering source callback.
- translated pages render from a new backend kind such as `translatedPage` with a `pageNumber`.
- page completion events update only the relevant page cache key.

The source pane can continue using the existing `source` PDF renderer. The translated pane should support mixed state: translated page PDFs, placeholders, and preserved-source indicators.

### Tauri Commands

Add narrow commands instead of broad filesystem access:

- `get_rosetta_pdf_page_status(job_id)`;
- `translate_rosetta_pdf_pages(job_id, page_selection, options)`;
- `cancel_rosetta_pdf_page_translation()`;
- `render_rosetta_pdf_translated_page_as_png(job_id, page_number, target_width)`;
- `export_rosetta_pdf_with_page_translations(job_id, target_path)`.

Command names can be adjusted to match existing style, but the API boundary should stay page-focused.

## Error Handling

- Invalid page ranges show a local validation error before starting translation.
- `pdf2zh` failure marks only the current page as failed, continues with later selected pages, and leaves the run in a partial-failure state if any page failed.
- Cancel preserves already translated pages.
- Export succeeds even when not all pages are translated, because untranslated pages are preserved from source.
- If PDF assembly fails, keep page caches intact so the user can retry export without retranslating.

## Testing

Backend tests:

- page range parser accepts `1-3,5` and rejects invalid/out-of-range input;
- translating a selected subset records page status;
- stale translating page state is restored on load;
- export assembly preserves source page count;
- export assembly substitutes only translated pages;
- failed page status does not block export of other pages.

Frontend tests or focused component checks:

- range input and checkboxes stay synchronized;
- already translated pages are skipped by default;
- retranslation toggle includes translated pages;
- per-page completion updates the translated pane without requiring full job reload.

Validation commands when implementing:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Do not run dev servers or production builds unless specifically requested.

## Implementation Slice

Start with a vertical slice:

1. parse page selection;
2. translate one selected page through `pdf2zh --pages`;
3. cache the page-level translated PDF;
4. render that translated page in the right pane;
5. export a complete PDF for a 3-page fixture where only page 2 is translated.

Once that works, expand to multiple selected pages, stop/retry behavior, and retranslation controls.
