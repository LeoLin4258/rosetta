# PDFMathTranslate Rosetta Batch Backend Plan

Date: 2026-07-02

Status: implementation started. The ONNX-based Windows dogfood PDF component
has been built and manually imported successfully; PDF translation is usable
again. The remaining work is performance-oriented: batch diagnostics, wider
RWKV batch utilization, and official PDF pack publication.

Update 2026-07-02:

- A separate agent produced a Windows PDF component based on the newer
  PDFMathTranslate / ONNX layout path, and the user manually imported the
  locally built PDF ZIP pack.
- PDF translation now completes with the new component.
- Warmup is dramatically faster. Latest successful 10-page runs report
  `pdf2zhWarmup=2ms` and `modelReadyMs=0`, with layout inference using
  `AzureExecutionProvider,CPUExecutionProvider`.
- Translation throughput has not materially improved yet. Latest full 10-page
  Lightning runs were `31.422s` and `29.581s`, with 10 RWKV requests and
  average batch size `13.2`.
- This means the layout/runtime modernization phase succeeded, but the
  translation scheduler is still effectively using the old PDF shim batching
  shape.
- The Rust `/rosetta/batch-translations` endpoint now has a Lightning-specific
  direct wide-batch path that calls `translate_batch_via_lightning(...)`
  without re-entering the legacy `PendingTranslation` queue. The old queue path
  remains for non-Lightning providers and OpenAI-compatible fallback.
- The persistent worker now probes `translator.translate_many(...)` in addition
  to single `translator.translate(...)` calls, so the next benchmark can show
  page-local native batch counts and item totals without logging source text.
- A forced retranslate benchmark after the direct path confirmed the diagnosis:
  the 10-page PDF still produced 10 Lightning requests with average batch size
  `13.2` and the same batch distribution as the old queue path
  (`8x2, 11x1, 13x2, 14x2, 15x1, 18x2`). The direct path removed old queue
  assembly wait (`868ms -> 0ms`) and improved wall time from `29.581s` to
  `27.943s`, but it did not create cross-page batches. The remaining bottleneck
  is PDFMathTranslate/page-local sequencing, not Rust shim assembly.
- Phase 7 first slice has started in Rosetta's persistent worker. For
  `service="rosetta-batch"`, the worker now performs a chunk-local collect /
  global translate / replay pass so all translatable units from the requested
  pages can be sent through one ordered Rosetta batch request before page PDFs
  are written.
- A forced retranslate benchmark after the Phase 7 first slice confirmed the
  cross-page batching shape. The same 10-page benchmark completed in `16.760s`
  with one Lightning request, average provider batch size `132.0`, and
  `rwkv.totalRequestMs=3.931s`. The previous direct-path run was `27.943s`
  with 10 Lightning requests and `rwkv.totalRequestMs=19.626s`.
- The new bottleneck is no longer RWKV request scheduling. Timeline
  diagnostics for the Phase 7 run show `crossPageBatch.translate=4.192s`,
  `crossPageBatch.collect=3.636s`, `page.yoloPredict=4.700s` across two
  layout passes, and `page.saveSinglePdf=3.385s`.
- The official installer profile still points at the older published Windows
  PDF pack. The local dogfood artifact has not yet been uploaded to
  `rosetta-assets`, so `managed_pdf2zh/profile.rs` must not be updated with the
  local SHA until the release asset exists.

Local upstream checkout:

```txt
C:\Users\Leo\Documents\GitHub\PDFMathTranslate
```

Upstream project:

```txt
https://github.com/Byaidu/PDFMathTranslate
```

The currently published Rosetta PDF pack installs `pdf2zh 1.7.9`. The local
dogfood pack installs patched `pdf2zh 1.9.11` from
`C:\Users\Leo\Documents\GitHub\PDFMathTranslate`, using `onnx` / `onnxruntime`
for document layout. Rosetta distributes pdf2zh as a prebuilt PDF component
from `LeoLin4258/rosetta-assets`, not as a runtime git dependency.

Local dogfood Windows artifact:

```txt
C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\dist\pdf-layout\rosetta-pdf2zh-windows-amd64.zip
```

Local artifact metadata:

```txt
sizeBytes: 320275049
sha256: c36133090f1221542b1d4a53f8d576e2b80e5f96ec31129c1ea1c74318f49542
pdf2zhVersion: 1.9.11
pythonVersion: 3.12.13
pythonBuildRelease: 20260602
layoutModel: doclayout_yolo_docstructbench_imgsz1024.onnx
layoutModelSha256: fece9af02f618b603ff7921ccec6861d13e7e1f9830e091dfb7e8ad9311e5b21
```

## Summary

Rosetta's current visual PDF path uses PDFMathTranslate / pdf2zh for parsing,
layout analysis, and PDF rewriting. Translation is connected through an
OpenAI-compatible local shim. That compatibility layer has become the main
obstacle to large Windows NVIDIA Lightning speedups.

The next high-leverage path is to maintain a Rosetta-specific PDFMathTranslate
distribution that keeps the useful PDF layout/rendering behavior but removes
or bypasses product surfaces and dependencies Rosetta does not use. This should
replace the per-paragraph OpenAI-style translation loop with a Rosetta-owned
batch translation backend, and it should also attack warmup time, cancellation
behavior, package size, and process lifetime.

