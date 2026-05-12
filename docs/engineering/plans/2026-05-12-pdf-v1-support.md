# PDF v1 Support Plan

## Summary

Rosetta v1 must support PDF import because PDF is a required first-version document format. The baseline PDF milestone is text-based PDF import into the existing Rosetta document pipeline. High-fidelity PDF structure or layout restoration is valuable, but it is a nice-to-have enhancement path rather than the minimum acceptance gate for v1.

PDF support must preserve Rosetta's product boundary:

- local file parsing
- no cloud upload
- no chat, Q&A, summarization, or rewriting
- document structure converted into Rosetta IR before translation
- preview and export reuse the existing block/segment/translation-file workflow

## v1 Scope

Supported in v1:

- Import text-based `.pdf` files.
- Extract readable text from pages.
- Convert extracted text into `RosettaDocument`, `RosettaSourceFile`, `RosettaBlock[]`, and `Segment[]`.
- Preserve stable reading order as far as the parser can determine.
- Store minimal PDF provenance in `RosettaBlock.style`, such as page number and order within page.
- Translate PDF-derived segments through the existing RWKV batch translation workflow.
- Preview PDF-derived content through the existing virtualized block preview.
- Export translated or bilingual results through text-like outputs, initially TXT/Markdown-style output rather than writing back into a PDF file.

Nice to have in v1, if the PDF contributor can deliver it without destabilizing the baseline pipeline:

- Better multi-column reading order recovery.
- More accurate heading, caption, footnote, table, and list detection.
- Page-aware preview affordances, as long as the primary translation reading view stays block-virtualized.
- Higher-fidelity export experiments, including layout-aware intermediate data, when guarded behind clear implementation boundaries.
- Near-original PDF format restoration for a constrained class of text-based PDFs.

Not part of the v1 baseline:

- OCR for scanned PDFs.
- Recreating the original PDF layout for arbitrary PDFs.
- Writing translated text back into the source PDF as the default export path.
- Perfect support for all multi-column papers, equations, tables, footnotes, headers, or captions.
- Executing embedded scripts, loading remote resources, or trusting PDF metadata as executable content.

## Ownership Boundary

PDF work should be implemented as a document importer, not as a separate product flow.

Recommended Rust module boundary:

```txt
src-tauri/src/rosetta_jobs/
  mod.rs
  import.rs
  export.rs
  store.rs
  formats/
    mod.rs
    txt.rs
    markdown.rs
    pdf.rs
```

The PDF contributor should primarily own:

- `formats/pdf.rs`
- PDF fixtures
- PDF parser unit tests
- small integration points in import path detection and file picker filters
- preview labels or copy that mention PDF support

They should not own:

- RWKV API connector behavior
- translation scheduling policy
- global Zustand state structure
- broad Tauri permissions
- updater/runtime management
- unrelated workbench UI refactors

If the contributor pursues high-fidelity restoration, keep it as a bounded PDF submodule or experimental exporter. It should not require broad changes to translation scheduling, job selection, RWKV API behavior, or the default preview workflow.

## Data Model Contract

PDF files should enter the same durable model as other formats:

```txt
PDF file
  -> RosettaDocument(format: "pdf")
  -> RosettaSourceFile(format: "pdf")
  -> RosettaBlock[]
  -> Segment[]
  -> RosettaTranslationFile
  -> export result
```

Initial block mapping:

- normal extracted text -> `paragraph`
- detected headings, if reliable -> `heading`
- table/caption/footnote detection -> optional, only when parser confidence is high
- empty, duplicated, or structural noise -> skipped `metadata`

Initial PDF-specific metadata should stay inside `style`:

```ts
style: {
  pdf: {
    page: 1,
    orderOnPage: 12
  }
}
```

Do not add bounding boxes, font details, or layout fields to top-level core models until the parser strategy is stable and an ADR explains why those fields must be durable. Layout-rich metadata may be kept inside `style.pdf` during exploration, but consumers must tolerate it being absent.

## Parser Requirements

The v1 parser should:

- reject non-PDF files by extension and parser validation
- fail clearly when no extractable text exists
- avoid panics on encrypted, malformed, or image-only PDFs
- limit file size and page count during the first implementation
- preserve page order
- avoid creating thousands of tiny blocks from line wrapping
- mark skipped/noise blocks explicitly instead of silently dropping everything

Heuristics should be deterministic and covered by tests. If a rule is uncertain, prefer a simpler paragraph extraction for the baseline importer. More advanced layout recovery can exist as an enhancement layer, but it must degrade to the baseline output instead of blocking import.

## Test Fixtures

Add fixtures under a dedicated test fixture path, for example:

```txt
src-tauri/fixtures/pdf/
  simple-one-page.pdf
  multi-page.pdf
  academic-two-column.pdf
  image-only.pdf
  encrypted-or-invalid.pdf
```

Unit tests should cover:

- valid text PDF produces a non-empty document
- page order is stable
- extracted blocks have `fileId`
- segments point back to existing blocks
- image-only PDF returns a clear unsupported/OCR-needed error
- invalid/encrypted PDF returns a clear parse error
- no test fixture contains private or sensitive text

## Frontend Requirements

Frontend changes should be small:

- Update import copy from TXT/Markdown to TXT/Markdown/PDF.
- Add `.pdf` to the Tauri file picker filter after the backend parser exists.
- Reuse `DocumentPreview`; do not create a PDF viewer for v1 translation reading.
- Keep preview virtualized at block level.
- If showing source format labels, render PDF as a source format, not a separate job type.

PDF v1 should not introduce a landing page, chat pane, cloud upload prompt, or generic AI-document assistant UI.

## Validation

For PDF work, the relevant validation set is:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Do not run dev or build unless explicitly requested for a release or UI verification pass.

## Open Decisions

- Which Rust PDF extraction crate to use.
- Initial max PDF file size and max page count.
- Whether v1 PDF export should default to `.txt` or `.md`.
- Whether multi-column detection is attempted in v1 or explicitly postponed.
- Whether high-fidelity PDF restoration should become a durable product promise after the contributor validates feasibility on real fixtures.

Choosing a new PDF parsing crate is a dependency decision. Record the choice in a change-log entry; add an ADR if the crate or layout strategy creates a long-term data model or architecture constraint.
