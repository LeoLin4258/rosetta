# 2026-06-29 PDF llama.cpp Generation Handover

## Purpose

This is the current handover for the PDF no-truncation / 50% speedup work.
It supersedes the earlier runtime handovers when there is a conflict:

```txt
docs/engineering/plans/2026-06-26-pdf-no-truncation-performance-current-handover.md
docs/engineering/plans/2026-06-26-pdf-runtime-benchmark-handover.md
```

The important update is that pure runtime context tuning has reached a useful
limit. The latest benchmark shows the remaining raw llama.cpp failure is now a
small-input repetition runaway that hits the request `n_predict=1024` cap, not
a per-slot context shortage.

The next agent should focus on llama.cpp `/completion` generation-parameter
hardening and then rerun the same real PDF benchmark through the checker.

## Product And Repo Constraints

Keep Rosetta narrow:

- local translation;
- privacy-sensitive documents;
- long text and document structure preservation;
- batch translation through a local model API.

Do not add cloud upload, login, sync, telemetry, chat, summarization,
rewriting, document Q&A, or generic AI assistant behavior.

Do not run dev servers or production builds unless the user explicitly asks.
Runtime PDF benchmark verification is allowed only when the user explicitly
asks for it in the active conversation. The user has been running the real PDF
benchmarks manually after setting env vars.

## Current Worktree Snapshot

Latest observed `git status --short` included:

```txt
 M docs/engineering/pdf-pipeline.md
 M rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
 M rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
 M rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
 M rosetta-app/src-tauri/src/rwkv_api.rs
 M rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
?? .claude/settings.local.json
?? docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
?? docs/engineering/plans/2026-06-26-pdf-no-truncation-performance-current-handover.md
?? docs/engineering/plans/2026-06-26-pdf-runtime-benchmark-handover.md
?? docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
?? rosetta-app/scripts/check-pdf-translation-run.mjs
```

Do not delete or revert the untracked plan/checker files. They are part of the
current task state. Treat `.claude/settings.local.json` as unrelated local/user
state unless the user explicitly asks about it.

## Implemented So Far

### llama.cpp response correctness

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
- tests cover truncated, limit, empty, non-JSON, normal responses, and runtime
  env override parsing.

### runtime experiment knobs

Files:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
rosetta-app/src-tauri/src/rwkv_api.rs
```

Implemented:

- default managed llama.cpp launch is now:

  ```txt
  --ctx-size 16384 --parallel 16
  ```

- local benchmark env overrides:

  ```txt
  ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=<tokens>
  ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=<slots>
  ```

- the `PARALLEL` override also caps:
  - PDF OpenAI-shim llama.cpp batch width;
  - pdf2zh `thread` count, still capped by the PDF worker ceiling;
  - regular llama.cpp text translation batch planning.

Reason: a benchmark should not accidentally run server `--parallel 8` while the
client still sends 16 concurrent requests.

### PDF shim no-truncation backstop

File:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Implemented:

- generic providers still use larger PDF chunk budgets;
- llama.cpp PDF shim uses smaller proactive chunks:
  - body/caption target: about `56` prompt tokens;
  - reference target: about `42` prompt tokens;
- failed llama.cpp batches retry the affected texts through smaller split
  chunks;
- a final serial split retry uses an even smaller prompt budget;
- only unrecovered failures increment shim-level `failedRequestCount`.

Important: latest failed raw completions were recovered by this backstop before
page finalization. The checker still fails because the acceptance gate rejects
any raw bottom-level `stop_type=limit`/truncation, even if recovered.

### page/run failure propagation

File:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/mod.rs
```

Implemented:

- after `invoke_pdf2zh()` returns successfully, final shim metrics are checked;
- if `failedRequestCount > 0`, the chunk is treated as failed;
- any page artifacts from that invocation are cleared;
- affected pages and the run are marked failed instead of translated/completed;
- whole-PDF generation rejects the same condition before marking translation
  ready.

This fixed the earlier false-success bug. Latest raw failures had
`failedRequestCount=0` because the split backstop recovered them.

### llama.cpp generation profile

File:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

Implemented:

- `/completion` requests now send a translation-focused generation profile:
  - `temperature: 0.25`;
  - `top_k: 20`;
  - `top_p: 0.9`;
  - `min_p: 0.05`;
  - `repeat_penalty: 1.18`;
  - `repeat_last_n: 192`;
  - `penalize_nl: false`;
  - language-label stop strings such as `\nEnglish:` / `\nChinese:`;
