# ADR 0009: PDF Translation Engine Contract v2

Date: 2026-07-03

## Status

Accepted.

## Context

Rosetta beta's PDF path grew from a pragmatic pdf2zh integration:

- pdf2zh called an OpenAI-compatible shim.
- Rosetta adapted RWKV behind that shim.
- Later cross-page batching used deferred collection plus replay translators.
- Page quality was inferred from worker diagnostics and shim metrics.

That worked well enough to learn the product constraints, but it created the
wrong ownership boundary. PDF layout, unit extraction, and page rendering lived
inside Python/pdf2zh, while translation correctness and commit decisions were
partly inferred from logs and diagnostics outside that engine. This made blank
translated pages possible when translation replay or output accounting drifted.

Rosetta is still beta and can reset existing PDF derived artifacts as long as
the job-local source PDF is preserved. We should therefore optimize for a clean
product contract rather than compatibility with the beta PDF page state.

## Decision

Rosetta PDF translation v2 uses a Rosetta-native typed engine contract in the
PDFMathTranslate fork as the only product entry point.

The PDF engine owns:

- PDF preprocessing and font injection.
- Layout inference and pdfminer/pdf2zh page parsing.
- Translation unit collection.
- Rendering pages from `unitId -> translation`.
- Single-page translated artifact generation.
- Structured `PageResult` reporting.

The PDF engine must not call:

- RWKV.
- OpenAI-compatible APIs.
- Rosetta HTTP translation endpoints.
- translator services or shim servers.

The product protocol is:

```txt
prepare_pdf_window
translate units in Rust
render_pdf_window
dispose_pdf_window
```

`TranslationUnit` is the only text payload boundary between the PDF engine and
Rust translation orchestration. It includes `unitId`, `pageNumber`,
`orderOnPage`, `sourceText`, `sourceChars`, `kind`, and
`requiresTranslation`.

`PageResult` is the only page commit boundary. Rust does not read diagnostics
to decide whether a page is successful.

## Page Commit Contract

Rust commits a page only when `PageResult` satisfies the formal contract:

- `status="translated"` requires a readable one-page PDF artifact.
- `sourceUnitCount > 0 && translatedChars == 0` fails the page.
- `emptyTranslationCount > 0` fails the page unless the engine counted only
  non-required units as empty.
- `placeholderMismatchCount > 0` fails the page.
- `status="no_text"` completes the page as `resultKind="no_text"` without
  pretending there is translated text or a translated page artifact.
- `status="failed"`, provider failure, translation count mismatch, truncation,
  worker crash, and render failure all produce explicit page/run failure.

Diagnostics remain useful for performance and postmortems, but not for business
control flow.

## State And Compatibility

PDF page state is bumped to schema version 2. v2 page records include minimal
engine result metadata:

- `resultKind`
- `sourceUnitCount`
- `translatedUnitCount`
- `sourceChars`
- `translatedChars`
- `artifactBytes`
- `artifactCompression`
- `lastRunId`

PDF v2 does not migrate beta PDF translation artifacts. When Rosetta reads v1
PDF page state, it removes derived translated artifacts and PDF page-state
files, preserves `source.pdf`, and returns an empty pending v2 state. Users can
retranslate from the preserved source PDF.

## Performance Guardrails

The v2 contract keeps the lessons from the beta PDF path:

- Small PDFs keep the live page-by-page feel. 1-30 page runs may use a wide
  window.
- Long PDFs use 10-page windows so the app stays responsive and memory pressure
  is bounded.
- Runs selecting more than 50 pages require user confirmation.
- Lightning aggregates all units in a window into large ordered batches to keep
  RWKV fed.
- A full document must not block the first visible page: once a window's
  translations return, pages render and commit in order.
- Long active PDF runs pause translated PNG live raster; status remains live.
- Fast single-page artifacts are allowed on the hot path, but committed byte
  size is recorded and background compression handles disk pressure. Background
  compression must subset embedded fonts before deflate/object-stream saving so
  each page does not keep a full CJK font copy.
- Worker prewarm loads the PDF engine, the ONNX layout model, and runs one
  synthetic layout prediction before reporting ready capabilities.

## Privacy Guardrails

The local protocol may pass source text to Rust translation providers. Logs,
diagnostics, timeline files, profiles, and benchmark summaries must not record
source text, translations, prompts, or raw provider responses.

## Consequences

Positive:

- Blank translated artifacts are rejected by a typed commit contract.
- The PDF component has a clear API Rosetta can version-check.
- Translation provider behavior is centralized in Rust.
- The Python worker becomes a thin protocol host instead of a second
  translation orchestrator.
- Long-PDF stability and Lightning batching are both first-class policy rather
  than emergent shim behavior.

Costs:

- Existing beta PDF derived artifacts are discarded and must be regenerated.
- The PDF component pack must include a PDFMathTranslate fork with the v2
  Rosetta engine API.
- Old packs fail clearly instead of silently falling back to CLI or shim paths.

## Rejected Alternatives

- Keep the OpenAI-compatible shim and add more diagnostics checks.
  This preserves the wrong boundary and continues to infer correctness from
  side channels.
- Keep the deferred/replay translator outside the PDF fork.
  This keeps Rosetta coupled to pdf2zh internals while still duplicating layout
  ownership outside the engine.
- Migrate v1 page artifacts.
  Rosetta is beta and source PDFs are preserved, so migration would add
  complexity without protecting stable user data.