The goal is not to rewrite arbitrary PDF layout from scratch on day one. The
goal is to make a Rosetta PDF engine: a stripped, local-only, long-document PDF
translation component whose translation scheduling, lifecycle, diagnostics, and
dependency graph match Rosetta's app requirements.

## Problem Statement

The current PDF translation flow is:

```txt
pdf2zh TranslateConverter.receive_layout()
  -> builds sstk paragraph strings for one page/layout
  -> ThreadPoolExecutor(max_workers=thread)
  -> translator.translate(one_string)
  -> OpenAI SDK
  -> Rosetta OpenAI-compatible shim
  -> Rust-side batch aggregation window
  -> RWKV provider
```

This creates three performance problems for Lightning:

- pdf2zh exposes one synchronous `translate(text)` call per paragraph, so
  Rosetta only sees translation units after they have already become many
  independent calls.
- Rust-side shim batching is opportunistic. It can only batch whatever requests
  happen to arrive within a small time window.
- Direct concurrent requests to the Lightning runtime are not a safe speedup.
  The RTX 5070 experiment with concurrent shim requests produced immediate
  HTTP 409 responses while other requests were still running. This indicates
  the runtime accepts wide batches inside one request but rejects overlapping
  generation requests.

The correct Lightning shape is:

```txt
many PDF translation units
  -> one Rosetta scheduler
  -> one wide /v1/batch/completions request at a time
  -> ordered results back to pdf2zh
```

The current OpenAI-compatible shim cannot reliably create that shape because it
sits too late in the pipeline.

There are also two non-translation problems that make the current pack feel
heavy inside Rosetta:

- Warmup is dominated by importing PyTorch / doclayout_yolo in the currently
  shipped `pdf2zh 1.7.9` pack. Rosetta's worker comments record that importing
  the layout stack costs roughly 13 seconds while the actual model load is much
  smaller.
- Cancellation kills the whole warm worker process group. That is robust, but
  it throws away the already-imported Python process and layout model, so the
  next PDF operation has to pay the full prewarm cost again.

## Goals

- Keep PDFMathTranslate's layout analysis, formula protection, font handling,
  and translated PDF rewriting.
- Add a Rosetta-specific translation backend that supports ordered batch
  translation.
- Remove unused upstream surfaces from the Rosetta pack where possible:
  Web UI, cloud translator integrations, MCP/backend server extras, Gradio,
  Celery/Redis, and generic API products that Rosetta does not expose.
- Eliminate PyTorch from the Rosetta PDF pack if the current ONNX layout path
  can match or exceed the shipped layout quality and speed.
- Avoid re-prewarming after user cancellation. Prefer cooperative cancellation
  inside the persistent worker over killing the entire worker for normal user
  stops.
- Make PDF translation feed Lightning wide native batches, initially targeting
  batch widths around 64-256 on Windows NVIDIA.
- Preserve stable MLX and llama.cpp behavior unless explicitly opted into the
  new backend.
- Preserve Rosetta's local-only privacy model.
- Preserve Rosetta's durable PDF page state, cancellation, recovery, and export
  behavior.
- Produce measurements that separate PDF layout/render time from model
  translation time.

## Non-Goals

- Do not add cloud translation, login, sync, telemetry, or external document
  upload.
- Do not turn Rosetta into a generic assistant, PDF Q&A tool, summarizer, or
  rewrite product.
- Do not rewrite the full PDF layout engine in this pass.
- Do not remove layout analysis, formula protection, or PDF rewrite fidelity as
  a shortcut for speed.
- Do not change ordinary TXT/Markdown translation scheduling.
- Do not remove llama.cpp Vulkan or macOS MLX PDF support.
- Do not store source text, translated text, prompts, or raw model responses in
  persistent diagnostics.
- Do not make the PDFMathTranslate fork diverge for unrelated GUI, web API,
  OCR, or cloud-service changes.
- Do not carry unused upstream dependencies in the Rosetta pack merely for
  upstream feature completeness.

## Current Evidence

Observed on 2026-07-02 with Windows NVIDIA Lightning:

- Markdown/TXT batching works well after raising Lightning ordinary-document
  batch width to 100.
- Stable PDF shim batching improved the 10-page PDF baseline from about
  `36.8s` to about `25.6s`, but still exposed only small-to-medium observed
  batches.
- An 18-page PDF completed in about `42.1s` with `thread=100`, 20 Lightning
  requests, average batch size about `10.55`, and max observed batch `24`.
- Raising pdf2zh/shim knobs further did not expose batch 100. A wide-window /
  `thread=512` sweep was slower and produced p95 request latency around
  `9.5s`.
- Direct concurrent shim requests with `ROSETTA_PDF_SHIM_LIGHTNING_IN_FLIGHT_REQUESTS=32`
  failed after page 1 with repeated HTTP 409 responses.
- The failed 409 run showed many immediate `batchSize=1` failed requests while
  a few requests were still completing normally, which strongly suggests
  Lightning generation is single-flight at the request level.
