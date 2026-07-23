# PDF v3 Async Renderer-Owning Page Processor

Date: 2026-07-18

## Summary

Connected the identity-bound PDF v3 planner and async local-provider bridge to
renderer resolution and the durable translation worker through a page-bounded
async processor.

## Implementation

- replaced the durable translation worker's synchronous page closure with an
  async processor contract;
- added a pre-commit cancellation gate after async page processing;
- added explicit provider/model/language/revision/renderer/font processor
  configuration and scheduler-binding validation;
- composed the planner, local provider, patch reassembly and renderer staging
  into one sequential page pipeline;
- reused one selected-page index, ownership index, document font registry and
  accumulated disposable object delta across processed pages;
- returned explicit preservation for pages with no safe translation units;
- kept overflow and unsupported geometry as resolved entry preservation while
  rejecting hard renderer failures before patch storage;
- resolved indirect PDF `/Contents` arrays to physical stream IDs before
  ownership analysis.

## Validation

- focused async worker cancellation, renderer failure and durable commit-order
  tests;
- Windows processor tests for success, preservation, overflow, missing glyph,
  cancellation and runtime identity validation;
- page-index indirect `/Contents` regression tests;
- full PDF v3, job, Rust check/format and frontend typecheck validation recorded
  with the implementation commit.

## Current Boundary

The isolated native pipeline now processes a claimed PageGraph through the
local provider and renderer into durable resolved patch authority without a
synchronous runtime bridge. Job-level prepared-font construction, Tauri
component/runtime ownership, frontend control/status, export orchestration and
real complex 500/1,000-page end-to-end validation remain pending.
