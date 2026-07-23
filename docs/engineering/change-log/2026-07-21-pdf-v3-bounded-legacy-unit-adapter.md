# Bounded Native PDF Unit Adapter

Date: 2026-07-21

## Summary

Added the first production-independent slice of the native PDF preparse
adapter. It converts one persisted v3 PageGraph at a time into a compact,
legacy-compatible unit candidate window without activating the failed v3
region renderer or changing the production PDF workbench.

## Safety Boundary

- The adapter processes one page per call.
- Page source text is capped at 16 MiB.
- Unit count is capped at 25,000 per page.
- It does not retain a document-wide unit list or translation map.
- It preserves both source and provider text for the later renderer bridge;
  no output is sent to RWKV by this slice.

## Validation

```text
cargo test pdf_v3::legacy_adapter --lib -- --nocapture
  1 passed
cargo check
  passed
```

This is not yet the production preparse replacement. The next slice must add a
bounded persisted window reader and a command-driven long-document probe before
the adapter can replace pdf2zh unit collection.

## Ten-Page Probe

The ignored Windows probe was run against the exact
`2604.17278v1.pdf` ten-page source with no source or translation text printed:

```text
pages=10
elapsedMs=4837
units=433
mergedUnits=171
sourceChars=36873
pageGraphDiskBytes=3971220
maxUnitsPerPage=235
maxMergedUnitsPerPage=34
maxWorkingSet=44929024
processPeakWorkingSet=45277184
```

The result is bounded and materially faster than the old cache-miss preparse,
but the 171 merged units are not yet proven compatible with the old provider
batch shape or renderer unit identity. Production routing remains unchanged
until that compatibility and long-document behavior are tested.

## Follow-Up: Legacy Renderer Memory Bound

The production `pdf2zh` path remains the visual authority. During page commits,
the Rust bridge now releases the committed page's unit index and translated
strings immediately, and moves the prepared unit vector into the translation
task instead of cloning it. Existing RWKV batches and the renderer contract
are unchanged, while translated text and duplicate unit storage no longer
grow for the entire selected document.

This is a memory-safety improvement only; it does not claim that the native
adapter is already a drop-in replacement for `pdf2zh` layout preparation.