- benchmark env overrides:
  - `ROSETTA_LLAMA_CPP_TEMPERATURE`;
  - `ROSETTA_LLAMA_CPP_TOP_K`;
  - `ROSETTA_LLAMA_CPP_TOP_P`;
  - `ROSETTA_LLAMA_CPP_MIN_P`;
  - `ROSETTA_LLAMA_CPP_REPEAT_PENALTY`;
  - `ROSETTA_LLAMA_CPP_REPEAT_LAST_N`;
  - `ROSETTA_LLAMA_CPP_N_PREDICT`;
- tests cover default request fields and env override parsers.

Reason: the latest failure shape looked like a small-input repetition runaway
that reached the request `n_predict=1024` cap. The profile tries to prevent the
runaway itself; it does not relax the strict checker, and `stop_type=limit`
remains a provider failure.

### adaptive llama.cpp PDF chunk profile

File:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
```

Implemented in the current continuation pass:

- default `16384/16` uses the current strict-correct adaptive llama.cpp PDF
  profile:
  - body target/hard: `72/88`;
  - caption target/hard: `72/88`;
  - reference target/hard: `42/56`;
- if benchmark env overrides lower the effective slot context below `1024`, the
  shim falls back to the conservative body/caption `56/72` and reference
  `42/56` profile;
- very short `[N] ...` reference fragments are preserved by deterministic
  passthrough instead of being sent to llama.cpp;
- split retry backstops stay unchanged at `36`, then `24`;
- strict llama.cpp parser behavior is unchanged;
- shim debug logs now include the effective chunk profile at spawn time;
- local benchmark sweeps can override the PDF shim budgets with:
  - `ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET`;
  - `ROSETTA_PDF_SHIM_LLAMA_BODY_HARD`;
  - `ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET`;
  - `ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD`;
  - `ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET`;
  - `ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD`.

Reason: the strict-correct `16384/16` run had `343` raw completions, p50 prompt
size about `30` tokens and p95 about `49` tokens. The previous normal chunk
profile was still sized for a 512-token slot. A first wider `112/144` body
experiment proved completion count can drop, but it reintroduced raw
`truncated=true` / `stop_type=limit`; the committed default was pulled back to
a safer middle profile. A follow-up middle profile with references at `56/72`
still produced two raw reference-list failures, so references are now back at
the conservative budget and tiny reference fragments are passthrough.

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
model responses. The underlying `rwkv-io-debug.jsonl` contains full local
source/translation data and must never be committed.

## Validation Already Passed

After the runtime knob pass, these passed:

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

After the latest benchmark documentation updates, `git diff --check` was run
again and still printed LF/CRLF warnings only.

After the generation-profile code pass, these passed:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo fmt -- --check
cargo check
cargo test llama_cpp
cargo test managed_rwkv::lifecycle
cargo test rosetta_jobs

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node --check scripts/check-pdf-translation-run.mjs
.\node_modules\.bin\tsc.CMD --noEmit
```

`pnpm typecheck` was also attempted, but the pnpm wrapper stopped at dependency
build approval (`ERR_PNPM_IGNORED_BUILDS`) before running TypeScript. The local
`tsc --noEmit` command above is the same typecheck script without changing
machine-level pnpm approvals.

After the adaptive PDF shim chunk-profile code pass, these passed:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node --check scripts/check-pdf-translation-run.mjs
.\node_modules\.bin\tsc.CMD --noEmit

cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app\src-tauri
cargo fmt -- --check
cargo check
cargo test managed_pdf2zh::openai_shim
cargo test llama_cpp
cargo test managed_rwkv::lifecycle
cargo test rosetta_jobs

