# ADR 0038: PDF v3 Versioned Preview Raster Cache Bridge

Date: 2026-07-18

Status: Accepted

Amends ADR 0035 and ADR 0037.

## Context

ADR 0037 made a resolved `TranslationPatch` reproducibly generate a compact,
exactly-one-page PDF artifact. The bounded cache already supported a
`previewPng` output kind, but no production bridge populated or read those
entries.

Preview rasterization has its own output contract. A patch renderer can remain
unchanged while a PDFium build, render configuration, size policy or PNG
encoder changes pixels or bytes. Keying PNGs only by the patch renderer would
allow stale preview hits. Silently clamping a requested width would also make
the key disagree with the actual artifact and weaken UI state ownership.

## Decision

PDF v3 adds an isolated preview rasterizer after single-page PDF generation.
It accepts only a `TranslationPatchPagePdf` whose patch is fully resolved and
uses the current translation renderer contract.

The current preview contract identity is:

```text
rosetta-pdf-v3-preview-rasterizer/1
```

A preview cache key combines it with the current patch renderer identity:

```text
rosetta-pdf-v3-translation-patch-renderer/1+rosetta-pdf-v3-preview-rasterizer/1
```

This combined identity must change when the PDFium render configuration, PNG
encoding policy or bundled raster behavior changes in a way that can affect
output.

The initial API supports exact pixel widths from 200 through 1,800 inclusive.
Out-of-range requests are typed errors rather than clamped values. The key's
`pixelWidth` therefore always equals the rendered PNG width; height is derived
from page geometry.

The rasterizer reloads the page artifact in PDFium, requires exactly one page,
renders page index zero, converts to RGBA and encodes a fast adaptive-filter
PNG. It validates the exact requested width, nonzero height and PNG signature
before returning an artifact.

The preview artifact owns a private, complete cache key. Cache insertion does
not accept separate source, patch or width arguments, so callers cannot place
valid bytes into the wrong namespace. Lease reads retain the render cache's
length, SHA-256 and signature checks. Corrupt bodies are invalidated and
reported as cache misses.

Preview render and insertion remain separate. Cache quota or I/O failure does
not affect the durable patch or the ability to recreate either the page PDF or
PNG. Width variants consume separate bounded entries and remain subject to the
shared 384 MiB / 4,096-entry default LRU policy.

## Evidence

The Windows AMD automated bridge test covers:

- preview miss, render, insert, hit and byte identity;
- exact PNG decode dimensions and signature;
- distinct key identity for 1,200 and 900 pixel widths;
- combined renderer/rasterizer version identity;
- explicit rejection below the supported width bound;
- coexistence of one page PDF and one preview PNG in the same bounded cache.

The 30-page real-paper fixture produced a 1,200x1,697 PDFium PNG of 1,054,528
bytes from its 104,857-byte translated page artifact. Visual inspection showed
the full page, expected translated footer and no clipping, blank page or
unrelated layout movement. Independent Poppler rendered the same page PDF at
1,200x1,698; the one-pixel height difference is rounding between raster
engines, not page-geometry drift.

## Consequences

### Positive

- Preview bytes are disposable, reproducible and bounded instead of held in an
  unbounded process cache.
- Rasterizer changes cannot silently reuse PNGs from another output contract.
- UI width state and cached artifact dimensions remain exact and inspectable.
- A bad preview entry cannot invalidate the durable translation patch.

### Costs

- A preview miss may first rebuild the page PDF and then rasterize it.
- One 1,200-pixel paper page is about 1 MiB with the low-latency PNG policy;
  the hard shared quota remains necessary.
- The initial API supports width-addressed previews only. Scale-addressed keys
  remain reserved by the cache schema.
- This bridge does not yet expose a Tauri command or schedule long-document
  preview work; those belong to the later orchestrator and UI integration.

## Rejected Alternatives

- Reuse the patch renderer version alone for PNG cache identity.
- Clamp arbitrary width requests while retaining the caller's width in the key.
- Cache raw RGBA bitmaps.
- Make preview cache insertion part of durable patch commit.
- Rasterize the original multi-page document for every translated preview.
