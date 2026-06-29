# 2026-06-26 PDF Translation No-Truncation And 50% Speedup Handoff

## Purpose

This document is a handoff for the next agent. The task is larger than one
conversation window, so every step must leave durable notes in this file or in a
follow-up document under `docs/engineering/plans/`.

User goal:

1. PDF translation must not lose source content.
2. In particular, llama.cpp/RWKV output must not be truncated.
3. End-to-end PDF translation time, measured from the moment the user starts
   translation after worker/runtime prewarm is complete until all requested pages
   are translated, should improve by about 50%.
4. The user believes this is realistic because the model is a 0.4B RNN
   translation model and should support high concurrency.

Do not require the user to click through the app for validation. Build or use an
automated translation test/harness so the next runs can be repeated without user
intervention.

## Product Constraints

Keep Rosetta narrow:

- local translation;
- privacy-sensitive documents;
- long text and document structure preservation;
- batch translation through a local model API.

Do not add cloud upload, accounts, telemetry, chat, document Q&A, summarization,
or generic assistant behavior.

Before making architectural or broad pipeline changes, read:

- `docs/rosetta_project_plan.md`
- `docs/engineering/README.md`
- `docs/engineering/conventions/frontend.md`
- `docs/engineering/conventions/data-models.md`
- `docs/engineering/plans/2026-05-12-pdf-v1-support.md`
- `docs/engineering/plans/2026-06-11-pdf-translation-stability-performance-roadmap.md`
- `docs/engineering/change-log/2026-06-26-pdf-first-page-latency-investigation.md`
- `docs/engineering/change-log/2026-06-26-pdf-timeline-diagnostics.md`

For stack-level changes, also check `docs/engineering/decisions/`.

## Current Evidence

Latest observed job:

```txt
jobId:      job-1782471162628-2604-17278v1
runId:      run-pdf-1782471277785
file:       2604.17278v1.pdf
target:     zh-CN
pages:      1-10
status:     completed
```

Relevant local logs on Windows:

```txt
%APPDATA%\com.rosetta.desktop\jobs\job-1782471162628-2604-17278v1\diagnostics\pdf-translation-profile-run-pdf-1782471277785.json
%APPDATA%\com.rosetta.desktop\jobs\job-1782471162628-2604-17278v1\diagnostics\pdf-timeline.jsonl
%APPDATA%\com.rosetta.desktop\jobs\job-1782471162628-2604-17278v1\pdf_pages.zh-CN.json
%APPDATA%\com.rosetta.desktop\logs\rosetta.log
%APPDATA%\com.rosetta.desktop\logs\rwkv-io-debug.jsonl
%LOCALAPPDATA%\com.rosetta.desktop\managed-rwkv\logs\runtime.log
```

Latest profile summary:

```txt
total:              141156 ms
pdf2zhProcess:      141096 ms
pagesRequested:     10
pagesTranslated:    10
pagesFailed:        0
shim requestCount:  20
totalRequestMs:     130247
averageRequestMs:   6512
maxRequestMs:       10913
totalInputChars:    36387
totalOutputChars:   11832
```

Important distinction:

```txt
profile.rwkv.requestCount = 20
```

This is the number of pdf2zh shim batches, not the number of individual
llama.cpp `/completion` requests.

With `ROSETTA_RWKV_IO_DEBUG=1`, the actual llama.cpp calls for the same job were:

```txt
actual /completion records: 131
HTTP OK:                    131
empty outputs:              0
totalInputChars:            36387
totalOutputChars:           11832
overall output/input ratio: 0.325
stop_type=eos:              105
stop_type=limit:            26
truncated=true:             26
avg predicted speed:        21.47 tokens/s
min predicted speed:        5.73 tokens/s
max predicted speed:        77.42 tokens/s
```

The input/output character gap is partly normal English-to-Chinese compression,
but the `26` truncated completions are not normal and are the first correctness
target.

Known recent timings for the same 10-page PDF:

```txt
run-pdf-1782458102030: 117796 ms
run-pdf-1782468738209: 119420 ms
run-pdf-1782470387196: 139688 ms
run-pdf-1782471277785: 141156 ms
```

Use `141156 ms` as the current no-debug-ish baseline for the newest run. The
50% target for this sample is approximately:

```txt
target total after prewarm: <= 70600 ms
```

Do not count app startup, Python import, YOLO predict warmup, or RWKV runtime
health startup in this metric. The metric starts when the translation command is
issued after the PDF worker and RWKV runtime are ready.

## Suspected Root Cause For Truncation

The managed llama.cpp runtime was launched like this:

```txt
llama-server.exe \
  --model ...\RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf \
  --host 127.0.0.1 \
  --port <port> \
  --alias rwkv-translate \
  --ctx-size 4096 \
  --gpu-layers auto \
  --parallel 16
```

`runtime.log` showed:

```txt
n_slots = 16
n_ctx = 256
```

So each concurrent slot only gets about 256 context tokens. Long PDF chunks can
consume 100-160 prompt tokens, leaving too little room for Chinese output. The
request says `n_predict=1024`, but this cannot help when the effective per-slot
context is about 256 tokens. The raw llama.cpp responses show:

```txt
truncated: true
stop_type: "limit"
tokens_cached: 255
```

The likely correctness fix is to stop oversplitting the context across too many
slots, increase the total context, reduce per-request input size, or use a
better native batch/concurrency path that preserves enough output room per item.

## Acceptance Criteria

Correctness gates:

1. For the 10-page `2604.17278v1.pdf` sample or an equivalent automated fixture:
   - every requested page is translated;
   - `pagesFailed = 0`;
   - every actual RWKV completion has `ok=true`;
   - every actual RWKV completion has non-empty output;
   - `truncated=true` count is `0`;
   - `stop_type=limit` count is `0`.
2. No source text should be silently dropped by chunking, batching, or
   postprocessing. If text is intentionally passed through because it is a
   formula/placeholder, record it separately and verify it is represented in the
   output.
3. The automated test should fail if any completion truncates.

Performance gates:

1. Measure from translation command start after prewarm to all requested page
   artifacts committed.
2. Target for the current 10-page sample: `<= 70600 ms`.
3. Track first-page commit time, but do not optimize it at the expense of total
   throughput or correctness.
4. Record:
   - total time;
   - page commit timeline;
   - actual completion count;
   - shim batch count;
   - average and p95 batch latency;
   - average and p95 completion latency;
   - predicted tokens/sec;
   - prompt tokens/sec;
   - truncation count;
   - input/output character totals.

## Automated Test Requirement

Do not ask the user to manually import a PDF and click translate.

Create or use a repeatable harness. Viable approaches:

1. Add a focused Rust integration test or ignored benchmark under
   `rosetta-app/src-tauri` that calls lower-level job/import/translation
   functions directly.
2. Add a local developer script that:
   - starts or reuses the managed RWKV runtime;
   - ensures the pdf2zh worker is ready;
   - imports a PDF fixture or copies a local AppData sample into a temp job;
   - invokes the same backend path as `translate_rosetta_pdf_pages`;
   - enables `ROSETTA_RWKV_IO_DEBUG=1` only for the test run;
   - parses logs and exits non-zero on truncation or regression.
3. If a Tauri window is unavoidable, automate it with a script/browser driver.
   The test must still be push-button for the agent. The user should not have to
   click anything.

Suggested command-level entry points to inspect:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
  import_rosetta_document_from_path
  translate_rosetta_pdf_pages
  translate_pdf_pages_inner

rosetta-app/src/lib/rosettaJobs.ts
  importRosettaDocumentFromPath
  translateRosettaPdfPages
