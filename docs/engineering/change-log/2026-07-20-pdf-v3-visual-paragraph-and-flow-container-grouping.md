# 2026-07-20 PDF v3 Visual Paragraph and Flow-Container Grouping

## Summary

- Upgraded PageGraph to schema 6 and added explicit `flow-container` groups.
- Derived deterministic visual lines, visual paragraphs and flow containers
  after source reconciliation without changing the production translator or
  renderer yet.
- Stored privacy-safe group identity, ordered atom ownership, page-space bounds
  and confidence; source text is not copied into group diagnostics.
- Added hard per-page atom/line/paragraph/container limits and fail-closed store
  validation for group IDs, geometry, confidence, atom references, per-layer
  ownership and complete hierarchy coverage.
- Invalidated schema-5 extraction artifacts through the existing runtime/store
  identity binding; beta artifacts are rebuilt rather than migrated.

## Windows AMD Drylab Evidence

The three-page Drylab source used for the reported loose, fragmentary backfill
produced the following hierarchy after gutter and column-track calibration:

```text
page 1: 1438 eligible atoms, 35 lines, 8 paragraphs, 5 flow containers
page 2: 2797 eligible atoms, 72 lines, 7 paragraphs, 3 flow containers
page 3: 1414 eligible atoms, 36 lines, 8 paragraphs, 3 flow containers
```

Grouping itself took approximately 1.6-3.7 ms per page. Full extraction,
source mapping, reconciliation and grouping took approximately 19-42 ms per
page in the manual probe.

Poppler-rendered page overlays confirmed separate regions for the title, left
and right columns, image-separated column continuations, the page-1 statistics
block, the page-1 horizontal footer and the page-3 full-width note. No flow
container crossed the central gutter or an image boundary in this fixture.

## Scope

This milestone establishes the layout authority required by ADR 0076. It does
not change visible translated output yet: paragraph provider plans,
region-level TranslationPatch entries, source neutralization and container-wide
target-language reflow remain subsequent milestones.

## Validation

```text
cargo test pdf_v3::visual_grouping --lib
cargo test pdf_v3::page_graph_store --lib
cargo test manual_windows_external_visual_grouping_probe --lib -- --ignored --nocapture
```
