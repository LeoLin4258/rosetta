# PDF Authoritative Render Slots and Partial Fallback

Date: 2026-07-24

## Summary

Replaced recurring PDF unit-count symptom patches with a renderer-aligned unit
contract. Final page-layout translation callbacks now define the authoritative
slots. A bad translation value falls back only its own slot to source text, so
valid neighboring units remain translated and the page remains usable.

## Root Cause

pdf2zh calls `TranslateConverter` while recursively parsing nested figure
contents and again when it lays out the final page. Rosetta previously collected
units from both phases, while render replay used only the final page callbacks.
The extra nested callbacks created phantom required units, producing errors such
as `expected 19, actual 14` even though the renderer had not lost five final
page slots.

The prior staging patch tried to compensate by classifying panel and diagram
labels and by scanning source text to recover order drift. Those heuristics
could make individual fixtures pass, but they could not establish identity in
documents with repeated text and could not eliminate new layout-specific count
mismatches.

## Changes

- Disable unit collection during recursive content parsing and enable it only
  for the final page-layout callback.
- Require render replay to match the collected slot IDs, order, source text,
  and total count exactly. Removed `_match_expected_unit` source-text scanning
  and the order-drift recovery path from staged engines.
- Treat missing, non-string, empty, and placeholder-invalid values for a known
  unit as source fallback for that unit. Provider transport/protocol failures,
  batch-count failures, unknown IDs, structural drift, and artifact failures
  remain page failures.
- Add `fallbackUnitCount` to the engine result, worker contract, durable page
  state, timeline events, and frontend type.
- Commit a page only when
  `translatedUnitCount + fallbackUnitCount == sourceUnitCount`. A page with
  fallback units uses `status="translated"`, `resultKind="partial"`, keeps its
  artifact visible, and shows translated/fallback counts in the preview.
- Keep empty provider results under their known `unitId` so the renderer can
  apply the source fallback policy instead of failing the whole page before
  rendering.

## Validation

Local PDF component imports now persist an explicit `customPack` manifest flag.
Status checks continue to enforce the pinned release hash and size for normal
online installs, while an explicitly imported local test pack remains usable
after restart when its installer-computed SHA-256 is valid. Older manifests
default to the pinned-release validation path.

The macOS/Linux local staging helper now writes the same custom-pack manifest
after its engine smoke test. Its SHA-256 covers the staged engine, converter,
layout model, launcher, and bundled fonts, preventing a successfully staged
developer pack from being misreported as an outdated official component.

- PDFMathTranslate Rosetta engine tests cover nested callback exclusion,
  strict replay, empty/missing fallback, placeholder fallback, and text-bearing
  artifacts.
- Rosetta staging patch tests cover fresh and already-patched engine upgrades,
  idempotency, authoritative-slot markers, and removal of the old matcher.
- Rust tests cover partial commit accounting, structural mismatch rejection,
  and forwarding known empty provider output to renderer fallback.
- On CityBehavEx page 2, the old path collected 19 units but rendered 14. The
  authoritative path collected and rendered 14. An identity render retained
  text drawing operators and extractable text.
- A mixed-validity page rendered 13 translated units plus one source fallback
  as a normal artifact instead of failing the page.

Final product acceptance remains a user-run App translation of the previously
failing PDFs on a freshly staged component. No release package is produced by
this change.