```

If writing a harness, prefer backend entry points over UI automation so timing is
stable and independent of the React workbench.

## How To Read Logs

Job-level PDF diagnostics:

```txt
<job>/diagnostics/pdf-translation-profile-<runId>.json
<job>/diagnostics/pdf-timeline.jsonl
<job>/pdf_pages.<targetLang>.json
<job>/pdf_run.<targetLang>.json
```

App/runtime diagnostics:

```txt
%APPDATA%\com.rosetta.desktop\logs\rosetta.log
%APPDATA%\com.rosetta.desktop\logs\rwkv-io-debug.jsonl
%LOCALAPPDATA%\com.rosetta.desktop\managed-rwkv\logs\runtime.log
```

Enable full RWKV input/output logging:

```txt
ROSETTA_RWKV_IO_DEBUG=1
```

Warning: `rwkv-io-debug.jsonl` contains full source text and full translations.
It is local-only debug data and must not be committed.

Useful log interpretations:

- `pdf-translation-profile-*.json`
  - `durationsMs.total`: command start to run complete.
  - `durationsMs.pdf2zhProcess`: worker processing time.
  - `rwkv.requestCount`: shim batch count, not individual completions.
  - `rwkv.totalInputChars` / `totalOutputChars`: aggregate source and translated
    characters seen by the shim.
- `pdf-timeline.jsonl`
  - `page.committed`: when a translated page artifact becomes available.
  - `worker.stage=page.yoloPredict`: DocLayout/YOLO cost.
  - `worker.stage=page.processPage.receiveLayout`: includes translation wait.
  - `worker.stage=page.processPage.translateRequest`: pdf2zh text request
    timing. These are not necessarily one-to-one with llama.cpp calls.
- `rosetta.log`
  - `[pdf2zh-llama-cpp] assembled N item(s) in batch`: shim batch width.
  - `[pdf2zh-shim] split long request into N chunk(s)`: chunker behavior.
  - `[pdf2zh-shim] translation_preview=...`: useful quick quality signal.
- `rwkv-io-debug.jsonl`
  - one record per actual provider call.
  - parse `rawResponse` JSON and inspect `truncated`, `stop_type`,
    `tokens_predicted`, `tokens_evaluated`, and `timings`.
- `runtime.log`
  - look for `n_slots` and `n_ctx`.
  - if `n_ctx` is very small, per-request output room is constrained even when
    `n_predict` is high.
  - inspect `print_timing` lines for prompt and generation throughput.

Useful ad hoc parser shape:

```powershell
@'
const fs = require("fs");
const log = process.env.APPDATA + "\\com.rosetta.desktop\\logs\\rwkv-io-debug.jsonl";
const context = "pdf-job:<job-id>";
const records = fs.readFileSync(log, "utf8")
  .trim()
  .split(/\r?\n/)
  .filter(Boolean)
  .map(JSON.parse)
  .filter((record) => record.context === context);
