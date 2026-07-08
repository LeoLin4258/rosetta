# 2026-07-07 PDF llama.cpp Limit Truncation Handoff

## Status

Unresolved. This is a handoff note for the next agent.

The PDF Windows font-weight parity issue from the same session was resolved and
verified separately. This document records a new PDF translation failure the
user observed when translating a different PDF.

## User-Visible Failure

While translating another PDF in the local Windows Tauri dev app, page 1 failed
with:

```txt
第 1 页翻译失败
失败原因：PDF 译文生成失败：llama.cpp 响应格式不可用: llama.cpp completion was truncated (truncated=true, stop_type=limit)
```

Interpretation: Rosetta rejected a llama.cpp `/completion` response because the
model hit its generation limit and returned partial output. This rejection is
intentional; partial translated output must not be accepted silently.

## Environment At Time Of Report

- OS: Windows
- App mode: user was running `pnpm tauri dev`
- Target language observed in the active PDF testing session: `zh-CN`
- Local managed provider in use for PDF translation:
  `llama-cpp-chat-completions`
- Clash was available on port `7897`, but the PDF translation path involved
  local managed runtimes and did not need network for the completed font-weight
  work.

The exact failing PDF/job/run IDs were not captured in this handoff. The next
agent should inspect the most recent PDF job diagnostics under:

```txt
%APPDATA%\com.rosetta.desktop\jobs\<job-id>\diagnostics\pdf-timeline.jsonl
%APPDATA%\com.rosetta.desktop\jobs\<job-id>\diagnostics\pdf-translation-profile-<run-id>.json
%APPDATA%\com.rosetta.desktop\logs\rosetta.log
%APPDATA%\com.rosetta.desktop\logs\rwkv-io-debug.jsonl
```

Do not commit `rwkv-io-debug.jsonl`; it can contain document text and model
responses.

## Relevant Existing Behavior

The provider-level parser intentionally rejects limit/truncated responses:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

Current key logic:

```txt
parse_translation(...)
  if response.truncated || response.stop_type == "limit":
    return Err("llama.cpp completion was truncated ...")
```

The PDF unit translation path records this as a truncation-like provider
failure:

```txt
rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs
```

Current key logic:

```txt
if result.message contains "truncated=true" or "stop_type=limit":
  metrics.truncated_count += 1
  return Err(result.message)
```

That behavior should not be weakened. Do not fix this by accepting partial
llama.cpp output, trimming the output, or ignoring `stop_type=limit`.

## Related Historical Work

Read these before changing the runtime, chunking, or response handling:

```txt
docs/engineering/pdf-pipeline.md
docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
docs/engineering/plans/2026-06-26-pdf-no-truncation-performance-current-handover.md
docs/engineering/plans/2026-06-29-pdf-llama-cpp-generation-handover.md
```

Important history:

- `truncated=true` and `stop_type=limit` used to be false-success risks;
  Rosetta now treats them as hard failures.
- The current managed Windows llama.cpp runtime was previously tuned around
  `--ctx-size 16384 --parallel 16`.
- The current llama.cpp generation profile and chunk budgets were chosen to
  avoid truncation on the known 10-page benchmark, not to guarantee every PDF.
- Existing local knobs include:

```txt
ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE
ROSETTA_MANAGED_LLAMA_CPP_PARALLEL
ROSETTA_LLAMA_CPP_TEMPERATURE
ROSETTA_LLAMA_CPP_TOP_K
ROSETTA_LLAMA_CPP_TOP_P
ROSETTA_LLAMA_CPP_MIN_P
ROSETTA_LLAMA_CPP_REPEAT_PENALTY
ROSETTA_LLAMA_CPP_REPEAT_LAST_N
ROSETTA_LLAMA_CPP_N_PREDICT
ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET
ROSETTA_PDF_SHIM_LLAMA_BODY_HARD
ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET
ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET
ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD
```

## First Investigation Steps

1. Identify the failing job and run.

   Use the job index and recent diagnostics:

   ```powershell
   $root = Join-Path $env:APPDATA 'com.rosetta.desktop\jobs'
   Get-ChildItem -LiteralPath $root -Directory |
     Sort-Object LastWriteTime -Descending |
     Select-Object -First 10 Name,LastWriteTime
   ```

   Then inspect the newest failing job's timeline:

   ```powershell
   Get-Content -LiteralPath "$job\diagnostics\pdf-timeline.jsonl" -Tail 120
   ```

