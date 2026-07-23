# ADR 0059: PDF v3 Two-Pass Translation Font Planning

Date: 2026-07-18

Status: Accepted

Refines ADR 0023 and ADR 0058.

## Context

PDF v3 must use one deterministic document-wide translation-font subset per
used face, but the complete translated character set does not exist until the
provider has translated every selected page. Requiring prepared document fonts
when constructing the page processor creates a dependency cycle.

Holding all PageGraphs and translated text until that set is known would break
the bounded-memory long-document design. Persisting pending patches or page PDF
deltas would create a second translation authority and increase disk usage.

## Decision

Use the same immutable font assets in two bounded passes.

During translation processing, each provider result first becomes a validated
pending TranslationPatch. The processor classifies its safe entries as Regular
or Bold from PageGraph style authority, builds temporary page-local character
sets, prepares deterministic temporary subsets, and stages a temporary font
registry plus replacement delta. The renderer uses that state only to resolve
fit or preservation decisions. The processor then discards every temporary
font object and replacement object and returns only the resolved patch.

The processor configuration owns immutable Regular and optional Bold
`TranslationFontAsset` values, not prepared subsets. Missing optional Bold
assets produce entry-level preservation through the renderer's existing typed
fallback.

During final export, a streaming font planner walks the exact requested
PageSet. It loads one durable PageGraph and its resolved TranslationPatch at a
time, includes only `Fitted` entry text, and immediately releases that page
state. Preserved entries add no glyphs. The resulting sorted Regular/Bold sets
prepare the one document-wide subset per used face before resolved patches are
replayed into the export delta.

Each weight is limited to 65,535 Unicode scalars, matching the current 16-bit
CID representation. Limit checks are transactional and cannot leave a partial
plan. A missing PageGraph, missing patch, source mismatch, invalid resolved
decision, unsupported fitted style, or limit overflow fails export planning
explicitly.

Fit equivalence relies on one invariant: glyph advances and font metrics come
from the same immutable font asset and do not depend on which additional glyphs
are present in a subset. Automated Windows tests compare advances between a
page-sized subset and a larger document-sized subset.

## Consequences

### Positive

- Translation of a large PDF retains only one page graph, one pending patch,
  and small page-local font/delta state at a time.
- Restart does not need pending patches, prepared fonts, or accumulated PDF
  object deltas.
- Final output embeds each used translation face once and excludes glyphs from
  preserved entries.
- Bold assets are prepared only for pages and documents that actually use bold
  fitted translations.
- Page processing and export derive from the same durable patch authority.

### Costs

- Font subsetting happens once per translated page for fit decisions and once
  per document for export.
- Final export requires a complete first pass over durable page and patch
  authorities before object staging begins.
- The final export coordinator performs multi-page resolved-patch replay and
  incremental commit; runtime/job lifecycle integration remains above it.
- Complex-script shaping remains outside this decision.

## Rejected Alternatives

- Prepare a document subset before provider translation.
- Keep every translated PageGraph and patch in memory until the job completes.
- Persist pending patches or page replacement deltas.
- Embed each temporary page subset in the final PDF.
- Include preserved-entry text in the final font subset.
