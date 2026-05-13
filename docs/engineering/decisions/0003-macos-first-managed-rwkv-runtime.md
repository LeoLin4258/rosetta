# 0003 macOS-First Managed RWKV Runtime

## Status

Accepted

## Date

2026-05-13

## Context

[ADR 0002](0002-pause-managed-rwkv-runtime.md) 暂停了 Rosetta 的托管本地 RWKV runtime 工作，等待 RWKV 工程侧确认 runtime 方案、模型格式、批量翻译 API 契约和分发策略。该 ADR 留下的恢复门槛是"出现可信的首发 runtime 方案，并经过实测验证"。

2026-05-12 工程侧确认了首发方向；2026-05-13 在 Mac mini M4（Apple Silicon arm64, macOS 26）上完成上游契约验证（[`docs/engineering/plans/2026-05-13-rwkv-mobile-macos-validation-notes.md`](../plans/2026-05-13-rwkv-mobile-macos-validation-notes.md)），结果满足 ADR 0002 列出的全部恢复条件：

- 推荐 runtime：[`MollySophia/rwkv-mobile`](https://github.com/MollySophia/rwkv-mobile)，**WebRWKV 后端**（基于 wgpu/Metal）。原计划想走 MLX 后端不可行：HuggingFace 上 `mollysama/rwkv-mobile-models` 的 `mlx/` 目录已更名为 `coreml/`，当前没有 MLX 格式的翻译权重；且 MLX 后端依赖额外的 Swift/Xcode FFI 静态库构建。
- 首发硬件：Apple Silicon arm64（M1 及以后）。Intel Mac 与 Windows 延后到独立 phase（见本仓库实施计划的 Phase 8）。
- 模型格式：`.prefab`（nf4 量化），WebRWKV 兼容。首发型号 `RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`，实测 1.3 GB。
- 批量翻译契约：`POST /v1/batch/chat` + `POST /v1/chat/roles`（设置 user_role/assistant_role）+ `GET /v1/batch/supported_batch_sizes`。choices[].index 严格还原请求顺序。响应 content 包含原文回显 + `\n<assistant_role>:` 前缀 + 译文，前端解析按 assistant_role 动态切分。
- 隐私 / 绑定：sidecar 必须显式 `--host 127.0.0.1`，默认 `0.0.0.0` 不安全。
- 分发：rwkv-mobile 仓库与 Rosetta 同公司同事项目（[memory: project-rwkv-mobile-same-company](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_same_company.md)），不存在 license 阻塞，可放心打包分发。

## Decision

**恢复**托管本地 RWKV runtime 工作，按以下边界推进，详细计划见 [`docs/engineering/plans/2026-05-13-macos-rwkv-one-click-implementation.md`](../plans/2026-05-13-macos-rwkv-one-click-implementation.md)：

### 1. v1 范围

- 首发目标平台：**macOS Apple Silicon (arm64)**。
- 首发 runtime：rwkv-mobile + WebRWKV 后端。
- 首发模型：1.5B nf4 prefab，EN↔ZH 双向。
- 形态：Tauri sidecar 独立进程，HTTP 仅绑 `127.0.0.1` + ephemeral port。
- 用户体验：UI 一键安装（下载模型 + 启动 sidecar），不开终端、不装 Python/brew/CUDA。

### 2. v1 不做

- Windows、Intel Mac 一键 runtime（留到 Phase 8）。
- MLX 后端（等上游发布 MLX 格式翻译权重）。
- 多模型选择 / 模型动物园 / 通用 AI 助手能力。
- 云端默认、登录、同步、遥测、协作。
- 聊天 UI、prompt 沙盘、文档 Q&A、摘要、改写。

### 3. 架构边界

三层严格分开，不互相耦合：

```
document pipeline ─── translation provider adapter ─── managed runtime provider
（导入/分段/译文/预览/导出）  （请求响应/协议契约）       （安装/启动/停止/探活）
```

- document pipeline 不感知后端是 MLX、CUDA、WebRWKV 还是远程 API。
- translation runner 通过 provider 抽象选择本地 sidecar 或外部 API。
- managed runtime 只负责生命周期，不知道翻译 prompt 怎么拼。

### 4. provider 抽象（已落地）

- `rwkv-lightning-contents` — 外部 API（[ADR 0002](0002-pause-managed-rwkv-runtime.md) 时期的 `/v1/chat/completions` + `contents[]`），保留为外部 API/远程后端路径。
- `rwkv-mobile-batch-chat` — 本地 sidecar 的 `/v1/batch/chat` 路径，前置 `/v1/chat/roles` 设置方向。
- `custom-rwkv-api` — 用户显式配置的其他后端（未来）。

provider 选择由前端 `selectProvider()` 根据 runtime 就绪状态/用户 override 决定。

### 5. 单运行时单方向

`/v1/chat/roles` 是全局 server 状态。v1 内同一时刻只支持单一翻译方向；切换方向需要 stop & start 或重新调 roles。多方向并发翻译留给未来。

### 6. 分发与签名

- sidecar (`rwkv_server`) + dylib (`librwkv_mobile.dylib`) + 词表 (`b_rwkv_vocab_v20230424.txt`) 由 Rosetta 自己的 CI 在 macOS arm64 runner 上从源码编译并打包。
- 模型权重不进 app 包，首次启动从 HuggingFace（走代理）或 ModelScope（推荐首选）下载，~1.3 GB，SHA256 校验。
- macOS codesign + notarize 整链路是 Phase 7 发布加固的硬性退出条件。

### 7. 隐私要求继承

ADR 0002 的隐私条款全部继承且加严：

- sidecar 绑定 `127.0.0.1`，不暴露任何外部端口。
- 源文档、译文、segment 文本、prompt 不进 runtime 日志、manifest、错误诊断或上传通道。
- 远程后端只能由用户显式选择，UI 必须明示译文将离开本机。
- 不引入登录、同步、遥测、协作能力。

## Consequences

- ADR 0002 的"暂停"状态被本 ADR 取代；本 ADR 在该文件末尾标注 supersession note，不删除其历史上下文。
- 现有 `rwkv_runtime.rs`（Windows libtorch 骨架）保留，Phase 3 重构时拆为平台 profile，Windows profile 暂置 `enabled: false`。
- 现有外部 API connector 路径不动，已重命名为 `rwkv-lightning-contents` provider，作为长期可选后端。
- 本 ADR 不禁止未来增加 Windows/Linux 一键 runtime；它们各自需要新的 ADR（runtime 选型 + 后端可行性）并独立 phase。
- "傻瓜式本地 LLM" 的产品承诺只对 macOS Apple Silicon 用户做出；其他平台用户在 UI 中看到外部 API 配置作为回落路径。

## Open Questions

下列需要在后续 phase 中确认或验证，不阻塞当前决策：

- M1 8GB 实机能否稳定加载 1.5B nf4（M4 已验证；最低目标机未测）。
- 取消语义：发起 batch 后中断 HTTP 连接，sidecar 是否干净中止 generation 而不留 zombie 状态。
- 日志默认级别下是否完全不打印 prompt / 译文（M4 启动日志干净，但 batch 翻译过程中的日志未做完整审计）。
- macOS codesign 链路对嵌入式 sidecar + 第三方 dylib 的实际限制（Phase 2/7 实测）。
- HuggingFace 国内可达性的长期稳定性（hf-mirror.com 在 LFS 重定向上不稳，2026-05-13 实测；Phase 4 默认走 ModelScope）。
