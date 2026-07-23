# PDF2zh Production Performance and Artifact Bounds

Date: 2026-07-21

## Summary

Optimized the restored `pdf2zh` production path without changing RWKV request
behavior or replacing the proven pdf2zh renderer.

## Changes

- Reuse pdfminer font and CMap resources across selected pages.
- Remove scalar NumPy overhead from converter coordinate hot loops.
- Preserve duplicate-layer decisions while avoiding unnecessary full text
  similarity calculations.
- Share output font objects across page resource dictionaries.
- Subset fonts in durable page artifacts using a best-effort fallback.
- Use DirectML for Windows layout inference with a five-page bounded batch,
  tensor-shape grouping, stable page ordering, and automatic CPU fallback.
- Keep the native bounded scheduler, stores, caches, recovery work, and legacy
  adapter isolated from the production visual renderer.

## Result

On the exact ten-page benchmark, median cache-miss `prepareRun` time fell from
7,473.7 ms to 4,335.4 ms, a 42.0% reduction. Immediate durable page artifacts
fell from 209,914,018 bytes to 139,294,288 bytes, a 33.6% reduction; the
existing post-translation background compression reduced them further to
91,440,222 bytes. Translation units, source characters, payload hash, and all
ten rasterized pages were unchanged.

No RWKV prompt, batch, concurrency, or request path changed. Windows warm
cache-miss `prepareRun` median fell again from 4,335.4 ms to 2,529.2 ms.
Isolated layout time fell from 2,923.0 ms to 1,388.9 ms. The cumulative warm
reduction from the original 7,473.7 ms median is 66.2%.

DirectML raises the fixed PDF worker peak from approximately 497 MB to 1,367 MB
on the ten-page fixture. The batch is capped at five pages, so this allocation
does not scale with document length. A controlled fixed-token probe found no
median RWKV throughput regression, but complete App translation remains a
manual acceptance gate because one resident-DirectML sample showed higher
variance.

## Compatibility

No persistent schema changed. Font subsetting can be disabled with the
internal `singlePageSubsetFonts` engine option if an unsupported document needs
the previous artifact behavior. Windows DirectML initialization or inference
failure automatically returns to the CPU provider; other platforms keep their
existing provider path.
