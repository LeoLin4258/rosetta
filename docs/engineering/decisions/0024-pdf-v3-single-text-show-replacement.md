# ADR 0024: PDF v3 Single Text-Show Replacement

Date: 2026-07-17

Status: Accepted

Fit-bound input amended by ADR 0025. Style and paint validation amended by ADR
0026. Anchored later shows and multi-show transactions amended by ADR 0027.

## Context

ADR 0023 proved that Rosetta can encode searchable Chinese with one unified
translation font. The remaining renderer question is whether translated text
can replace an original PDF text-show in place without raster overlays,
background-colored rectangles or source-font reuse.

Replacing only string bytes is not sufficient because the content stream still
uses the source font. Changing an earlier `Tf` can affect unrelated shows.
Changing a show that is followed by another show in the same text object can
also change the later text matrix position. Font objects, page resources and
the rewritten content stream must commit together.

## Decision

PDF v3 adds a deliberately narrow single text-show replacement path.

PageGraph schema advances to v4. Atom provenance now carries the structured
renderer state needed to address and validate a replacement:

- stream object number and generation;
- operation index;
- unqualified source font resource name;
- source `Tf` size;
- source `Tz` horizontal scaling;
- existing operand hash and Form invocation path.

The mapping walk records `Tf`, `Tz`, `q` and `Q` state and propagates it through
Form invocation contexts. Renderer correctness does not parse stream IDs or
font state from display strings.

### Initial safety gate

The first replacement path accepts only a target that:

- belongs directly to one selected page content stream;
- is referenced exactly once by that page and by no other page;
- has an unchanged text-show operator and operand hash;
- is inside `BT`/`ET` with font resource, size and horizontal scaling matching
  provenance;
- is the final text-show before the matching `ET`;
- has non-empty translated text fully covered by the prepared font;
- fits within a validated maximum text advance without reducing horizontal
  scale below the caller's readability floor.

The current tests use a minimum fit scale of 90%. Overflow preserves the source
instead of forcing unreadable compression.

### Content rewrite

At the original operation position, the renderer inserts:

1. unified translation font `Tf` with the source font size;
2. fitted `Tz` based on the source horizontal scaling;
3. a translated `Tj`, `'` or `"` show encoded as two-byte CIDs;
4. source font `Tf` restoration;
5. source `Tz` restoration.

`TJ` is replaced by one translated `Tj`; source kerning numbers are not reused
for a different language. The initial path required the last show in the text
object so its changed advance could not shift a later show. ADR 0027 permits a
later show only when a validated text-position operator removes that dependency.

The original fill/stroke color, opacity, text matrix, clipping context and
graphics state remain in force. No background rectangle is painted and no
raster content is introduced.

The six staged font objects, cloned page resource dictionary and cloned content
stream are validated before any document object is changed. On success they
commit together; on overflow or stale provenance, `max_id` and the complete
object table remain unchanged.

## Evidence

Automated Windows tests prove:

- a unique top-level text-show can be replaced with a unified-font string;
- PDFium re-extracts the new text;
- font objects are staged rather than committed during validation;
- an intentionally oversized translation returns typed overflow;
- overflow leaves `Document.max_id` and every object unchanged.

The Source Han Sans CN manual probe replaced the first source line of
`002-trivial-libre-office-writer.pdf` with `统一字体安全回填`:

- replacement fit scale: 1.0;
- natural and fitted advance: 80 text units;
- replacement stage: about 3 ms;
- source PDF: 12,609 bytes;
- output PDF: 16,483 bytes;
- PDFium extraction contains the new Chinese text;
- Poppler renders the new line at the original baseline and size;
- all following source lines keep their positions;
- visual review found no overlap, clipping, missing glyphs or background
  damage.

## Consequences

### Positive

- PDF v3 now has a real source-text-to-Chinese replacement proof.
- Original text is removed at the content operator instead of visually covered.
- Existing color and placement state are reused without source-font reuse.
- Unified font embedding and content mutation share one atomic commit boundary.
- Overflow and stale provenance preserve the source document.

### Costs

- This path does not yet support Form targets or shared page streams.
- ADR 0027 allows anchored later shows and atomically groups several requests in
  one text object. Unanchored consecutive shows remain preserved.
- ADR 0025 replaces caller-supplied maximum advance with PageGraph-derived
  text-space fit bounds.
- Only one line/show is replaced; paragraph line breaking and vertical fitting
  remain pending.
- Significant horizontal compression is rejected rather than reflowed.
- Regular/Bold face selection and validated device-color inheritance are added
  by ADR 0026. Mixed-style translation spans and protected-span assembly remain
  disconnected.

## Rejected Alternatives

- Draw an opaque rectangle over source text and place translation above it.
- Keep the source show invisible while adding duplicate extractable text.
- Reuse source `TJ` kerning numbers for translated text.
- Change one shared `Tf` and let later shows inherit the unified font.
- Replace a show before another show without preserving its text advance.
- Commit font resources before fit and source-identity validation succeeds.
- Shrink any translation until it fits regardless of readability.
