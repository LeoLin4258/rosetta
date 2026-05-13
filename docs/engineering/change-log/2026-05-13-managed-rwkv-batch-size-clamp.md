# 2026-05-13 Managed RWKV Batch-Size Clamp (Phase 5 follow-up)

## Context

Phase 5 端到端验证时翻译立即失败。sidecar log 给出原因：

```
[ERROR] chat_batch: batch size 16 is not supported
```

Rosetta 在 [JobsPage.tsx](../../../rosetta-app/src/features/jobs/JobsPage.tsx) 把 `BATCH_SIZE = 16` 当成所有 provider 的默认值。外部 API (`rwkv-lightning-contents`) 历史上能吃 16，但新接入的本地 sidecar (`rwkv-mobile-batch-chat`) 的 `/v1/batch/supported_batch_sizes` 上限是 12，**超界直接 hard-fail 整个 batch**——不是软降级。

Phase 0 的 [验证笔记](../plans/2026-05-13-rwkv-mobile-macos-validation-notes.md) 写了 `supported_batch_sizes: [1..12]`，但没把"超过 → 整个 run fail"这层含义传到 batch 调度代码里。

## Changes

### 前端 — provider 感知的 batch size

[`rosetta-app/src/features/jobs/JobsPage.tsx`](../../../rosetta-app/src/features/jobs/JobsPage.tsx)：

- 新增常量 `LOCAL_RUNTIME_BATCH_SIZE = 8`，仅给 `rwkv-mobile-batch-chat` 用（保守留余量，sidecar 上限 12）。
- `BATCH_SIZE = 16` 不动，继续给外部 API 用。
- 每次 run 启动前按 `provider.id` 选 batch size 传给 `runTranslationBatches`。

### Rust — 防御性 clamp

[`rosetta-app/src-tauri/src/rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs)：

- `start_mobile_batch_chat_run` 入口加 `SIDECAR_MAX_BATCH_SIZE = 12` 硬 clamp。前端无论传多少，Rust 都会压到 12 以内，避免任何前端 regression 把整个翻译 run 弄废。

## Known Boundary

- 两个常量都是 dev 防护栏，不是产品决策。Phase 6 的"动态 batch size"会改成每次 run 启动前 GET `/v1/batch/supported_batch_sizes`，取实际上限再按 segment 长度桶调小。届时这两个常量都可以删掉。
- 不同机型 / 不同模型可能给出不同的 `supported_batch_sizes` 数组——12 是 M4 mini + 1.5B G1c nf4 实测值，不是协议保证值。
- 外部 API 路径没有这个限制，因为它的"batch"是单 HTTP 请求里多个 source text，server 内部串行/并行调度，没有 GPU slot 概念。

## Verification

- `cargo check`：通过。
- `pnpm typecheck`：通过。
- **实机验证**：Apple Silicon M4 mini，本地 sidecar (`http://127.0.0.1:<ephemeral>`)，跑一篇真实 Markdown 中→英 翻译——sidecar log 不再报 `batch size 16 is not supported`，翻译成功完成。Phase 5 端到端首次跑通。

## Memory

记入 [memory: project-rwkv-mobile-batch-size-limit](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_batch_size_limit.md)，方便未来会话 / 新开发者第一次写 batch 调度时不踩同一坑。
