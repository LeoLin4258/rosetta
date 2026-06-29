# 2026-06-26 PDF No-Truncation Performance Current Handover

## Purpose

This is the current handover for the PDF no-truncation / 50% speedup work.
It supersedes the earlier runtime handover when there is a conflict.

The next agent should start from this state:

- PDF completion correctness is now passing on the real 10-page benchmark.
- The original false-success bug is fixed.
- The speed target is still failing and is the next useful focus.

Primary plan:

```txt
docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
```

Historical runtime handover:

```txt
docs/engineering/plans/2026-06-26-pdf-runtime-benchmark-handover.md
```

## Product And Repo Constraints

Keep Rosetta narrow:

- local translation;
- privacy-sensitive documents;
- long text and document structure preservation;
- batch translation through a local model API.

Do not add cloud upload, login, sync, telemetry, chat, summarization,
rewriting, document Q&A, or generic AI assistant behavior.

The repo rule normally says not to run dev servers or production builds unless
the user explicitly asks. Previous runtime/dev verification for this task was
explicitly allowed by the user, and a Tauri dev runtime was already running
when the last benchmark was executed. Do not assume that permission continues
for unrelated future work.

## Current Worktree

Latest observed `git status --short`:

```txt
 M docs/engineering/pdf-pipeline.md
 M rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
 M rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
 M rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
 M rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
?? docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
?? docs/engineering/plans/2026-06-26-pdf-runtime-benchmark-handover.md
?? docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
?? rosetta-app/scripts/check-pdf-translation-run.mjs
```

Do not delete or revert the untracked plan/checker files. They are part of the
current task state.

## Implemented Behavior

### llama.cpp completion hardening

File:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

Implemented:

- parse `truncated` from llama.cpp `/completion` responses;
- parse `stop_type`;
- reject `truncated=true`;
- reject `stop_type="limit"`;
- reject empty content;
- unit tests cover truncated, limit, empty, non-JSON, and normal responses.

### managed llama.cpp runtime context

File:

```txt
rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
```

Implemented:

- managed Windows llama.cpp launches with:

  ```txt
  --ctx-size 8192 --parallel 16
  ```

- effective slot context observed in `runtime.log`:

  ```txt
  n_slots = 16
  n_ctx = 512
  ```

Latest observed runtime process when this handover was written:

```txt
process id: 100552
port: 54287
command: ... llama-server.exe ... --ctx-size 8192 --gpu-layers auto --parallel 16
```

The model filename still contains `ctx4096`; the active launch argument is what
matters here.

### PDF shim batch width and chunk profile

File:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Implemented:

- generic PDF OpenAI-shim providers still use max batch width `8`;
- llama.cpp PDF shim uses batch width `16`;
- llama.cpp PDF shim uses a smaller provider-specific chunk profile:
  - body/caption target prompt budget: about `56` estimated tokens;
  - reference target prompt budget: about `42` estimated tokens;
  - generic providers keep the older larger budgets.

Reason:

- `--ctx-size 8192 --parallel 16` gives about `512` tokens per slot;
- even input chunks around 272-424 chars previously hit output-room limits;
- smaller prompt chunks leave more room for generated Chinese output.

### truncation retry/split backstop

File:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Implemented:

- if a llama.cpp shim batch fails, retry the affected texts through a smaller
  split profile;
- if that still fails, try a final serial split with an even smaller budget;
- only unrecovered failures are surfaced to pdf2zh and counted in
  `failedRequestCount`.

Important observation from the latest real benchmark:

- the proactive llama.cpp chunk profile eliminated truncation;
- no `batch failed`, `retry split`, or `final serial` lines were observed in
  `rosetta.log` for `run-pdf-1782477804133`;
- so the backstop exists as a guardrail, but the latest run did not need it.

### page/run failure propagation

File:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
```

Implemented:

- after `invoke_pdf2zh()` returns successfully, `translate_pdf_pages_inner()`
  now checks final shim metrics;
- if `failedRequestCount > 0`, the chunk is treated as failed;
- any page artifacts from that invocation are cleared;
- affected pages and the run are marked failed instead of translated/completed;
- `generate_rosetta_translated_pdf()` rejects the same condition before marking
  a whole-PDF translation ready.

This fixes the critical false-success bug:

```txt
Before: bottom-level completion failed/truncated, but page/run could still say completed.
Now: unrecovered bottom-level failures prevent successful page/run finalization.
```

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

It fails on:

- missing requested translated pages;
- failed pages;
- missing per-completion debug records, unless explicitly allowed;
- `ok=false`;
- empty outputs;
- `truncated=true`;
- `stop_type=limit`;
- optional total-duration threshold breach.

Privacy note: it does not print source text, translated text, prompts, or raw
model responses. The underlying `rwkv-io-debug.jsonl` does contain full local
source/translation data and must never be committed.

## Validation Already Passed

These commands passed after the failure-propagation and split-backstop pass:

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

`git diff --check` also exited successfully. It only printed LF/CRLF warnings.

## Latest Real Benchmark

The latest real benchmark was a forced 10-page retranslation.

```txt
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782477804133
file: 2604.17278v1.pdf
target: zh-CN
runtime args: --ctx-size 8192 --parallel 16
effective slot n_ctx: 512
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782477804133 --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782477804133.json"
```

Checker summary:

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
result: FAIL, because total 189984 ms exceeds 70600 ms
```

