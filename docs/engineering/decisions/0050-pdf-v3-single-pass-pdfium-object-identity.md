# ADR 0050: PDF v3 Single-Pass PDFium Object Identity

Date: 2026-07-18

Status: Accepted

Refines ADR 0016 for the extraction adapter boundary.

## Context

PDF v3 needs exact character-to-text-object ownership for stable source
provenance, style recovery and low-level content operand reconciliation. The
public `pdfium-render` 0.9.1 API exposes exact ownership through
`chars_for_object()`, but that method scans every page character and calls
`FPDFText_GetTextObject` once for every text object. A page with `O` text
objects and `C` characters therefore performs approximately `O * C` FFI
identity queries.

Rosetta must not replace exact ownership with text, bounds or font heuristics.
Those keys can collide on duplicate text layers, overprinting and reused Form
content, which would weaken the provenance required by the renderer.

## Decision

Rosetta vendors a trimmed `pdfium-render` 0.9.1 source adapter with the PDFium
7763 binding used by the desktop application. The local difference adds two
read-only methods that expose the same opaque, process-local text-object
identity from a text object and a page-text character.

Extraction enumerates page and recursive Form text objects once, builds a
transient identity-to-snapshot index, then scans page characters once. The
identity is valid only while the containing PDFium page is open. It is never
serialized, included in `PageGraph`, interpreted as an indirect PDF object
number or used across document lifetimes.

Source fingerprinting now streams the source through a fixed 64 KiB buffer,
while lopdf and PDFium open the immutable file path independently. The
`DocumentHandle` no longer retains an additional source-sized Rust byte vector.

## Evidence

The real-paper fixture has 30 pages, is 1,590,242 bytes and produces 39,783
atoms across the first ten pages. Three Windows AMD debug runs changed from
4,692-4,791 ms before the adapter work to 784-874 ms after it. Final extraction
proper was 242-257 ms; exact object identity queries took 2.2-2.5 ms, content
operand mapping took 432-498 ms and reconciliation took 72-79 ms.

An automated equivalence test recursively runs the previous PDFium
`chars_for_object()` API and compares every source object ID, object text,
mapped count and first/last character index with the single-pass result. A
separate test compares cached object style with direct per-character style.

## Consequences

### Positive

- Exact object provenance is retained with linear character identity work.
- Selected-page extraction remains independent of unselected page layout.
- Large sources no longer require an extra complete Rust source buffer.
- The adapter is deterministic and fully local; no runtime download or cloud
  service is introduced.

### Costs

- Rosetta owns a small upstream patch and must rebase it when updating
  `pdfium-render` or the PDFium binding version.
- The vendored source adds about 3.2 MB to the repository, but it does not add a
  second library to the application package or affect translated-PDF size.
- Current timings are debug measurements on one fixture, not a release or full
  corpus performance guarantee.

## Rejected Alternatives

- Match objects by Unicode text, bounds, font or matrices.
- Continue the object-by-object full-page scan.
- Read private Rust struct layout with `transmute` or pointer-offset assumptions.
- Persist PDFium pointer identities in PageGraph or TranslationPatch data.