cd C:\Users\Leo\Documents\GitHub\rosetta
git diff --check
```

`pnpm typecheck` was attempted again, but pnpm still stopped at dependency
build approval (`ERR_PNPM_IGNORED_BUILDS`) before TypeScript. Direct
`tsc --noEmit` passed. `git diff --check` printed LF/CRLF normalization
warnings only and no whitespace errors.

## Real Benchmark History

All benchmark runs below used:

```txt
jobId: job-1782474427044-2604-17278v1
file: 2604.17278v1.pdf
pages: 1-10
target: zh-CN
mode: retranslate-all for forced runs
speed target: <= 70600 ms
```

### 8192/16 correctness baseline

```txt
runId: run-pdf-1782477804133
runtime args: --ctx-size 8192 --parallel 16
effective slot n_ctx: 512
total: 189984 ms
first page: 14596 ms
shim batches: 26
completion records: 343
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
throughput prompt/predicted: 53.01 / 63.14 tok/s
result: correctness passed, speed failed
```

This is the clean no-raw-failure baseline, but it is far too slow.

### 16384/16 runtime experiment

```txt
runId: run-pdf-1782480171604
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024
total: 140547 ms
first page: 23315 ms
shim batches: 26
completion records: 347
ok=false / empty / truncated / limit: 1 / 1 / 1 / 1
throughput prompt/predicted: 72.24 / 93.87 tok/s
result: speed improved, strict correctness failed
```

Failure metadata without text content:

```txt
input chars: 167
raw content chars before rejection: 982
prompt_n: 40
predicted_n: 982
tokens_cached: 1023
stop_type: limit
truncated: true
slot: 9
```

Interpretation:

- faster than 8192/16;
- one small input still ran until the 1024-token slot filled;
- split backstop recovered the page-level output.

### 32768/16 runtime experiment

```txt
runId: run-pdf-1782480966873
runtime args: --ctx-size 32768 --parallel 16
effective slot n_ctx: 2048
total: 154087 ms
first page: 11075 ms
shim batches: 26
completion records: 367
ok=false / empty / truncated / limit: 1 / 1 / 0 / 1
throughput prompt/predicted: 67.80 / 89.34 tok/s
result: truncated=true removed, stop_type=limit remains, speed failed
```

Failure metadata without text content:

```txt
input chars: 120
raw content chars before rejection: 1026
prompt_n: 51
predicted_n: 1024
tokens_cached: 1076
stop_type: limit
truncated: false
slot: 13
```

Additional no-text structure check:

```txt
raw content chars: 1026
unique chars: 26
top repeated char counts: 336 / 329 / 329
language-label markers observed: 0
```

Interpretation:

- `32768/16` removed `truncated=true`;
- the remaining failure hit request `n_predict=1024`, not the 2048-token slot;
- output shape looks like repetition runaway on a small input;
- runtime got slower than `16384/16`;
- simply increasing `ctx-size` again is probably the wrong next move.

### 2026-06-29 latest app run after generation-profile code pass

```txt
jobId: job-1782708384464-2604-17278v1
runId: run-pdf-1782716166556
runtime args: --ctx-size 8192 --parallel 16
effective slot n_ctx: not re-read from runtime header for this run; expected 512
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 138919 ms
first page: 10429 ms
shim batches: 26
shim failedRequestCount: 0
shim average/max: 4900 / 6388 ms
input/output chars: 34952 / 12481
page state: 10 translated, 0 failed
page artifacts: all 10 translated-pages/zh-CN/page-*.pdf files present
```

Checker command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782708384464-2604-17278v1 --run-id run-pdf-1782716166556 --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782708384464-2604-17278v1\diagnostics\pdf-benchmark-check-run-pdf-1782716166556.json"
```

Result:

- page/run state passed;
- speed gate failed: `138919 ms > 70600 ms`;
- strict raw completion gate could not be evaluated because
  `rwkv-io-debug.jsonl` had no matching records for this run. The log file was
  last modified on 2026-06-26, so this run appears to have been executed
  without `ROSETTA_RWKV_IO_DEBUG=1`;
- runtime log tail around the latest activity did not show `truncated = 1`,
  but this is weaker than the checker because it does not prove `stop_type`.

Timeline bottleneck:

```txt
page.processPage.translateRequest: 115 completed events, total 826872 ms
page.processPage/endPage: roughly 131 s aggregate page processing wall time
YOLO total: 5636 ms
single-page PDF save total: 49 ms
```

Interpretation:

- latest page-level correctness looks good;
- the strict benchmark is still not accepted because raw completion logs were
  missing and total time is about 1.97x the target;
- the bottleneck remains model translation wait, not layout inference or PDF
  artifact assembly.

### 2026-06-29 app run with RWKV IO debug enabled

```txt
jobId: job-1782708384464-2604-17278v1
runId: run-pdf-1782717037079
runtime args: --ctx-size 8192 --parallel 16
effective slot n_ctx: 512, inferred from failed record tokens_cached=511
generation profile: active
  temperature=0.25
  top_k=20
  top_p=0.9
  min_p=0.05
  repeat_penalty=1.18
  repeat_last_n=192
  stop strings present
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 149780 ms
first page: 9426 ms
shim batches: 26
shim failedRequestCount: 0
shim average/max: 5341 / 21981 ms
completion records: 367
ok=false / empty / truncated / limit: 1 / 1 / 1 / 1
throughput prompt/predicted: 69.49 / 87.45 tok/s
input/output chars from debug records: 36344 / 13245
page state: 10 translated, 0 failed
```

