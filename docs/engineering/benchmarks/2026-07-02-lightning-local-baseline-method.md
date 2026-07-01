# 2026-07-02 Lightning Local Baseline Method

## Purpose

This note defines the first repeatable local-only benchmark method for the
Windows NVIDIA `RWKV Lightning CUDA` path.

The goal is measurement before tuning:

- record Markdown and PDF throughput through `rwkv-lightning-contents`;
- identify actual Lightning batch sizes, request latency, and client overhead;
- keep diagnostics local and privacy-safe by default.

This benchmark intentionally does not optimize or compare llama.cpp Vulkan or
macOS MLX paths.

## Privacy Boundary

Use the performance log for baseline runs:

```powershell
$env:ROSETTA_RWKV_PERF_DEBUG="1"
pnpm tauri dev
```

`ROSETTA_RWKV_PERF_DEBUG=1` writes:

```txt
%APPDATA%\com.rosetta.desktop\logs\rwkv-performance.jsonl
```

Each record contains only:

- provider id and request context id;
- endpoint path / URL;
- source and target language ids;
- batch size;
- input and output character counts;
- status code and success/failure flag;
- request preparation, HTTP, response read, response parse, and total latency.

It must not contain source text, translated text, prompt bodies, document
structure content, or raw model responses.

Avoid using `ROSETTA_RWKV_IO_DEBUG=1` for performance baselines unless you
specifically need full request/response debugging. That older log intentionally
records local source and output text for deep debugging and is not suitable for
committed benchmark evidence.

## Added Instrumentation

Rust app log:

```txt
%APPDATA%\com.rosetta.desktop\logs\rwkv-performance.jsonl
```

PDF job diagnostics:

```txt
%APPDATA%\com.rosetta.desktop\jobs\<job-id>\diagnostics\pdf-translation-profile-<run-id>.json
%APPDATA%\com.rosetta.desktop\jobs\<job-id>\diagnostics\pdf-timeline.jsonl
```

The PDF profile now includes Lightning-relevant shim metrics:

- `requestCount`
- `failedRequestCount`
- `totalRequestMs`
- `averageRequestMs`
- `maxRequestMs`
- `p95RequestMs`
- `totalBatchItems`
- `averageBatchSize`
- `batchSizeDistribution`
- `totalAssemblyWaitMs`
- `averageAssemblyWaitMs`
- `maxAssemblyWaitMs`
- `totalInputChars`
- `totalOutputChars`

`batchSizeDistribution` and assembly wait time answer whether pdf2zh is feeding
Lightning full batches or mostly serial small requests.

## Summary Script

The local summary script is:

```txt
rosetta-app/scripts/summarize-lightning-performance.mjs
```

Example for a PDF run:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/summarize-lightning-performance.mjs `
  --job-id <job-id> `
  --profile "$env:APPDATA\com.rosetta.desktop\jobs\<job-id>\diagnostics\pdf-translation-profile-<run-id>.json" `
  --output "$env:APPDATA\com.rosetta.desktop\jobs\<job-id>\diagnostics\lightning-baseline-<run-id>.json"
```

For the batch-width sweep, set the Lightning-only PDF shim batch size before
starting Tauri:

```powershell
$env:ROSETTA_RWKV_PERF_DEBUG="1"
$env:ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE="100"
pnpm tauri dev
```

After the first real baselines, the Lightning-only defaults are intentionally
more aggressive than the MLX / llama.cpp defaults:

| Setting | Default | Env override | Scope |
| --- | ---: | --- | --- |
| Ordinary-document Lightning batch width | 100 | none yet | Markdown/TXT through `rwkv-lightning-contents` |
| PDF Lightning shim batch width | 256 | `ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE` | PDF shim only |
| PDF Lightning pdf2zh worker count | 100 | `ROSETTA_PDF_SHIM_LIGHTNING_THREAD_COUNT` | PDF shim only |
| PDF Lightning in-flight RWKV requests | 32 | `ROSETTA_PDF_SHIM_LIGHTNING_IN_FLIGHT_REQUESTS` | PDF shim only |
| PDF Lightning batch assembly | off | `ROSETTA_PDF_SHIM_LIGHTNING_ASSEMBLE_BATCHES=1` | PDF shim only |
| PDF Lightning page chunk size | 100 pages | `ROSETTA_PDF_LIGHTNING_PAGE_CHUNK_SIZE` | PDF run loop only |
| PDF Lightning shim aggregation window | 80 ms | `ROSETTA_PDF_SHIM_LIGHTNING_BATCH_WINDOW_MS` | PDF shim only |
| PDF Lightning body chunk target / hard | 150 / 190 estimated prompt tokens | `ROSETTA_PDF_SHIM_LIGHTNING_BODY_TARGET`, `ROSETTA_PDF_SHIM_LIGHTNING_BODY_HARD` | PDF shim only |
| PDF Lightning caption chunk target / hard | 150 / 190 estimated prompt tokens | `ROSETTA_PDF_SHIM_LIGHTNING_CAPTION_TARGET`, `ROSETTA_PDF_SHIM_LIGHTNING_CAPTION_HARD` | PDF shim only |
| PDF Lightning reference chunk target / hard | 130 / 170 estimated prompt tokens | `ROSETTA_PDF_SHIM_LIGHTNING_REFERENCE_TARGET`, `ROSETTA_PDF_SHIM_LIGHTNING_REFERENCE_HARD` | PDF shim only |

Run separate PDF translations with shim batch values such as `256`, `512`, and,
if stable, `1024`. This value controls the Lightning shim batch width only.
pdf2zh worker count is intentionally separate because `thread=512` was slower
in the first local run. These settings do not change llama.cpp Vulkan or macOS
MLX behavior.

Example for a Markdown run:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/summarize-lightning-performance.mjs `
  --job-id <job-id> `
  --run-id <run-id> `
  --output "$env:APPDATA\com.rosetta.desktop\logs\lightning-markdown-baseline-<run-id>.json"
```

Use `--context <text>` when the exact `lightning-run:*` or `pdf-job:*` context
is known.

## Workloads

Record at least these workloads before changing performance behavior:

| Workload | Fixture | Notes |
| --- | --- | --- |
| Small Markdown | TBD | Enough to measure cold/warm overhead. |
| Long Markdown | TBD | Long enough to produce multiple Lightning batches. |
| Small PDF | TBD | 1-2 pages, exposes worker and shim warmup. |
| Realistic PDF | TBD | Enough pages to expose shim aggregation behavior. |

## Baseline Table

Fill this table only from real local runs.

| Workload | Provider | Pages / segments | Requests | Avg batch | p95 request | Total wall | Input chars/s | Output chars/s | Errors |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small Markdown | `rwkv-lightning-contents` | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Long Markdown | `rwkv-lightning-contents` | 571 segments | 36 | 15.86 | `0.812s` | `15.232s` | 429.36 | 1,313.88 | 0 |
| Small PDF | `rwkv-lightning-contents` | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| Realistic PDF | `rwkv-lightning-contents` | 10 pages / 131 shim items | 20 | 6.55 | `2.069s` | `36.812s` | 1,098.34 | 406.32 | 0 |
| Realistic PDF, Lightning thread 100 | `rwkv-lightning-contents` | 10 pages / 131 shim items | 10 | 13.10 | `2.430s` | `25.623s` | 1,657.80 | 613.10 | 0 |
| Realistic PDF, Lightning thread 100 | `rwkv-lightning-contents` | 18 pages / 211 shim items | 20 | 10.55 | `2.379s` | `42.059s` | 1,411.89 | 525.61 | 0 |

## First Real PDF Baseline

Recorded after the first instrumentation pass.

Environment:

| Field | Value |
| --- | --- |
| OS | Windows 11 Pro, build 26200 |
| GPU | NVIDIA GeForce RTX 5070 |
| NVIDIA driver | 596.21 |
| CUDA runtime reported by `nvidia-smi` | 13.2 |
| Provider | `rwkv-lightning-contents` |
| PDF shim Lightning max batch size | 8 |
| Workload | `2604.17278v1.pdf`, pages `1-10` |
| Job id | `job-1782929196219-2604-17278v1` |
| Run id | `run-pdf-1782929204834` |
| Summary JSON | `%APPDATA%\com.rosetta.desktop\jobs\job-1782929196219-2604-17278v1\diagnostics\lightning-baseline-run-pdf-1782929204834.json` |

PDF profile:

| Metric | Value |
| --- | ---: |
| Status | completed |
| Pages requested | 10 |
| Pages translated | 10 |
| Pages failed | 0 |
| Total wall time | `36.812s` |
| pdf2zh process time | `36.781s` |
| Lightning request count | 20 |
| Lightning total request time | `30.200s` |
| Lightning average request time | `1.510s` |
| Lightning p95 request time | `2.069s` |
| Lightning max request time | `2.198s` |
| Shim items translated | 131 |
| Average batch size | 6.55 |
| Batch size distribution | `1x1, 2x1, 3x1, 5x2, 6x2, 7x1, 8x12` |
| Total assembly wait | `0.807s` |
| Average assembly wait | `0.040s` |
| Max assembly wait | `0.096s` |
| Input chars | 36,387 |
| Output chars | 13,461 |
| Source chars / observed wall second | 1,098.34 |
| Output chars / observed wall second | 406.32 |

Interpretation:

- This baseline was captured before raising the Lightning PDF default to 100.
- The PDF shim is usually reaching the current Lightning max batch size: 12 of
  20 requests used batch size 8.
- Assembly wait is small relative to model request time: `0.807s` total wait
  versus `30.200s` total Lightning request time.
- The first bottleneck to investigate is likely Lightning batch width or runtime
  generation throughput rather than shim aggregation delay.
- Run a controlled sweep with `ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE` before
  accepting any default batch-size change.

Follow-up high-concurrency run:

- After this baseline, the Lightning-only PDF default was raised to 100 to
  test the RTX 5070 path closer to RWKV Lightning's high-concurrency design.
- A real `thread=100` rerun completed successfully:
  - Job id: `job-1782930346478-2604-17278v1`
  - Run id: `run-pdf-1782930351911`
  - Summary JSON:
    `%APPDATA%\com.rosetta.desktop\jobs\job-1782930346478-2604-17278v1\diagnostics\lightning-baseline-run-pdf-1782930351911.json`
- The high-concurrency run cut total wall time from `36.812s` to `25.623s`
  (`30.4%` faster) and reduced Lightning requests from `20` to `10`.
- The max observed batch was only `18`, not `100`, because this 10-page PDF
  exposes only 131 shim items and pdf2zh does not present them all to the shim
  at once. Larger page selections are needed to test whether Lightning benefits
  from 64+ or 100+ item batches.
- Request p95 rose from `2.069s` to `2.430s`, but fewer requests and better
  overlap more than offset the per-request latency increase.

## Follow-Up Markdown And 18-Page PDF Runs

Recorded after starting Tauri with:

```powershell
$env:ROSETTA_RWKV_PERF_DEBUG="1"
pnpm tauri dev
```

Because the app build used here had already raised the Lightning-only PDF shim
default to 100, no `ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE` override was
needed for this pass.

Long Markdown run:

| Field | Value |
| --- | --- |
| Workload | `rosetta_project_plan.md` |
| Job id | `job-1782930683245-rosetta-project-plan` |
| Run id | `run-1782930718488-13f8ca` |
| Direction recorded by perf log | `zh-CN` -> `en` |
| Summary JSON | `%APPDATA%\com.rosetta.desktop\logs\lightning-markdown-baseline-run-1782930718488-13f8ca.json` |