2. Capture the exact failing run metadata.

   Record:

   ```txt
   jobId:
   runId:
   source PDF filename:
   pageSelection:
   pageNumber:
   providerId:
   chunkSize / requestedPages:
   pdf2zh worker ready state:
   runtime args from rosetta.log:
   effective n_slots / n_ctx from managed runtime log:
   ```

3. If `ROSETTA_RWKV_IO_DEBUG=1` was enabled, inspect the matching completion
   record locally.

   Look for the failed `/completion` response with:

   ```txt
   truncated: true
   stop_type: limit
   n_predict: ...
   source/input character count
   output shape
   ```

   Do not paste source text or model responses into docs or commits.

4. Determine which path produced the request.

   The current Rosetta-native PDF v2 path uses:

   ```txt
   rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs
   ```

   The older OpenAI-shim/pdf2zh path and many historical knobs live in:

   ```txt
   rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs
   ```

   Confirm from diagnostics whether the failing run used the native PDF unit
   path or any shim-era path. Do not assume the older shim split backstop is in
   effect for this new failure.

5. Reproduce with the smallest safe scope.

   Prefer re-running only the failing page first, with forced retranslation,
   rather than immediately re-running the whole PDF. Keep the source document
   local and private.

## Likely Investigation Directions

The error can mean one of two things:

- the input chunk was too large for the effective per-slot context or output
  room;
- the model entered a repetition/runaway pattern and hit `n_predict`, even if
  context was otherwise sufficient.

Useful checks:

- compare the failing chunk's estimated input length to
  `NON_LIGHTNING_HARD_PROMPT_TOKENS` and the chunking in
  `unit_translation.rs`;
- check whether the failure involves references, captions, formulas, or a
  short/noisy fragment;
- confirm whether `ROSETTA_LLAMA_CPP_N_PREDICT=1024` is still the effective
  request cap or whether a local env override changed it;
- check whether lowering the PDF unit chunk budget or adding a targeted split
  rule fixes the page without regressing the known 10-page benchmark;
- consider a provider-level retry/split backstop for the native PDF unit path
  if the current path lacks the older shim's recovery behavior.

## Acceptance Criteria For A Fix

A correct fix should:

- keep rejecting raw `truncated=true` and `stop_type=limit`;
- make the failing PDF page translate successfully without accepting partial
  output;
- preserve existing placeholder/formula reconstruction behavior;
- avoid sending private source text to logs committed to the repo;
- pass targeted unit tests for the changed chunking/retry behavior;
- pass the existing PDF/job validation suite.

Suggested validation:

```powershell
cd rosetta-app
node --check scripts/check-pdf-translation-run.mjs
pnpm typecheck

cd src-tauri
cargo fmt -- --check
cargo check
cargo test llama_cpp
cargo test rosetta_jobs
```

If a real PDF retranslation is needed, use the user's explicit permission and
record the job/run IDs and checker result without source or translated text.

## Suggested Prompt For The Next Agent

```txt
请继续 Rosetta 的 PDF 翻译新问题排查。用户在 Windows 本地 `pnpm tauri dev` 翻译另一个 PDF 时，第 1 页失败：

PDF 译文生成失败：llama.cpp 响应格式不可用: llama.cpp completion was truncated (truncated=true, stop_type=limit)

先阅读：
- docs/engineering/plans/2026-07-07-pdf-llama-cpp-limit-truncation-handoff.md
- docs/engineering/pdf-pipeline.md
- docs/engineering/change-log/2026-06-26-pdf-no-truncation-runtime-and-harness.md
- docs/engineering/plans/2026-06-29-pdf-llama-cpp-generation-handover.md

注意：不要通过接受 partial output、忽略 `stop_type=limit` 或放松 strict checker 来修。需要找出失败 chunk/run 的真实原因，优先定位最新 failing job 的 diagnostics、runtime args、effective n_ctx、是否走 native PDF unit path，然后用最小页面范围复现。修复后请加针对性测试并跑现有 PDF/job 验证。
```