- Rosetta's current persistent worker explicitly imports `torch` and
  `doclayout_yolo` during warmup. The current upstream checkout's
  `pyproject.toml` is `pdf2zh 1.9.11` and uses `onnx`, `onnxruntime`,
  `opencv-python-headless`, `pymupdf`, `pdfminer-six`, `pikepdf`, `fontTools`,
  and `babeldoc` without listing PyTorch as a required dependency.
- After the first local Windows ZIP test of the new component, warmup is no
  longer the dominant problem. A representative successful 10-page run:
  `total=29.581s`, `pdf2zhWarmup=2ms`, `pdf2zhProcess=29.529s`,
  `rwkv.requestCount=10`, `rwkv.averageBatchSize=13.2`,
  `rwkv.totalRequestMs=19.283s`, `rwkv.p95RequestMs=2.376s`.
- The new `RosettaBatchTranslator.translate_many(...)` path exists, but the
  Rust `/rosetta/batch-translations` endpoint still routes texts through
  `translate_pdf_texts(...)`, which splits each text with the existing PDF shim
  chunk profile and enqueues chunks into the old batch processor. That preserves
  compatibility, but it prevents the endpoint from becoming a direct wide
  Lightning batch request.
- After the Rust direct path, a forced retranslate still produced 10
  Lightning requests and average batch size `13.2`, confirming the remaining
  limit was page-local PDF sequencing.
- After the Phase 7 worker-local collect / global translate / replay slice,
  the same 10-page benchmark completed in `16.760s`, with `pagesTranslated=10`,
  `rwkv.requestCount=1`, `rwkv.averageBatchSize=132.0`,
  `rwkv.totalRequestMs=3.931s`, `rwkv.totalInputChars=36,381`, and
  `rwkv.totalOutputChars=13,355`.
- The Phase 7 timeline makes the next optimization target concrete:
  duplicate layout/replay work and page artifact saving now dominate the
  non-model portion (`page.yoloPredict=4.700s` over 20 events and
  `page.saveSinglePdf=3.385s` over 10 events).

Conclusion:

PDFMathTranslate's per-paragraph translation loop is now the main reason PDF
cannot feed Lightning the same kind of wide batches that ordinary Markdown can.
The older PyTorch/doclayout_yolo dependency chain is also a major reason the
PDF component feels slow to become ready and expensive to restart.

After the ONNX component test, refine that conclusion:

- Warmup has been largely addressed.
- The next bottleneck is now Rosetta's Rust-side `/rosetta/batch-translations`
  implementation. It must stop treating Rosetta batch input as many legacy shim
  items and instead call the provider with a deliberately wide ordered batch.

## Upstream Touchpoints

Initial inspection of `C:\Users\Leo\Documents\GitHub\PDFMathTranslate` shows
the relevant current-path files:

```txt
pdf2zh/converter.py
pdf2zh/translator.py
pdf2zh/high_level.py
pdf2zh/pdfinterp.py
```

Important current behavior:

- `pdf2zh/converter.py::TranslateConverter.receive_layout()` builds `sstk`,
  the paragraph/string list for the current layout.
- The same method translates with a `ThreadPoolExecutor(max_workers=self.thread)`
  and calls `self.translator.translate(s)` once per item.
- `pdf2zh/translator.py::BaseTranslator.translate()` is a synchronous
  single-string API with cache lookup and `do_translate(text)` override.
- `pdf2zh/translator.py::OpenAITranslator` uses the OpenAI Python SDK and
  `chat.completions.create(...)`.
- `pdf2zh/high_level.py::translate_patch()` constructs `TranslateConverter`
  and calls `interpreter.process_page(page)`.

The local checkout also includes `pdf2zh/doclayout.py`, which uses ONNX Runtime
for layout inference, and a `pdf2zh/kernel/PDFMathTranslate-next.git` subtree.
This plan should compare two base choices before implementation:

- Patch the exact packaged `pdf2zh 1.7.9` source for minimum behavior drift.
- Move the Rosetta pack to a stripped `pdf2zh 1.9.11` base to remove PyTorch
  and use the ONNX layout path.

The second option is now strategically interesting because it can attack warmup
time and pack size, not only translation batching.

## Proposed Architecture

### Rosetta PDF Engine Boundary

Treat the patched PDFMathTranslate distribution as a Rosetta PDF engine, not as
a general-purpose end-user pdf2zh app.

Keep:

- PDF parsing and rewriting primitives
- layout detection needed for visual PDF translation
- formula/rich-text placeholders
- font handling needed for translated PDF output
- page-range processing
- deterministic tests and fixtures

Remove or exclude from the Rosetta pack where practical:

- Gradio/Web UI
- MCP server
- Celery/Redis backend
- cloud translator SDKs not used by Rosetta
- generic OpenAI/DeepL/Bing/Google/Ollama/Xinference integrations in the
  installed Rosetta runtime, once `rosetta-batch` is the default
- demo assets and docs not needed at runtime
- any OCR stack unless Rosetta explicitly ships OCR later

The fork may keep compatibility code in source control if useful for upstream
rebases, but the shipped Rosetta pack should be aggressively pruned.

