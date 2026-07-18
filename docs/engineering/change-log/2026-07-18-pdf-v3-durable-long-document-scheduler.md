# PDF v3 Durable Long-Document Scheduler

Date: 2026-07-18

## Summary

Started Phase 6 with an independent durable PDF v3 scheduler for bounded
long-document work, page-local recovery and exact-page control.

## Implementation

- added a versioned run manifest and 64-page state shards without user-visible
  chunk semantics;
- added typed pending, extracted, completed, preserved and failed page states;
- added owner and page-stage leases with claim/commit/fail validation;
- added independent extraction, extracted-backlog and translation capacities;
- added pause, resume, retry, cancellation and bounded status windows;
- added stale-owner recovery against validated PageGraph and TranslationPatch
  inventories;
- added promotion of artifacts committed before scheduler state, plus demotion
  of invalid completed state;
- added synced temp/backup atomic replacement and interrupted-write candidate
  recovery;
- made initial run creation stage all shards and the manifest before exposing
  the canonical directory with one rename;
- rebuild manifest summaries from authoritative shards on open;
- kept the scheduler isolated from legacy PDF v1/v2 run and rendered-page
  artifact state.

## Validation

- a 1,000-page synthetic run uses 16 shards with at most 64 records each;
- oversized claim requests remain within 3 extracting, 5 extracted-waiting and
  2 translating page limits;
- stale leases resume only missing pages while valid patch/extraction authority
  is retained;
- simulated missing canonical manifest/shard files recover from synced backup
  candidates;
- completion, preservation, pause, retry and cancellation transitions pass.

## Current Boundary

The durable scheduler core is implemented and tested independently. PDFium
extraction workers, TranslationPatch commit inventory, Tauri commands and the
frontend run-status surface are not connected to it yet.
