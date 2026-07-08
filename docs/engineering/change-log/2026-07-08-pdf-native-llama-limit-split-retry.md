# 2026-07-08 PDF Native llama.cpp Limit Split Retry

## Summary

Fixed a PDF v2 native translation failure where a llama.cpp `/completion`
request could hit `truncated=true` / `stop_type=limit` and fail an entire PDF
page window.

The observed local Windows failure was:

```txt
PDF 译文生成失败：llama.cpp 响应格式不可用: llama.cpp completion was truncated (truncated=true, stop_type=limit)
```

The failing job used the native PDF unit translation path with
`llama-cpp-chat-completions`, not the older OpenAI-compatible PDF shim path.
The managed llama.cpp runtime was already using the current packaged baseline:

```txt
--ctx-size 16384 --parallel 16
```

Runtime logs showed one short prompt running away until the per-slot context was
exhausted. The strict provider parser correctly rejected that partial response.

## Root Cause

The native PDF unit translator split normal non-Lightning PDF text into
moderate chunks, but if one llama.cpp item in a batch returned a limit or
truncation failure, the whole batch failed immediately.

The older PDF shim path had split-retry recovery for this class of failure; the
native v2 unit path did not.

## Change

For the native PDF unit path only:

- llama.cpp `truncated=true` or `stop_type=limit` responses are still rejected
  as provider failures;
- when a llama.cpp PDF unit batch fails for that reason, Rosetta retries the
  affected batch items with smaller PDF chunk budgets;
- retry output is joined back into the original PDF unit before rendering;
- non-limit provider errors still fail immediately;
- the same placeholder splitting and reconstruction behavior is preserved.

The retry budgets are intentionally narrower than the first-pass chunk budget:

```txt
first pass:  target 72 / hard 88 estimated prompt tokens
retry:       target 36 / hard 44
final retry: target 24 / hard 32
```

This does not accept partial llama.cpp output. It treats the raw failed
completion as failed, records the truncation-like metric, and succeeds only if
a fresh smaller retry produces complete responses.

## Validation

Targeted Rust test:

```powershell
cd rosetta-app\src-tauri
cargo test unit_translation
```

Result:

```txt
10 passed; 0 failed
```

Formatting note:

```powershell
cd rosetta-app\src-tauri
cargo fmt -- --check
```

This still reports an existing rustfmt diff in
`src/managed_pdf2zh/layout.rs`. That file was not changed as part of this fix.

