# 2026-06-25 llama.cpp Vulkan PDF Replay Benchmark

## Purpose

This benchmark compares Rosetta's in-app PDF translation path with a bare
llama.cpp `/completion` replay using the same PDF-derived source text.

The goal is narrow: measure whether the observed PDF translation time is coming
from Rosetta/pdf2zh orchestration or from the managed llama.cpp Vulkan runtime
not being fed enough parallel work.

## Test Case

- PDF: `2604.17278v1.pdf`
- App job: `job-1782367389913-2604-17278v1`
- Page selection: `1-10`
- Source language: `en`
- Target language: `zh-CN`
- Provider: `llama-cpp-chat-completions`
- Runtime process: `llama-server.exe`
- Runtime pack: `llama-cpp-vulkan-b9775`
- Model family: RWKV v7 G1d 0.4B Translate via llama.cpp Vulkan
- Backend endpoint: `http://127.0.0.1:57808/completion`

The source records came from:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\logs\rwkv-io-debug.jsonl
```

The app-side profile came from:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1782367389913-2604-17278v1\diagnostics\pdf-translation-profile-run-pdf-1782367396913.json
```

## Method

1. Enable or reuse `rwkv-io-debug.jsonl` records produced by an in-app PDF
   translation run.
2. Select the latest PDF job context from that log:

   ```txt
   pdf-job:job-1782367389913-2604-17278v1
   ```

3. Filter records to successful `llama-cpp-chat-completions` entries.
4. Replay each original llama.cpp prompt from `rawResponse.prompt` directly
   against `/completion`.
5. Run replay with different client-side concurrency levels.

The replay tool added for this measurement is:

```txt
rosetta-app/scripts/benchmark-llama-cpp-pdf-debug.mjs
```

Example command:

```powershell
cd C:\Users\Leo\Documents\GitHub\rosetta\rosetta-app
node scripts/benchmark-llama-cpp-pdf-debug.mjs --concurrency 16 --output "C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\logs\benchmark-llama-cpp-pdf-debug-c16.json"
```

## App Baseline

The in-app PDF run completed successfully.

| Metric | Value |
| --- | ---: |
| Pages requested | 10 |
| Pages translated | 10 |
| Pages failed | 0 |
| App total duration | `120.170s` |
| pdf2zh process duration | `120.162s` |
| pdf2zh worker `translateMs` | `119.055s` |
| pdf2zh worker `yoloMs` | `6.596s` |
| Shim RWKV aggregate request count | 37 |
| Shim RWKV aggregate request time | `109.422s` |
| Shim RWKV average request time | `2.957s` |
| Shim RWKV max request time | `5.044s` |
| Shim total input chars | 36,387 |
| Shim total output chars | 11,654 |

The request-level debug log contained 131 successful llama.cpp `/completion`
records for the same job. From those records, the estimated completion window
was:

| Metric | Value |
| --- | ---: |
| Completion records | 131 |
| Estimated completion wall time | `115.765s` |
| Prompt tokens reported by log | 9,078 |
| Predicted tokens reported by log | 10,090 |
| Total tokens reported by log | 19,168 |
| Aggregate predicted throughput | `87.16 tok/s` |
| Aggregate total throughput | `165.58 tok/s` |
| Per-request latency p50 | `2.164s` |
| Per-request latency p90 | `3.862s` |
| Per-request latency p99 | `4.673s` |

## Bare Replay Results

All replay runs succeeded with 131/131 requests. The runs below were executed
sequentially. An earlier attempt ran concurrency 8 and 16 in parallel against
the same server process and was discarded because the two benchmarks contended
for the same llama.cpp runtime.

| Mode | Client concurrency | Success | Wall time | Relative to app total | Relative to app debug completion window | Predicted throughput |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| In-app full PDF pipeline | app pipeline | 10/10 pages | `120.170s` | `1.000x` | `1.038x` | n/a |
| In-app debug completion baseline | app observed | 131/131 | `115.765s` | `0.963x` | `1.000x` | `87.16 tok/s` |
| Bare replay | 4 | 131/131 | `124.316s` | `1.034x` | `1.074x` | `83.99 tok/s` |
| Bare replay | 8 | 131/131 | `105.249s` | `0.876x` | `0.909x` | `97.42 tok/s` |
| Bare replay | 16 | 131/131 | `58.799s` | `0.489x` | `0.508x` | `173.91 tok/s` |

Detailed replay JSON outputs were written locally:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\logs\benchmark-llama-cpp-pdf-debug-c4.json
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\logs\benchmark-llama-cpp-pdf-debug-c8.json
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\logs\benchmark-llama-cpp-pdf-debug-c16.json
```

## Interpretation

At concurrency 4, bare replay was not faster than the in-app observed completion
window:

- bare replay: `124.316s`
- app debug completion window: `115.765s`
- ratio: `1.074x` slower

At concurrency 16, bare replay was much faster:

- bare replay: `58.799s`
- app full PDF pipeline: `120.170s`
- ratio: `0.489x` of app total duration
- speedup versus app full PDF pipeline: about `2.04x`
- speedup versus app debug completion window: about `1.97x`

This points away from Tauri or Rosetta shell overhead as the main bottleneck.
The dominant gap is that the PDF shim's llama.cpp provider path is not feeding
the Vulkan backend enough parallel `/completion` work for this 0.4B model.

Current relevant behavior:

- PDF shim uses `DEFAULT_MAX_BATCH_SIZE = 4` for `ShimProviderConfig::LlamaCpp`.
- The llama.cpp provider sends one `/completion` request per source text inside
  that batch.
- The server process handled a client-side concurrency of 16 successfully and
  nearly doubled aggregate predicted-token throughput versus the in-app debug
  completion baseline.

## Caveats

- The replay benchmark measures HTTP `/completion` throughput, not full PDF
  reconstruction quality or end-to-end artifact assembly.
- Prompt token counts vary between runs because llama.cpp server-side cache can
  affect reported prompt evaluation. Wall time and successful request count are
  the most reliable comparison points here.
- Output text may differ between replay runs because generation is not pinned to
  a deterministic seed in this benchmark.
- The benchmark reuses prompts from `rawResponse.prompt`, so it intentionally
  measures the same request shape that the app sent to llama.cpp.

## Follow-up

For Windows llama.cpp Vulkan PDF translation, test increasing the PDF shim
llama.cpp batch ceiling from 4 toward 16, or make it profile-driven. The change
should be validated with:

- the same PDF replay benchmark;
- a real in-app PDF translation profile;
- error rate and timeout checks on lower-end Vulkan devices;
- confirmation that pdf2zh worker concurrency still reaches the shim reliably.
