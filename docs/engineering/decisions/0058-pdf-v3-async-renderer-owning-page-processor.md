# ADR 0058: PDF v3 Async Renderer-Owning Page Processor

Date: 2026-07-18

Status: Accepted

Refines ADR 0055, ADR 0056 and ADR 0057.

## Context

The durable translation worker previously accepted a synchronous closure while
all local providers are asynchronous. Blocking the Tokio runtime inside that
closure would make cancellation and long-document backpressure unreliable. The
planner and provider bridge also stopped at a pending TranslationPatch, which
must never enter durable storage before renderer decisions are resolved.

The processor must keep runtime identity explicit, reuse document-level fonts
and ownership indexes, retain only one PageGraph across provider I/O, and leave
the source page untouched when planning or rendering is unsafe.

## Decision

Replace the translation worker's synchronous closure boundary with a single
async `PdfV3TranslationPageProcessor` contract. The worker continues to own
leases, PageGraph loading, patch validation, durable patch commit and scheduler
commit. It awaits one processor page at a time and checks processor cancellation
again immediately before durable patch storage.

Add a concrete local processor with an explicit immutable configuration:

- source fingerprint, source and target language;
- local provider config plus provider and model identity;
- translation revision and renderer version/policy;
- shared cancellation state;
- prepared Regular and Bold unified translation fonts.

Construction validates the configuration against the scheduler translation
binding and immutable source page count. Provider/model identity is supplied by
the caller's runtime/component manifest; it is never inferred from provider
output.

For each claimed page, the processor performs:

```text
PageGraph
  -> identity-bound TranslationPagePlan
  -> async local provider
  -> identity-keyed results
  -> pending TranslationPatch
  -> renderer staging and decision resolution
  -> resolved TranslationPatch
```

The processor owns a reusable selected-page index, direct-stream ownership
index, document font registry and accumulated in-memory `PdfObjectDelta`.
Physical PDF mutation remains staged. Only the resolved patch is returned to
the durable worker; the delta is disposable render/export state and is not a
translation authority.

A page with no safe plan units returns explicit page preservation without
provider I/O. Recoverable overflow or unsupported entry geometry becomes a
resolved entry-level preservation decision. Invalid plans, provider failure or
hard renderer failure return typed stable failure reasons and cannot commit a
patch.

`PdfPageIndex` now resolves indirect `/Contents` arrays to their physical stream
object IDs. Ownership analysis therefore never treats an array container as a
content stream, while selected-page traversal remains bounded.

## Evidence

Automated tests cover:

- provider success through pending patch to resolved fitted patch;
- no-safe-unit page preservation without provider I/O;
- overflow converted to resolved entry preservation;
- missing prepared glyph converted to a hard renderer failure;
- cancellation before provider work and immediately before durable commit;
- invalid runtime identity rejected during processor construction;
- renderer failure never entering TranslationPatchStore;
- indirect `/Contents` arrays resolving to physical stream IDs.

## Consequences

### Positive

- No runtime blocking bridge is required between the durable worker and local
  providers.
- One PageGraph remains the complete in-memory ownership boundary across
  provider and renderer work.
- Provider, model, renderer, language and revision identity are explicit and
  reproducible.
- Pending patches and hard renderer failures cannot become durable translation
  authority.
- Font resources and cross-page ownership analysis are reusable across pages.

### Costs

- Prepared font subsets must currently be supplied before the processor starts;
  job-level character planning and component asset selection remain above this
  slice.
- Accumulated render deltas are process-local and disposable. Restart/export
  rebuilds them deterministically from durable patches.
- The concrete processor still resides beside the legacy PDF job provider code
  until the app switches orchestration and removes pdf2zh ownership.
- Tauri lifecycle commands, typed frontend status and real 500/1,000-page
  provider/render/export stress validation remain pending.

## Rejected Alternatives

- Block the async provider from the old synchronous worker closure.
- Let provider output or response metadata choose provider/model identity.
- Persist pending patches and resolve renderer decisions during export.
- Retain multiple PageGraphs so translation units can be batched across pages.
- Treat indirect `/Contents` array objects as physical content streams.