### Layout Runtime Modernization

Evaluate replacing the current shipped PyTorch/doclayout_yolo path with the
current PDFMathTranslate ONNX Runtime layout path.

Target runtime shape:

```txt
PyMuPDF/pdfminer
  -> ONNX Runtime layout model
  -> PDFMathTranslate converter/rewrite
  -> Rosetta batch translator
```

Expected benefits:

- much faster worker prewarm if PyTorch import disappears
- smaller Windows/macOS PDF component packs
- less GPU/runtime conflict with RWKV Lightning because layout inference no
  longer imports a full Torch stack
- simpler install and fewer binary dependency failures

Validation gate:

- same PDF pages produce acceptable translated layout compared with the
  current pack
- one-page first-use latency improves materially
- worker ready time improves materially
- per-page layout inference time is no worse on the target machines
- Windows pack can run without PyTorch installed

Because Rosetta is still beta, the ONNX layout pack is allowed to become a
hard component update. If ONNX layout quality regresses on important fixtures,
fix the Rosetta PDF component or hold the release; do not keep the old Torch
layout pack as the default product fallback.

### Add a Batch Translation Interface

Extend `BaseTranslator` with an optional ordered batch API:

```python
def translate_many(self, texts: list[str], ignore_cache: bool = False) -> list[str]:
    return [self.translate(text, ignore_cache=ignore_cache) for text in texts]
```

Default behavior preserves all existing translators.

For Rosetta, implement `RosettaBatchTranslator.translate_many(...)` with real
batch semantics:

```txt
input texts[] in pdf2zh order
  -> filter blank/formula-only passthroughs
  -> per-item cache lookup
  -> send misses to Rosetta batch endpoint
  -> validate result count and order
  -> write per-item cache
  -> return results[] in original order
```

### Modify TranslateConverter Translation Stage

Change only the paragraph translation block in
`TranslateConverter.receive_layout()`:

```txt
current:
  ThreadPoolExecutor -> translator.translate(one item)

target:
  if translator supports real batch:
    translator.translate_many(translatable_items)
  else:
    existing ThreadPoolExecutor path
```

This keeps non-Rosetta translators behaviorally close to upstream while giving
Rosetta a first-class batch path.

### Rosetta Batch Backend Protocol

Use a Rosetta-local protocol, not OpenAI SDK semantics, for the new backend.
The first implementation can be HTTP loopback because Rosetta already has local
server/shim infrastructure.

Endpoint:

```txt
POST /v1/rosetta/batch-translations
```

The PDF component receives `ROSETTA_BATCH_BASE_URL` and appends
`/rosetta/batch-translations`, so Rosetta should pass a base URL that already
includes `/v1`.

Required request properties:

- `sourceLang`
- `targetLang`
- `texts`
- `jobId` or `runId` for privacy-safe diagnostics correlation
- optional `timeoutMs`

Required response properties:

- `translations`
- same item count as `texts`
- same order as `texts`
- error object if the whole batch fails

The protocol must not log source text or translated text. Diagnostics may
record counts, timings, batch sizes, page numbers, status codes, and character
counts.

Implementation warning from the first local ZIP test:

- It is not enough for PDFMathTranslate to call `translate_many(...)`.
- The Rosetta endpoint that receives `texts[]` must preserve that vector as the
  provider batch whenever the active provider is Lightning and the individual
  texts fit the model budget.
- Long-text chunking should be an exception path for over-budget items, not the
  default mechanism that re-fragments every Rosetta batch into the old shim
  queue.

### Scheduler Shape

The Rust/Rosetta side should own final provider scheduling:

```txt
PDFMathTranslate RosettaBatchTranslator
  -> Rosetta batch endpoint
  -> provider-aware scheduler
  -> Lightning /v1/batch/completions
```

Lightning policy:

- single in-flight provider request by default
- wide native batch inside that request
- default target batch width initially 100 or 128
- ceiling configurable for local sweeps
- no direct concurrent Lightning requests unless a future runtime version
  explicitly supports it without HTTP 409
- `/rosetta/batch-translations` should call Lightning as one ordered native
  batch for normal PDF paragraph inputs. If `texts.len() == 132`, the target
  shape is approximately one or two provider requests, not 10 small assembled
  requests.

MLX / llama.cpp policy:

- keep existing conservative PDF behavior unless specifically opted into the
  new path after separate measurements.

### Cooperative Cancellation

Replace normal user cancellation from "kill the whole warm worker" with a
cooperative stop path when the worker is healthy.

Current behavior:

```txt
cancel
  -> kill worker process tree
  -> lose imported Python modules and layout model
  -> next PDF operation prewarms from scratch
```

Target behavior:

```txt
cancel
  -> send cancel token to worker
  -> stop scheduling new pages / new translation batches
  -> let in-flight batch return or abort at Rosetta scheduler
  -> discard partial chunk artifacts
  -> keep worker process warm and ready
```

Keep hard-kill as a fallback for:

- worker protocol deadlock
- child process loss
- stuck PDF library call that does not observe cancellation
- app exit
- component install/reset

This change is important independently of raw throughput because it prevents
one user stop from turning the next PDF operation into another full PyTorch or
layout-runtime prewarm.

