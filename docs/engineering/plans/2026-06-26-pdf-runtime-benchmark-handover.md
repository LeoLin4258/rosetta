# 2026-06-26 PDF Runtime Benchmark Handover

## Purpose

This document captures the current implementation and runtime state for the PDF
translation no-truncation / 50% speedup work. It is intended for the next agent
to continue without re-discovering the same benchmark evidence.

Primary task document:

```txt
docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
```

Latest explicit user request before this handover:

```txt
continue, restart the runtime, run one real PDF benchmark, and dev/runtime
verification is allowed
```

The normal repo rule says not to run dev servers or production builds unless
asked. For this task, the user explicitly allowed runtime/dev verification.

## Current Worktree

Latest observed `git status --short`:

```txt
 M docs/engineering/pdf-pipeline.md
 M rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
 M rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
 M rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
?? docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
?? docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
?? rosetta-app/scripts/check-pdf-translation-run.mjs
```

Do not delete or revert the untracked plan document. It is the user-provided
implementation target for this work.

## Runtime State

Latest observed managed llama.cpp process:

```txt
name: llama-server.exe
process id: 122560
port: 65308
```

The command line includes:

```txt
--ctx-size 8192 --gpu-layers auto --parallel 16
```

The model filename still contains `ctx4096`, but the active server launch arg is
`--ctx-size 8192`.

## Implemented So Far

### llama.cpp response hardening

File:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

Changes:

- added `DEFAULT_SERVER_CTX_SIZE: usize = 8192`;
- parse `truncated` from llama.cpp completion responses;
- parse `stop_type` from llama.cpp completion responses;
- reject `truncated=true` as a provider failure;
- reject `stop_type="limit"` as a provider failure;
- added unit tests that verify truncated / limit completions are rejected.

### managed runtime launch args

File:

```txt
rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
```

Changes:

- changed managed llama.cpp runtime launch from:

```txt
--ctx-size 4096 --parallel 16
```

to:

```txt
--ctx-size 8192 --parallel 16
```

- updated the lifecycle test expectation from `4096` to `8192`.

### PDF shim batch width

File:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Changes:

- kept generic PDF shim providers at batch width `8`;
- changed llama.cpp PDF shim batch width to `16`.

Intent: preserve the high-concurrency Vulkan path while increasing per-slot
context from about 256 tokens to about 512 tokens.

### benchmark checker

File:

```txt
rosetta-app/scripts/check-pdf-translation-run.mjs
```

The checker reads:

- PDF translation profile;
- PDF page state;
- PDF timeline;
- `rwkv-io-debug.jsonl`.

It fails non-zero on:

- missing debug records, unless explicitly allowed;
- untranslated pages;
- failed pages;
- `ok=false` completion records;
- empty outputs;
- `truncated=true`;
- `stop_type=limit`;
- optional `--max-total-ms` threshold breach.

Privacy note: it does not print source text, translated text, prompts, or raw
model responses.

### docs

Updated:

```txt
docs/engineering/pdf-pipeline.md
docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
```

Added:

```txt
docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
```

## Validation Already Run

All of these passed before the latest real benchmark:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node --check scripts/check-pdf-translation-run.mjs
pnpm typecheck

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo fmt -- --check
cargo check
cargo test rosetta_jobs
cargo test llama_cpp
cargo test managed_rwkv::lifecycle

cd C:\Users\Leo\Documents\GitHub\rosetta
git diff --check
```

`git diff --check` only emitted LF/CRLF warnings, not whitespace errors.

## Real PDF Benchmark Evidence

Benchmark job:

```txt
jobId: job-1782474427044-2604-17278v1
target: zh-CN
PDF: 2604.17278v1.pdf
```

### Partial / cancelled run

```txt
runId: run-pdf-1782474433671
status: cancelled
requested pages: 10
translated pages: 2
duration: 47060 ms
completion records: 38
truncated: 0
stop_type=limit: 0
empty output: 0
```

This is not a valid full benchmark.

### Remaining-pages run

```txt
runId: run-pdf-1782474745057
status: completed
requested pages: 8
page state after run: 10/10 translated, 0 failed
total: 160493 ms
completion records: 109 ok
truncated: 0
stop_type=limit: 0
empty output: 0
```

This run failed the `70600 ms` performance gate.

Checker output:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782474745057.json
```

