# ADR 0072: PDF v3 Native Workbench Control

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0066, ADR 0068, ADR 0069 and ADR 0071.

## Context

The workbench could inspect and lazily preview a native PDF v3 run, but its
primary translation action still prewarmed and preparsed pdf2zh, invoked the
legacy page translator, and created a fake frontend segment run. Run discovery
and visible-page polling were also coupled inside the preview hook. This left
two competing authorities for activity, progress, page state and output.

A long PDF must remain controllable without expanding all page records into
React state. Navigation or restart must reconstruct the selected run from
native durable state, and stale ownership must never be guessed in the UI.

## Decision

The workspace owns one `usePdfV3RunControl` projection keyed by job and target
language. It discovers only the newest validated run, polls one-record bounded
control status, and is the only frontend caller for create, pause, resume,
cancel, recover and exact-page retry. The preview receives that selected status
and fetches only its visible 64-record page window, retaining at most four
windows.

Before creation, persist the selected file source/target languages through the
existing narrow job-language command. Trusted creation still accepts only job,
exact PageSet and target language; native code derives source language and all
runtime/component identity from durable authority.

Native state drives topbar activity and progress. `completed + preserved` is
the completed-page count. A nonterminal run locks page selection. Current-owner
runs expose pause/resume and cancel; non-owner runs expose recovery only after
native eligibility; retry appears only for retryable failed pages owned by the
current session.

Discovery is fail closed. Until it succeeds, creating a run is disabled. The
workbench no longer subscribes to legacy PDF progress events, reads legacy page
state, renders legacy translated PDF paths, or projects PDF work as a frontend
`ActiveTranslationRun`.

No public native v3 export command exists yet. When the PDF v3 workflow is in
use, hide PDF export and reject any direct fallback attempt instead of exporting
a legacy artifact.

## Consequences

### Positive

- One typed native authority controls execution, ownership and page state.
- Frontend status memory remains bounded for 500/1,000-page documents.
- Pause, recovery and retry survive navigation and app restart without a
  persistent frontend current-run pointer.
- Source-language selection cannot silently diverge from trusted run metadata.
- Legacy page PDFs cannot be mistaken for v3 output.

### Costs

- The active workbench currently observes only its selected job and target
  language; cross-job v3 activity is not yet projected into the global sidebar.
- A status-window request is required when scrolling into an uncached region.
- PDF export remains unavailable until the native export coordinator receives a
  narrow Tauri command and workbench integration.

## Rejected Alternatives

- Continue mirroring native runs into frontend segment-run state.
- Let preview discovery and topbar control poll independently.
- Fetch every requested page record for progress or retry controls.
- Allow creation while latest-run discovery is incomplete.
- Retain legacy pdf2zh preview/export as an implicit fallback.
- Persist a frontend current-run ID or owner identity.
