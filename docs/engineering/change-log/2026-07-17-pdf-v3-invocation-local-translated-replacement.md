# PDF v3 Invocation-Local Translated Replacement

Date: 2026-07-17

## Summary

Connected unified-font translated text-show transactions to invocation-local
copy-on-write. A validated Form invocation or cross-page shared top-level
stream can now receive a searchable translation without changing sibling Form
invocations or unselected pages.

The implementation remains isolated from PDF v2, jobs, UI, persistent
`TranslationPatch`, preview cache and export.

## Implementation

Added:

- complete Form invocation path matching in PageGraph geometry resolution;
- transaction validation requiring one page, stream and invocation path;
- shared-stream mapping classification only after decode, font and atom
  coverage gates pass;
- a generic staged resource binding model for invocation-local copy-on-write;
- effective resource materialization and unified-font attachment on cloned Form
  leaves;
- selected-page font attachment for cross-page top-level stream clones;
- deterministic font-first, clone-second object ID reservation;
- atomic commit of font objects, stream clones, page resources and `max_id`;
- per-show diagnostic schema `/6` with `formInvocationDepth`;
- transaction diagnostic schema `/3` with `formInvocationDepth`,
  `clonedStreamCount` and `pageContentRewired`.

Unique top-level replacement retains the minimal in-place path. Identity
operand patches reuse the generalized copy-on-write stage with no resource
bindings and retain their previous semantics.

## Windows AMD Results

Synthetic Form fixture with two invocations on one page:

- only the selected second invocation was translated;
- fit scale: 1.0;
- cloned streams: 2;
- replacement time: about 4 ms in the debug probe;
- output size: 16,564 bytes;
- original Form bytes: unchanged;
- PDFium extraction: sibling source text plus selected translated text;
- Poppler difference region: selected second line only;
- visual review: no clipping, overlap, missing glyphs or later-text movement.

Synthetic two-page fixture with one shared top-level content stream:

- only page 1 received a cloned `/Contents` stream;
- page 2 continued to reference and extract the original stream;
- cloned streams: 1;
- Poppler page 1 differences were confined to the selected line;
- Poppler page 2 was pixel-exact.

An invalid Form `Do` path after font staging left every document object and
`max_id` unchanged.

## Current Boundary

- one transaction targets one underlying stream and one invocation path;
- multi-target Form clone-tree merging is not implemented;
- repeated same-stream page content references remain ambiguous;
- unanchored consecutive `Tj`/`TJ` remains unsupported;
- paragraph reflow, protected spans and durable `TranslationPatch` persistence
  remain disconnected;
- bounded-memory streaming export is still pending.

## Validation

- `cargo fmt --all -- --check`: passed;
- `cargo check`: passed;
- `cargo test pdf_v3`: 68 passed, 0 failed, 10 ignored manual probes;
- `cargo test rosetta_jobs`: 78 passed, 0 failed;
- PDFium selected/sibling invocation text extraction: passed;
- PDFium selected/unselected page text extraction: passed;
- Poppler selected-page visual diff: confined to target regions;
- Poppler sibling page render: pixel-exact;
- rendered page visual review: passed.
