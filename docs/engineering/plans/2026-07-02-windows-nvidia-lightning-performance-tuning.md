# Windows NVIDIA Lightning Performance Tuning Plan

Date: 2026-07-02

Status: in progress; Task 1 instrumentation and local summary method added.
First Windows NVIDIA Lightning Markdown and PDF baselines recorded. First
Lightning-only throughput tuning pass implemented.

## Summary

Rosetta now has a working Windows NVIDIA Lightning path:

- Clean onboarding defaults to `RWKV Lightning CUDA`.
- Markdown translation works through Lightning.
- PDF translation works through Lightning.
- Settings can install llama.cpp Vulkan.
- Switching Lightning -> llama.cpp changes the active translation provider.
- Switching llama.cpp -> Lightning changes the active translation provider back.

The next important work is performance tuning for the Lightning path.

This tuning is intentionally narrow:

- Optimize `RWKV Lightning CUDA` on Windows NVIDIA.
- Do not spend this pass optimizing llama.cpp Vulkan.
- Do not spend this pass optimizing macOS MLX.
- Treat llama.cpp Vulkan and macOS MLX as basically stable / near-optimal for
  now unless a regression is discovered while measuring Lightning.

Rosetta remains a local-first document translation workbench. This work must
not add cloud translation, telemetry, account features, chat, summarization,
document Q&A, or generic assistant behavior.

## Current Product Baseline

Runtime selection implementation is effectively complete through Task 8 of:

```txt
docs/engineering/plans/2026-07-01-windows-nvidia-lightning-runtime-selection.md
```

User-confirmed NVIDIA Windows validation on 2026-07-02:

- Clean onboarding installs Lightning by default.
- Markdown translation through Lightning succeeds.
- PDF translation through Lightning succeeds.
- Settings can install llama.cpp.
- Switching Lightning -> llama.cpp changes the translation provider.
- Switching llama.cpp -> Lightning changes the translation provider back.

Remaining runtime-selection validation, deferred for later:

- App exit process cleanup.
- Local data reset process cleanup.
- Local data reset removal of both runtime and model families.

Those deferred cleanup checks are important for release confidence, but they do
not block starting performance investigation.

## Performance Tuning Scope

Primary target:

- Windows x64
- NVIDIA GPU with SM75+ support
- Rosetta-owned `RWKV Lightning CUDA` runtime artifact
- Provider id: `rwkv-lightning-contents`
- Endpoint: `/v1/batch/completions`
- Model:
  `RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth`

In scope:

- Markdown long-document translation throughput through Lightning.
- PDF translation throughput through Lightning.
- Request batching and shim aggregation behavior for Lightning.
- Provider adapter request / response overhead for Lightning.
- Runtime launch arguments that are specific to Lightning.
- Measurement harnesses or scripts that record local-only timing data.
- UI-visible progress smoothness only if it reflects real pipeline throughput
  or blocking behavior.

Out of scope for this pass:

- llama.cpp Vulkan performance tuning.
- macOS MLX performance tuning.
- Model quality changes.
- Prompt rewriting as a product feature.
- Cloud fallback, cloud benchmarking, telemetry, or upload-based diagnostics.
- Generic multi-provider optimization that would destabilize stable paths.

## Non-Goals

- Do not change provider preference semantics.
- Do not make Lightning mandatory on all Windows machines.
- Do not remove llama.cpp Vulkan fallback.
- Do not alter PDF layout fidelity as a shortcut for speed.
- Do not log source document text, translated text, prompt bodies, or document
  structure in performance diagnostics.
- Do not start dev servers or production builds unless a later task explicitly
  asks for runtime UI verification or release packaging.

## Baseline Needed Before Tuning

Before changing code, capture repeatable numbers on the NVIDIA Windows machine.
The baseline should include at least:

- Device:
  - GPU name.
  - CUDA / driver version if available.
  - Windows version.
  - Rosetta app build or git commit.
- Runtime:
  - Lightning profile id.
  - Runtime artifact filename / SHA256.
  - Runtime process command line with host and port redacted only if needed.
  - Model filename and SHA256.
- Workloads:
  - A small Markdown document.
  - A realistic long Markdown document.
  - A small PDF.
  - A realistic PDF with enough pages to expose shim / batching behavior.