| Metric | Value |
| --- | ---: |
| Status | completed |
| Segments translated | 571 |
| Lightning request count | 36 |
| Lightning failed requests | 0 |
| Lightning summed request time | `15.548s` |
| Lightning average request time | `0.432s` |
| Lightning median request time | `0.376s` |
| Lightning p95 request time | `0.812s` |
| Lightning max request time | `0.940s` |
| Average batch size | 15.86 |
| Batch size distribution | `11x1, 16x35` |
| Input chars | 6,540 |
| Output chars | 20,013 |
| Observed wall time from perf log | `15.232s` |
| Source chars / observed wall second | 429.36 |
| Output chars / observed wall second | 1,313.88 |
| Segments / observed wall second | 37.49 |

18-page PDF run:

| Field | Value |
| --- | --- |
| Workload | `2605.14926v2--1.pdf`, pages `1-18` |
| Job id | `job-1782930613972-2605-14926v2--1` |
| Run id | `run-pdf-1782930633238` |
| Direction | `en` -> `zh-CN` |
| Summary JSON | `%APPDATA%\com.rosetta.desktop\jobs\job-1782930613972-2605-14926v2--1\diagnostics\lightning-baseline-run-pdf-1782930633238.json` |

| Metric | Value |
| --- | ---: |
| Status | completed |
| Pages requested | 18 |
| Pages translated | 18 |
| Pages failed | 0 |
| pdf2zh invocations | 2 |
| Total wall time | `42.059s` |
| pdf2zh process time | `41.999s` |
| Lightning request count | 20 |
| Lightning failed requests | 0 |
| Lightning total request time | `29.404s` |
| Lightning average request time | `1.470s` |
| Lightning p95 request time | `2.413s` in PDF profile, `2.379s` in perf-log percentile |
| Lightning max request time | `2.413s` |
| Shim items translated | 211 |
| Average batch size | 10.55 |
| Max observed batch size | 24 |
| Batch size distribution | `1x3, 2x2, 6x1, 7x1, 8x2, 11x1, 12x2, 14x1, 15x2, 16x2, 18x1, 22x1, 24x1` |
| Total assembly wait | `1.776s` |
| Average assembly wait | `0.088s` |
| Max assembly wait | `0.096s` |
| Input chars | 53,920 |
| Output chars | 20,073 |
| Source chars / observed wall second | 1,411.89 |
| Output chars / observed wall second | 525.61 |

PDF chunk details:

| Chunk | Pages | Wall | Requests | Avg batch | Max batch | Input chars | Output chars |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 1-10 | `26.562s` | 11 | 14.18 | 24 | 37,840 | 14,897 |
| 1 | 11-18 | `15.443s` | 9 | 6.11 | 15 | 16,080 | 5,176 |

Interpretation:

- Markdown is now saturating its current app-level batch width: 35 of 36
  requests used batch size 16, with no failures.
- The 18-page PDF confirmed the Lightning PDF path is really running with
  `thread=100`, but the largest observed shim batch was only 24. The limit is
  therefore not the configured Lightning ceiling in this workload; it is the
  number and timing of pdf2zh shim items available inside each chunk.
- The first PDF chunk had much healthier aggregation than the second chunk
  (`14.18` average batch size versus `6.11`). This suggests tuning should look
  at PDF chunking / shim feed shape before simply raising the Lightning ceiling
  beyond 100.
- Assembly wait is still small relative to Lightning request time: `1.776s`
  total assembly wait versus `29.404s` total request time.

## Rejected PDF Sweep: 512 Workers, 250 ms Window, Wide Chunks

The first aggressive PDF run used:

```powershell
$env:ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE="512"
$env:ROSETTA_PDF_SHIM_LIGHTNING_BATCH_WINDOW_MS="250"
$env:ROSETTA_PDF_LIGHTNING_PAGE_CHUNK_SIZE="100"
```

