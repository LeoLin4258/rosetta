# ADR 0076: PDF v3 Visual Paragraph Planning and Flow-Container Reflow

Date: 2026-07-20

Status: Accepted

Supersedes the source-object translation granularity in ADR 0056. Retains its
identity-bound provider result and protected-token requirements.

## Context

The first end-to-end native v3 implementation deliberately aligned one
translation unit with one provenance-bearing PDF text-show/source object. That
made low-level replacement safe, but real document evidence shows that this is
the wrong product-level unit.

PDF producers commonly split one visual sentence or paragraph across many
`Tj`/`TJ` operations, text objects, fonts and streams. Translating those pieces
independently removes paragraph context and creates incomplete or mixed-language
output. Replacing each result inside the source object's small advance box also
forces Chinese text into English fragments, producing gaps, collisions and
unnatural line breaks. Font fitting cannot repair this structural mismatch.

The Drylab three-page regression demonstrates all failure modes together:
fragment-level bilingual output, disconnected bold/color spans, large unused
areas and a hard renderer failure on page 2. Continuing to tune per-show scale
or font selection would preserve the root cause.

## Decision

Separate translation granularity from rendering granularity.

### Page layout hierarchy

Each reconciled page derives a deterministic, page-local hierarchy:

```text
atom/style run
  -> visual line
    -> visual paragraph
      -> flow container
```

A flow container is one independently reflowable area such as a column,
heading block, table cell, caption, header or footer. It owns ordered source
atoms, region geometry, reading order, style runs and the complete low-level
text-show provenance needed to neutralize source text safely.

Grouping is geometry- and paint-order-based, bounded to one page and
deterministic. It must account for baseline direction, font metrics, gutters,
column boundaries, vertical gaps, indentation and overlapping non-text page
objects. Confidence and typed preservation reasons are part of the derived
result. A low-confidence region is preserved as a whole; it is never partially
translated through the legacy object planner.

### Translation units

The provider receives one visual paragraph per translation unit. A unit may
span multiple source objects, text shows, styles and streams, but its identity
is still derived from the ordered source atom identities and exact page
authority. Provider result order and source-text equality have no authority.

Protected citations, numbers, URLs, formulas and symbols continue to use exact
tokens. Style boundaries use a separate balanced marker contract so a complete
paragraph can be translated with context while bold, color, opacity, link and
other supported span intent maps back to translated ranges. Missing, duplicated,
unknown, crossed or reordered markers reject the unit instead of guessing.

Provider-output validation is also container-atomic. An empty, malformed,
token-invalid or clearly fragmented paragraph result preserves its complete
flow container while unrelated containers on the page continue. Structural
result mismatches, such as an unknown or duplicate unit identity, still reject
the page because they invalidate the provider-result binding itself. Mixed-
language detection must distinguish model fragments from legitimate names and
acronyms; short Latin runs alone are not evidence of corruption.

### Rendering units

The renderer lays out all translated paragraphs in one flow container together.
It performs target-language line breaking, paragraph spacing and span shaping
with Rosetta's unified translation font family. Source font family is not
reused; validated size, weight, color, opacity and emphasis intent remain style
inputs.

The renderer neutralizes every owned source text show through validated
provenance and paints the reflowed container once. It must not cover source
content with opaque rectangles. Source neutralization, translated text objects,
font resources, content-stream clones and page rewiring commit atomically.
Any ownership, paint-order, clipping, graphics-overlap or capacity uncertainty
preserves the complete container.

The renderer performs a final encoding preflight against the prepared font
subsets before it mutates page objects. If layout retained a character the
selected translation font cannot encode, the affected container is preserved
with a typed reason instead of failing the complete page during content-stream
serialization.

Paragraph translation results are durable authority. Shaped glyph runs, line
breaks and raster previews are reproducible caches and are not persisted as a
second translation truth. A new region-level TranslationPatch schema will bind
ordered source atom hashes, translated text, restored protected spans and
translated style spans; it will not store source text.

