# PDF v3 Native Workbench Control

Date: 2026-07-19

## Summary

Moved the primary PDF workbench workflow from legacy pdf2zh orchestration to
native PDF v3 run authority while retaining bounded long-document state.

## Implementation

- added typed frontend wrappers for native create, pause, resume, cancel,
  stale-owner recovery and failed-page retry;
- centralized newest-run discovery and one-record status polling in a
  job/language-scoped workbench controller;
- persisted selected source/target language metadata before trusted run
  creation;
- removed workbench pdf2zh prewarm/preparse/translate calls and fake PDF
  `ActiveTranslationRun` projection;
- drove topbar page progress and controls from native state and ownership;
- locked page selection for nonterminal runs and failed closed during run
  discovery;
- added owner-gated retry actions to retryable failed-page placeholders;
- kept visible page status bounded to four 64-record windows;
- removed legacy PDF progress-event, page-state and translated-path authority
  from the workbench preview;
- hid legacy PDF export for the native workflow because no public v3 export
  command exists yet;
- updated long-PDF confirmation copy for background page processing and native
  pause/resume.

## Validation

- `pnpm typecheck`;
- `git diff --check`.

## Current Boundary

Native PDF v3 now owns the active workbench lifecycle and lazy page preview.
Public native export integration, global cross-job run projection and real
complex 500/1,000-page Windows AMD acceptance runs remain pending.