Checker output:

```txt
result: FAIL
- 1 completion record(s) have ok=false
- 1 completion record(s) have empty output
- 1 completion record(s) have truncated=true
- 1 completion record(s) have stop_type=limit
- total 149780 ms exceeds limit 70600 ms
```

Failed raw completion metadata without source/translation text:

```txt
record index: 303
input chars: 120
raw content chars before rejection: 461
unique chars: 20
top repeated codepoint counts: 101 / 50 / 50 / 49 / 49 / 49 / 49 / 49
prompt_n: 53
predicted_n: 459
tokens_cached: 511
stop_type: limit
truncated: true
slot: 14
total raw completion latency: 9170 ms
```

Interpretation:

- the generation-profile request fields are definitely reaching llama.cpp;
- page/run truthfulness and split recovery are working: the page artifacts were
  committed and `failedRequestCount=0` because the shim recovered the failed
  raw item;
- strict no-raw-failure acceptance still fails;
- unlike the prior `32768/16` failure that hit `n_predict=1024`, this run hit
  the per-slot context boundary at about 512 tokens. The next experiment should
  test the generation profile with a larger effective slot context, such as
  `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=16384` with `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=16`,
  before further generation-profile tightening.

### 2026-06-29 16384/16 run with generation profile and RWKV IO debug

```txt
jobId: job-1782708384464-2604-17278v1
runId: run-pdf-1782717712046
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024 expected
generation profile: active
  temperature=0.25
  top_k=20
  top_p=0.9
  min_p=0.05
  repeat_penalty=1.18
  repeat_last_n=192
  stop strings present
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 128287 ms
first page: 9076 ms
shim batches: 26
shim failedRequestCount: 0
shim average/max: 4502 / 5832 ms
completion records: 343
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
throughput prompt/predicted: 78.46 / 95.28 tok/s
input/output chars from debug records: 34952 / 12740
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- total 128287 ms exceeds limit 70600 ms
```

This is the first recorded run in this series where the strict bottom-level
completion correctness gate passed with IO debug enabled:

- all 343 raw completions reported `stop_type=eos`;
- `truncated=true`: 0;
- empty outputs: 0;
- provider failures: 0.

The run still misses the speed target by about `57.7 s`. It improved over the
previous debug-enabled `8192/16` run:

```txt
149780 ms -> 128287 ms
completion records: 367 -> 343
prompt/predicted throughput: 69.49 / 87.45 -> 78.46 / 95.28 tok/s
```

Timeline bottleneck:

```txt
page.processPage.translateRequest: 115 completed events, total 731525 ms
page.processPage/endPage: roughly 120 s aggregate page processing wall time
YOLO total: 5733 ms
single-page PDF save: negligible
```

Interpretation:

- `16384/16` plus the generation profile is the current correctness baseline;
- remaining work is now primarily throughput, not truncation;
- the next useful experiments are likely concurrency/chunking tradeoffs rather
  than more anti-repetition tuning. Candidate next operating points:
  `24576/16`, `32768/16`, or reducing completion count by cautiously raising the
  llama.cpp PDF chunk budgets while keeping the checker strict.

### 2026-06-29 24576/16 run with generation profile and RWKV IO debug

```txt
jobId: job-1782708384464-2604-17278v1
runId: run-pdf-1782718255808
runtime args: --ctx-size 24576 --parallel 16
effective slot n_ctx: 1536 expected
generation profile: active
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 129234 ms
first page: 9073 ms
shim batches: 26
shim failedRequestCount: 0
shim average/max: 4545 / 5666 ms
completion records: 343
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
throughput prompt/predicted: 77.99 / 93.05 tok/s
input/output chars from debug records: 34952 / 12462
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- total 129234 ms exceeds limit 70600 ms
```

Strict bottom-level completion correctness stayed clean:

- all 343 raw completions reported `stop_type=eos`;
- `truncated=true`: 0;
- empty outputs: 0;
- provider failures: 0.

Compared with `16384/16`, `24576/16` was effectively flat to slightly worse:

```txt
total: 128287 ms -> 129234 ms
completion records: 343 -> 343
prompt/predicted throughput: 78.46 / 95.28 -> 77.99 / 93.05 tok/s
```

Timeline bottleneck:

```txt
page.processPage.translateRequest: 115 completed events, total 770006 ms
page.processPage/endPage: roughly 121 s aggregate page processing wall time
YOLO total: 5630 ms
single-page PDF save: negligible
```

