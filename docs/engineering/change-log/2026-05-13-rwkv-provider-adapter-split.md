# 2026-05-13 RWKV Provider Adapter Split (Phase 1.A)

## Context

[ADR 0003](../decisions/0003-macos-first-managed-rwkv-runtime.md) 恢复了托管本地 RWKV runtime 工作，确定了 macOS Apple Silicon + `rwkv-mobile` WebRWKV 后端 + `/v1/batch/chat` API 契约。在动到 sidecar 打包、runtime 模块、UI 之前，第一步是**让 Rosetta 的翻译 connector 能区分 "外部 API（旧契约）" 和 "本地 sidecar（新契约）"**，否则 Phase 3 之后的代码会被迫继续硬编码旧 connector 形状。

实施计划 [`2026-05-13-macos-rwkv-one-click-implementation.md`](../plans/2026-05-13-macos-rwkv-one-click-implementation.md) Phase 1 即此次工作。本次完成 Phase 1.A（新增 mobile-batch-chat 适配器、新 Tauri commands、前端 provider 选择器），Phase 1.B（把旧 lightning-contents 路径也搬到同一抽象下做去重）推到下一迭代。

## Changes

### 新增 provider 抽象层

- 新建 [`rosetta-app/src-tauri/src/rwkv_providers/mod.rs`](../../../rosetta-app/src-tauri/src/rwkv_providers/mod.rs) — 共享 `ProviderTranslateBatch` / `ProviderTranslateResult` 类型，为后续多 provider 提供统一的批量翻译输入/输出形状。
- 新建 [`rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs`](../../../rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs) — 完整的 `rwkv-mobile-batch-chat` provider 实现：
  - `translate_batch()` — 先 POST `/v1/chat/roles` 设置 user/assistant role，再 POST `/v1/batch/chat` 发起批量请求。
  - `probe()` — 用两条短句做 probe，便于 UI 显示"本地 RWKV 已就绪"。
  - 响应解析按 `assistant_role` 动态切分 `<原文>\n\n<lang>: <译文>` 格式（**不**硬编码 `Chinese:`，方向反过来时前缀变 `English:`）。详见 [memory: project-rwkv-mobile-translate-response-format](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_translate_response_format.md)。
  - 全套 13 个单测覆盖：含前缀/无前缀/多行原文/缺失 index/乱序/空 content/非 JSON/role 子串干扰。
  - 取消支持：HTTP 请求与响应读取都用 `tokio::spawn + abort` 配合 `AtomicBool` cancel token，与现有 `rwkv_api.rs` 行为一致。

### 新增 Tauri commands

在 [`rosetta-app/src-tauri/src/rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs) 末尾追加（旧 5 个 command 一字未动）：

- `probe_rwkv_mobile_batch_chat`
- `translate_rwkv_mobile_batch_chat_texts`
- `start_rwkv_mobile_batch_chat_run`

cancel 与 status 命令保持共享（`cancel_rwkv_translation_run` / `get_rwkv_translation_run_status` 按 runId 工作，与 provider 无关）。

[`rosetta-app/src-tauri/src/lib.rs`](../../../rosetta-app/src-tauri/src/lib.rs) 注册新 commands 并加 `mod rwkv_providers;`。

### 前端 provider 路由

- [`rosetta-app/src/types/rosetta.ts`](../../../rosetta-app/src/types/rosetta.ts) — 新增 `RwkvProviderId` 联合类型、`RwkvProviderHandle` 区分联合、三个 mobile-batch-chat 请求 payload 类型。
- [`rosetta-app/src/lib/rwkvApi.ts`](../../../rosetta-app/src/lib/rwkvApi.ts) — 新增 `probeRwkvMobileBatchChat` / `translateRwkvMobileBatchChatTexts` / `startRwkvMobileBatchChatRun` bridge 函数。共享生命周期函数（cancel / status）保持现签名。
- 新建 [`rosetta-app/src/lib/providers/index.ts`](../../../rosetta-app/src/lib/providers/index.ts) — `selectProvider()` 选择器：按 `override` > `managedRuntimeReady` > 默认 `rwkv-lightning-contents` 三级回落。当前所有现有调用点 `managedRuntimeReady=false`，行为完全等同于 Phase 1.A 之前。
- [`rosetta-app/src/lib/translationRunner.ts`](../../../rosetta-app/src/lib/translationRunner.ts) — `runTranslationBatches()` 接受可选 `provider: RwkvProviderHandle`，未传则自动派生 lightning-contents handle。`startRunForProvider` 按 provider id 调用相应 Tauri command。

## Compatibility

- 既有外部 API 翻译流：**行为完全不变**。旧 5 个 commands 不动；`translationRunner` 默认走 `selectProvider({managedRuntimeReady: false})` → `rwkv-lightning-contents`，调用 `startRwkvTranslationRun`。
- 翻译文件 JSON / segment 状态机：不变。
- 翻译运行注册表 (`RwkvTranslationRunRegistry`)：保持共享，新旧 provider 都注册同一 registry，cancel/status 跨 provider 一致工作。
- TypeScript public types：仅新增，不修改已有类型。
- Cargo 依赖：未新增。

## Known Boundary

- mobile-batch-chat 路径的 run orchestrator (`start_mobile_batch_chat_run`) 当前与 lightning-contents 路径（`start_translation_run`）是接近复制粘贴。**Phase 1.B 会把两者抽到同一个 generic loop**；为了让 Phase 1.A 是一次干净的非破坏性增量，去重推后。已在 `rwkv_api.rs` 加 TODO 注释。
- mobile-batch-chat provider 还没有真正在生产 UI 中被调用——`selectProvider` 当前永远回落到 lightning-contents。要触发新路径需要 Phase 5 把 managed runtime 状态接进 store；本次仅在 Phase 0 已启动的本地 sidecar 上用 curl 验证了相同 HTTP shape + 响应解析算法。
- mobile-batch-chat 请求体目前 `max_tokens: 1024` 写死。Phase 6 接 batch size 调度时会调成按 segment 长度桶动态选取，并查 `/v1/batch/supported_batch_sizes` 缓存。
- `/v1/chat/roles` 是 server 全局状态，每个 batch 都会重设。后续若发现高频重设有性能/稳定性影响，再做"方向未变跳过重设"的小优化。

## Verification

- `cargo check` 通过。
- `cargo test --lib` 通过，78 个测试（含 13 个新 mobile_batch_chat 单测）。
- `cargo clippy --lib --all-targets` 无新增警告（5 个预存 warning 全在 `rosetta_jobs`/`rwkv_api`/`rwkv_runtime`，与本次改动无关）。
- `pnpm typecheck` 通过。
- 端到端冒烟（用 Rust 代码完全一致的 HTTP body 打 Phase 0 已启动的本地 sidecar）：`/v1/chat/roles` 接受 `English`/`Chinese` 设置，`/v1/batch/chat` 返回 2 段含前缀 content，本地用 Python 模拟 `strip_response_prefix` 算法正确提取纯中文译文。

## Next

下一步进入 Phase 2：macOS CI 编译 sidecar + Tauri 打包配置 + macOS bundle target，让 Rosetta 能从源码产出可分发的 `.app`/`.dmg`。
