# 2026-07-14 PDF Prepared Window Cache

## Summary

Reduced repeated PDF translation latency after profiling a 10-page Linux run:
PDF preparation consumed about 11.8 seconds, RWKV Lightning about 3.5 seconds,
and render/commit about 2.0 seconds. Layout inference and unit collection were
the largest preparation costs.

## Change

- Added detailed content-free timing fields for font assets, prepared-document
  construction, layout inference, unit collection, prepare overhead, cache
  reset, and page rendering.
- The persistent Python worker now retains one successful prepared PDF window.
- Cache identity includes source path, size, modification time, selected pages,
  language pair, and engine thread count.
- The patched v2 engine stores pristine prepared-PDF bytes and reopens them on
  a cache hit before rendering, preventing translations from a prior run from
  leaking into the next run.
- A different cache key, failure, cancellation, or worker restart disposes the
  active entry. Older packs without `resetRun` fall back to a full prepare.

The cache is process-local and bounded to one entry. No persistent job schema,
artifact contract, or engine contract version changed.

## Validation

- `cargo fmt -- --check`
- `cargo check`
- `cargo test rosetta_jobs`
- Python worker and patch script compilation
- PDF patch regression tests, including pristine prepared-document reset

Ubuntu RTX 4090 validation on the same 10-page, roughly 14,000-character PDF:

- first run: `20.567s` total, including `14.404s` prepare, `3.887s`
  Lightning, and `2.205s` render;
- immediate retranslation: `5.638s` total with one cache hit, including `5ms`
  prepare (`3ms` reset), `3.499s` Lightning, and `2.059s` render;
- all 10 committed page artifacts reopened successfully as readable,
  single-page PDFs with extractable text.

The immediate retranslation was about `3.65x` faster. The remaining warm-run
cost is almost entirely model generation and page rendering.
