# 2026-05-14 Managed RWKV Dynamic Batch + Once-Per-Run Roles (Phase 6.A)

## Context

Phase 5 端到端跑通后留下两块"临时活"，本次清理：

1. **batch size 硬编码**：Phase 5 临时常量 `LOCAL_RUNTIME_BATCH_SIZE = 8` 与
   Rust `SIDECAR_MAX_BATCH_SIZE = 12` 把"上限"当魔法数字硬编码进了 product 路径。
   两者都是 dev 防护栏，不是产品决策（[memory: project-rwkv-mobile-batch-size-limit](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_batch_size_limit.md)）。
2. **/v1/chat/roles 每个 batch 都重设**：sidecar 的 roles 是 server 全局状态，
   一次 50 段 batch=8 的 run 会浪费 6 次 RTT 重设同一对角色。

Phase 6.A 改成 Rust 启动每个 run 时**先查一次** `/v1/batch/supported_batch_sizes`
拿到模型实际上限再决定 batch size，**roles 也只在 run 开头设置一次**。

## Changes

### `rwkv_providers/mod.rs`

- `ProviderTranslateBatch` 删除 `source_lang` 字段。原因：translate_batch 不再
  调 set_roles，方向信息从此由 orchestrator 在 run 开头通过
  `set_chat_roles_for_pair` 处理。保留 `target_lang` 用于响应解析（strip
  assistant_role 前缀）。
- 加注释说明"为什么 source_lang 不在这里"，给未来 provider 实现者一个明确信号。

### `rwkv_providers/mobile_batch_chat.rs`

- **新增 `query_supported_batch_sizes(config) -> Result<Vec<u32>, String>`**：
  GET `/v1/batch/supported_batch_sizes`，解析 `{"supported_batch_sizes":[...]}`，
  空数组 / 非 2xx / 解析失败都返回 Err（fail-loud）。8 秒超时。
- **新增 `pick_batch_size(supported, hint) -> usize`**：
  - `hint = 0` → 用 supported.max（"auto"）
  - `hint > 0` → `min(hint, supported.max)`
  - 都做 floor=1 保护
- **新增 `set_chat_roles_for_pair(config, source_lang, target_lang, cancel)`**：
  独立 pub fn，orchestrator 在 run 开头调一次。
- **`translate_batch` 不再调 `/v1/chat/roles`**：注释里写明这是 caller 的责任。
- `probe()` 自己调 `set_chat_roles_for_pair` 再调 `translate_batch`，保持
  一次性 probe 的语义独立。
- 5 个新单测覆盖 `pick_batch_size` 的 4 个分支 + `SupportedBatchSizesResponse`
  的 Phase 0 真实响应形状解析。

### `rwkv_api.rs::start_mobile_batch_chat_run`

- **删除 `SIDECAR_MAX_BATCH_SIZE = 12` 硬编码 clamp**——dynamic 接管。
- 启动顺序：
  1. 解析 jobs / segments / targets（不变）
  2. 注册 run + 写初始状态（不变）
  3. **新**：`query_supported_batch_sizes` → 失败立即 fail run + 返回错误（不
     marking 任何 segment 为 translating，重试时状态干净）
  4. **新**：`pick_batch_size(supported, request.batch_size)`
  5. **新**：`set_chat_roles_for_pair(...)` 一次性设置方向，失败同样 fail run
  6. 循环 `targets.chunks(effective_batch_size)` 调 `translate_batch`（每次不
     再走 roles，省一个 HTTP RTT）
- `translate_rwkv_mobile_batch_chat_texts`（一次性 ad-hoc 翻译命令）也加了
  set_chat_roles_for_pair 调用，行为与 probe 对齐。

### `JobsPage.tsx`

- 删除 `LOCAL_RUNTIME_BATCH_SIZE = 8` 临时常量及其 provider 分支。
- `runTranslationBatches({ batchSize: BATCH_SIZE })` 给所有 provider 用同一
  hint；mobile-batch-chat 在 Rust 端 clamp，外部 API 用 16 verbatim。
- 注释说明 16 是 hint，Rust 会按 sidecar 实际上限 clamp。

## Compatibility

- `cargo test --lib`：100 passed（旧 95 + 新 5）。
- `cargo check` / `cargo clippy --lib --all-targets`：通过；Phase 6 改动**零新警告**。
- `pnpm typecheck`：通过。
- 外部 API（`rwkv-lightning-contents`）路径**完全不变**——它不走 mobile_batch_chat
  模块，也不查 supported_batch_sizes。
- 公开 Tauri 命令名 / 请求形状不变；frontend bridge 代码无 breaking change。

## Known Boundary

- **没做 segment 长度桶选 batch（Phase 6.B）**：当前所有 segment 用同一
  effective_batch_size。后续若发现长段 (>1000 字) batch=12 容易超时或 OOM，再
  按长度桶调小（短段最大、超长段先 split）。
- **没做"中途换方向"语义**：set_roles 只在 run 开头调一次。如果用户在 run 进行
  中切换 source/target lang 创建新 run，会自动重设——不是问题。但如果两个 run
  在同一 sidecar 进程并发跑（目前 UI 不允许），后启动的 run 会覆盖前者的 roles。
  v1 单运行时单方向（[ADR 0003](../decisions/0003-macos-first-managed-rwkv-runtime.md)
  第 5 节）已锁定这个约束。
- **`/v1/batch/supported_batch_sizes` 是 sidecar 进程生命周期内不变的常量**：
  Phase 6.A 不缓存（每个 run 调一次，~ms 级 RTT，可以接受）。如果未来发现单
  sidecar 频繁短 run 时这是热点，再加进程级缓存。
- **取消 / 日志审计未在本次完成**：Phase 0 还差三项（cancel test、日志审计、
  M1/M2 基准）；Phase 6.A 把 batch 调度收口，那三项放 Phase 6.C 或单独迭代。

## Verification

- 静态：cargo / pnpm / clippy 全绿（细节见 Compatibility）。
- **实机待验证**：
  - 用户在 Apple Silicon M4 mini 上 `pnpm tauri dev` 启动；Settings 启动 sidecar；
    Jobs 翻译 ~50 段 Markdown，应看到：
    - sidecar log 没有 `chat_batch: batch size N is not supported`。
    - sidecar log 仅在每个 run 开头有 **一次** `set_chat_roles_for_pair` 调用，
      而不是每个 batch 都有。
    - 翻译总耗时比 Phase 5（8/batch + roles 重设）略快（少了 N-1 个 roles RTT）。

## Next

Phase 6.B（可选 / 按需）：
- Segment 长度桶 batch 选择：长段 batch 减半，超长段 split 后再翻译。
- 进程级 `supported_batch_sizes` 缓存（如果 dev 测时发现 RTT 是瓶颈）。

Phase 6.C：
- Cancel 中断行为实测（HTTP abort 后 sidecar 是否干净退出 in-flight generation）。
- 日志审计：grep sidecar log 确认 segment 文本不进 disk-persisted log。
- M1 / M2 实机基准（plan 顶部状态表里 Phase 0 的"3 项待补"）。
