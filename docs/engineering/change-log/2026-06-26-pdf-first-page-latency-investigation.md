# 2026-06-26 PDF First Page Latency Investigation

## Summary

Investigated and reduced PDF first-page latency using job-local logs instead of
guesswork. The investigation added full PDF lifecycle diagnostics, worker
internal stage timings, YOLO predict prewarm, `page.processPage` substage
timings, and an instrumented PDF paragraph concurrency adjustment.

The latest measured state on the 3-page `drylab.pdf` sample:

```txt
Before worker-stage diagnosis:
  first page committed: 5287 ms
  total run:            9572 ms

After YOLO predict prewarm:
  first page committed: 3476 ms
  total run:            7451 ms

After processPage tracing and thread=8:
  first page committed: 3310 ms
  total run:            6801 ms
```

## Logs Used

The primary source of truth is the job-local timeline:

```txt
<job>/diagnostics/pdf-timeline.jsonl
```

Supporting files:

```txt
<job>/diagnostics/pdf-translation-profile-<runId>.json
<job>/pdf_pages.<targetLang>.json
<app-data>/logs/rosetta.log
```

The timeline is append-only diagnostics. It must never contain source text,
translated text, prompts, model responses, or raw message bodies.

## Key Findings

### "First token" Was Not A Streaming Token

The PDF path was not exposing a true model token stream. It used the pdf2zh
visual pipeline and surfaced progress when a page artifact was committed. The
initial user-visible delay was therefore "time to first translated page PDF",
not "time to first token".

### Rosetta Scheduling Was Not The Main Bottleneck

Top-level run events showed request/run/chunk scheduling was near-zero:

```txt
translation.requested -> run.started: 0-1 ms
run.started -> chunk.started:         1-6 ms
page artifact save/commit:            millisecond-level
```

The latency was inside the persistent PDF worker and RWKV/pdf2zh processing.

### Startup Prewarm Existed But Did Not Warm YOLO Predict

The app already started the persistent pdf2zh worker after the window opened.
That prewarm covered Python import and the worker ready handshake, but the CPU
path did not run a real page-sized DocLayout-YOLO `predict` before the first
translation.

Worker-stage logs showed the first page paid this predict-time setup:

```txt
Before YOLO predict prewarm:
  page 1 page.yoloPredict: ~1950 ms
  page 2 page.yoloPredict:  ~341 ms
  page 3 page.yoloPredict:  ~346 ms
```

### YOLO Predict Prewarm Removed The Cold Path

The worker now performs one synthetic blank-page YOLO prediction before it
reports `ready`.

Observed ready log:

```txt
yoloWarmupStatus=completed
yoloWarmupMs=~1800-2025
yoloWarmupDevice=cpu
```

After this change:

```txt
page 1 page.yoloPredict: ~341-351 ms
```

First-page commit improved by about 1.8 seconds on `drylab.pdf`:

```txt
5287 ms -> 3476 ms
```

### page.processPage Became The Dominant First-Page Cost

After YOLO prewarm, page 1 was dominated by pdf2zh's `TextConverter` work:

```txt
page 1 total:       ~2627-2785 ms
page.yoloPredict:   ~341-351 ms
page.processPage:  ~2251-2387 ms
```

`page.processPage` was expanded into:

```txt
page.processPage.beginPage
page.processPage.renderStreams
page.processPage.endPage
page.processPage.receiveLayout
page.processPage.translateRequest
page.processPage.patchStreams
```

The split showed PDF stream rendering and patching were not the problem:

```txt
renderStreams: ~8-19 ms/page
patchStreams:  0 ms/page
```

The wall time was almost entirely:

```txt
endPage -> receiveLayout -> translateRequest
```

### thread=4 Produced Multiple TextConverter Waves

With pdf2zh `thread=4`, page 1 had 9 paragraph translation requests and waited
on three waves:

```txt
wave 1: 4 requests, ~1295 ms
wave 2: 4 requests,  ~460 ms
wave 3: 1 request,   ~586 ms
```

The shim batched paragraph requests into RWKV calls, but the page still waited
for TextConverter waves before the page artifact could be emitted.

### thread=8 Improved Throughput More Than First-Page Latency

The default PDF shim paragraph batch width was raised from 4 to 8. This also
passes `thread=8` into the persistent pdf2zh worker for providers that do not
report their own supported batch size.

Observed effect on `drylab.pdf`:

```txt
thread:              4 -> 8
total run:        7617 -> 6801 ms
RWKV requestCount:  7 -> 4
RWKV total:       4871 -> 4202 ms
first page:       3416 -> 3310 ms
```

The first page did change from `4 + 4 + 1` waves to `8 + 1` waves:

```txt
thread=8 page 1:
  wave 1: 8 requests, ~1711 ms
  wave 2: 1 request,   ~580 ms
```

This is a net win, but it mostly improves total document throughput. The first
page only improved by about 106 ms because the larger first batch had higher
single-batch latency.

## Fixes Implemented

### Job-Local Timeline

Added:

```txt
<job>/diagnostics/pdf-timeline.jsonl
```

It records import, translation request, run, chunk, page commit, worker-stage,
duration, page count, file size, provider, and aggregate RWKV timing events.

### Worker Stage Diagnostics

The persistent worker emits `worker.stage` events for:

```txt
job
preprocess.openPrepareAndSavePdf
model.getYoloModel
pdfminer.initializeInterpreter
pdfminer.loadPages
page.pixmapAndImage
page.yoloPredict
page.layoutMask
page.prepareContentStream
page.processPage
page.applyPatches
page.saveSinglePdf
page.emitPageEvent
cleanup.*
```

### YOLO Predict Prewarm

The worker now warms DocLayout-YOLO with a synthetic blank 596x842 image
(`imgsz=832`) before reporting ready. This moves YOLO first-predict setup into
background app startup.

### page.processPage Substage Diagnostics

`page.processPage` is now expanded with equivalent pdf2zh/pdfminer steps and
per-page `translator.translate(...)` probes. Request probes record only:

```txt
pageNumber
requestIndex
sourceChars
outputChars
durationMs
status
errorType
```

They intentionally do not record any document text or prompt/response content.

### PDF Paragraph Batch Width

The default PDF OpenAI-shim paragraph batch width is now 8. The same value
drives the worker's pdf2zh `thread` count when the provider does not report a
supported batch size.

## Current Interpretation

The first-page latency stack is now approximately:

```txt
worker dispatch before first stage: ~540-780 ms
YOLO predict:                       ~340 ms
TextConverter receiveLayout:       ~2300 ms on page 1
save/commit:                       millisecond-level
```

The largest remaining first-page component is TextConverter translation wait,
not YOLO and not page artifact assembly.

The thread=8 adjustment should remain for now because it improves total PDF
throughput without hurting the first page. It should not be blindly raised
again without measuring first-page batch latency, because larger batches reduce
wave count but can increase the wall time of the first batch.

## Deferred Follow-Ups

Potential next optimizations, left intentionally out of this change:

- Short-text passthrough for obviously non-translatable fragments such as very
  short punctuation/numeric placeholders.
- Smarter grouping of tiny TextConverter paragraphs before they reach RWKV.
- More precise Rust-side dispatch timing between `chunk.started` and the first
  worker `job.started` stage.
- Provider-specific tuning for batch width versus first-page latency.

## Validation

Commands run during the implementation:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo fmt
cargo check
cargo test rosetta_jobs
```

The bundled Python worker was also syntax-checked with:

```powershell
python -m py_compile rosetta-app\src-tauri\src\managed_pdf2zh\rosetta_pdf2zh_worker.py
```
