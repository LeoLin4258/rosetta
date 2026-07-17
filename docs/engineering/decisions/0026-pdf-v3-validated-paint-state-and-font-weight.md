# ADR 0026: PDF v3 Validated Paint State and Translation Font Weight

Date: 2026-07-17

Status: Accepted

Amends ADR 0023, ADR 0024 and ADR 0025. Transaction boundary amended by ADR
0027.

## Context

The first native replacement proof inherited the active PDF graphics state but
did not independently prove that the content stream's paint state still matched
the extracted PageGraph style. It also prepared only the Regular translation
face. This was insufficient for colored text and for source spans whose bold
intent is represented inconsistently by PDFium font-weight values.

Real fixtures demonstrate that numeric weight is not authoritative by itself:
PDFium reports Arial Bold at 380 and CMBX12 at 545, while other medium/bold faces
report 700. A renderer that uses only a fixed threshold would silently flatten
valid bold intent. A renderer that trusts PageGraph color without replaying the
content stream could apply a stale or incorrect style.

## Decision

Single text-show replacement now requires one validated `PageStyle` shared by
all PageGraph atoms in the source object. The style gate accepts only:

- a unique style with a valid source font weight;
- non-italic text;
- `FilledUnstroked` render mode;
- finite normalized fill color and opacity whose alpha values agree.

Italic, stroked, clipping, missing-weight and inconsistent-style source objects
remain original with typed errors.

Translation font assets and prepared subsets carry an explicit `Regular` or
`Bold` face intent. The renderer must use the face selected by the style plan;
a mismatched prepared font is rejected before mutation. Bold selection uses a
numeric weight of at least 600 or an explicit bold-family marker such as
`bold`, `black`, `heavy`, `demi`, `semi`, `cmbx` or `-medi` after removing a PDF
subset prefix. This is a controlled classification policy, not source-font
reuse.

Before replacement, the renderer replays graphics and text state up to the
target operation. It tracks `q`/`Q`, `Tf`, `Tz`, `Tr`, DeviceGray, DeviceRGB and
DeviceCMYK fill/stroke operators. The effective fill color and render mode must
match PageGraph. Color-space selection, arbitrary color operators and external
graphics state (`cs`, `CS`, `sc`, `SC`, `scn`, `SCN`, `gs`) remain typed
preservation fallbacks until they have complete interpreters.

Style/font validation occurs before staged font, page resource or content
objects commit. Diagnostic schema
`rosetta-pdf-v3-text-show-replacement/3` reported style ID, source weight,
selected translation face, normalized fill color, opacity and render mode, but
never source or translated text. ADR 0027 advances this schema to `/4` and adds
transaction schema `/1`.

## Evidence

Automated tests prove that:

- a Regular prepared font cannot replace a PageGraph object classified Bold;
- font-weight mismatch leaves the document object table and `max_id` unchanged;
- an inserted DeviceRGB fill is replayed, matched to PageGraph and inherited by
  the translated show;
- italic and clipping styles are rejected;
- explicit Arial Bold, CMBX and Nimbus medium names override misleading numeric
  weights;
- face intent survives asset loading, subsetting and font preparation.

The Windows/AMD Source Han Sans CN Bold probe replaced one independent source
show with `粗体安全回填` at fit scale 1.0 in about 4 ms. PDFium re-extracted the
text and identified the embedded Source Han Bold face. The 16,101-byte output
retained the source black fill. Poppler visual review confirmed the original
baseline and all following lines remained intact without clipping, overlap or
missing glyphs.

## Consequences

### Positive

- Color inheritance is now validated against both PageGraph and the content
  stream rather than assumed.
- Rosetta can use one controlled translation family while preserving validated
  Regular/Bold intent.
- A misleading PDFium numeric weight no longer silently flattens known bold
  faces.
- Unsupported style and paint states fail closed before PDF mutation.

### Costs

- Italic translation requires approved italic assets and remains preserved.
- External graphics-state opacity and non-device color spaces are not yet
  interpreted.
- Mixed styles within one source object remain preserved; translated style-span
  mapping is not yet implemented.
- ADR 0027 supports several same-face shows when each later position is
  independently anchored. Unanchored consecutive shows and mixed-face
  transactions remain preserved.

## Rejected Alternatives

- Always use the Regular translation face.
- Treat PDFium numeric font weight as authoritative.
- Load an operating-system font matching each source face.
- Trust extracted PageGraph color without replaying content-stream state.
- Approximate unsupported color spaces or external graphics state.
- Relax the final-show gate without validating text-position independence.
