# 2026-05-14 Managed RWKV Cancel Trace + Pre-Loop State Semantics

## Context

Phase 6.B 实机验证时，用户报告点击"停止"按钮对本地 sidecar 翻译无视觉效果——
段位仍显示"翻译中"、按钮不变、翻译继续。代码层面 cancel 链路看起来完整
（`cancel_rwkv_translation_run` 设置 AtomicBool → run loop 多点 `cancel.load()`
检查 → `send_request_with_cancel` 每 50ms 轮询 abort），但是体验上不通。

这一改加了两件事：

1. **沿链路插入 `[rwkv-cancel]` 诊断日志**，让下次 cancel 异常时能在 dev 终端
   直接看到信号链断在哪一环（cancel command 入口 → run loop 多个 check 点
   → HTTP-level `send_request_with_cancel` / `read_text_with_cancel` 的 abort 点）。
2. **修 Failed/Cancelled 语义 bug**：之前 `query_supported_batch_sizes` /
   `set_chat_roles_for_pair` 在 cancel 触发时返回 Err，run 状态被设成 `Failed`
   并向前端抛错；现在先 `cancel.load()` 判断，cancel 引发的 Err 走 `cancel_current_run`
   → 状态 `Cancelled` + 当前批次段回退 pending。

实机验证：用户重启 dev 后，点击停止 → 段立即回到"待翻译"、按钮立即变"翻译"、
几乎瞬间停下。预期成立。

## Changes

### `rwkv_api.rs`

- `cancel_rwkv_translation_run`：进入时打一行 `[rwkv-cancel] setting flag` 含
  当前 run state；找不到 run_id 时也打一行 `NOT FOUND` 便于诊断早 cancel。
- `start_mobile_batch_chat_run`：
  - 在 `query_supported_batch_sizes` **之前** 加一个 `cancel.load()` 检查（防御性，
    catch 极少数"用户在 run 注册到 registry 后、HTTP 还没开始时点 stop"的窗口）。
  - `query_supported_batch_sizes` / `set_chat_roles_for_pair` 的 Err 分支：若
    `cancel.load()` 为 true → 走 `cancel_current_run`（状态 Cancelled）而非
    `update_run_status(... Failed)`。
  - 主 batch loop：保留原有的"入口 + 翻译后" cancel 检查，**每个 check 点加
    一行 `[rwkv-cancel]` 诊断**（含 batch index、翻译结果 ok 字段）。
  - `for batch in &planned_batches` 改成 `for (batch_index, batch) in planned_batches.iter().enumerate()`
    以便诊断里能给 batch 编号。

### `rwkv_providers/mobile_batch_chat.rs`

- `send_request_with_cancel` / `read_text_with_cancel`：cancel 检查触发 abort
  时打一行 `[rwkv-cancel] ... aborting`。

## Diagnostic Quick-Reference

未来 cancel 异常时，从 `pnpm tauri dev` 那个终端筛 `[rwkv-cancel]` 行即可推断：

| 看到的行 | 说明 |
| --- | --- |
| 0 行 | cancel 命令根本没到 Rust——前端 stop 按钮或 cancelPromise 没 resolve |
| 只有 `cancel_rwkv_translation_run: NOT FOUND` | runId 不匹配（前端 / Rust 时序问题，理论上不该出现） |
| `setting flag` 出现，但没有后续行 | flag 设了但 run 自然完成 / cancel.load() 没读到（极少见，需要进一步排查内存序） |
| `setting flag` → `send_request_with_cancel: ... aborting` | cancel 在 HTTP POST 阶段拦截，正常 |
| `setting flag` → `read_text_with_cancel: ... aborting` | cancel 在等 sidecar 响应阶段拦截，正常 |
| `setting flag` → `mobile_batch_chat: cancel before batch #N` | cancel 落在两个 batch 之间，正常 |
| `setting flag` → `mobile_batch_chat: cancel after batch #N` | 当前 batch 翻译已完成但 cancel 触发了，cancel_current_run 回退该批次段 |

## Compatibility

- `cargo check`：通过。
- `cargo test --lib`：107 个测试不变（这一改不动业务逻辑，只加 trace 与
  纠正 state 语义）。
- 行为兼容：cancel 正常路径未变；Failed/Cancelled 区分只在用户主动 stop 时
  生效，常规 sidecar 故障仍标记 Failed。

## Known Boundary

- **`[rwkv-cancel]` 行直接走 `eprintln!`**，不是结构化日志，发版前考虑：
  - 换 `tracing::debug!` 并通过 `RUST_LOG` 控制
  - 或保留，cancel 是用户主动操作、频率极低，stderr 一两行噪音可接受
- 诊断行**不包含** segment 文本或译文（仍遵守 ADR 0003 隐私约定）。
- 未实测 `read_text_with_cancel` 的 abort 在 sidecar 持续 streaming 时是否
  会有 0–2 秒延迟（取决于 hyper 内部 buffer 刷新点）。当前用户体感"几乎
  瞬间停下"说明对 1.5B nf4 模型来说不是问题。