### Cross-Page Translation Queue

The first `translate_many()` implementation may be page-local because
`receive_layout()` works on one layout at a time. For maximum Lightning
throughput, Rosetta should be ready to batch across page boundaries:

```txt
page worker(s)
  -> enqueue translation units with stable ids
  -> Rosetta batch queue fills target width or short deadline
  -> one Lightning batch request
  -> results routed back to waiting page/layout code
```

This is more invasive than a page-local batch, but it is the path most aligned
with RWKV Lightning's strengths. It also avoids trying to create concurrency by
running many independent provider requests, which already produced HTTP 409.

### Optional Deeper Rewrite: Two-Stage PDF Pipeline

If patching `receive_layout()` remains too constrained, split PDF translation
inside the Rosetta fork into two explicit stages:

```txt
Stage A: extract page translation units + layout operations
Stage B: batch translate all units through Rosetta scheduler
Stage C: replay translated units into PDF rewrite
```

This is the most powerful architecture because it gives Rosetta full control
over batching, cancellation, progress, retries, and diagnostics. It is also the
largest fork. Treat it as Phase 7, not the first implementation, unless the
minimal batch translator fails to produce meaningful speedups.

## Implementation Phases

### Phase 0: Pin, Compare, and Choose Base

Output:

- Record the exact PDFMathTranslate commit hash.
- Compare the local checkout against the `pdf2zh 1.7.9` package Rosetta
  currently ships.
- Decide whether the first patch should be based on the exact packaged `1.7.9`
  source or the newer local `1.9.11` checkout.
- Measure import/prewarm time for both candidates on Windows.
- Measure whether the `1.9.11` ONNX layout path can run without importing
  PyTorch.
- Record dependency and package-size deltas.

Validation:

- No Rosetta app behavior changes.
- Document the chosen base commit and reason.

Decision preference:

- Prefer `1.9.11` or newer if ONNX layout quality is acceptable and PyTorch can
  be removed from the Rosetta pack.
- Prefer exact `1.7.9` only if layout/rewrite behavior differs too much to
  absorb in this performance pass.

### Phase 1: Strip Runtime Surface for Rosetta

Output:

- Define the minimal runtime modules required for Rosetta PDF translation.
- Remove unused console/web/backend entry points from the Rosetta pack build.
- Remove unused cloud translator packages from the installed pack after
  `rosetta-batch` works.
- Keep source-level fork changes small enough to rebase.

Validation:

- `import pdf2zh` or the chosen Rosetta worker imports only the modules needed
  for PDF visual translation.
- `python -X importtime` or equivalent shows a clear import-time reduction.
- Pack size is smaller than the current Windows pack.

### Phase 2: Layout Runtime Replacement

Output:

- Integrate the ONNX Runtime layout path if selected in Phase 0.
- Bundle the ONNX layout model in the Rosetta PDF component.
- Remove PyTorch/doclayout_yolo from the Rosetta pack when the ONNX path passes
  fixture checks.

Validation:

- Worker prewarm no longer imports `torch`.
- First warmup reports layout runtime readiness without PyTorch.
- Existing PDF fixture translations remain visually acceptable.
- Real benchmark PDFs complete.

### Phase 3: Minimal Batch API in PDFMathTranslate

Output:

- Add `BaseTranslator.translate_many(...)` default loop.
- Add tests proving default translators preserve order and exception behavior.
- Modify `TranslateConverter.receive_layout()` to use `translate_many(...)`
  only when the translator declares native batch support.

Suggested flag:

```python
supports_batch = False
```

Rosetta translator sets:

```python
supports_batch = True
```

Validation:

- Existing PDFMathTranslate tests pass.
- Non-Rosetta translators still use the old thread-pool path.
- Formula-only and blank strings remain passthrough.
- Output order exactly matches `sstk` order.

### Phase 4: RosettaBatchTranslator

Output:

- Add service name such as `rosetta-batch`.
- Implement an HTTP loopback client that talks to Rosetta's local batch
  endpoint without using the OpenAI SDK.
- Preserve per-item cache semantics.
- Add deterministic tests with a fake local server or monkeypatched client.
- Make failures explicit and page-safe: result count mismatch, timeout, HTTP
  error, cancellation, and invalid JSON must become clear translation errors.

Validation:

- A fake backend receives one batch for many input items.
- Response count mismatch fails the batch.
- Cache hits do not call the backend.
- Mixed cache hits and misses preserve full output order.

Current status:

- The Python-side translator and `translate_many(...)` plumbing appear to be
  present in the local PDFMathTranslate checkout and the new Windows pack.
- Performance data shows this phase is not sufficient by itself because the
  Rust receiver still re-enters the old shim queue.

### Phase 5: Cooperative Cancellation and Worker Reuse

Output:

- Add worker protocol support for canceling the active PDF run without killing
  the warm worker in the normal case.
- Ensure in-flight Rosetta batch requests can be cancelled or ignored without
  poisoning the worker.
- Keep hard-kill fallback for deadlocks, app exit, install/reset, and process
  loss.

Validation:

