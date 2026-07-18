# PDF v3 Local Provider Bridge

Date: 2026-07-18

## Summary

Connected the identity-bound PDF v3 translation plan to Rosetta's existing
local provider batching without introducing a dependency on the old pdf2zh
worker unit contract.

## Implementation

- moved provider chunking, batching, retry and reconstruction onto a private
  provider-owned unit type;
- retained the old PDF entry point as a thin compatibility conversion;
- added an async `TranslationPagePlan` bridge for Lightning, mobile batch and
  llama.cpp provider configurations;
- returned provider output as PDF v3 `TranslationUnitResult` values with exact
  unit identity and text-free metrics;
- retained placeholder isolation so protected `{vN}` tokens never enter model
  source batches;
- added typed retryability for cancellation, provider failure, invalid plans
  and pages with no safe units;
- prevented provider raw error strings from entering the PDF v3 control-state
  boundary.

## Validation

- focused generic unit and PDF v3 async bridge tests;
- complete PDF v3, job, Rust check/format and frontend typecheck validation
  recorded with the implementation commit.

## Current Boundary

The PDF v3 planner can now execute against the selected local provider and
return exact identity-keyed results. The next slice must supply runtime
provider/model identity, reassemble the pending patch, resolve it through the
renderer and hand the resolved patch to the durable translation worker.
