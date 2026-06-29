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

- default managed llama.cpp launch remains:

  ```txt
  --ctx-size 8192 --parallel 16
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

## Current Diagnosis

The task is not accepted.

Current facts:

```txt
strict no-raw-failure gate: failing
speed gate <= 70600 ms: failing
page/run truthfulness: fixed
split backstop: working
runtime env knobs: working
```

The most recent failure mode is not source chunk size and not server context
capacity. It is a llama.cpp generation behavior problem:

```txt
small input -> repetitive raw output -> request hits n_predict=1024 ->
parse_translation rejects stop_type=limit -> split backstop recovers page
```

The next useful work is generation-parameter hardening for llama.cpp
`/completion`, especially repetition/runaway control and a more reliable stop
condition.

## Recommended Next Work

### 1. Read relevant local code

Start with:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

Important functions:

```txt
build_completion_request()
translate_one()
parse_translation()
```

Current request shape is minimal:

```txt
prompt
n_predict: 1024
temperature: 1.0
stream: false
```

Then inspect:

```txt
rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
rosetta-app/src-tauri/src/rwkv_api.rs
```

### 2. Verify llama.cpp `/completion` parameter names

Use primary llama.cpp docs/source if network/docs are needed. Do not rely on
blog posts. Candidate areas to verify:

```txt
repeat_penalty
repeat_last_n
penalize_nl
top_k
top_p
min_p
temperature
seed
stop / stopping strings, if supported by this endpoint/build
n_predict
```

The goal is not creative generation. It is deterministic local translation.
Favor conservative, low-entropy settings that stop repetition.

### 3. Add a llama.cpp generation profile

Recommended implementation shape:

- extend `CompletionRequest` with optional serde fields for verified
  llama.cpp `/completion` parameters;
- keep defaults compatible, but tune the managed translate path toward
  low-entropy/repetition-safe generation;
- consider env overrides for benchmark-only generation experiments, similar to
  the runtime knobs, so values can be tested without source edits;
- add unit tests that request JSON contains the chosen fields and defaults.

Potential initial experiment values to verify against llama.cpp docs/source:

```txt
temperature: lower than 1.0, possibly 0.2-0.7
top_k: small, or disabled if greedy settings are better for this RWKV model
top_p/min_p: conservative
repeat_penalty: >1.0
repeat_last_n: enough to catch runaway loops
n_predict: do not raise blindly; raising it can make runaway slower
```

Do not accept partial raw output by trimming or ignoring `stop_type=limit`.
The checker must continue to fail raw bottom-level limit/truncation.

### 4. Benchmark protocol

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
   node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id <new-run-id> --max-total-ms 70600 --output "$env:APPDATA\com.rosetta.desktop\jobs\job-1782474427044-2604-17278v1\diagnostics\pdf-benchmark-check-<new-run-id>.json"
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

### 5. Record every real run

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
- runtime env knobs 已实现：
  - `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE`
  - `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL`
- 8192/16: 无 raw failure，但 189984 ms，太慢；
- 16384/16: 140547 ms，但 1 个 raw `truncated=true + stop_type=limit`；
- 32768/16: 154087 ms，`truncated=true` 没了，但 1 个小输入重复 runaway，撞上 `n_predict=1024`，`stop_type=limit`。

下一步不要继续单纯加 ctx-size。请聚焦 `rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs` 的 llama.cpp `/completion` generation 参数。验证 llama.cpp 当前版本 `/completion` 支持的参数名，优先用官方/源码资料；然后为翻译请求加入低熵、重复抑制或可靠 stop condition 的 generation profile，必要时加 env knobs 方便 benchmark。不能通过忽略 `stop_type=limit` 或接受部分输出来绕过 checker。

完成代码后请跑：
- cd rosetta-app && node --check scripts/check-pdf-translation-run.mjs
- cd rosetta-app && pnpm typecheck
- cd rosetta-app/src-tauri && cargo fmt -- --check
- cd rosetta-app/src-tauri && cargo check
- cd rosetta-app/src-tauri && cargo test llama_cpp
- cd rosetta-app/src-tauri && cargo test managed_rwkv::lifecycle
- cd rosetta-app/src-tauri && cargo test rosetta_jobs

真实 PDF benchmark 需要用户或当前对话明确允许/执行。每次真实运行后，用：
node scripts/check-pdf-translation-run.mjs --job-id job-1782474427044-2604-17278v1 --run-id <new-run-id> --max-total-ms 70600

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
