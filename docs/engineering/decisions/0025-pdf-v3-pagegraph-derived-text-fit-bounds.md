# ADR 0025: PDF v3 PageGraph-Derived Text Fit Bounds

Date: 2026-07-17

Status: Accepted

Style resolution and paint-state validation amended by ADR 0026. Multi-show
transaction boundaries amended by ADR 0027.

## Context

ADR 0024 introduced a single text-show replacement path but accepted
`maxAdvance` from its caller. That value could not be independently verified by
the renderer. A stale planner, unit mismatch or guessed page width could allow
translated text to overlap adjacent content or reject text that actually fits.

PageGraph already contains reconciled per-character source provenance, origin,
loose bounds and the PDFium character transform. The renderer can derive the
source text-show's visual advance from those values and convert it back to PDF
text space before any document mutation.

## Decision

The PDF v3 renderer no longer accepts a caller-supplied maximum advance for
single text-show replacement. It derives a typed `TextShowFitBounds` from the
selected PageGraph and replacement provenance.

The derivation must:

- require the current PageGraph schema and selected page number;
- resolve the target by text-show ID, stream object/generation and operation;
- require verified or ToUnicode-corrected atoms with matching source `Tf` and
  `Tz` state;
- resolve exactly one PDFium source text object;
- include synthetic whitespace belonging to that source object so trailing
  source spacing is not silently discarded;
- require finite origins, loose bounds and one consistent character transform;
- project the complete source-object geometry along the transform's text
  baseline direction;
- divide page-space advance by the transform baseline scale to obtain the
  maximum text-space advance used by font fitting.

The initial safe implementation supports page-axis-aligned baselines, including
normal, 90-degree, 180-degree and 270-degree directions with positive or
negative matrix scale. PDFium currently exposes axis-aligned character bounds,
not glyph quads. Arbitrary-angle baselines are therefore a typed preservation
fallback because projecting their axis-aligned boxes would overestimate the
available region.

The renderer reports page advance, baseline scale, derived maximum advance and
geometry atom count without reporting source or translated text. Geometry
validation happens before font/page/content objects are committed. A stale or
invalid PageGraph leaves the complete document object table and `max_id`
unchanged.

## Evidence

The LibreOffice fixture's first text-show derives:

- page advance: 453.68 points;
- baseline matrix scale: 1.0;
- maximum text-space advance: 453.68;
- Source Han translated natural advance: 80.0;
- fit scale: 1.0.

The resulting 16,483-byte PDF remains searchable through PDFium. Poppler visual
comparison confirms the original baseline and following line positions remain
unchanged with no clipping, overlap, missing glyphs or background damage.

Automated tests cover horizontal scale, 90-degree rotation, reverse direction,
synthetic trailing whitespace, arbitrary-angle rejection, overflow and stale
PageGraph zero-mutation behavior.

## Consequences

### Positive

- The renderer owns fit units and cannot be given a guessed page width.
- PageGraph geometry and low-level provenance now participate in one safety
  decision.
- Horizontal/vertical scale and orthogonal rotation have deterministic tests.
- Stale extraction data fails closed before any PDF object commit.

### Costs

- Current fitting uses the source text-show's own visual advance, not unused
  paragraph or column width.
- Arbitrary-angle text remains preserved until extraction records validated
  glyph quads or the content interpreter provides exact source advances.
- Multi-show paragraphs still require a transaction-level layout plan.
- Validated device-color inheritance and Regular/Bold face selection are added
  by ADR 0026. Line breaking, mixed styles and protected spans remain separate
  renderer/planner work.

## Rejected Alternatives

- Continue trusting caller-supplied `maxAdvance`.
- Use page width or right margin as the fit region.
- Use only tight glyph ink bounds and discard source whitespace.
- Project axis-aligned PDFium bounds for arbitrary-angle text.
- Fit after committing font or page resource objects.