Interpretation:

- `24576/16` does not improve throughput over `16384/16`;
- the larger slot context is not buying speed for this sample once truncation is
  gone;
- keep `16384/16` as the current best correctness baseline and move the next
  experiment toward reducing completion count or improving batching. A
  `32768/16` run may still be useful as a one-off confirmation, but the
  stronger hypothesis is now PDF shim chunk/batch tuning under the strict
  checker.

### 2026-06-29 16384/16 run with initial wide adaptive PDF shim profile

```txt
jobId: job-1782721358429-2604-17278v1
runId: run-pdf-1782721364388
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024 expected
generation profile: active
initial adaptive chunk target/hard:
  body: 112/144
  caption: 96/128
  reference: 84/112
pages: 1-10
target: zh-CN
mode: continue on fresh imported job
total: 167662 ms
first page: 26800 ms
shim batches: 14
shim failedRequestCount: 0
shim average/max: 11262 / 46961 ms
completion records: 251
ok=false / empty / truncated / limit: 2 / 2 / 2 / 2
throughput prompt/predicted: 66.78 / 92.25 tok/s
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- 2 completion record(s) have ok=false
- 2 completion record(s) have empty output
- 2 completion record(s) have truncated=true
- 2 completion record(s) have stop_type=limit
- total 167662 ms exceeds limit 70600 ms
```

Failed raw completion metadata without source/translation text:

```txt
record index: 8
input chars: 345
prompt_n: 119
predicted_n: 905
stop_type: limit
truncated: true
raw completion latency: 14346 ms

record index: 188
input chars: 272
prompt_n: 92
predicted_n: 932
stop_type: limit
truncated: true
raw completion latency: 17488 ms
```

Interpretation:

- the wider chunk profile reduced raw completion count (`343 -> 251`);
- strict no-raw-failure correctness regressed because two larger chunks ran
  into raw llama.cpp truncation/limit;
- total time regressed badly because split backstop recovered the page output
  after expensive failed batches;
- the source defaults were backed off after this run to a safer middle profile:
  body `72/88`, caption `72/88`, reference `56/72`;
- the failed `112/144` profile should only be retried through explicit
  `ROSETTA_PDF_SHIM_LLAMA_*` env sweeps, not as the default.

### 2026-06-29 16384/16 run with middle adaptive PDF shim profile

```txt
jobId: job-1782721358429-2604-17278v1
runId: run-pdf-1782722782529
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024 expected
generation profile: active
middle adaptive chunk target/hard:
  body: 72/88
  caption: 72/88
  reference: 56/72
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 167094 ms
first page: 7098 ms
shim batches: 20
shim failedRequestCount: 0
shim average/max: 7822 / 31764 ms
completion records: 330
ok=false / empty / truncated / limit: 2 / 2 / 2 / 2
throughput prompt/predicted: 66.03 / 91.22 tok/s
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- 2 completion record(s) have ok=false
- 2 completion record(s) have empty output
- 2 completion record(s) have truncated=true
- 2 completion record(s) have stop_type=limit
- total 167094 ms exceeds limit 70600 ms
```

Failed raw completion metadata without source/translation text:

```txt
record index: 130
input chars: 24
prompt_n: 15
predicted_n: 1009
stop_type: limit
truncated: true
raw completion latency: 16842 ms
shape: short reference-list fragment

record index: 272
input chars: 132
prompt_n: 61
predicted_n: 963
stop_type: limit
truncated: true
raw completion latency: 15544 ms
shape: reference-list fragment with placeholders
```

Interpretation:

- middle chunking almost returned to the old completion count (`343 -> 330`)
  but still failed strict no-raw-truncation;
- one failure was a tiny reference fragment, so this is partly a small-input
  reference runaway rather than only a chunk-size problem;
- the code has been updated after this run:
  - effective `>=1024` references are back to conservative `42/56`;
  - very short `[N] ...` reference fragments are deterministic passthrough;
  - body/caption remain at `72/88`.

### 2026-06-29 16384/16 run with reference fallback and short-reference passthrough

```txt
jobId: job-1782721358429-2604-17278v1
runId: run-pdf-1782723901909
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024 expected
generation profile: active
current adaptive chunk target/hard:
  body: 72/88
  caption: 72/88
  reference: 42/56
  short reference passthrough: enabled
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 123559 ms
first page: 7714 ms
shim batches: 23
shim failedRequestCount: 0
shim average/max: 4882 / 6442 ms
completion records: 304
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
throughput prompt/predicted: 79.77 / 97.44 tok/s
input/output chars from debug records: 34991 / 12574
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- total 123559 ms exceeds limit 70600 ms
```

