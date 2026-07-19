# PDF v3 Failed-Page Retry

Date: 2026-07-19

## Summary

Added an owner-gated exact-page retry command with trusted inactive-worker
restart semantics for durable PDF v3 failures.

## Implementation

- added `retry_rosetta_pdf_v3_page(jobId, runId, pageNumber)`;
- required current native ownership, a running or paused run and
  `retryable=true` durable page failure;
- restored extraction failures to pending and translation failures to their
  retained extracted authority through the existing scheduler transition;
- returned the unchanged bounded status schema with its window beginning at
  the retried page;
- re-resolved and synchronously validated source, runtime manifest, live
  component, provider/model and unified fonts before restarting an inactive
  supervisor;
- reused the same binding validator before stale recovery changes ownership;
- retained the one-worker-per-canonical-run registry invariant.

## Validation

- focused PDF v3 run-control tests for running, paused, foreign-owner,
  non-retryable and terminal states;
- focused worker registry tests;
- full PDF v3, Rosetta jobs and managed RWKV suites;
- Rust check/format, TypeScript typecheck and diff checks.

## Current Boundary

Native PDF v3 now supports creation, execution, pause/resume/cancel, stale
recovery and exact failed-page retry. Run enumeration, frontend workflow
integration and real managed-runtime 500/1,000-page translation/export
validation remain pending.