- Metrics:
  - Total job wall time.
  - Runtime warm start versus cold start where relevant.
  - Number of source segments.
  - Number of PDF pages / chunks.
  - Number of provider requests.
  - Batch sizes actually sent to `/v1/batch/completions`.
  - Average / median / p95 provider request latency.
  - Segments per second.
  - Source chars per second.
  - Output chars per second.
  - Error / retry count.

If possible, collect a comparable llama.cpp run only as a reference point, not
as an optimization target.

## Likely Investigation Areas

### 1. Lightning batch width

Question:

- What batch size gives the best throughput for the current Lightning runtime
  and 0.4B `.pth` translation model?

Evidence to collect:

- Batch size 1, 2, 4, 8, 12, 16 if the runtime accepts them.
- Latency per request.
- Segments per second.
- GPU utilization if easily visible locally.
- Whether larger batches produce quality issues, truncation, stalls, or
  unstable response ordering.

Do not assume llama.cpp's best batch behavior applies to Lightning.

### 2. PDF shim aggregation

Question:

- Is PDF translation slow because the shim sends too many small Lightning
  requests, waits too long to assemble batches, or serializes work that could
  safely overlap?

Evidence to collect:

- Shim request count per PDF page / chunk.
- Average assembled batch size.
- Time waiting for batch assembly.
- Time spent in Lightning HTTP requests.
- Time spent in pdf2zh layout / render work.

Privacy note:

- Diagnostics may record counts, timings, page numbers, chunk ids, and batch
  sizes.
- Diagnostics must not record source text, translated text, prompt bodies, or
  document structure content.

### 3. Provider adapter overhead

Question:

- Is Rosetta spending significant time outside the Lightning runtime request
  itself?

Evidence to collect:

- Time to prepare request bodies.
- Time from HTTP response arrival to parsed translations.
- Response parsing cost for streaming versus non-streaming paths, if relevant.
- Difference between direct `curl` / script requests and Rosetta provider calls.

### 4. Runtime startup and readiness

Question:

- Is user-perceived slowness caused by cold startup/model load, or by steady
  state translation throughput?

Evidence to collect:

- Time from process spawn to `/v1/models` ready.
- First translation request latency after ready.
- Warm request latency after one or more completed batches.
- Whether switching away and back causes unnecessary cold starts.

### 5. Request contract and generation settings

Question:

- Are Lightning request parameters conservative or mismatched for the local
  translation workload?

Evidence to collect before changing:

- Exact current request body shape.
- Runtime-supported parameters for `/v1/batch/completions`.
- Stop behavior.
- Max token behavior.
- Whether failures are caused by truncation, malformed output, or runtime
  instability.

Any generation-setting change must include a quality sanity check, not only a
speed number.

## Proposed Task Breakdown

### Task 1: Lightning Benchmark Harness / Notes

Output:

- A repeatable local-only benchmark method for Lightning Markdown and PDF.
- A baseline table committed to docs.
- Privacy-safe `rwkv-performance.jsonl` instrumentation for Lightning request
  timing.
- PDF shim batch distribution and assembly-wait metrics in job diagnostics.
- A local summary script for Markdown and PDF Lightning runs.
- `ROSETTA_PDF_SHIM_LIGHTNING_MAX_BATCH_SIZE` for local PDF batch-width sweeps
  without changing llama.cpp Vulkan or macOS MLX.
- Lightning-only ordinary-document batch width raised from 16 to 100.
- Lightning-only PDF default shim batch width raised from 8 to 256.
- Lightning-only PDF pdf2zh worker count defaults to 100 and is decoupled from
  shim batch width after `thread=512` proved slower.
- Lightning-only PDF direct concurrent worker requests were tested and rolled
  back from the default path after the Lightning runtime returned immediate
  HTTP 409 responses under concurrent `/v1/batch/completions` requests.
- Lightning-only PDF now keeps the stable serial assembled-request shim by
  default. Direct concurrent requests require the explicit experimental env
  `ROSETTA_PDF_SHIM_LIGHTNING_DIRECT_CONCURRENT=1`.
- Lightning-only PDF page chunk size raised from 10 to 100 pages.
- Lightning-only PDF shim aggregation window remains 80 ms after a 250 ms sweep
  increased wait time without improving observed batch size.
- Lightning-only PDF shim text chunk budgets remain at the smaller proven
  values after wider chunks caused `~9.5s` p95 request latency.

Validation:

- Harness does not log source text, translated text, or prompts.
- Results include enough metadata to reproduce the run.
- First realistic PDF baseline is recorded.
- A real long Markdown run is recorded: 571 segments in `15.232s`, with 36
  Lightning requests and no failures.
- A real 18-page PDF run is recorded: 211 shim items in `42.059s`, with
  `thread=100`, 20 Lightning requests, and no failures.
- The latest PDF data shows the configured Lightning ceiling is not yet the
  limiting factor for this workload: max observed shim batch was 24 even with
  `thread=100`.

### Task 2: Markdown Lightning Throughput Baseline

Output:

- Baseline numbers for small and long Markdown translation through Lightning.
- Request count and batch size distribution.
- Clear comparison between cold and warm runtime behavior.

Current evidence:

- A long Markdown run has been captured for `rosetta_project_plan.md`.
- The run translated 571 segments in `15.232s` observed wall time with
  `35x16 + 1x11` Lightning batch distribution and zero failed requests.
- Small Markdown and cold/warm separation are still pending.

### Task 3: PDF Lightning Throughput Baseline

Output:

- Baseline numbers for small and realistic PDF translation through Lightning.
- Shim aggregation timings and batch size distribution.
- Split between PDF layout/render time and Lightning provider time.

Current evidence:

- Two realistic PDF baselines have been captured.
- The 10-page `thread=100` run was `30.4%` faster than the 10-page batch-8
  baseline.
- The 18-page `thread=100` run completed in `42.059s` with two pdf2zh chunks,
  20 Lightning requests, average batch size 10.55, and max observed batch 24.
- Small PDF is still pending.

### Task 4: Batch Width Experiment

Output:

- Batch-size sweep for Lightning.
- Recommended Lightning batch width for Markdown and PDF, if different.
- Notes about quality, ordering, stalls, or memory pressure.

Current implementation after the RTX 5070 PDF 409 rollback:

- Markdown/TXT through `rwkv-lightning-contents`: batch 100.
- PDF through `rwkv-lightning-contents`: stable serial assembled shim requests,
  pdf2zh worker count 100, 100-page pdf2zh chunks, and smaller proven PDF text
  chunk budgets.
- MLX and llama.cpp retain their existing conservative values.
- The failed direct-concurrent PDF run translated page 1 and then stopped with
  repeated HTTP 409 responses. Perf logs showed many immediate `batchSize=1`
  409s while a few requests were still running normally, indicating the
  Lightning runtime rejects overlapping generation requests even when it
  supports wide native batches inside a single request.
- Next benchmark should not sweep PDF in-flight request counts through the
  current shim. The next likely speedup needs a pdf2zh-side integration change:
  collect many pdf2zh translation units into one Lightning batch request, or
  fork/patch pdf2zh so page workers feed a Rosetta-owned scheduler instead of
  independently calling the OpenAI-compatible shim.

### Task 5: Targeted Implementation

Entry criteria:

- Tasks 1-4 identify a specific bottleneck.

Output:

- Minimal code changes for the measured bottleneck only.
- No changes to llama.cpp or MLX unless required to keep shared interfaces
  correct.

### Task 6: Regression And Release Notes

Output:

- Before/after Lightning performance table.
- Confirmation that llama.cpp Vulkan and macOS MLX paths still compile and keep
  their existing behavior.
- Change-log entry if the tuning result is accepted.

## Acceptance Criteria

The Lightning performance tuning work is accepted when:

- A baseline and final benchmark are recorded in docs.
- The final result improves real user workflows, not only a synthetic endpoint.
- Markdown and PDF translation both still complete correctly through Lightning.
- Runtime switching to and from llama.cpp still works.
- No source document text, translated text, prompts, or document structure are
  written to diagnostics.
- llama.cpp Vulkan and macOS MLX remain stable and are not dragged into
  speculative tuning.

## Open Questions

- Which NVIDIA Windows machine should be the primary performance baseline?
- Which Markdown and PDF fixtures should become the repeatable local benchmark
  set?
- Does Lightning expose reliable server-side timing fields, or do we need to
  rely entirely on Rosetta-side timing?
- Should Markdown and PDF use the same Lightning batch width, or should PDF
  keep a separate shim-level aggregation policy?
- Is the first optimization target total throughput, time-to-first-visible
  progress, or avoiding long stalls during PDF translation?
