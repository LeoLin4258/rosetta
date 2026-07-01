# 2026-07-02 Lightning Performance Instrumentation

## Summary

Added the first privacy-safe instrumentation and tuning pass for Windows
NVIDIA `RWKV Lightning CUDA` performance work.

This change does not alter provider preference semantics, runtime selection,
generation parameters, PDF layout output semantics, llama.cpp Vulkan tuning, or
macOS MLX behavior.

## Changes

- Added `ROSETTA_RWKV_PERF_DEBUG=1`, which writes
  `rwkv-performance.jsonl` under the app log directory.
- Logged Lightning request timing summaries without source text, translated
  text, prompt bodies, document structure content, or raw responses.
- Extended PDF shim metrics with request p95, batch-size distribution, average
  batch size, and batch assembly wait timings.
- Added Lightning-only tuning controls for local sweeps without changing
  llama.cpp Vulkan or macOS MLX:
  - `ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE`
  - `ROSETTA_PDF_SHIM_LIGHTNING_THREAD_COUNT`
  - `ROSETTA_PDF_SHIM_LIGHTNING_IN_FLIGHT_REQUESTS`
  - `ROSETTA_PDF_SHIM_LIGHTNING_BATCH_WINDOW_MS`
  - `ROSETTA_PDF_LIGHTNING_PAGE_CHUNK_SIZE`
  - `ROSETTA_PDF_SHIM_LIGHTNING_BODY_TARGET`
  - `ROSETTA_PDF_SHIM_LIGHTNING_BODY_HARD`
  - `ROSETTA_PDF_SHIM_LIGHTNING_CAPTION_TARGET`
  - `ROSETTA_PDF_SHIM_LIGHTNING_CAPTION_HARD`
  - `ROSETTA_PDF_SHIM_LIGHTNING_REFERENCE_TARGET`
  - `ROSETTA_PDF_SHIM_LIGHTNING_REFERENCE_HARD`
- Raised the Lightning-only ordinary-document batch width from 16 to 100.
- Raised the Lightning-only PDF shim batch width to 256 by default.
- Kept the Lightning-only PDF worker count at 100 by default and decoupled it
  from shim batch width, after `thread=512` proved slower in local testing.
- Added concurrent in-flight Lightning requests for the PDF shim, default 8, so
  small PDF batches can overlap instead of serializing every RWKV request.
- Raised the Lightning-only PDF page chunk size from 10 pages to 100 pages by
  default.
- Kept the Lightning-only PDF shim aggregation window at 80 ms after a 250 ms
  sweep did not improve observed batch size.
- Kept the Lightning-only PDF shim text chunk budgets at the smaller proven
  values after wider chunks caused large request tail latency. Non-Lightning
  providers still use the existing conservative PDF chunking and pdf2zh worker
  ceiling.
- Added `scripts/summarize-lightning-performance.mjs` to summarize Lightning
  Markdown and PDF baseline runs from local logs and PDF profiles.
- Added a benchmark method note for collecting the first Lightning baseline.

## Validation

Run from `rosetta-app` when touching this area:

```powershell
pnpm typecheck
cd src-tauri
cargo test rosetta_jobs
cargo check
```

Runtime benchmark validation still requires a real local Windows NVIDIA run
with `ROSETTA_RWKV_PERF_DEBUG=1`.
