# PDF v3 Identity-Bound Translation Planning

Date: 2026-07-18

## Summary

Added the bounded in-memory bridge from one reconciled PageGraph to exact
provider units and from identity-keyed provider results to a pending
TranslationPatch draft.

## Implementation

- generated stable unit IDs from source page and ordered PageGraph atom
  identity rather than source strings;
- restricted initial units to complete, same-style, same-text-show source
  objects supported by the current renderer;
- emitted typed preserved-region reasons for unsafe source objects;
- tokenized PageGraph protected spans and restored exact values with validated
  UTF-8 patch placements;
- rejected missing, duplicate, reordered and unknown protected tokens;
- accepted provider result reordering by exact unit ID while rejecting missing,
  duplicate and unknown results;
- rebound every plan to the current PageGraph before reassembly;
- added hard per-unit, per-page and unit-count limits;
- prohibited empty successful patches when a page has no safe units.

## Validation

- focused planner, identity, result-set, protected-token, preservation and
  bounded-size tests;
- full PDF v3, job, Rust check/format and frontend typecheck validation recorded
  with the implementation commit.

## Current Boundary

PageGraph can now produce a deterministic provider-neutral translation plan and
reassemble exact results into a validated pending patch. The concrete async
local-provider bridge, PageGraph protected-span detection and renderer-owning
page processor remain the next integration slices.