let truncated = 0;
let inputChars = 0;
let outputChars = 0;
for (const record of records) {
  const raw = JSON.parse(record.rawResponse);
  const input = (record.inputs || []).join("\n");
  const output = (record.outputs || []).join("\n");
  inputChars += input.length;
  outputChars += output.length;
  if (raw.truncated || raw.stop_type === "limit") truncated += 1;
}
console.log({ records: records.length, inputChars, outputChars, truncated });
'@ | node -
```

## Component Documentation To Read

The next agent should not rely only on local guesses. Read the authors' docs or
source comments for each component before changing its assumptions:

1. llama.cpp / `llama-server`
   - server batching and continuous batching behavior;
   - relation between `--ctx-size`, `--parallel`, slots, and per-slot `n_ctx`;
   - `/completion` vs OpenAI-compatible endpoints;
   - prompt cache and recurrent/RWKV checkpoint behavior;
   - best practices for throughput with many small requests.
2. RWKV / RWKV GGUF runtime behavior
   - whether this RWKV model supports more efficient native batching than the
     current many parallel `/completion` calls;
   - whether recurrent state or prompt cache settings interact poorly with
     llama.cpp slots;
   - recommended concurrency for 0.4B translate models on Vulkan/iGPU.
3. pdf2zh / pdfmathtranslate
   - thread/concurrency model;
   - paragraph/text box extraction and splitting;
   - translation callback expectations;
   - cache and retry behavior;
   - best practices for preserving layout while using local models.
4. Rosetta local docs listed in the Product Constraints section.

Prefer primary docs and source code over blog posts. If external documentation
changes the architecture, record the decision in `docs/engineering/decisions/`.

## Likely Work Packages

### Step 1: Build The Automated Benchmark Harness

Goal:

- Reproduce the current 10-page run without manual UI interaction.
- Produce a machine-readable summary and fail on truncation.

Record in this document:

```txt
date:
commit:
sample:
command:
runtime launch args:
provider:
pages:
total ms:
first page ms:
actual completions:
shim batches:
truncated completions:
notes:
```

Do not proceed to tuning until the harness can catch the current truncation
problem.

### Step 2: Fix No-Truncation First

Candidate experiments:

1. Reduce `--parallel` from `16` to `8`, then `4`, and inspect `runtime.log`
   `n_ctx`.
2. Increase total context if memory allows, such as `--ctx-size 8192` with
   `--parallel 8`.
3. Make the pdf2zh chunker aware of effective per-slot context, not only rough
   source token estimates.
4. Dynamically split long source text so input plus expected output stays below
   the effective slot context.
5. Add an automated retry path: if a completion returns `truncated=true` or
   `stop_type=limit`, split that source text smaller and retry. This is a
   correctness backstop, not the final performance strategy.

Acceptance for this step:

```txt
truncated completions: 0
stop_type=limit:       0
empty outputs:         0
pages failed:          0
```

Record before/after metrics.

### Step 3: Recover And Improve Throughput

After truncation is fixed, optimize speed. Candidate experiments:

1. Find the best `ctx-size` / `parallel` pair. The target is enough per-slot
   context with high enough concurrency.
2. Evaluate whether the current design is wasting the RNN model's native
   concurrency by issuing many independent `/completion` calls.
3. Investigate using a true batch endpoint/provider if available and stable.
4. Tune shim `max_batch_size`, `BATCH_WINDOW_MS`, and pdf2zh thread ceiling.
5. Reduce pathological tiny requests:
   - passthrough formula-only placeholders;
   - merge adjacent tiny text fragments where layout allows;
   - avoid sending one-character text to the model.
6. Avoid overlong reference-list chunks that are slow and often low-value for
   semantic translation. Preserve content, but consider smaller deterministic
   chunks.

Acceptance for this step:

```txt
same sample total after prewarm: <= 70600 ms
truncated completions:           0
pages failed:                    0
```

If the 50% target is not reached, record why with data and the next strongest
experiment.

### Step 4: Add Regression Coverage

Add tests that protect both correctness and performance-sensitive behavior:

- unit tests for chunk splitting budgets;
- unit tests for formula/placeholder passthrough;
- an ignored integration benchmark for local runtime PDF translation;
- a parser test for `rwkv-io-debug.jsonl` summaries if a parser script is added;
- tests around retry-on-truncation if implemented.

Validation commands when relevant:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

If adding a benchmark script, document its command and expected output.

### Step 5: Document The Final Operating Point

When a stable configuration is found, update:

- this handoff or a follow-up plan document;
- `docs/engineering/change-log/`;
- `docs/engineering/decisions/` if the runtime architecture changes;
- any user-facing or support docs if the runtime requirements changed.

Include:

```txt
chosen ctx-size:
chosen parallel:
effective n_ctx per slot:
chosen batch size:
chosen pdf2zh thread ceiling:
test sample:
baseline total:
final total:
speedup:
truncation count:
quality notes:
```

## Initial Hypotheses To Validate

1. `--parallel 16` is too high for `--ctx-size 4096` because it leaves only
   `n_ctx=256` per slot, causing truncation.
2. Dropping to `--parallel 8` may remove truncation with an acceptable speed
   tradeoff; the RNN model may still run enough concurrent slots to be fast.
3. Increasing `--ctx-size` may recover both correctness and throughput if GPU/iGPU
   memory permits it.
4. Current pdf2zh request splitting was tuned against estimated source tokens,
   but it did not account for output budget under per-slot context pressure.
5. The current `/completion`-per-text approach may not be the best use of RWKV's
   native high-concurrency behavior. Check upstream docs and local provider
   options before assuming this is optimal.

## Notes From Current Run

Most severe truncated examples were long inputs:

```txt
input chars: 713, output chars: 128, ratio: 0.180, truncated: true
input chars: 733, output chars: 136, ratio: 0.186, truncated: true
input chars: 666, output chars: 135, ratio: 0.203, truncated: true
input chars: 602, output chars: 124, ratio: 0.206, truncated: true
input chars: 627, output chars: 148, ratio: 0.236, truncated: true
```

Grouped by input size:

```txt
input <100 chars:   output/input 0.374
input 100-299:      output/input 0.385
input 300-499:      output/input 0.340
input >=500:        output/input 0.236
```

This strongly suggests long blocks lack output room. Do not treat low
output/input ratio alone as failure, but do treat `truncated=true` and
`stop_type=limit` as hard failures.

## Progress Log

Append every substantial step here.

### 2026-06-26 Initial Handoff

- Created this plan after diagnosing `job-1782471162628-2604-17278v1`.
- Current baseline: `141156 ms` for 10 pages after prewarm.
- Current correctness issue: `26 / 131` actual completions truncated.
- Primary suspected cause: `--ctx-size 4096 --parallel 16` gives only
  `n_ctx=256` per slot in llama.cpp.
- Next step: build an automated benchmark/harness that reproduces this without
  requiring the user to click through the app.

### 2026-06-26 Runtime Context And Harness Pass

- Added `rosetta-app/scripts/check-pdf-translation-run.mjs`.
  - It reads one run's PDF profile, page state, timeline, and
    `rwkv-io-debug.jsonl`.
  - It exits non-zero on missing debug records, failed pages, empty outputs,
    `truncated=true`, `stop_type=limit`, or a breached `--max-total-ms`.
  - It does not print source text, translated text, prompts, or raw responses.
- Raised the managed Windows llama.cpp runtime from
  `--ctx-size 4096 --parallel 16` to `--ctx-size 8192 --parallel 16`.
  - Expected effective slot context is about 512 tokens.
  - This preserves the 16-way concurrency target while addressing the 256-token
    slot that caused output-room truncation.
- Raised the PDF shim llama.cpp batch width to 16 while leaving generic
  mobile/lightning PDF providers at the previous width of 8.
- Hardened llama.cpp response parsing so `truncated=true` or
  `stop_type=limit` fails instead of accepting partial output.
- Added change-log entry:
  `docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md`.
- Ran the new checker against old profile `run-pdf-1782471277785`.
  - It correctly failed the `70600 ms` performance gate.
  - The local `rwkv-io-debug.jsonl` had already been cleared by later activity,
    so the checker also failed on missing per-completion records.
- Still required for final acceptance:
  - restart the managed runtime so the new llama.cpp args are active;
  - run a fresh 10-page `2604.17278v1.pdf` benchmark with
    `ROSETTA_RWKV_IO_DEBUG=1`;
  - verify `truncated=0`, `stop_type=limit=0`, `empty outputs=0`, and
    `total <= 70600 ms` with the checker.

### 2026-06-26 Runtime Benchmark Handover

- Added follow-up handover:
  `docs/engineering/plans/2026-06-26-pdf-runtime-benchmark-handover.md`.
- The managed llama.cpp runtime was confirmed running with
  `--ctx-size 8192 --parallel 16`.
- A full forced 10-page retranslation was run:
  `job-1782474427044-2604-17278v1`,
  `run-pdf-1782475009793`.
- The run completed at the page-state level but failed the benchmark checker:
  - total: `299176 ms`;
  - failed completions: `4`;
  - `truncated=true`: `4`;
  - `stop_type=limit`: `4`;
  - empty outputs: `4`.
- Critical next fix: page/job state must not report clean success when any
  required lower-level completion failed, returned empty output, or hit
  `truncated=true` / `stop_type=limit`.
- Do not rerun the same benchmark again before fixing that failure propagation
  and adding a truncation retry/split backstop.

### 2026-06-26 Failure Propagation And Split Backstop

- Fixed the page/run truthfulness gap after the failed full benchmark:
  - if a pdf2zh invocation completes but final shim metrics contain
    `failedRequestCount > 0`, Rosetta now clears artifacts produced by that
    invocation and marks the affected pages/run as failed;
  - the whole-document PDF generation path also rejects such an invocation
    instead of marking the PDF translation ready.
- Added a llama.cpp-specific PDF chunk profile:
  - body/caption target: about 56 prompt tokens;
  - reference target: about 42 prompt tokens;
  - generic mobile/lightning shim providers keep the earlier larger chunk
    budgets.
- Added a llama.cpp split retry backstop:
  - failed llama.cpp shim batches retry the affected texts with smaller chunks;
  - a final serial split retry uses an even smaller prompt budget before
    surfacing the error to pdf2zh;
  - only unrecovered failures increment the shim-level `failedRequestCount`.
- Validation passed:
  - `node --check scripts/check-pdf-translation-run.mjs`;
  - `pnpm typecheck`;
  - `cargo fmt -- --check`;
  - `cargo check`;
  - `cargo test rosetta_jobs`;
  - `cargo test llama_cpp`;
  - `cargo test managed_rwkv::lifecycle`.
- Next step: rerun the real 10-page PDF benchmark and check the new run with
  `scripts/check-pdf-translation-run.mjs`.

### 2026-06-26 Real Benchmark After Backstop

- Ran a forced 10-page retranslation of `2604.17278v1.pdf`:
  - job: `job-1782474427044-2604-17278v1`;
  - run: `run-pdf-1782477804133`;
  - runtime: `--ctx-size 8192 --parallel 16`, effective slot `n_ctx=512`;
  - target: `zh-CN`.
- Correctness passed:
  - pages requested: `10`;
  - pages translated: `10`;
  - pages failed: `0`;
  - completion records: `343`;
  - `ok=false`: `0`;
  - empty output: `0`;
  - `truncated=true`: `0`;
  - `stop_type=limit`: `0`;
  - final shim `failedRequestCount`: `0`.
- Performance still failed:
  - total: `189984 ms`;
  - target gate: `70600 ms`;
  - first page: `14596 ms`;
  - shim batches: `26`;
  - shim average/max: `6561 / 8850 ms`;
  - completion latency p50/p95/max: `4396 / 6857 / 7582 ms`;
  - throughput prompt/predicted: `53.01 / 63.14 tok/s`.
- Checker output:
  `C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782477804133.json`.
- Interpretation:
  - smaller llama.cpp chunks fixed truncation without triggering the split
    retry backstop on this run;
  - the conservative chunking increased completion count from the previous
    full forced run's `188` to `343`;
  - total improved from the failed `299176 ms` run but is still far above the
    `70600 ms` speed target.
- Next performance work should compare runtime operating points such as
  `--ctx-size 16384 --parallel 16`, `--ctx-size 8192 --parallel 8`, or an
  adaptive chunk/batch strategy that gives more output room without doubling
  completion count.

### 2026-06-26 Runtime Experiment Knobs

- Added env-configurable managed llama.cpp runtime settings:
  - `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE` controls the managed
    `llama-server --ctx-size` launch argument;
  - `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL` controls the managed
    `llama-server --parallel` launch argument.
- The parallel override is shared by the PDF OpenAI shim and the regular
  llama.cpp text translation scheduler:
  - PDF shim batch width follows the managed parallel setting instead of being
    fixed at 16;
  - pdf2zh `thread` count follows that width, still capped by the PDF worker
    ceiling;
  - regular text translation batches are capped to the same value.
- Default behavior remains the latest no-truncation baseline:
  `--ctx-size 8192 --parallel 16`, with the llama.cpp PDF body chunk target at
  about 56 prompt tokens.
- Validation passed:
  - `node --check scripts/check-pdf-translation-run.mjs`;
  - `pnpm typecheck`;
  - `cargo fmt -- --check`;
  - `cargo check`;
  - `cargo test managed_rwkv::lifecycle`;
  - `cargo test llama_cpp`;
  - `cargo test rosetta_jobs`;
  - `git diff --check` with LF/CRLF normalization warnings only.
- No new real PDF benchmark has been recorded for these knobs yet. The speed
  gate remains open until a fresh run passes the checker with
  `total <= 70600 ms`.

### 2026-06-26 Benchmark With 16384/16 Runtime

- The user reran the real forced 10-page benchmark with the first env override
  operating point:
  - job: `job-1782474427044-2604-17278v1`;
  - run: `run-pdf-1782480171604`;
  - runtime: `--ctx-size 16384 --parallel 16`;
  - effective slot context: `n_ctx=1024`;
  - target: `zh-CN`.
- Correctness did not pass the strict bottom-level completion gate:
  - pages requested: `10`;
  - pages translated: `10`;
  - pages failed: `0`;
  - shim `failedRequestCount`: `0`;
  - completion records: `347`;
  - `ok=false`: `1`;
  - empty output: `1`;
  - `truncated=true`: `1`;
  - `stop_type=limit`: `1`.
- The failed raw completion was recovered by the llama.cpp split backstop:
  - input chars: `167`;
  - raw content chars before parser rejection: `982`;
  - `prompt_n`: `40`;
  - `predicted_n`: `982`;
  - `tokens_cached`: `1023`;
  - `stop_type`: `limit`.
- Performance improved but still failed:
  - total: `140547 ms`;
  - target gate: `70600 ms`;
  - first page: `23315 ms`;
  - shim batches: `26`;
  - shim average/max: `4964 / 16111 ms`;
  - completion latency p50/p95/max: `3031 / 4447 / 13088 ms`;
  - throughput prompt/predicted: `72.24 / 93.87 tok/s`.
- Interpretation:
  - larger total context improved throughput versus `8192/16`
    (`189984 ms -> 140547 ms`);
  - `1024` effective tokens per slot still allowed one small input to run until
    the slot filled;
  - next runtime experiments should try about `2048` tokens per slot, such as
    `--ctx-size 32768 --parallel 16`, `--ctx-size 16384 --parallel 8`, or
    `--ctx-size 24576 --parallel 12`.

### 2026-06-26 Benchmark With 32768/16 Runtime

- The user reran the real forced 10-page benchmark with:
  - job: `job-1782474427044-2604-17278v1`;
  - run: `run-pdf-1782480966873`;
  - runtime: `--ctx-size 32768 --parallel 16`;
  - effective slot context: `n_ctx=2048`;
  - target: `zh-CN`.
- Strict bottom-level completion gate still failed:
  - pages requested: `10`;
  - pages translated: `10`;
  - pages failed: `0`;
  - shim `failedRequestCount`: `0`;
  - completion records: `367`;
  - `ok=false`: `1`;
  - empty output: `1`;
  - `truncated=true`: `0`;
  - `stop_type=limit`: `1`.
- The failed raw completion was again recovered by the llama.cpp split
  backstop:
  - input chars: `120`;
  - raw content chars before parser rejection: `1026`;
  - `prompt_n`: `51`;
  - `predicted_n`: `1024`;
  - `tokens_cached`: `1076`;
  - `stop_type`: `limit`;
  - `truncated`: `false`.
- A no-text structure check showed the failed output had only `26` unique
  characters, with the top repeated characters occurring `336 / 329 / 329`
  times, suggesting repetition runaway rather than insufficient context.
- Performance still failed and regressed versus `16384/16`:
  - total: `154087 ms`;
  - target gate: `70600 ms`;
  - first page: `11075 ms`;
  - shim batches: `26`;
  - shim average/max: `5461 / 27837 ms`;
  - completion latency p50/p95/max: `2886 / 4601 / 16875 ms`;
  - throughput prompt/predicted: `67.80 / 89.34 tok/s`.
- Interpretation:
  - increasing to 2048 effective tokens per slot removed `truncated=true`;
  - the remaining raw failure hit the request `n_predict=1024` generation cap,
    not the slot context;
  - next useful work should focus on llama.cpp generation-parameter hardening
    or a more reliable stop condition, not simply increasing total context.

### 2026-06-29 llama.cpp Generation Profile Pass

- Added a translation-focused llama.cpp `/completion` generation profile:
  - `temperature: 0.25`;
  - `top_k: 20`;
  - `top_p: 0.9`;
  - `min_p: 0.05`;
  - `repeat_penalty: 1.18`;
  - `repeat_last_n: 192`;
  - `penalize_nl: false`;
  - language-label stop strings for source/target role labels.
- Added benchmark-only env overrides:
  - `ROSETTA_LLAMA_CPP_TEMPERATURE`;
  - `ROSETTA_LLAMA_CPP_TOP_K`;
  - `ROSETTA_LLAMA_CPP_TOP_P`;
  - `ROSETTA_LLAMA_CPP_MIN_P`;
  - `ROSETTA_LLAMA_CPP_REPEAT_PENALTY`;
  - `ROSETTA_LLAMA_CPP_REPEAT_LAST_N`;
  - `ROSETTA_LLAMA_CPP_N_PREDICT`.
- Added unit coverage for default request fields and override parsers.
- Validation passed:
  - `cargo fmt -- --check`;
  - `cargo test llama_cpp`;
  - `node --check scripts/check-pdf-translation-run.mjs`.
- No real PDF benchmark has been recorded yet with the new generation profile.
  The next run should still use the strict checker and keep
  `stop_type=limit` / `truncated=true` as hard failures.
