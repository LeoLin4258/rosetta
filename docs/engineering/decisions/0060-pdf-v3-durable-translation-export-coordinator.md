# ADR 0060: PDF v3 Durable Translation Export Coordinator

Date: 2026-07-18

Status: Accepted

Refines ADR 0040, ADR 0041, ADR 0044 and ADR 0059.

## Context

PDF v3 already had durable PageGraph and resolved TranslationPatch stores, a
document-wide font registry, immutable page-delta staging, and an atomic
incremental writer. Final export still lacked one production coordinator that
composed these authorities without loading all pages or rebuilding pending
translation state.

An export must also succeed when every selected entry is preserved. That case
has no font or page delta, while the incremental writer deliberately rejects an
empty commit. Falling back to an ordinary filesystem copy would lose source
identity validation, cancellation and atomic destination replacement.

## Decision

Add a translation export coordinator whose request binds:

- the immutable source fingerprint and lazy source-object store;
- the exact PageSet;
- the PageGraph and target-language TranslationPatch stores;
- immutable Regular and optional Bold translation-font assets;
- renderer policy, destination path and shared cancellation state.

The coordinator validates source/store identity and page count before export.
It then performs two sequential passes over the exact PageSet.
Cancellation from either pass or the final atomic writer is normalized to one
top-level export outcome so lifecycle callers do not depend on internal phases.

The first pass loads one PageGraph and resolved patch at a time, checks
cancellation between pages, and builds the bounded fitted-entry font plan from
ADR 0059. It prepares one document-wide subset for each required weight and
stages those font objects once.

The second pass reopens one PageGraph and patch at a time. Each resolved patch
is replayed through the current renderer against an overlay of the immutable
source plus the accumulated export delta. The renderer must reproduce every
stored fitted or preserved decision exactly. The page delta is merge-checked
before the next page starts; PageGraph and patch values are then released.

Decision replay and output embedding use separate prepared-font views. A
temporary page-local decision subset contains every classifiable entry needed
to reproduce fit or overflow decisions. The document output subset still
contains only fitted-entry characters. This prevents preserved overflow text
from increasing final PDF size without weakening deterministic replay.

After the final page, a non-empty delta is passed directly to the atomic
incremental writer. The writer copies the immutable source through its fixed
64 KiB buffer, verifies byte count and SHA-256, appends only changed/new
objects, syncs the temporary file and atomically replaces the destination.

If every entry is preserved, the coordinator uses a separate verified atomic
source-copy operation. It shares the same fixed buffer, source length and
SHA-256 checks, cancellation points, sync and destination replacement rules.
The result is byte-identical to the immutable source and contains no artificial
incremental section.

TranslationPatch remains the only durable translation authority. Prepared
fonts, page deltas and the accumulated export delta remain transient and are
never stored as independent artifacts.

## Evidence

Automated Windows/PDFium tests cover:

- two durable resolved patches exported from a 30-page source;
- one shared six-object Type0 font subset across both pages;
- ten total delta objects and an appended section below 25% of source size;
- both translated strings extractable only after export;
- changed pixels confined to the two selected pages with below 5% drift;
- the unselected third page remaining pixel-exact;
- an all-preserved patch producing a byte-exact verified source copy;
- overflow-preserved text replaying with temporary metrics but zero embedded
  font objects;
- cancellation during streaming font planning;
- atomic verified source-copy replacement without temporary sidecars.

## Consequences

### Positive

- Final native export is now derived entirely from durable PageGraph and
  resolved TranslationPatch authorities.
- PageGraph, patch and renderer working state remain page-local during both
  passes.
- One document subset per used weight keeps translated PDF growth small.
- Renderer-decision drift fails before destination replacement.
- All-preserved documents export successfully without adding bytes.
- Source identity and existing destination safety apply to both commit modes.

### Costs

- Export reads each selected PageGraph and patch twice.
- The merge-checked object delta still grows with the number and complexity of
  modified pages until the final atomic writer starts.
- A real complex 500/1,000-page export stress run is required to decide whether
  delta objects need a bounded disk spool.
- Runtime-manifest asset binding and Tauri/job lifecycle commands remain above
  this native coordinator.

## Rejected Alternatives

- Rebuild pending patches or call the provider during export.
- Persist prepared fonts or page-level PDF deltas.
- Embed one font subset per page.
- Trust stored renderer decisions without deterministic replay.
- Use a non-atomic filesystem copy when every entry is preserved.
- Build the complete translated `lopdf::Document` before saving.
