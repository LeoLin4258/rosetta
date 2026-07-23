# ADR 0015: PDF v3 Native PageGraph and Patch-First Architecture

Date: 2026-07-16

Status: Accepted

## Context

Rosetta beta's PDF implementation is centered on a managed Python/pdf2zh
worker, prepared page windows and complete translated page PDF artifacts. The
implementation has improved latency and recovery, but it still has structural
limits:

- extraction and replay decisions are hidden inside a large external engine;
- translation units do not retain sufficient glyph, style or object provenance;
- long PDFs require user-visible or implementation-visible page windows to keep
  memory and preview pressure bounded;
- each translated page can duplicate fonts and PDF resources, producing very
  large job directories;
- component readiness, cache state and engine health are not one explicit
  capability-controlled system;
- exact citation, mixed-style and complex-layout backfill cannot be guaranteed
  by paragraph text replacement.

Rosetta is beta and the source PDF is authoritative. Existing derived PDF
artifacts do not require migration.

## Decision

Rosetta will build a new PDF v3 path without preserving v1/v2 implementation,
protocol or derived-artifact compatibility.

### Product priority

Visual fidelity is the primary output contract. When a region cannot be safely
translated and reinserted, Rosetta preserves the original region and reports a
typed preservation reason. It does not emit a guessed or flattened translation.

### Ownership boundary

Rosetta owns the stable PDF contracts and orchestration:

- `PageSet` for exact page addressing;
- `DocumentHandle` for bounded random access;
- `PageGraph` for glyph/run/object provenance;
- `TranslationPatch` for durable translation state;
- deterministic page rendering and export validation;
- long-document scheduling and recovery;
- component lifecycle and capability negotiation.

PDF libraries are adapters behind the native core interface. PDFium is the first
existing candidate; MuPDF is explicitly allowed when its object-level fidelity
is necessary and its licensing is accepted. The engine choice is made by a
fixture-driven extraction and identity-render spike.

### Persistence

The authoritative translated state is a compact page patch referencing stable
PageGraph atoms and spans. Complete translated page PDFs are not durable source
of truth. Rendered pages are bounded, disposable cache entries. Final export
creates one document-level PDF with shared fonts and resources.

### Long documents

Long PDFs use a bounded, streaming page task scheduler. Translation may batch
units across pages, but page extraction, patch commit, recovery and memory
release remain page-addressable. Fixed 10-page chunking is not part of the
public API or durable state.

### Complex content

Tables, formulas, annotations, links, layers, images and other non-translated
objects remain unchanged unless a typed patch targets them. Citations, URLs,
numbers and formula symbols are protected spans. Mixed style restoration is
allowed only when span mapping validates. Unsupported or low-confidence content
is preserved.

### Component control

The component manager exposes signed/versioned manifests, capabilities,
installation state, engine health, active operations, self-test, repair and
diagnostics as separate typed state. Frontend readiness must not be inferred
from cache markers or worker history.

### Compatibility

There is no migration requirement for beta PDF page state, translated page
artifacts, old worker protocol or old PDF commands. Existing source PDFs remain
usable and all v3 derived data is regenerated under a new schema and directory
boundary.

## Consequences

### Positive

- Native ordinary-page extraction can avoid the ONNX/Python cold path.
- Exact page selection is available to every operation.
- Glyph/run provenance makes citation and style behavior explainable.
- Unsupported complex regions fail safe by preserving source content.
- Patch-first storage prevents per-page font/resource duplication.
- Long documents become resumable streams rather than large in-memory windows.
- Engine replacement does not force a new frontend or translation data model.
- Versioned contracts and identity rendering make regressions testable.

### Costs

- A new extraction and rendering core must be built and validated.
- The engine spike may select a library with non-trivial license cost.
- Existing beta PDF derived data is intentionally discarded.
- Multiple output and fallback capabilities require a formal fixture corpus.
- Complex PDFs may show preserved source regions rather than translated text.

## Rejected Alternatives

- Continue expanding pdf2zh heuristics as the primary architecture.
- Keep complete translated page PDFs as the durable translation database.
- Make 10-page windows the long-PDF public model.
- Let the frontend infer component state from worker/cache events.
- Guarantee translation of every visual text-like region regardless of layout
  confidence.
- Preserve old beta PDF artifacts at the cost of carrying obsolete contracts.

## Required Follow-up

- Add the v3 plan to the implementation queue.
- Complete the PDFium/MuPDF extraction and identity-render spike before coding
  the production renderer.
- Update [docs/engineering/pdf-pipeline.md] after the v3 contracts are fixed.
- Add a change-log entry when the first v3 implementation slice lands.