Strict bottom-level completion correctness is clean again:

- all 304 raw completions completed without provider failure;
- `truncated=true`: 0;
- `stop_type=limit`: 0;
- empty outputs: 0.

Compared with the previous best strict-correct `16384/16` baseline:

```txt
completion records: 343 -> 304
total: 128287 ms -> 123559 ms
first page: 9076 ms -> 7714 ms
prompt/predicted throughput: 78.46 / 95.28 -> 79.77 / 97.44 tok/s
```

Timeline bottleneck remains translation wait:

```txt
page.processPage.translateRequest: 115 completed events, total 793634 ms
YOLO total: 5924 ms
single-page PDF save: negligible
```

Interpretation:

- reference fallback plus short-reference passthrough restored strict
  no-truncation while keeping fewer raw completions than the original 343-run;
- this is now the best recorded strict-correct operating point;
- speed is still far from the `70600 ms` target, so further work should focus
  on reducing pdf2zh translate waves or safely removing/passthroughing more
  non-semantic PDF fragments, not widening references again.

### 2026-06-29 16384/16 body/caption 80/96 env sweep

```txt
jobId: job-1782721358429-2604-17278v1
runId: run-pdf-1782724542179
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024 expected
generation profile: active
env chunk overrides:
  ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET=80
  ROSETTA_PDF_SHIM_LLAMA_BODY_HARD=96
  ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET=80
  ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD=96
  reference: default 42/56
  short reference passthrough: enabled
pages: 1-10
target: zh-CN
mode: retranslate-all
total: 132397 ms
first page: 9932 ms
shim batches: 23
shim failedRequestCount: 0
shim average/max: 5238 / 8206 ms
completion records: 288
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
throughput prompt/predicted: 74.44 / 91.18 tok/s
input/output chars from debug records: 35007 / 12583
page state: 10 translated, 0 failed
```

Checker result:

```txt
result: FAIL
- total 132397 ms exceeds limit 70600 ms
```

Strict correctness stayed clean, but the run was slower than the default
`72/88` body/caption profile:

```txt
completion records: 304 -> 288
total: 123559 ms -> 132397 ms
completion latency p50/p95/max: 3242 / 4865 / 5780 ms -> 3363 / 5987 / 7587 ms
prompt/predicted throughput: 79.77 / 97.44 -> 74.44 / 91.18 tok/s
```

Interpretation:

- the sweep reduced completion count but made individual completions slower
  enough to lose overall;
- do not promote body/caption `80/96` to the default;
- keep the default at body/caption `72/88`, reference `42/56`;
- further gains probably need fewer pdf2zh translateRequest waves or more
  targeted passthrough of non-semantic fragments, not larger chunks.

## Current Diagnosis

The task is not accepted.

Current facts:

```txt
strict no-raw-failure gate: passing at 16384/16 and 24576/16
speed gate <= 70600 ms: failing
page/run truthfulness: fixed
split backstop: working
runtime env knobs: working
generation profile: implemented and verified in rwkv-io-debug raw responses
current best correctness baseline:
  packaged default --ctx-size 16384 --parallel 16
  body/caption 72/88, reference 42/56, short-reference passthrough
  run-pdf-1782723901909, 123559 ms, 304 completions, 0 raw failures
body/caption 80/96 env sweep:
  strict correctness passed, but slower at 132397 ms despite 288 completions
```

The most recent accepted-correctness run is:

```txt
jobId: job-1782721358429-2604-17278v1
runId: run-pdf-1782723901909
runtime args: --ctx-size 16384 --parallel 16
completion records: 304
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
total: 123559 ms
target: 70600 ms
```

The 24576/16 follow-up also passed strict correctness but did not improve
throughput:

```txt
runId: run-pdf-1782718255808
runtime args: --ctx-size 24576 --parallel 16
completion records: 343
ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
total: 129234 ms
```

The bottleneck is now throughput, not raw truncation. Timeline diagnostics for
the best runs show almost all wall time in `page.processPage.translateRequest`;
YOLO and PDF artifact save are small. The current code pass widens normal
llama.cpp PDF chunks only when effective slot context is at least `1024` tokens.
The first real `16384/16` benchmark with the initial wide profile reduced
completion count but reintroduced raw `truncated=true` / `stop_type=limit`.
A middle-profile rerun still failed on reference-list fragments, including a
24-character fragment that ran to `n_predict`. The current code now keeps only
body/caption widened, returns references to the conservative budget, and
deterministically preserves very short reference fragments without model
translation. The latest strict benchmark passed correctness and became the best
recorded operating point, but the speed target remains open.

