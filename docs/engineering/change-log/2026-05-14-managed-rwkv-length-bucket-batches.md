# 2026-05-14 Managed RWKV Length-Bucket Batch Planner (Phase 6.B)

## Context

Phase 6.A 把 batch ceiling 从硬编码 12 改成"查 `/v1/batch/supported_batch_sizes` + 按 user hint clamp"。但运行时仍然用 `targets.chunks(N)` 把所有 segment 当成同质单元来切——一个 batch 里既可能塞 12 段 50 字的标题，也可能塞 12 段 2000 字的长段。后者意味着 sidecar 要在一次前向里同时处理大量 token，**容易触发 token 上限 / 高显存压力 / 单段超时把整批拖死**。

Phase 6.B 让 Rust 在 run 内按 segment 长度自适应：**短段保持 ceiling、中段减半、长段降到 1/4、超长段单独跑**。文档顺序不打乱，单段不预切（预切留 Phase 6.C）。

## Changes

### `rwkv_providers/mobile_batch_chat.rs`

新增两个 pub fn + 7 个单测：

- **`pick_batch_for_length(ceiling, max_chars) -> usize`**：返回当前批次能容纳的最大 slot 数。bucket 阈值：
  - `0..=300` 字符 → ceiling（短段满速）
  - `301..=1200` → `ceiling.div_ceil(2)`（中段，避免单 batch token 总量爆）
  - `1201..=2500` → `ceiling.div_ceil(4)`（长段，给 sidecar 留缓冲）
  - `> 2500` → 1（巨段单独跑，Phase 6.C 可改为先 split）
  - 所有结果都 `.max(1).min(ceiling)`，保证 batch ≥ 1 且不超 ceiling。
- **`plan_batches<T, F>(targets, ceiling, text_len_fn) -> Vec<Vec<&T>>`**：贪心装桶。语义：
  1. 顺序遍历 targets，保持文档顺序。
  2. 维护当前 batch 的 `max_chars`；新 segment 加入会更新 `max_chars`。
  3. 用更新后的 `max_chars` 计算 cap：若当前 batch + 1 已超过 cap，**先 flush 当前 batch**，新 segment 起一个新批。
  4. 闭包 `text_len_fn` 把"长度"这个概念从 `Segment` 解耦——测试用 `String::chars().count()`，run loop 用 `Segment::source_text`。

行为示例（ceiling=12）：

| 输入序列 | 计划批次 |
| --- | --- |
| 15 个短段 | `[12, 3]` |
| 8 短 + 1×1500 字长 + 8 短 | `[8, 3, 6]` |
| 短 + 3000 字超长 + 短 | `[1, 1, 1]` |
| ceiling=1 + 5 段任意 | 5 个单元素 batch |

### `rwkv_api.rs::start_mobile_batch_chat_run`

- 把 `pick_batch_size(...)` 结果改名 `ceiling`（语义更明确）。
- `targets.chunks(effective_batch_size)` 替换为 `plan_batches(&targets, ceiling, |s| s.source_text.chars().count())`，返回 `Vec<Vec<&Segment>>`。
- 循环改为 `for batch in &planned_batches`，内部 `batch.iter()` 现在产 `&&Segment`，靠 auto-deref 调 `.id.clone()` / `.source_text.clone()`。
- `batch.len()` 用法不变，验证 `result.translations.len() != batch.len()` 仍然正确。

## Compatibility

- 旧的"所有 batch 同 size"行为是 Phase 6.B 行为的 **degenerate case**：如果文档里所有 segment 都 ≤ 300 字符，`plan_batches` 等价于 `chunks(ceiling)`。
- 段顺序保持。`Segment::order` 仍按文档顺序，preview / export 不需要任何特殊处理。
- 不影响外部 API（`rwkv-lightning-contents`）路径——它不走 mobile_batch_chat。

## Known Boundary

- **不预切超长段**：当前 > 2500 字的 segment 整段一次性发给 sidecar（batch=1）。如果实测有 segment 触发 sidecar 上下文长度上限（4096 tokens 训练长度），需要 Phase 6.C 加 pre-split：用换行 / 句号切分 → 多个子 segment → 翻译 → 拼接。
- **bucket 阈值是启发式**：300/1200/2500 是经验值，没有针对 M1/M2/M3/M4 实测过。Phase 6.C 的 M1 基准跑出来后再调。改这几个数字不需要改调用点。
- **没有进程级缓存**：每个 run 仍调一次 `query_supported_batch_sizes`。loopback HTTP < 1ms RTT，不是热点。未来若发现单 sidecar 反复短 run 时 RTT 累计有感，再加按 `base_url` 索引的 `Mutex<HashMap>` 缓存。
- **`chars().count()` 不等于 token 数**：中文一个字符约 1.5–2 token，英文一个 char ≈ 0.25 token。bucket 用 chars 是因为它免预 tokenize，且对绝大多数文本类型来说 char 长度和 token 长度是单调相关的——足够当 bucket 信号。如果未来要更精确，得在 sidecar 暴露 tokenize 端点或在 Rust 嵌入 tokenizer。

## Verification

- `cargo test --lib`：**107 passed**（旧 100 + 新 7）。
- `cargo check` / `cargo clippy --lib --all-targets`：通过；Phase 6.B 改动**零新警告**。
- `pnpm typecheck`：通过。
- **实机待验证**：随便丢一个混合长度的 Markdown（短段 + 中段 + 一两段超长）进 Rosetta，预期：
  - 短段部分仍以 ceiling 大小批量处理（吞吐不退步）。
  - 长段 / 超长段自动落入小批，sidecar log 中不会再出现 `chat_batch: batch size N is not supported` 或单 batch 超时 fail。
  - 完成顺序与文档顺序一致。

## Next

Phase 6.C 备选：
- 超长段 pre-split（按换行 / 句号），翻译后拼接。
- M1 / M2 实机基准，根据真实数据调 bucket 阈值。
- Cancel 中断实测 + sidecar 日志审计（Phase 0 剩下 3 项）。