- Start a PDF translation, cancel it, then start another PDF translation.
- The second run should not repeat full worker prewarm.
- Durable page state restores `translating` pages to retryable state.
- No stale partial PDF artifacts are committed after cancellation.

### Phase 6: Rosetta Worker Integration

Output:

- Update `rosetta_pdf2zh_worker.py` to use the patched PDFMathTranslate package
  and `service="rosetta-batch"` for Rosetta-managed PDF translation.
- Add a Rosetta local batch endpoint or reuse an internal one if already
  suitable.
- Keep the old OpenAI-compatible translator only behind
  `ROSETTA_PDF_FORCE_OPENAI_SHIM=1` as a local engineering escape hatch. It is
  not the default product fallback.

Validation:

- One-page PDF completes.
- Ten-page benchmark PDF completes.
- The previous 409 direct-concurrent failure does not recur.
- Diagnostics show larger batch sizes produced before reaching Lightning.

Current status:

- One-page and ten-page PDF completion are working with the manually imported
  ZIP pack.
- The previous 409 direct-concurrent failure did not recur on the stable path.
- Diagnostics do not yet show larger Lightning batch sizes. The next
  implementation task is the Rust-side direct wide-batch path for
  `/rosetta/batch-translations`.

### Phase 6.5: Direct Wide-Batch Provider Path

Entry criteria:

- PDFMathTranslate is calling `rosetta-batch`.
- `/rosetta/batch-translations` receives multiple texts in one request.
- Latest profile still shows many small RWKV requests and average batch size
  near the old shim baseline.

Output:

- First add batch-level diagnostics so this phase can be measured without
  source-text logging:
  - incoming Rosetta batch request count
  - incoming `texts.len()`
  - incoming total chars
  - skipped/passthrough item count
  - split item count
  - provider request count
  - provider batch-size histogram
  - queue wait and model request latency
  - output chars and error count
- Add a worker-side `translate_many(...)` probe because the old probe wraps
  `translator.translate(...)` and therefore misses the new native batch calls.
- Add a Lightning-specific fast path inside `/rosetta/batch-translations`.
- Preserve input/output order exactly.
- For normal in-budget PDF texts, call `translate_batch_via_lightning(...)`
  directly with the received `texts[]`.
- For over-budget texts, split only those specific items, translate their
  chunks, and reassemble them back into the original item positions.
- Keep the old `PendingTranslation` queue path for OpenAI-compatible fallback,
  MLX, llama.cpp, and any provider that still needs legacy shim behavior.
- Record diagnostics for received batch size, provider batch size, split item
  count, provider request count, and timings.

Current status:

- Implemented in Rosetta's Rust shim for Lightning only. The endpoint now
  prepares passthrough items, splits only over-budget items, sends provider
  chunks directly through `translate_batch_via_lightning(...)`, records the
  existing RWKV metrics against those direct requests, and reassembles outputs
  in the original item order.
- The old `PendingTranslation` queue is still used for MLX/mobile batch,
  llama.cpp, and the OpenAI-compatible fallback route.
- Added worker-side `page.processPage.translateBatch` stage events plus
  aggregate `translateBatchCount`, `translateBatchItems`,
  `translateBatchFailedCount`, and `translateBatchMs` fields in the
  receive-layout summaries.
- This phase still needs a fresh runtime PDF benchmark. If the next 10-page
  run still shows roughly 10 provider requests, the remaining bottleneck is
  page-local sequencing in the PDF component, and Phase 7 becomes the required
  throughput path.
- A forced retranslate runtime benchmark met that condition: each page emitted
  one native `translate_many(...)` call, 10 pages emitted 10 calls total, and
  Rust split those page-local inputs into 132 provider chunks across the same
  10 Lightning requests seen before the direct path. Phase 7 is now the
  required path for material throughput gains.

Validation:

- A 10-page run should show the real PDF component incoming batch shape, e.g.
  page-local batch item counts before any Rust-side regrouping.
- The same 10-page PDF should show provider request count lower than 10.
- Average provider batch size should rise materially above `13.2`.
- Total RWKV request time should drop below the latest `19.283s` figure.
- Wall time should beat the current stable best of about `25.6s` before the
  new component is considered a translation-throughput win.
- No HTTP 409 should occur because the fast path still uses a single in-flight
  provider request by default.

### Phase 7: Cross-Page Queue or Two-Stage Pipeline

Entry criteria:

- Page-local `translate_many()` is stable but still does not feed Lightning
  enough batch width, or performance remains far below RTX 5070 expectations.

Output:

- Add a Rosetta-owned translation queue that batches across page/layout calls,
  or split the fork into extract / translate / replay stages.
- Make retry operate on translation-unit ids rather than whole pages whenever
  feasible.
- Keep page artifacts durable only after the page output is complete.

Current status:

- Implemented a first worker-local two-pass slice for `rosetta-batch`:
  - collect pass runs pdfminer/pdf2zh layout with a deferred translator and
    records all translatable units across the current chunk;
  - translate pass calls the real Rosetta batch translator once with the
    collected units;
  - replay pass runs pdfminer/pdf2zh again with a pretranslated translator and
    writes the normal page-level PDF artifacts.
