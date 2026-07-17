# ADR 0037: PDF v3 Reproducible Single-Page Render Cache Bridge

Date: 2026-07-18

Status: Accepted

Amends ADR 0035 and ADR 0036.

## Context

ADR 0035 defined a bounded disposable render cache, and ADR 0036 connected
pending `TranslationPatch` data to the atomic low-level renderer. The two
components still lacked a safe bridge.

A cache key cannot be known from a pending patch because renderer decisions are
part of `patchId`. More importantly, a durable resolved patch must regenerate a
cache miss after restart. If the renderer only accepts pending patches, cached
page PDFs silently become a second translation authority.

Saving the whole working document after one page replacement would also make
each `translatedPagePdf` artifact scale with source document size. That would
recreate the disk-growth failure PDF v3 is intended to remove.

## Decision

The TranslationPatch renderer accepts exactly two lifecycle states:

- all entries pending, for the first decision/render pass;
- all entries resolved, for deterministic cache-miss reconstruction.

Mixed state is rejected before mutation. A resolved replay performs the same
source, anchor, style, font, geometry, fit and encoding preflight, then requires
every recomputed decision to equal the stored fitted/preserved decision. Any
strategy, scale or reason-code drift is typed failure and leaves the working
document unchanged.

The current renderer contract identity is:

```text
rosetta-pdf-v3-translation-patch-renderer/1
```

Rendering and cache addressing both require an exact version match. A newer
implementation cannot silently replay a patch created for another renderer
contract or reuse its cache namespace.

`translatedPagePdf` serialization consumes an explicitly owned working
`lopdf::Document`. After atomic replacement it:

1. removes every unselected page;
2. removes document-level outlines, page mode, open action, page labels, names
   and structure-tree navigation from the disposable page artifact;
3. prunes unreachable objects;
4. renumbers objects and compresses streams;
5. serializes and reloads the result;
6. requires a PDF signature and exactly one page.

The API consumes the working document instead of cloning it internally. This
makes memory ownership visible and allows a later bounded scheduler or page
working-document implementation without changing artifact identity.

The cache bridge accepts only resolved patches. Its key binds exact source
fingerprint, page, resolved patch ID/revision, current renderer version and
`translatedPagePdf` options. A checksum/signature-corrupt lease is invalidated
and returned as a cache miss. The page artifact binds its source fingerprint at
render time; insertion uses that bound identity and does not accept a second
caller-supplied fingerprint that could place bytes in the wrong namespace.

Rendering and cache insertion remain separate operations. Cache quota, lease or
I/O failure cannot discard the resolved patch or prevent it from being
committed to the patch store. Source PDF plus resolved patch remain sufficient
to rebuild all page artifacts.

## Evidence

Automated Windows tests prove:

- a resolved patch rerenders to the same patch identity and identical page PDF
  bytes from a fresh source document;
- a valid but different stored fit decision fails with zero document mutation;
- an old renderer contract cannot render or address the current cache;
- a 30-page source produces an exactly-one-page artifact smaller than source;
- pending patches cannot construct a cache key;
- cache miss, insert, hit and resolved-patch rebuild return identical bytes.

On the 30-page real-paper fixture, the 1,590,242-byte source produced a
104,857-byte page artifact. Independent Poppler rendering at 150 DPI changed
2,718 pixels, 0.1249% of the 1241x1754 page, confined to the selected footer
row. Visual inspection found no clipping or unrelated changes. The source page
and artifact both retained 26 annotations and the RWKV external link.
Independent `pypdf` extraction found `Bounded cached page`; `pdfinfo` confirmed
the output contains exactly one A4 page.

## Consequences

### Positive

- Render cache loss no longer loses the ability to reproduce page output.
- Per-page PDF disk size follows selected-page reachable objects rather than
  the whole source document.
- Renderer upgrades cannot accidentally reuse stale patches or artifacts.
- Cache failures remain disposable-state failures, not translation failures.
- Link annotations and page geometry survive the tested pruning path.

### Costs

- A resolved replay repeats renderer preflight on cache miss.
- Current page rendering still needs one explicitly owned working document;
  the long-document scheduler must bound how those documents are created and
  held.
- Document-level outlines and navigation are intentionally absent from cached
  one-page PDFs; final streaming export must preserve them from the source.
- Preview PNG cache population and streaming whole-document export remain
  separate work.

## Rejected Alternatives

- Treat cached page PDFs as durable translation authority.
- Rebuild a resolved patch by resetting it to pending without checking stored
  decisions.
- Save one complete source document for every translated page.
- Hide a full-document clone inside the page-artifact API.
- Make patch-store commit depend on successful cache insertion.
- Let renderer version be an unchecked caller-provided cache string.
