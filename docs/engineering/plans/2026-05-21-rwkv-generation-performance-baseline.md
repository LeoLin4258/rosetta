# RWKV Generation Performance Baseline

Date: 2026-05-21

Status: evidence captured

## Purpose

This note records the evidence behind the statement:

> The local RWKV model generation speed is normal on the tested Mac mini M4. The slow user-visible translation experience is not explained by Rosetta launching the model incorrectly or by unusually slow core token generation.

Use this document when someone asks why long-document translation feels slow. The short answer is:

> Core model generation is in the expected range. The remaining slowdown is in the translation endpoint / batching / document pipeline layer, not in raw model inference speed.

## Device And Runtime

- Device: Mac mini M4, 16 GB memory
- Runtime binary: `/Applications/Rosetta.app/Contents/MacOS/rwkv-server`
- Backend: `web-rwkv`
- Model: `RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`
- Tokenizer: `b_rwkv_vocab_v20230424.txt`
- Rosetta-managed test port: `127.0.0.1:64092`
- Direct terminal test port: `127.0.0.1:65001`
- Both runs used the same binary, model, tokenizer, backend, and request shape.

Rosetta's managed launch command shape is:

```bash
/Applications/Rosetta.app/Contents/MacOS/rwkv-server \
  --model "$HOME/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab" \
  --tokenizer "/Applications/Rosetta.app/Contents/Resources/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt" \
  --backend web-rwkv \
  --host 127.0.0.1 \
  --port <ephemeral-port> \
  --model-name rwkv-translate
```

## Official Reference

RWKV's official performance page reports Apple Silicon `web-rwkv` `tg128` generation throughput in the same rough range:

- M4 Pro 12-core, RWKV7-G1 2.9B, nf4/float4: `32.95 tokens/s`
- M4 Pro 12-core, RWKV7-G1 2.9B, fp16: `33.98 tokens/s`
- M4 Pro 12-core, RWKV7-G1 2.9B, int8: `47.70 tokens/s`
- M2 8-core, RWKV7-G1 2.9B, nf4/float4: `21.65 tokens/s`

Sources:

- [RWKV official Apple and other hardware performance data](https://www.rwkv.cn/docs/RWKV-Performance-Data/others)
- [RWKV-Inference-Performance-Test issue #22](https://github.com/RWKV-Vibe/RWKV-Inference-Performance-Test/issues/22)

The official numbers are not a perfect apples-to-apples comparison because they use a benchmark workload and a 2.9B model. They are still useful as a sanity check for `web-rwkv` generation throughput on Apple Silicon.

## Core Generation Test

Endpoint:

```text
POST /v1/completions
```

Request shape:

```json
{
  "model": "rwkv-translate",
  "prompt": "<fixed prompt>",
  "max_tokens": 128,
  "temperature": 0.0,
  "top_p": 0.7,
  "top_k": 20
}
```

The server response includes `timings.predicted_per_second`, which is the closest available measurement to the official `tg128 tokens/s` number.

Each backend was tested with three prompts and six repeats per prompt. The first repeat for each prompt was treated as warm-up and excluded from the summary.

### Results

| Backend | Samples | Avg generated tokens | Avg tokens/s | Median tokens/s | Avg elapsed | Avg process CPU | Max sampled CPU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rosetta-managed | 15 | 57.0 | 32.83 | 33.28 | 1752.9 ms | 33.1% | 38.4% |
| Direct terminal | 15 | 57.0 | 34.80 | 34.98 | 1652.0 ms | 28.1% | 35.1% |

Interpretation:

- Rosetta-managed generation was only about 6% slower than the direct terminal launch.
- Both paths are in the same performance class.
- Rosetta's launch path is not causing a large inference slowdown.
- The observed `32.83-34.80 tokens/s` is close to the official M4 Pro `web-rwkv` nf4 reference of `32.95 tokens/s`.

Important caveat: the translate model often emitted a stop condition before 128 generated tokens, so these are `tg128-ish` measurements rather than perfect full-length `tg128` runs. The server's own timing field is still the best available local evidence for core generation speed.

## Translation Endpoint Test

Endpoint:

```text
POST /v1/batch/chat
```

Language roles were set first:

```text
POST /v1/chat/roles
{"user_role":"English","assistant_role":"Chinese"}
```

Both backends reported supported batch sizes:

```json
{
  "model": "rwkv-translate",
  "supported_batch_sizes": [1,2,3,4,5,6,7,8,9,10,11,12]
}
```

Each batch size was tested with six repeats. The first repeat was treated as warm-up and excluded.

### Results

| Backend | Batch | Avg latency | Segments/s | Source chars/s | Output chars/s | Avg process CPU | Max sampled CPU |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rosetta-managed | 1 | 1589.7 ms | 0.629 | 86.2 | 123.7 | 31.0% | 35.6% |
| Rosetta-managed | 2 | 2271.4 ms | 0.881 | 117.6 | 165.1 | 16.5% | 39.5% |
| Rosetta-managed | 4 | 3803.0 ms | 1.052 | 139.9 | 195.4 | 13.9% | 49.6% |
| Rosetta-managed | 8 | 7840.3 ms | 1.022 | 142.8 | 196.6 | 13.8% | 53.0% |
| Rosetta-managed | 12 | 12113.5 ms | 0.993 | 138.4 | 190.9 | 13.4% | 54.1% |
| Direct terminal | 1 | 1396.1 ms | 0.717 | 98.2 | 140.5 | 28.2% | 32.3% |
| Direct terminal | 2 | 2131.1 ms | 0.941 | 125.6 | 175.7 | 23.6% | 37.4% |
| Direct terminal | 4 | 3855.0 ms | 1.040 | 138.3 | 192.5 | 17.8% | 33.6% |
| Direct terminal | 8 | 7381.1 ms | 1.084 | 151.5 | 208.0 | 14.8% | 36.1% |
| Direct terminal | 12 | 12045.1 ms | 0.998 | 139.1 | 191.2 | 11.6% | 40.9% |

Interpretation:

- The `/v1/batch/chat` translation endpoint tops out around `1 segment/s` for this fixture.
- Throughput improves from batch 1 to batch 4, then largely plateaus.
- Direct terminal launch and Rosetta-managed launch remain close.
- This points away from Rosetta's runtime launch path and toward translation endpoint behavior, batch scheduling, stop behavior, or document pipeline overhead.

## Why The Mac May Stay Cool

The process CPU samples were low to moderate during the tests. That matches the user's observation that the Mac mini stayed warm rather than hot and the fan did not ramp.

This does not contradict the generation-speed conclusion. The core `/v1/completions` test still produced `32-35 tokens/s`, close to the official Apple Silicon `web-rwkv` reference. A cool machine here most likely means:

- the workload is not saturating the whole M4 package;
- the generation path is efficient enough not to produce much heat;
- `/v1/batch/chat` may have service-layer or scheduling limits that keep utilization low;
- temperature is not a reliable proxy for model correctness or token throughput.

`powermetrics` GPU / power / thermal sampling was not captured because the command required a sudo password in the test session.

## Responsibility Boundary

Supported by the measurements above:

- Rosetta is launching the same `rwkv-server` binary with the expected `web-rwkv` backend.
- Rosetta-managed core generation is within about 6% of a direct terminal launch.
- Core generation speed is close to official Apple Silicon `web-rwkv` reference data.
- The user's slow experience is not primarily caused by Rosetta launching the model incorrectly.

Not proven by these measurements:

- That `/v1/batch/chat` is optimally implemented.
- That batch scheduling is optimal for translation workloads.
- That document-level Rosetta orchestration has no overhead.
- That GPU / Neural Engine / full-package power is maximally utilized.
- That current translation model stop behavior is ideal for long documents.

## Recommended Answer When Asked Why Translation Feels Slow

Use this wording:

> We measured raw RWKV generation separately from the document translation path. Raw `/v1/completions` generation on the Mac mini M4 is about `32.8 tokens/s` through Rosetta-managed startup and `34.8 tokens/s` through direct terminal startup. That is close to RWKV's official Apple Silicon `web-rwkv` reference numbers. So the local model is not abnormally slow and Rosetta's launch path is not the main bottleneck. The slow part is the translation endpoint / batching / document pipeline layer, where `/v1/batch/chat` plateaus around `1 segment/s` on the tested fixture.

Shorter version:

> The model is fast enough. The translation pipeline is the bottleneck.

## Follow-Up Work

Recommended next investigations:

1. Measure `/v1/batch/chat` internals: prefill time, generation time, stop reason, active batch shrink steps, and output token count per segment.
2. Reduce or disable sidecar INFO logs that print full source text and cache details.
3. Investigate why multiple orphaned `rwkv-server` processes remain after Rosetta sessions. The test machine had 11 Rosetta-launched sidecar processes before the direct benchmark was started.
4. Add a repeatable local benchmark script that records both `/v1/completions` token/s and `/v1/batch/chat` document-style throughput.
5. If GPU / package utilization matters, rerun with `sudo powermetrics --samplers cpu_power,gpu_power,thermal -i 1000` during the benchmark.
