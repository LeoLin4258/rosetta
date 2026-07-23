# PDF v3 Native Worker Supervisor

Date: 2026-07-19

## Summary

Connected trusted PDF v3 runs to process-native bounded extraction and
translation execution without frontend timers or legacy Python orchestration.

## Implementation

- added one registered supervisor identity per canonical run directory;
- retained PDFium extraction state in a blocking loop and one lazy renderer
  source view in a separate blocking translation loop;
- serialized process-wide PDFium calls per bounded batch while leaving native
  translation independent;
- kept page claims behind the durable `2 / 4 / 1` scheduler backpressure;
- revalidated source, runtime manifest, live component, provider, model and
  unified font identity before work starts;
- carried one native verified source identity from creation/recovery into
  PDFium opening, avoiding repeated full-file hashes;
- started workers only after atomic run commit and after validated stale
  recovery;
- made pause quiescent, cancellation level-triggered and terminal/owner-loss
  cleanup automatic;
- waited for native workers before heartbeat/runtime shutdown on app exit and
  before local-data reset removes jobs or model files;
- made single-job deletion wait only for native supervisors owned by that job;
- upgraded bounded run-control status to schema 4 with privacy-safe worker
  health.

## Validation

- focused worker registry, cancellation, unload and shutdown tests;
- focused typed run-control tests;
- full PDF v3, Rosetta jobs and managed RWKV suites;
- Rust check/format, TypeScript typecheck and diff checks.

## Current Boundary

New and recovered PDF v3 runs now progress through native extraction and local
translation under durable page authority. Explicit failed-page retry, run
enumeration, frontend workflow integration and real complex 500/1,000-page
translation/export validation remain pending.