Results:

| Workload | Pages | Requests | Avg batch | p95 request | Total wall | Source chars/s | Output chars/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `2604.17278v1.pdf` | 10 | 10 | 9.90 | `9.499s` | `46.404s` | 869.80 | 324.79 |
| `2602.22286v2.pdf` | 15 | 15 | 10.60 | `9.316s` | `64.293s` | 877.47 | 388.04 |

Interpretation:

- Markdown benefited strongly from batch 100: 571 segments finished in
  `4.435s` with `5x100 + 1x71` batches.
- PDF did not benefit from `thread=512`; observed PDF batches were still only
  2-18 items.
- The widened PDF text chunk budgets caused much slower long-tail requests
  (`~9.5s` p95), reversing the earlier 10-page `thread=100` improvement.
- The underlying shim still serialized Lightning requests at this point, so
  multiple small PDF batches could not overlap even though RWKV Lightning can
  handle high concurrency.

Follow-up code change:

- Keep Markdown batch 100.
- Decouple PDF shim batch width from pdf2zh worker count.
- Default PDF Lightning worker count back to 100.
- Default PDF aggregation window back to 80 ms.
- Default PDF Lightning text chunk budgets back to the smaller proven values.
- Add `ROSETTA_PDF_SHIM_LIGHTNING_IN_FLIGHT_REQUESTS`, default 32, so multiple
  assembled PDF batches can be in flight to Lightning concurrently.

## Follow-Up PDF Run: Worker 100, Window 80 ms

Run settings:

```powershell
$env:ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE="256"
$env:ROSETTA_PDF_SHIM_LIGHTNING_THREAD_COUNT="100"
$env:ROSETTA_PDF_SHIM_LIGHTNING_IN_FLIGHT_REQUESTS="8"
$env:ROSETTA_PDF_LIGHTNING_PAGE_CHUNK_SIZE="100"
Remove-Item Env:\ROSETTA_PDF_SHIM_LIGHTNING_BATCH_WINDOW_MS -ErrorAction SilentlyContinue
```

Result:

| Workload | Pages | Requests | Avg batch | p95 request | Total wall | Source chars/s | Output chars/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `2604.17278v1.pdf` | 10 | 10 | 13.10 | `2.428s` | `25.877s` | 1,656.06 | 616.51 |

Interpretation:

- The 512-worker regression was reversed. Request p95 returned from `9.499s`
  to `2.428s`, matching the earlier best 10-page run.
- However, this still did not create meaningful overlap: observed wall
  `25.877s` is close to summed request time `19.149s`.
- The likely cause is that pdf2zh workers arrive in waves and block together
  behind each shim-assembled batch. While that batch is in flight, there are
  few or no new pending worker requests for the shim to consume.

Follow-up code change:

- PDF Lightning now defaults to direct concurrent worker requests instead of
  assembling a wave of workers into one batch.
- `ROSETTA_PDF_SHIM_LIGHTNING_ASSEMBLE_BATCHES=1` can restore the old assembly
  behavior for A/B testing.
- The next PDF benchmark should expect many `batchSize=1` Lightning records.
  That is intentional; the success metric is lower wall time and
  `observedWallSeconds << summedRequestSeconds`.

## Interpretation Checklist

Before changing code, check:

- whether PDF `averageBatchSize` is close to the Lightning shim max batch size;
- whether `totalAssemblyWaitMs` is material relative to `totalRequestMs`;
- whether request p95 is dominated by Lightning runtime latency or client-side
  preparation / parsing;
- whether Markdown and PDF differ mainly in request count, batch width, or PDF
  process time;
- whether failures, empty outputs, or response-count mismatches appear.

Only after this evidence points to a specific bottleneck should the Lightning
batch width, shim aggregation window, or generation settings be changed.