### Bounded long-document behavior

Derivation, translation planning and rendering remain page-local. A worker
retains at most the active PageGraph, its region hierarchy and one page's
renderer staging state. Region grouping must be `O(n log n)` or better in page
atom count, with hard limits on atoms, lines, paragraphs, containers and marker
count. No document-wide layout graph is retained.

## Delivery Sequence

1. Populate and validate line, paragraph and flow-container groups without
   changing durable patches or production rendering. Add privacy-safe group
   diagnostics and Drylab fixtures. Implemented in PageGraph schema 6 on
   2026-07-20; production still uses the legacy patch/renderer until steps 2-3.
2. Introduce paragraph provider plans and balanced style markers. Keep the old
   renderer disabled for grouped units until region patch validation exists.
   The clean visual-paragraph provider contract, protected-span restoration,
   flow-container atomic preservation and suspicious mixed-language output
   rejection were implemented on 2026-07-20. Visual paragraphs now remain one
   provider chunk until a real provider context/generation limit triggers the
   existing semantic retry split.
3. Add TranslationPatch v2 region entries and atomic source neutralization plus
   flow-container layout. Implemented on 2026-07-20 with region patch schema 2,
   patch-store schema 3 envelopes, region renderer `/1`, production worker,
   preview/cache/recovery and document export integration. The three Drylab pages
   passed Poppler PNG inspection before production switching.
   A subsequent live-App run exposed provider-quality and final-font failures;
   both now resolve to durable complete-container preservation. Fragmented-output
   detection was narrowed to reject CJK-adjacent single-letter shards while
   allowing proper-name and acronym sequences such as `NY`, `SF`, `AWS` and
   `L.A.`. Live rendering then exposed a neutralization-only font dependency:
   empty source-show replacements incorrectly requested the source style's
   translation font weight. Neutralization now stages no translation font.
   Region renderer `/2` also preserves small mixed-scale, mixed-color decorative
   callouts instead of forcing them through body-paragraph reflow.
4. Expand conservative support for tables, multi-column reading order, forms,
   clipping and overlapping graphics. Unsupported containers remain source.
5. Remove the object-level planner and renderer path after corpus, long-document,
   memory, cancellation, recovery and export acceptance passes.

## Acceptance

The Drylab regression must complete all three pages with paragraph-coherent
Chinese, correct two-column reading order, no mixed-language fragments, no
overlap or abnormal holes, and preserved title/body emphasis and color intent.
Validation uses PDFium and Poppler-rendered PNG inspection, extracted text and
patch identity checks.

The broader corpus must cover academic multi-column pages, citations, mixed
bold/color sentences, tables, captions, Form XObjects, headers/footers and pages
with overlapping graphics. A failed container must leave source pixels and
text intact.

## Consequences

### Positive

- The model receives natural paragraph context instead of producer-specific
  PDF fragments.
- Translation length differences are absorbed by container reflow instead of
  per-show horizontal compression.
- Span styling and protected content remain identity-bound and testable.
- Conservative failures preserve coherent source regions rather than producing
  partially corrupted pages.
- Page-local ownership keeps long-document memory bounded.

### Costs

- Reliable flow-container detection and paint-order safety are materially more
  complex than text-show replacement.
- TranslationPatch and PageGraph schemas require a beta reset or explicit
  migration; this rewrite chooses a beta reset.
- Tables, clipped text and overlapping graphics will remain conservative until
  their region ownership can be proven.
- Region layout needs target-language line breaking and later shaping support
  for scripts beyond direct CJK/Latin cmap rendering.

## Rejected Alternatives

- Continue tuning font scale or width for one text show at a time.
- Translate complete paragraphs and split the result back across source shows
  by character count.
- Redact source rectangles and overlay translated text without provenance.
- Persist rendered lines as translation authority.
- Build or retain a document-wide layout graph for hundreds of pages.
