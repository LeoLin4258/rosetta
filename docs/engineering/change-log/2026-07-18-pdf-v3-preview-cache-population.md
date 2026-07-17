# PDF v3 Preview Cache Population

Date: 2026-07-18

## Summary

Connected resolved PDF v3 single-page translation artifacts to on-demand
PDFium PNG rasterization and the bounded render cache. Preview entries now have
exact size identity and a separately versioned raster contract.

## Implementation

Added:

- isolated `pdf_v3::preview` raster and cache bridge;
- preview contract identity `rosetta-pdf-v3-preview-rasterizer/1`;
- cache identity combining patch renderer and preview rasterizer versions;
- strict 200..=1,800 pixel width validation without silent clamping;
- exactly-one-page PDFium reload before rasterization;
- exact-width/nonzero-height/PNG-signature validation;
- fast adaptive-filter RGBA PNG encoding;
- preview artifacts with private source/patch/width cache identity;
- bounded insert and lease-validated read helpers;
- corrupt cached PNG conversion to a rebuildable miss;
- width-variant, decode-dimension and cache round-trip tests.

Preview generation and cache insertion remain independent. Cache failure does
not affect `TranslationPatch` authority or page-PDF reproducibility.

## Current Boundary

- callers still supply or rebuild one translated page PDF before a preview
  miss can rasterize;
- preview work is not yet exposed through the v3 Tauri command surface;
- the long-document scheduler does not yet own preview backpressure;
- scale-addressed previews remain reserved but unimplemented;
- patch compression and streaming final export remain pending.

## Visual Verification

The Windows AMD probe used page 1 of the 30-page real-paper fixture.

- translated single-page PDF: 104,857 bytes;
- PDFium preview: 1,200x1,697 and 1,054,528 bytes;
- page content, two-column layout, chart, links and translated footer were
  visible without clipping or blank areas;
- independent Poppler output was 1,200x1,698, consistent with a one-pixel
  raster-height rounding difference;
- the translated footer remained the only intended content change.

## Validation

- targeted preview/cache bridge test: passed;
- manual Windows preview probe: passed;
- complete PDF v3 suite: 113 passed, 13 ignored manual probes;
- `rosetta_jobs`: 78 passed;
- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `pnpm typecheck`: passed;
- `git diff --check`: passed.
