# 2026-06-26 PDF No-Truncation Runtime And Harness

## Summary

Implemented the first correctness-focused pass from
`docs/engineering/plans/2026-06-26-pdf-translation-no-truncation-50pct-speedup.md`.

The main issue was that the Windows llama.cpp Vulkan runtime launched with
`--ctx-size 4096 --parallel 16`, which gave each server slot about 256 context
tokens. Recent `rwkv-io-debug.jsonl` records showed long PDF completions ending
with `truncated=true` and `stop_type=limit`, so Rosetta could silently accept
partial llama.cpp output.

## Changes

- Raised the managed llama.cpp server context from `4096` to `8192` while
  keeping `--parallel 16`.
  - Expected effective slot context: about `512` tokens per concurrent request.
  - This keeps the 16-way throughput target while giving long PDF chunks more
    output room.
- Kept generic PDF OpenAI-shim providers at batch width `8`, but raised the
  llama.cpp PDF shim path to batch width `16`.
  - This matches the replay benchmark that showed client concurrency 16 nearly
    halved the completion wall time for the 10-page `2604.17278v1.pdf` sample.
- Hardened the llama.cpp response parser.
  - `truncated=true` or `stop_type=limit` is now treated as a provider failure
    instead of returning partial text as a successful translation.
- Added a second correctness pass after the first full runtime benchmark:
  - the llama.cpp PDF shim now uses smaller provider-specific PDF chunk budgets
    than the generic OpenAI-shim providers;
  - failed llama.cpp batches retry through a smaller split backstop before the
    shim returns an error to pdf2zh;
  - page translation state now rejects a pdf2zh invocation whose final shim
    metrics report `failedRequestCount > 0`, clearing any artifacts from that
    invocation and marking the affected pages/run as failed.
- Added runtime experiment knobs for managed llama.cpp:
  - `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE` overrides the managed
    `llama-server --ctx-size` value;
  - `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL` overrides the managed
    `llama-server --parallel` value;
  - the parallel override also caps PDF shim batching/thread count and regular
    llama.cpp text translation batching so benchmark experiments are not a
    mixed server/client concurrency configuration.
- Added `rosetta-app/scripts/check-pdf-translation-run.mjs`.
  - Reads a PDF profile, page state, timeline, and `rwkv-io-debug.jsonl`.
  - Exits non-zero if requested pages are not translated, any completion has
    empty output, any completion is truncated, any completion stops at `limit`,
    or an optional total-duration threshold is exceeded.
  - Does not print source text, translated text, prompts, or raw responses.

## Validation Notes

The script was tested against the existing profile:

```powershell
cd rosetta-app
node scripts/check-pdf-translation-run.mjs --job-id job-1782471162628-2604-17278v1 --run-id run-pdf-1782471277785 --max-total-ms 70600
```

That old run correctly failed the performance gate. The current
`rwkv-io-debug.jsonl` file had already been cleared by later activity, so the
script also correctly failed with "no matching llama.cpp rwkv-io-debug records
found". Future benchmark runs must enable `ROSETTA_RWKV_IO_DEBUG=1` and run the
script before the debug log is cleared.

Full no-truncation and 50% speedup acceptance still require a fresh 10-page PDF
run after the new runtime settings take effect.

The follow-up state-propagation/backstop pass was validated with:

```powershell
cd rosetta-app
node --check scripts/check-pdf-translation-run.mjs
pnpm typecheck

cd src-tauri
cargo fmt -- --check
cargo check
cargo test rosetta_jobs
cargo test llama_cpp
cargo test managed_rwkv::lifecycle
```

The runtime experiment knob pass was validated with the same checker syntax
check and:

```powershell
cd rosetta-app
pnpm typecheck

cd src-tauri
cargo fmt -- --check
cargo check
cargo test managed_rwkv::lifecycle
cargo test llama_cpp
cargo test rosetta_jobs

cd ../..
git diff --check
```

`git diff --check` printed LF/CRLF normalization warnings only.

The first real forced benchmark after that pass was:

```txt
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782477804133
pages: 10
status: completed
pages translated / failed: 10 / 0
completion records: 343
completion ok=false / empty / truncated / limit: 0 / 0 / 0 / 0
total: 189984 ms
target gate: 70600 ms
```

So the correctness gate passed, but the speed gate still failed. The smaller
llama.cpp PDF chunks removed truncation at the cost of many more completion
requests.

The first runtime override benchmark was:

```txt
runtime args: --ctx-size 16384 --parallel 16
effective slot n_ctx: 1024
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782480171604
pages: 10
status: completed
pages translated / failed: 10 / 0
shim failedRequestCount: 0
completion records: 347
completion ok=false / empty / truncated / limit: 1 / 1 / 1 / 1
total: 140547 ms
target gate: 70600 ms
```

This improved throughput versus `8192/16` but still failed both the strict raw
completion correctness gate and the speed gate. The single truncated raw
completion was recovered by the split backstop before page finalization.

The second runtime override benchmark was:

```txt
runtime args: --ctx-size 32768 --parallel 16
effective slot n_ctx: 2048
jobId: job-1782474427044-2604-17278v1
runId: run-pdf-1782480966873
pages: 10
status: completed
pages translated / failed: 10 / 0
shim failedRequestCount: 0
completion records: 367
completion ok=false / empty / truncated / limit: 1 / 1 / 0 / 1
total: 154087 ms
target gate: 70600 ms
```

This removed `truncated=true`, but one raw completion still stopped with
`stop_type=limit` after hitting the request `n_predict=1024` cap. Its no-text
shape looked like repetition runaway on a small 120-character input, and the
split backstop recovered it before page finalization.

## Follow-Up

- Continue throughput work from the successful no-truncation operating point:
  `--ctx-size 8192 --parallel 16`, llama.cpp PDF chunk body target about 56
  prompt tokens.
- Compare stronger operating points by starting the app with overrides such as
  `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=16384` and
  `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=16`, or
  `ROSETTA_MANAGED_LLAMA_CPP_CTX_SIZE=8192` and
  `ROSETTA_MANAGED_LLAMA_CPP_PARALLEL=8`, then verify with:

  ```powershell
  cd rosetta-app
  node scripts/check-pdf-translation-run.mjs --job-id <job-id> --run-id <run-id> --max-total-ms 70600
  ```

- If truncation remains, lower the PDF shim chunk budgets or raise total
  llama.cpp context again, then record the measured operating point.