- This preserves Rosetta's durable page artifact model: no page is committed
  until replay has produced and saved the page PDF.
- The feature is scoped to Rosetta's native PDF translator. Non-Rosetta
  providers keep the old page-local path.
- Local diagnosis can disable the new path with
  `ROSETTA_PDF_CROSS_PAGE_BATCH=0` or
  `ROSETTA_PDF_DISABLE_CROSS_PAGE_BATCH=1`.
- A fake-backend smoke test against pages 1-2 of the benchmark PDF completed
  successfully: the worker wrote `page-0001.pdf` and `page-0002.pdf` and sent
  exactly one `/v1/rosetta/batch-translations` request containing 18 items and
  6,974 input characters.
- A real Lightning forced-retranslate benchmark on the 10-page PDF completed
  successfully and hit the expected scheduling shape:
  `rwkv.requestCount=1`, `rwkv.averageBatchSize=132.0`,
  `rwkv.totalRequestMs=3.931s`, and total wall time `16.760s`.
- This moves the active Phase 7 work from "feed Lightning wider batches" to
  reducing the duplicate extract/replay cost and single-page PDF save cost.
  The current two-pass implementation repeats layout inference during replay,
  visible as `page.yoloPredict=20` events for a 10-page run.
- The next Phase 7 slice removes that duplicate replay work inside Rosetta's
  worker: replay now reuses the collect-pass layout mask and cached pdfminer
  `LTPage` tree, so the replay pass can call `receive_layout(...)` directly
  instead of repeating ONNX layout inference and pdfminer stream rendering.
- Single-page page artifacts now default to speed-first saving
  (`deflate=0`) because `single.save(..., deflate=1)` dominated the fake
  backend smoke after replay reuse. Local diagnosis can restore compressed page
  artifacts with `ROSETTA_PDF_SINGLE_PAGE_DEFLATE=1`. This is a deliberate
  speed/disk-space tradeoff for local cache artifacts and should be revisited
  after real 10-page and 18-page benchmarks.
- A real 10-page forced-retranslate benchmark after replay reuse and
  speed-first page artifact saving completed in `8.636s`. It kept the desired
  model shape (`rwkv.requestCount=1`, `rwkv.averageBatchSize=132.0`) while
  reducing `page.yoloPredict` from `20` events / `4.700s` to `10` events /
  `2.373s`, eliminating replay `renderStreams`, and reducing
  `page.saveSinglePdf` from `3.385s` to `62ms`. The local translated page
  artifact cache for those 10 pages grew to about `137.8MB`, so the next
  product decision is whether to keep speed-first cache writes, restore
  compressed writes, or add background page-artifact compression after
  translation completes.
- A real 18-page forced-retranslate benchmark on the earlier Lightning
  investigation PDF completed in `14.663s`, down from the old `42.059s`
  baseline for the same job. It sent one Lightning request with provider batch
  size `212` instead of 20 small requests averaging `10.55`, and reduced summed
  RWKV request time from `29.404s` to `4.618s`. The translated page artifact
  cache totaled about `257.8MB` for 18 pages under speed-first page writes.

Validation:

- Benchmark logs show target batch widths on realistic PDFs. The first real
  Lightning Phase 7 run reached batch size `132` in one provider request.
- Cancellation during a large cross-page batch keeps the worker alive.
- Output page order and PDF page state remain deterministic.

### Phase 8: Package and Installer

Output:

- Build a new Rosetta PDF component pack from the patched PDFMathTranslate
  source.
- Update `managed_pdf2zh/profile.rs` URLs, size, and SHA256 only after upload.
- Treat the old pack as outdated during beta; Rosetta may force users to update
  to the new Rosetta-specific PDF component instead of preserving old pack
  compatibility.

Local dogfood artifact built on 2026-07-02:

- Path:
  `rosetta-app/dist/pdf-layout/rosetta-pdf2zh-windows-amd64.zip`
- Size: `320275049` bytes
- SHA256:
  `c36133090f1221542b1d4a53f8d576e2b80e5f96ec31129c1ea1c74318f49542`
- Layout model:
  `models/doclayout_yolo_docstructbench_imgsz1024.onnx`
- Layout model SHA256:
  `fece9af02f618b603ff7921ccec6861d13e7e1f9830e091dfb7e8ad9311e5b21`
- Runtime smoke test: imports `pdf2zh 1.9.11`, initializes `OnnxModel`, and
  confirms `RosettaBatchTranslator` advertises native batch support.

Validation:

- Fresh install downloads the new pack.
- Existing install upgrades cleanly.
- App reset and PDF component reinstall remove old worker processes on Windows.
- `get_pdf2zh_status` reports ready only when the patched package and bundled
  layout model are present.

### Phase 9: Benchmark Gate

Minimum benchmark set:

- same 10-page `2604.17278v1.pdf`
- same 18-page PDF used in the Lightning investigation
- one Markdown baseline to confirm ordinary-document throughput is unchanged
- one llama.cpp or MLX smoke PDF to confirm non-Lightning paths were not
  regressed, if those runtimes are available locally

Success criteria:

- PDF translation completes without HTTP 409.
- Ten-page Lightning PDF beats the current stable best of about `25.6s`.
- Eighteen-page Lightning PDF beats the current `42.1s` run.
- Worker ready/prewarm time improves materially compared with the current
  PyTorch/doclayout_yolo pack.
- Canceling and restarting a PDF translation does not force a full prewarm in
  the normal healthy-worker path.
- Provider request count drops materially.
- Observed Lightning batch sizes approach the configured target where the PDF
  has enough text units.
- Persistent diagnostics remain privacy-safe.

Stretch target:

- 2x improvement over the current stable PDF Lightning path on the RTX 5070
  workload.
- Sub-3-second warm worker readiness on a warm machine if the ONNX path and
  import pruning make that realistic.
- 3x-5x PDF throughput improvement on sufficiently text-heavy PDFs after
  cross-page batching.

## Fallback and Rollback

The beta product path is the Rosetta-specific PDF component: ONNX layout model
plus native `rosetta-batch` translation. Old PDF component packs may be marked
not ready and require reinstall/update.

Suggested env flags:

```txt
ROSETTA_PDF_COOPERATIVE_CANCEL=1
ROSETTA_PDF_FORCE_OPENAI_SHIM=1
```

Beta default progression:

1. Build and ship a Rosetta PDF component whose default translator is
   `rosetta-batch`.
2. Treat the new component as the normal beta path. App updates may require
   updating the PDF component pack; beta compatibility with the old PDF pack is
   not a product requirement.
3. Keep `ROSETTA_PDF_FORCE_OPENAI_SHIM=1` only as a local engineering escape
   hatch while validating the new component.
4. Remove the old OpenAI-compatible translator path once benchmarks and real
   beta PDF runs show the new component is stable. The local HTTP batch
   endpoint can remain if the native PDF component uses it as Rosetta's
   scheduler bridge.

## Privacy and Diagnostics

Allowed persistent diagnostics:

- job id / run id
- provider id
- page numbers
- batch size
- request count
- failed request count
- input/output character counts
- timing buckets
- status/error class

Disallowed persistent diagnostics:

- source text
- translated text
- prompts
- raw model responses
- reconstructed document text
- bounding-box-associated text content

Any debug mode that writes text must remain local, explicit, and disabled by
default. Prefer not adding text debug logs to the new path.

## Risks

- PDFMathTranslate may change the current converter/translator path in newer
  versions. Keep the first patch scoped to the packaged version.
- Moving from `1.7.9` to `1.9.11` may change PDF layout/rewrite behavior even
  if it removes PyTorch. This must be fixture-tested, not assumed safe.
- The upstream project is AGPL-3.0 licensed. Rosetta distribution and fork
  strategy must be reviewed before shipping a modified binary pack.
- A page-local `translate_many()` still may not expose all pages in one global
  batch if pdf2zh processes pages sequentially. If Phase 2 is not enough, add a
  Rosetta-side queue that batches across page calls while still sending one
  Lightning request at a time.
- Too-large batches may increase latency or memory pressure. Keep batch width
  configurable and benchmark target widths before raising defaults.
- Cache semantics must remain per item, not per joined batch, otherwise small
  edits or retries will become expensive.
- Cancellation must not strand pdf2zh worker state or leave Rosetta PDF pages
  permanently `translating`.
- Maintaining a fork has cost. Keep the patch small, document upstream merge
  points, and avoid unrelated changes.
- Over-pruning dependencies can break obscure PDF features. Prune in the pack
  build first, and keep source-level deletion conservative until tests are
  broad enough.

## Open Questions

- Should the first batch backend live in upstream-shaped PDFMathTranslate code
  as a generic `translate_many` improvement, or in a Rosetta-only branch with
  less upstream compatibility work?
- Should Rosetta rebase onto `pdf2zh 1.9.11` immediately to remove PyTorch, or
  first patch the exact shipped `1.7.9` source for lower layout risk?
- Does ONNX Runtime layout output match the current doclayout_yolo output on
  Rosetta's academic PDF fixtures?
- Should Rosetta's batch endpoint be HTTP loopback, stdin/stdout through the
  existing persistent worker protocol, or a local Unix/Windows named pipe later?
- Should batching be page-local in Phase 1, or should the first implementation
  immediately add a cross-page queue?
- Can normal cancellation be fully cooperative, or are there pdfminer/PyMuPDF
  calls that still require hard-kill in some stages?
- Which dependencies are truly required in the shipped pack after
  `rosetta-batch` becomes the only translator?
- What is the best Lightning target batch width on RTX 5070 for PDF text:
  `64`, `100`, `128`, `256`, or higher?
- When can `ROSETTA_PDF_FORCE_OPENAI_SHIM=1` be removed entirely after the
  Rosetta PDF component proves stable across beta fixtures?

## Documentation Follow-Ups

When implementation starts:

- Add a change-log entry under `docs/engineering/change-log/`.
- Record the PDFMathTranslate base commit and Rosetta patch commit.
- Update `docs/engineering/pdf-pipeline.md` if the default PDF translation
  architecture changes.
- Add an ADR if Rosetta commits long-term to maintaining a PDFMathTranslate
  fork or patched distribution.