Machine-readable checker output:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782477804133.json
```

Profile:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-translation-profile-run-pdf-1782477804133.json
```

## Interpretation

Correctness is fixed for the latest real run:

```txt
pages translated: 10/10
failed pages: 0
completion ok=false: 0
empty outputs: 0
truncated=true: 0
stop_type=limit: 0
shim failedRequestCount: 0
```

Performance is not fixed:

```txt
target gate: 70600 ms
latest total: 189984 ms
```

The smaller llama.cpp chunks removed truncation, but they increased completion
count:

```txt
previous full forced run: 188 completion records, but 4 failed/truncated
latest no-truncation run: 343 completion records, 0 failed/truncated
```

The next useful work is throughput tuning, not more correctness hardening.
Do not keep making chunks smaller unless a new benchmark shows truncation has
returned.

## Recommended Next Steps

1. Use the runtime experiment knobs.

   Env-configurable overrides have been added for managed llama.cpp
   `ctx-size` and `parallel` so future benchmark passes do not require source
   edits for every configuration:

   ```txt
   ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=<tokens>
   ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=<slots>
   ```

   The `PARALLEL` override also caps the PDF shim batch/thread width and the
   regular llama.cpp text scheduler, keeping server and client concurrency
   aligned during experiments. A managed runtime restart is still required for
   a new `ctx-size` / `parallel` launch setting to take effect.

2. Test larger per-slot context.

   Strong candidates:

   ```txt
   A: --ctx-size 16384 --parallel 16  # about 1024 tokens per slot
   B: --ctx-size 8192  --parallel 8   # about 1024 tokens per slot, lower concurrency
   C: --ctx-size 12288 --parallel 12  # about 1024 tokens per slot, middle point
   ```

   Hypothesis: more per-slot output room may allow larger PDF chunks, reducing
   completion count while preserving no-truncation.

3. Make chunk budget adaptive after context tuning.

   Current llama.cpp body target is about `56` prompt tokens. If per-slot
   context rises to about `1024`, try larger targets such as `80`, `100`, or
   `120`, and verify no truncation returns.

4. Keep the checker as the acceptance gate.

   Every real benchmark must satisfy:

   ```txt
   pages requested == pages translated
   pages failed == 0
   completion ok=false == 0
   empty output == 0
   truncated=true == 0
   stop_type=limit == 0
   total <= 70600 ms
   ```

5. Record exact numbers.

   Update this document or the primary plan after every real run with:

   ```txt
   runtime args:
   effective slot n_ctx:
   chunk target:
   runId:
   total ms:
   first page ms:
   shim batches:
   completion records:
   failed/truncated/limit/empty:
   throughput prompt/predicted:
   conclusion:
   ```

## Useful Commands

Check current managed llama.cpp process:

```powershell
Get-CimInstance Win32_Process -Filter "name='llama-server.exe'" |
  Select-Object ProcessId,CommandLine |
  Format-List
```

Run checker on the latest no-truncation benchmark:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782477804133 --max-total-ms 70600
```

Open latest benchmark JSON:

```powershell
Get-Content -LiteralPath "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782477804133.json"
```

Inspect latest run state:

```powershell
Get-Content -LiteralPath "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\pdf_run.zh-CN.json"
```

Set an operating point for the next managed runtime start from PowerShell:

```powershell
$env:ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE = "16384"
$env:ROSETTA_MANAGED_LLAMA_CPP_PARALLEL = "16"
```

Alternative lower-concurrency point:

```powershell
$env:ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE = "8192"
$env:ROSETTA_MANAGED_LLAMA_CPP_PARALLEL = "8"
```

After setting these, restart the managed RWKV runtime before benchmarking and
confirm the active `llama-server.exe` command line plus effective `n_ctx` in
`runtime.log`.

## Continuation Notes

### 2026-06-26 Runtime Experiment Knobs

Implemented after this handover was created:

- managed llama.cpp launch settings now read:
  - `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE`;
  - `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL`;
- default launch behavior remains `--ctx-size 8192 --parallel 16`;
- the parallel override also controls:
  - PDF OpenAI-shim llama.cpp batch width;
  - pdf2zh `thread` count, still capped by the PDF worker ceiling;
  - regular llama.cpp text translation batch planning;
- lifecycle tests now cover a `--ctx-size 16384 --parallel 8` experiment
  command line.

Validation passed:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo fmt -- --check

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node --check scripts/check-pdf-translation-run.mjs
pnpm typecheck

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo check
cargo test managed_rwkv::lifecycle
cargo test llama_cpp
cargo test rosetta_jobs

cd C:\Users\Leo\Documents\GitHub\rosetta
git diff --check
```