### Full forced retranslation run

This is the most important run.

```txt
runId: run-pdf-1782475009793
status: completed
pages requested: 10
pages translated: 10
pages failed: 0
total: 299176 ms
first page: 10305 ms
shim batches: 17
shim average: 16419 ms
shim max: 25541 ms
completion records: 188
ok completions: 184
failed completions: 4
truncated=true: 4
stop_type=limit: 4
empty output: 4
completion latency p50/p95/max: 11966 / 19718 / 24793 ms
throughput prompt: 12.53 tok/s
throughput predicted: 66.47 tok/s
```

Profile:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-translation-profile-run-pdf-1782475009793.json
```

Checker output:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782475009793.json
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782475009793 --max-total-ms 70600
```

Checker result: failed.

Failure reasons:

```txt
shim failedRequestCount = 4
4 ok=false completion records
4 empty output
4 truncated=true
4 stop_type=limit
total exceeds 70600 ms
```

## Critical Finding

Do not rerun the same benchmark before fixing correctness propagation.

The full forced retranslation run had lower-level completion failures:

```txt
failed completions: 4
truncated=true: 4
stop_type=limit: 4
empty output: 4
```

But the PDF run still ended as:

```txt
status: completed
pages translated: 10
pages failed: 0
```

That means page/job state can currently report success even when required
translation completions failed. This is the next bug to fix.

The acceptance invariant should be:

```txt
A PDF page/run must not be marked successful if any required lower-level
translation completion failed, returned empty output, truncated=true, or
stop_type=limit.
```

## Failed Completion Metadata

The failed completion metadata for `run-pdf-1782475009793` was parsed without
printing source text, translation text, prompts, or raw responses:

```json
[
  {
    "inputChars": 375,
    "outputChars": 0,
    "truncated": true,
    "stop": "limit",
    "prompt_n": 4,
    "predicted_n": 395,
    "tokens_cached": 511,
    "id_slot": 13
  },
  {
    "inputChars": 272,
    "outputChars": 0,
    "truncated": true,
    "stop": "limit",
    "prompt_n": 4,
    "predicted_n": 420,
    "tokens_cached": 511,
    "id_slot": 10
  },
  {
    "inputChars": 272,
    "outputChars": 0,
    "truncated": true,
    "stop": "limit",
    "prompt_n": 4,
    "predicted_n": 420,
    "tokens_cached": 511,
    "id_slot": 10
  },
  {
    "inputChars": 424,
    "outputChars": 0,
    "truncated": true,
    "stop": "limit",
    "prompt_n": 4,
    "predicted_n": 371,
    "tokens_cached": 511,
    "id_slot": 12
  }
]
```

Interpretation:

- These were not huge input chunks.
- Output generation hit the slot context limit.
- With `--ctx-size 8192 --parallel 16`, effective slot context appears to be
  about 512 tokens.
- Some chunks needed about 371-420 predicted tokens, so a 512-token slot still
  leaves too little room.

## Performance Finding

The latest full forced run is much slower than the target:

```txt
actual total: 299176 ms
target gate: 70600 ms
```

Likely contributing factors:

- `8192 / 16` still gives only about 512 tokens per slot;
- larger ctx may have slowed prompt/cache behavior;
- batch width 16 created long waves and retries;
- pdf2zh retried failed requests and eventually committed pages;
- lower-level failures remained in diagnostics even though page state said
  success.

## Files To Inspect Next

Start with:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Look for:

- `translate_chunks()`;
- `llama_cpp_batch_processor()`;
- `split_pdf_shim_text()`;
- provider failure handling;
- batch size and chunk budget constants.

Then inspect:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/rosetta_pdf2zh_worker.py
```

Look for:

- how translate failures are counted;
- how pdf2zh retries failed requests;
- whether a page can still be emitted after one or more failed translation
  requests.

Then inspect:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
```

