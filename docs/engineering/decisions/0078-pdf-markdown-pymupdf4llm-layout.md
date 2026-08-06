# ADR 0078: PDF Markdown Translation with PyMuPDF4LLM Layout

Date: 2026-08-06

Status: Accepted

## Context

Rosetta currently treats a PDF as a visual document. Import stores a job-local
`source.pdf`; translation is performed by the production `pdf2zh` pipeline and
the result is exported as PDF. This path preserves page appearance, but it does
not provide a structured, editable Markdown output.

Local experiments compared Docling, Xberg, pdftext, a direct `pdf_oxide`
recovery layer, and PyMuPDF4LLM. PyMuPDF4LLM Layout gave the best overall
reading order, heading, table, footnote, figure and caption separation on the
tested non-OCR PDFs while remaining materially smaller and faster than
Docling. Direct extraction plus Rosetta-owned layout rules failed on complex
figures and multi-column reading order; continuing that route would amount to
building and tuning a layout engine.

The selected product behavior is one imported PDF with two sibling translated
output modes in the workbench:

- `PDF`: the existing visual `pdf2zh` translation.
- `Markdown`: structured extraction followed by Rosetta's ordinary local text
  translation and deterministic Markdown rendering.

## Decision

Rosetta will add PDF-to-Markdown using these boundaries:

1. Pin `pymupdf4llm 1.28.0`, `pymupdf-layout 1.28.0`, and `PyMuPDF 1.28.0`.
   PDF Markdown v1 is CPU-only and calls extraction with `use_ocr=False`.
2. Treat `to_json()` as the vendor integration boundary. Rosetta will
   normalize the structured JSON into `RosettaBlock` and `Segment` records and
   will render Markdown itself. Raw `to_markdown()` output is not persistent
   authority.
3. Keep `source.pdf` as the sole source authority. Vendor JSON, extracted
   images and rendered previews are versioned, disposable derivatives.
4. Keep the existing production `pdf2zh` worker, page state, preview and PDF
   export unchanged. PDF Markdown runs in a separate Python worker process so
   its PyMuPDF version cannot alter the visual PDF pipeline.
5. Distribute the Markdown engine as an optional managed component. It may
   reuse the installed PDF pack's CPython, NumPy and ONNX Runtime, but its
   `site-packages` overlay and worker environment remain isolated from
   `pdf2zh`.
6. Add an explicit `outputFormat` to translation-file identity. The unique
   identity becomes `(sourceFileId, targetLang, outputFormat)`, allowing PDF
   and Markdown translations to coexist.
7. Preserve legacy translation-file IDs for a source's native output format.
   Existing PDF records infer `pdf`; Markdown records created from a PDF use a
   format-qualified ID. Loading old jobs must not eagerly rewrite their files.
8. Normalize PDF Markdown blocks into the existing job-level `document.json`
   and `segments.json`. PDF visual mode must continue to ignore these segments
   and derive its state only from durable PDF page/run authority. Markdown
   translation-file progress must not overwrite the legacy PDF page summary.
9. Markdown v1 always means the complete source document. Existing page-range
   selection remains a PDF-output control; 5-10 page extraction windows are an
   internal scheduling and recovery detail, not a partial-document product
   option.
10. Generate Markdown structure from trusted metadata, never from translated
    model output. Translate text payloads only. Omit page headers and footers,
    preserve formulas in the source language, export pictures as sibling
    assets, and translate captions, footnotes and table-cell text.

## Persistence

The selected job-local derivative layout is:

```text
<jobId>/
  source.pdf
  document.json
  segments.json
  translation_files.json
  translations/
  pdf-markdown/
    manifest.json
    extraction/
      pages/page-0001.json.gz
    images/
      page-0001-picture-01.png
    .tmp/<runId>/
```

`manifest.json` binds the source fingerprint, page count, engine and package
versions, extraction-policy version, OCR/image policy and committed page
shards. A mismatch makes the extraction cache stale; it does not invalidate
`source.pdf` or existing PDF output.

The exact normalized block metadata and migration rules are defined by the
active implementation plan. They must be recorded in the data-model
convention when implementation begins.

## Consequences

- Users can keep visual PDF and structured Markdown translations for the same
  PDF and language without re-importing the source.
- Rosetta adopts a mature layout engine instead of maintaining a large body of
  PDF reading-order rules.
- Markdown output is deterministic and testable because translation never
  owns Markdown syntax.
- The optional component adds packaging and cross-platform release work. The
  current Windows spike adds 60.3 MiB compressed to the existing PDF component.
  Together they are 429,302,670 bytes (409.4 MiB), so they do not yet satisfy
  the 400 MiB cumulative PDF-component budget. Release is therefore gated on a
  reproducible trimmed-pack result; this ADR does not promise that the current
  spike artifact will ship.
- Image extraction makes Markdown export a multi-file operation. Export must
  stage the `.md` file and its sibling asset directory before replacing the
  destination.
- Scanned/image-only PDFs, OCR, equation translation and exact visual layout
  preservation are outside Markdown v1.

## References

- [PDF Markdown implementation plan](../plans/2026-08-06-pdf-markdown-translation.md)
- [Lightweight alternatives benchmark](../benchmarks/2026-08-06-lightweight-pdf-to-markdown-alternatives.md)
- [Direct pdf_oxide validation](../benchmarks/2026-08-06-pdf-oxide-direct-validation.md)
- [PyMuPDF4LLM API](https://pymupdf.readthedocs.io/en/latest/pymupdf4llm/api.html)
- [PDF4LLM JSON schema](https://docs.pdf4llm.com/python/reference/JSON-schema)