## Recommended Next Work

### 1. Read relevant local code

Start with the PDF shim, because generation hardening has already landed:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
rosetta-app/src-tauri/src/rwkv_api.rs
```

Important functions:

```txt
managed_pdf2zh/openai_shim.rs:
  llama_cpp_chunk_profile()
  split_text_for_translation()
  translate_texts_with_split_retry()
  translate_pdf2zh_texts()

rwkv_providers/llama_cpp_chat.rs:
  build_completion_request()
  translate_one()
  parse_translation()
```

Current llama.cpp generation profile is:

```txt
n_predict: 1024
temperature: 0.25
top_k: 20
top_p: 0.9
min_p: 0.05
repeat_penalty: 1.18
repeat_last_n: 192
language-label stop strings
stream: false
```

Do not remove the strict parser behavior that rejects:

```txt
truncated=true
stop_type=limit
empty content
```

### 2. Keep the current generation profile as the correctness baseline

Do not start by changing generation settings. They are now verified to pass
strict no-truncation at 16384/16:

```txt
temperature=0.25
top_k=20
top_p=0.9
min_p=0.05
repeat_penalty=1.18
repeat_last_n=192
n_predict=1024
```

If generation settings must be changed, use the `ROSETTA_LLAMA_CPP_*` env
overrides for a real benchmark before committing code changes. Never accept
partial raw output by trimming or ignoring `stop_type=limit`; the checker must
continue to fail raw bottom-level limit/truncation.

### 3. Tune PDF shim chunking and batching

The current strict-correctness baseline still has too many completion calls:

```txt
completion records: 343
shim batches: 26
translateRequest events: 115
total: 128287 ms
```

Candidate experiments:

- rerun `16384/16` with the adaptive wider PDF shim profile now in
  `openai_shim.rs`;
- compare completion count against the previous `343` raw completions;
- if the first run is clean but still slow, sweep the local PDF shim env knobs
  rather than changing source again immediately;
- keep `--ctx-size 16384 --parallel 16` while tuning chunks, because 24576/16
  did not improve throughput;
- run the strict checker after every real benchmark;
- if raw `truncated=true` or `stop_type=limit` returns, back off the chunk
  budget or add a more targeted split rule;
- inspect whether tiny fragments can be merged or deterministic-passthrough
  without losing content, especially PDF reference/list/citation fragments.

Acceptance still requires both:

```txt
completion ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
total <= 70600 ms
```

Relevant local PDF shim budget overrides for sweeps:

```txt
ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET
ROSETTA_PDF_SHIM_LLAMA_BODY_HARD
ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET
ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD
```

### 4. Optional runtime confirmation

`32768/16` may be run once as confirmation, but the stronger current evidence
is that bigger context does not improve speed after correctness is fixed:

```txt
16384/16: 128287 ms, strict correctness passed
24576/16: 129234 ms, strict correctness passed
```

Do not spend many runs simply increasing ctx-size unless new evidence appears.

### 5. Benchmark protocol

After code changes and validation:

1. Restart the managed RWKV runtime.
2. Confirm the active process command:

   ```powershell
   Get-CimInstance Win32_Process -Filter "name='llama-server.exe'" |
     Select-Object ProcessId,CommandLine |
     Format-List
   ```

3. Confirm `runtime.log` effective slot context:

   ```txt
   n_slots = 16
   n_ctx = ...
   ```

4. Rerun the same forced 10-page translation.
5. Run:

   ```powershell
   cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
   node scripts/check-pdf-translation-run.mjs --job-id job-1782708384464-2604-17278v1 --run-id <new-run-id> --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782708384464-2604-17278v1\diagnostics\pdf-benchmark-check-<new-run-id>.json"
   ```

Acceptance still requires:

```txt
pages requested == pages translated
pages failed == 0
completion ok=false == 0
empty output == 0
truncated=true == 0
stop_type=limit == 0
total <= 70600 ms
```

### 6. Record every real run

Append exact numbers here or to the primary plan:

```txt
runtime args:
effective slot n_ctx:
generation params:
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

## Suggested Prompt For The Next Agent

Use this prompt if continuing in a fresh agent thread:

