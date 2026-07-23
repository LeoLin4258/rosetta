# PDF v3 Region Renderer Production Switch

Date: 2026-07-20

## Summary

Switched new native PDF v3 runs from source text-show translation patches to
durable visual-paragraph/flow-container region patches. This is a beta reset:
old PDF v3 translation derivatives are isolated by schema namespace and are not
migrated.

## Changes

- Added region TranslationPatch schema 2 and patch-store schema 3 envelopes with
  explicit `text-show` and `region` payload kinds.
- Kept each visual paragraph as one provider chunk until the provider reports a
  real context or generation limit.
- Added atomic region rendering: complete source-show neutralization, unified
  Regular/Bold font layout, color/opacity overlay, container-level preservation
  and exact resolved-patch replay.
- Switched production processor, worker commit, stale recovery inventory,
  translated preview, page PDF/PNG caches and native export to region authority.
- Added document-wide region export font planning. Regular/Bold subsets are
  embedded once per output document rather than once per page.
- Changed invalid provider paragraphs from page-terminal failures to durable
  complete-container preservation. Structural provider-result identity errors
  remain page-terminal.
- Refined the Chinese mixed-language quality gate to detect CJK-adjacent
  single-letter shards without rejecting legitimate abbreviations and names.
- Added final font-encoding preflight so an unencodable laid-out glyph preserves
  only the affected container instead of failing the page during serialization.
- Removed translation-font staging from empty source-show neutralization. A
  neutralized Bold source show no longer requires or embeds a Bold translation
  face when the reflowed output only uses the unified Regular face.
- Added conservative preservation for small decorative containers that combine
  multiple paragraphs with at least a 2x font-size difference and significantly
  different colors. This keeps infographic callouts intact instead of applying
  body-paragraph reflow that can overlap large numerals and labels.
- Advanced the production region renderer contract to
  `rosetta-pdf-v3-region-translation-renderer/2` so resolved decisions and cache
  artifacts from the new preservation policy cannot alias `/1` output.
- Added privacy-safe processor stage diagnostics without logging source or
  translated text.
- Updated the public export metrics to
  `rosetta-pdf-v3-region-translation-export/2` with rendered/preserved container
  and rendered-line counts.

## Evidence

- Drylab Poppler probes passed for all three pages: pages 1-3 reflowed 10 safe
  containers, neutralized 382 source text shows and preserved one narrow
  unsupported container without partial replacement.
- A real two-page academic PDF export regression verifies that each prepared
  Type0 font subset appears exactly once in the final incremental PDF.
- Focused runtime, preview, processor, patch-store, renderer and export tests pass
  on Windows AMD.
- A fresh three-page Drylab live run completed 3/3 pages with zero preserved or
  failed pages after the neutralization-font fix. Run creation took 202 ms with
  model and font digest cache hits; the remaining elapsed time was serialized
  RWKV paragraph generation.
- A Drylab page-1 PDF/Poppler probe verifies the `/2` decorative preservation
  policy: two-column body text remains reflowed while the `34 meetings` callout
  remains source-perfect without overlap.
- The final renderer `/2` live-App run completed all three Drylab pages with
  zero failures. Page 1 durably records
  `region-decorative-mixed-scale` for the callout while the other four
  containers reflow. PNG inspection passed for all three pages.
- That run created authority in 197 ms with model, sidecar and font cache hits.
  The first translated page completed about 10.7 seconds after run creation and
  all three serialized RWKV pages completed in about 48.4 seconds.

## Remaining Gates

- Add durable per-page provider/render timing so model generation, validation,
  patch commit and preview latency are visible separately in production runs.
- Run full 500/1,000-page memory, cancellation, recovery, cache and output-size
  acceptance before removing the legacy renderer.
- Record first-visible-page timing and full 500/1,000-page memory, cancellation,
  recovery, cache and output-size evidence before removing the legacy renderer.