`git diff --check` printed LF/CRLF normalization warnings only.

No new real PDF benchmark has been run after adding the knobs, so the speed
gate is still not accepted.

### 2026-06-26 Benchmark With 16384/16 Runtime

The user reran a forced 10-page PDF translation after setting the first
runtime experiment point:

```txt
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782480171604
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024
target: zh-CN
mode: retranslate-all
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782480171604 --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782480171604.json"
```

Result:

```txt
status: completed
pages requested: 10
pages translated: 10
pages failed: 0
total: 140547 ms
first page: 23315 ms
shim batches: 26
shim failed batches: 0
shim average/max: 4964 / 16111 ms
completion records: 347
ok completions: 346
failed completions: 1
empty output: 1
truncated=true: 1
stop_type=limit: 1
completion latency p50/p95/max: 3031 / 4447 / 13088 ms
throughput prompt/predicted: 72.24 / 93.87 tok/s
```

Failure details without text content:

```txt
input chars: 167
output chars accepted by provider: 0
raw content chars before rejection: 982
prompt_n: 40
predicted_n: 982
tokens_cached: 1023
stop_type: limit
truncated: true
slot: 9
```

Interpretation:

- Throughput improved materially over the previous `8192/16` run:
  `189984 ms -> 140547 ms`.
- The run still fails the `70600 ms` speed gate.
- One raw llama.cpp completion reached the 1024-token slot limit. The shim
  split backstop recovered it (`failedRequestCount=0`, pages completed), but
  the acceptance checker still fails because bottom-level truncation occurred.
- This failure was not caused by a huge source chunk; it was a 167-character
  input that generated until the slot filled. Further progress likely needs a
  larger per-slot context, a safer proactive split for risky small chunks, or a
  model-generation stopping improvement.

Strong next runtime experiments:

```txt
A: --ctx-size 32768 --parallel 16  # about 2048 tokens per slot, same concurrency
B: --ctx-size 16384 --parallel 8   # about 2048 tokens per slot, lower concurrency
C: --ctx-size 24576 --parallel 12  # about 2048 tokens per slot, middle point
```

Do not mark accepted until a new run has:

```txt
completion ok=false == 0
empty output == 0
truncated=true == 0
stop_type=limit == 0
total <= 70600 ms
```

### 2026-06-26 Benchmark With 32768/16 Runtime

The user reran the forced 10-page PDF benchmark with a larger per-slot context:

```txt
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782480966873
runtime args: --ctx-size 32768 --parallel 16
effective slot n_ctx: 2048
target: zh-CN
mode: retranslate-all
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id run-pdf-1782480966873 --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782480966873.json"
```

Result:

```txt
status: completed
pages requested: 10
pages translated: 10
pages failed: 0
total: 154087 ms
first page: 11075 ms
shim batches: 26
shim failed batches: 0
shim average/max: 5461 / 27837 ms
completion records: 367
ok completions: 366
failed completions: 1
empty output: 1
truncated=true: 0
stop_type=limit: 1
completion latency p50/p95/max: 2886 / 4601 / 16875 ms
throughput prompt/predicted: 67.80 / 89.34 tok/s
```

Failure details without text content:

```txt
input chars: 120
output chars accepted by provider: 0
raw content chars before rejection: 1026
prompt_n: 51
predicted_n: 1024
tokens_cached: 1076
stop_type: limit
truncated: false
slot: 13
```

Additional structure check:

```txt
raw content chars: 1026
unique chars: 26
top repeated char counts: 336 / 329 / 329
language-label markers observed: 0
```

Interpretation:

- `32768/16` removed `truncated=true`, but did not remove `stop_type=limit`.
- The failure hit the request `n_predict=1024` generation cap, not the
  2048-token slot context.
- The failed output shape looks like a repetition runaway on a small
  120-character input, then the split backstop recovered it before page
  finalization (`failedRequestCount=0`).
- Runtime got slower than `16384/16`:
  `140547 ms -> 154087 ms`.
- The next useful work is likely generation-parameter hardening for llama.cpp
  (`/completion` sampling/repetition controls or a more reliable stop
  condition), not simply increasing total context again.

## Do Not Claim Completion Yet

The task is not accepted yet because the speed gate still fails:

```txt
latest total: 189984 ms
required: <= 70600 ms
```

The next agent should treat this as a performance-tuning handoff from a now
truthful/correct PDF translation baseline.
