# Windows NVIDIA Lightning Performance Tuning Plan

Date: 2026-07-02

Status: planned; documentation-only setup, no implementation started.

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

Validation:

- Harness does not log source text, translated text, or prompts.
- Results include enough metadata to reproduce the run.

### Task 2: Markdown Lightning Throughput Baseline

Output:

- Baseline numbers for small and long Markdown translation through Lightning.
- Request count and batch size distribution.
- Clear comparison between cold and warm runtime behavior.

### Task 3: PDF Lightning Throughput Baseline

Output:

- Baseline numbers for small and realistic PDF translation through Lightning.
- Shim aggregation timings and batch size distribution.
- Split between PDF layout/render time and Lightning provider time.

### Task 4: Batch Width Experiment

Output:

- Batch-size sweep for Lightning.
- Recommended Lightning batch width for Markdown and PDF, if different.
- Notes about quality, ordering, stalls, or memory pressure.

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
