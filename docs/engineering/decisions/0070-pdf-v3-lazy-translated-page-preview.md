# ADR 0070: PDF v3 Lazy Translated-Page Preview

Date: 2026-07-19

Status: Accepted

Refines ADR 0040, ADR 0043, ADR 0044, ADR 0054, ADR 0065, ADR 0066 and
ADR 0069.

## Context

PDF v3 already persisted compact page patches and could generate bounded
single-page PDF and PNG cache artifacts, but the production app had no command
that joined those pieces without loading the complete source document. Preview
also must remain usable after the managed translation model stops, while still
rejecting stale scheduler, PageGraph, patch, source or font identity.

For hundreds-page documents, one preview request must do work proportional to
one selected page and its reachable PDF objects. A cache hit must not repeat
font preparation or whole-file hashing. Preview artifacts must remain
disposable and must never become translation authority.

## Decision

Expose one asynchronous binary Tauri command accepting only `jobId`, `runId`,
positive `pageNumber` and bounded `targetWidth`. Resolve the source and job
directories internally and return PNG bytes through `tauri::ipc::Response`.

The command accepts only an unleased scheduler `completed` page whose exact
extraction artifact/source-page hash and patch ID/revision match the current
PageGraph store, TranslationPatch store and immutable runtime manifest.
Pending, extracted, preserved, failed, leased, non-requested and stale pages
fail closed. Preserved pages continue to use source preview rather than a
fabricated translation patch.

Preview follows this ordered path:

1. Return a validated render-cache PNG immediately.
2. Otherwise verify source identity using a 32-entry file-stamp LRU, then
   rasterize a validated cached single-page PDF.
3. Otherwise resolve only the managed unified font assets, validate them
   against the immutable run manifest, lazily materialize the selected page,
   render its durable patch and rasterize it under the process-wide PDFium
   operation lock.

The source materializer follows only objects reachable from the selected page,
materializes inherited page resources and geometry, rejects cross-page page
tree references, strips document navigation/actions and enforces 65,536-object
and 128-depth limits. It consumes `PdfSourceObjectStore`; it does not construct
a complete `lopdf::Document` for the source.

The preview path does not require a healthy translation provider or model
process after translation completes. Unified font resolution is independent of
runtime health and occurs only on a full cache miss. Cache insertion is best
effort because both page PDF and PNG are quota-bounded disposable derivatives.

## Evidence

Windows tests prove page-state gating, scheduler/store authority mismatch
rejection, PNG-first lookup, cached single-page PDF fallback, source identity
cache invalidation and pixel-exact lazy materialization of a selected page from
a real multi-page paper under the 512-object / 16 MiB source cache ceiling.

## Consequences

### Positive

- Translated preview no longer requires a complete translated PDF or complete
  source object graph.
- Cached preview remains available after the translation model stops.
- Repeated page viewing avoids font work, and PNG hits avoid source hashing.
- The command exposes no filesystem path, document text or runtime credential.

### Costs

- A page-PDF cache hit still verifies source identity before rasterization.
- File size and modification time are only cache invalidation stamps; changed
  stamps require a complete source SHA-256 verification.
- Preserved pages require the UI to select source preview from typed page state.
- Older run revisions whose page patch has been superseded are rejected rather
  than silently previewing the newest revision.

## Rejected Alternatives

- Load the complete source through `lopdf::Document` for each preview.
- Require the managed model process to remain healthy for completed previews.
- Treat cached PNG or page PDF as durable completion authority.
- Re-prepare unified fonts on every preview request.
- Preview a newer stored patch when the selected run references an older one.