```txt
请继续 Rosetta 仓库里的 PDF no-truncation / 50% speedup 工作。

先阅读：
- docs/engineering/plans/2026-06-29-pdf-llama-cpp-generation-handover.md
- docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md
- docs/engineering/pdf-pipeline.md
- docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md

当前状态：
- page/run failure propagation 已修复；
- llama.cpp raw `truncated=true` / `stop_type=limit` 会被 provider 拒绝；
- PDF shim split backstop 能恢复失败 batch，但 strict checker 仍要求底层 raw completion 不能出现 limit/truncation；
- PDF shim 已加 adaptive llama.cpp chunk profile：默认 8192/16 仍用 56/42 保守 profile，16384/16 这类 effective slot >=1024 的运行当前用 body 72/88、caption 72/88、reference 42/56；
- 很短的 `[N] ...` reference fragment 现在 deterministic passthrough，保留内容但不送进 llama.cpp，避免小 reference 片段生成 runaway；
- 早先测试过更宽的 body 112/144、caption 96/128、reference 84/112，completion count 从 343 降到 251，但 strict checker 出现 2 个 raw `truncated=true + stop_type=limit`，总时长退化到 167662 ms；不要把这组值作为默认值；
- middle profile body 72/88、caption 72/88、reference 56/72 也测试过，completion count 330，但仍有 2 个 reference-list raw `truncated=true + stop_type=limit`，总时长 167094 ms；所以 reference 已回退到 42/56；
- llama.cpp `/completion` generation profile 已实现并通过 raw debug 证实生效：
  - temperature=0.25
  - top_k=20
  - top_p=0.9
  - min_p=0.05
  - repeat_penalty=1.18
  - repeat_last_n=192
  - language-label stop strings
- runtime env knobs 已实现：
  - `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE`
  - `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL`
- packaged app 默认 runtime 已改为 `--ctx-size 16384 --parallel 16`，用户不再需要设置 env 才能得到当前 strict-correct baseline；
- 当前最佳 correctness baseline 是 `--ctx-size 16384 --parallel 16`：
  - jobId: `job-1782708384464-2604-17278v1`
  - runId: `run-pdf-1782717712046`
  - total: `128287 ms`
  - completion records: `343`
  - ok=false / empty / truncated / limit: `0 / 0 / 0 / 0`
  - prompt/predicted throughput: `78.46 / 95.28 tok/s`
- `24576/16` 也通过 strict correctness，但没有提速：
  - runId: `run-pdf-1782718255808`
  - total: `129234 ms`
  - completion records: `343`
  - ok=false / empty / truncated / limit: `0 / 0 / 0 / 0`
- 速度目标仍未达成：`<= 70600 ms`。

下一步不要继续主要加 ctx-size，也不要先改 generation profile。请用当前代码在 `16384/16` 下 rerun 真实 PDF benchmark，确认 reference 回退 + 短 reference passthrough 是否恢复 strict correctness，同时 completion count 是否仍低于 343。strict checker 必须保持 `ok=false / empty / truncated / limit = 0 / 0 / 0 / 0`。如果它正确但仍慢，再用 `ROSETTA_PDF_SHIM_LLAMA_*` env knobs 做 chunk budget sweep；如果出现 raw `truncated=true` 或 `stop_type=limit`，继续回退 chunk budget 或加更精准 split/passthrough rule，不能通过接受 partial output 过 checker。

完成代码后请跑：
- cd rosetta-app && node --check scripts/check-pdf-translation-run.mjs
- cd rosetta-app && pnpm typecheck
- cd rosetta-app/src-tauri && cargo fmt -- --check
- cd rosetta-app/src-tauri && cargo check
- cd rosetta-app/src-tauri && cargo test llama_cpp
- cd rosetta-app/src-tauri && cargo test managed_rwkv::lifecycle
- cd rosetta-app/src-tauri && cargo test rosetta_jobs

真实 PDF benchmark 需要用户或当前对话明确允许/执行。每次真实运行后，用：
node scripts/check-pdf-translation-run.mjs --job-id job-1782708384464-2604-17278v1 --run-id <new-run-id> --max-total-ms 70600

验收必须同时满足：
- pages requested == pages translated
- pages failed == 0
- completion ok=false == 0
- empty output == 0
- truncated=true == 0
- stop_type=limit == 0
- total <= 70600 ms

请把每次真实 benchmark 的 runtime args、effective n_ctx、generation params、runId、total、first page、shim batches、completion records、failed/truncated/limit/empty、throughput 和结论写回工程计划文档。不要删除未跟踪的 plan/checker 文件。
```