Look around `translate_pdf_pages_inner()`. Current suspicion: once
`invoke_pdf2zh` returns `Ok(output)`, the job path treats the chunk as complete
even when:

```txt
output.rwkv_metrics.failed_request_count > 0
```

Also inspect:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs
```

Look at:

- `Pdf2zhInvokeOutput`;
- worker result parsing;
- worker error propagation.

## Recommended Next Steps

1. Fix correctness propagation first.

At minimum, a pdf2zh invocation with non-zero `failed_request_count` should not
be treated as a clean success unless those failures are known to have been fully
retried and excluded from committed output.

The safer behavior is:

- if any required provider completion fails, retry with a safer split;
- if retry still fails, return an HTTP error to pdf2zh;
- ensure the Python worker reports the page/chunk as failed;
- ensure Rust job state does not finalize the page/run as successful.

2. Add a truncation recovery path.

Candidate approaches:

- in `llama_cpp_chat::translate_batch`, split and retry a single failed source
  when the failure is caused by `truncated=true` or `stop_type=limit`;
- or do this inside `managed_pdf2zh/openai_shim.rs`, where PDF-specific chunking
  context already exists.

The current source-token splitter is too optimistic for reference/citation-heavy
chunks. Inputs with only 272-424 chars still produced 371-420 predicted tokens
before hitting the limit.

3. Revisit runtime ctx/parallel.

Current setting:

```txt
--ctx-size 8192 --parallel 16
```

Experiment candidates:

```txt
A: --ctx-size 16384 --parallel 16
B: --ctx-size 8192  --parallel 8
C: --ctx-size 12288 --parallel 12
```

Hypothesis:

- `16384 / 16` or `8192 / 8` gives about 1024 tokens per slot;
- this may eliminate `stop_type=limit`;
- performance must be measured again because `8192 / 16` took 299 seconds.

Consider adding env-configurable ctx/parallel overrides for local benchmark
experiments so the next pass does not require source edits for every setting.

4. Validate.

Run:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node --check scripts/check-pdf-translation-run.mjs
pnpm typecheck

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo fmt -- --check
cargo check
cargo test rosetta_jobs
cargo test llama_cpp
cargo test managed_rwkv::lifecycle
```

5. Rerun real PDF benchmark.

Use the checker:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id <new-run-id> --max-total-ms 70600
```

Acceptance for a valid run:

```txt
pages requested == pages translated
pages failed == 0
completion ok=false == 0
empty output == 0
truncated=true == 0
stop_type=limit == 0
total <= 70600 ms
```

If `total <= 70600 ms` is not met, document exact numbers and do not mark the
plan complete.

## Useful Commands

Check current managed runtime command:

```powershell
Get-CimInstance Win32_Process -Filter "name='llama-server.exe'" |
  Select-Object ProcessId,CommandLine |
  Format-List
```

Run checker on the failed full benchmark:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782475009793 --max-total-ms 70600
```

Parse failed completions without printing source or translated text:

```powershell
@'
const fs = require('fs');

const jobId = 'job-1782474427044-2604-17278v1';
const runId = 'run-pdf-1782475009793';
const appData = process.env.APPDATA;

const profilePath = `${appData}\\com.rosetta.desktop\\jobs\\${jobId}\\diagnostics\\pdf-translation-profile-${runId}.json`;
const logPath = `${appData}\\com.rosetta.desktop\\logs\\rwkv-io-debug.jsonl`;

const profile = JSON.parse(fs.readFileSync(profilePath, 'utf8'));
const start = Number(profile.startedAt) - 5000;
const end = Number(profile.endedAt) + 5000;

const rows = [];
for (const line of fs.readFileSync(logPath, 'utf8').trim().split(/\r?\n/).filter(Boolean)) {
  const r = JSON.parse(line);
  if (r.context !== `pdf-job:${jobId}`) continue;
  if (r.provider !== 'llama-cpp-chat-completions') continue;
  if ((r.timestampMs || 0) < start || (r.timestampMs || 0) > end) continue;

  let raw = {};
  try {
    raw = JSON.parse(r.rawResponse || '{}');
  } catch {}

  const input = (r.inputs || []).join('\n');
  const output = (r.outputs || []).join('\n');

  rows.push({
    ts: r.timestampMs,
    ok: r.ok,
    inputChars: input.length,
    outputChars: output.trim().length,
    truncated: !!raw.truncated,
    stop: raw.stop_type,
    prompt_n: raw.timings?.prompt_n ?? raw.tokens_evaluated ?? 0,
    predicted_n: raw.timings?.predicted_n ?? raw.tokens_predicted ?? 0,
    tokens_cached: raw.tokens_cached,
    id_slot: raw.id_slot,
    status: r.statusCode,
    error: r.error
  });
}

console.log(JSON.stringify(
  rows.filter(r => !r.ok || r.truncated || r.stop === 'limit' || r.outputChars === 0),
  null,
  2
));
'@ | node -
```

## Do Not Claim Completion Yet

The work is not accepted yet. The harness caught a real failure:

- runtime args are active;
- response parser rejects truncation;
- benchmark checker works;
- but full benchmark still had 4 truncated/limit completions;
- run/page state still reported success despite lower-level failures;
- total runtime was 299 seconds, far above the 70.6 second target.

The next implementation pass should make the job state truthful first, then
retry/split or change ctx/parallel to remove truncation, then rerun the real PDF
benchmark.

## 2026-06-26 Continuation Notes

Implemented after this handover:

- page/run failure propagation:
  - `translate_pdf_pages_inner()` now treats final shim metrics with
    `failedRequestCount > 0` as a chunk failure;
  - any page artifacts committed by that invocation are cleared before the
    affected pages are marked `failed`;
  - `generate_rosetta_translated_pdf()` now rejects the same condition before
    marking a whole-PDF translation ready.
- llama.cpp PDF chunking/backstop:
  - llama.cpp uses a provider-specific PDF chunk profile with smaller prompt
    budgets than the generic shim providers;
  - failed llama.cpp batches retry texts through smaller split chunks, with a
    final serial split retry before surfacing an error.
- validation passed:
  - `node --check scripts/check-pdf-translation-run.mjs`;
  - `pnpm typecheck`;
  - `cargo fmt -- --check`;
  - `cargo check`;
  - `cargo test rosetta_jobs`;
  - `cargo test llama_cpp`;
  - `cargo test managed_rwkv::lifecycle`.

Next action is now safe: rerun a real forced 10-page PDF benchmark and verify
the resulting run with the checker. Do not mark the no-truncation/speedup plan
complete unless the checker passes both correctness and the `70600 ms`
performance gate.

## 2026-06-26 Benchmark After Backstop

The real forced benchmark was rerun after the failure-propagation and split
backstop pass.

```txt
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782477804133
target: zh-CN
runtime args: --ctx-size 8192 --parallel 16
effective slot n_ctx: 512
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782477804133 --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782477804133.json"
```

Result:

```txt
status: completed
pages requested: 10
pages translated: 10
pages failed: 0
total: 189984 ms
first page: 14596 ms
shim batches: 26
shim average/max: 6561 / 8850 ms
completion records: 343
ok completions: 343
failed completions: 0
empty output: 0
truncated=true: 0
stop_type=limit: 0
completion latency p50/p95/max: 4396 / 6857 / 7582 ms
throughput prompt/predicted: 53.01 / 63.14 tok/s
```

This run passes the no-truncation correctness gate but fails the speed gate:

```txt
actual total: 189984 ms
target gate: 70600 ms
```

The split retry backstop did not appear to trigger in `rosetta.log` for this
run. The proactive llama.cpp chunk profile removed truncation, but it increased
completion count from the previous full forced run's `188` records to `343`
records. The next pass should focus on throughput, likely by comparing
`ctx-size` / `parallel` operating points or by making chunk size adaptive enough
to keep no-truncation while reducing completion count.
