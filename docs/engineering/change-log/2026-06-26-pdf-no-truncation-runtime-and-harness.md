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

- Raised the managed llama.cpp server context from `4096` to `8192`, then to
  the current strict-correct default `16384`, while
  keeping `--parallel 16`.
  - Expected effective slot context at the current default: about `1024`
    tokens per concurrent request.
  - This keeps the 16-way throughput target while giving PDF chunks enough
    output room for the strict no-truncation baseline.
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
- Added a llama.cpp `/completion` generation profile for translation:
  - default requests now use lower-entropy sampling, repetition control, and
    language-label stop strings instead of only `temperature: 1.0`;
  - benchmark experiments can override `temperature`, `top_k`, `top_p`,
    `min_p`, `repeat_penalty`, `repeat_last_n`, and `n_predict` through
    `ROSETTA_LLAMA_CPP_*` env vars;
  - the strict checker still treats `truncated=true` and `stop_type=limit` as
    failures, so the profile does not mask partial output.
- Added an adaptive llama.cpp PDF shim chunk profile for the successful
  `16384/16` correctness baseline.
  - The managed runtime default is now `16384/16`, so packaged app users get
    the strict-correct operating point without setting env vars.
  - Effective slot context `>=1024` uses moderately wider body/caption chunks
    while keeping references conservative: body `72/88`, caption `72/88`,
    reference `42/56` target/hard prompt tokens.
  - A wider `112/144`, `96/128`, `84/112` profile was tested against a real
    10-page run and reduced raw completion count, but it reintroduced two raw
    `truncated=true` / `stop_type=limit` completions and made the total slower
    through split retries.
  - A middle `72/88`, `72/88`, `56/72` profile was also tested and still
    reintroduced two raw reference-list failures, including one tiny 24-char
    reference fragment that ran to `n_predict`.
  - Very short `[N] ...` reference fragments are now preserved by deterministic
    passthrough instead of being sent to llama.cpp.
  - The resulting profile passed the strict raw-completion checker in
    `run-pdf-1782723901909`: `304` completions, `0` provider failures, `0`
    empty outputs, `0` raw `truncated=true`, and `0` raw `stop_type=limit`.
  - A body/caption `80/96` env sweep also passed strict correctness and reduced
    completions to `288`, but total runtime regressed to `132397 ms`; it should
    not replace the default `72/88` profile.
  - Split retry backstops remain smaller (`36`, then `24`) and strict raw
    `truncated=true` / `stop_type=limit` rejection is unchanged.
  - Local benchmark sweeps can override those PDF shim budgets with
    `ROSETTA_PDF_SHIM_LLAMA_BODY_TARGET`,
    `ROSETTA_PDF_SHIM_LLAMA_BODY_HARD`,
    `ROSETTA_PDF_SHIM_LLAMA_CAPTION_TARGET`,
    `ROSETTA_PDF_SHIM_LLAMA_CAPTION_HARD`,
    `ROSETTA_PDF_SHIM_LLAMA_REFERENCE_TARGET`, and
    `ROSETTA_PDF_SHIM_LLAMA_REFERENCE_HARD`.
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

The follow-up generation-profile code pass was validated locally with:

```powershell
cd rosetta-app
node --check scripts/check-pdf-translation-run.mjs
.\node_modules\.bin\tsc.CMD --noEmit

cd src-tauri
cargo fmt -- --check
cargo check
cargo test llama_cpp
cargo test managed_rwkv::lifecycle
cargo test rosetta_jobs
```

`pnpm typecheck` was attempted, but pnpm stopped before TypeScript with
`ERR_PNPM_IGNORED_BUILDS` for dependency build approval. Direct local
`tsc --noEmit` passed without changing pnpm approval state.

No real PDF benchmark has been recorded yet for the new generation defaults.

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
