# 0002 Pause Managed RWKV Runtime Work

## Status

Accepted

## Date

2026-05-08

## Context

Rosetta 的长期目标仍然是让普通用户可以在应用内傻瓜式运行本地 RWKV 翻译模型，不需要打开终端、不需要手动安装 Python 或 CUDA。

前期已经为托管本地 RWKV runtime 做了较多 skeleton：

- app data layout
- runtime/model manifest
- artifact catalog
- manual artifact scan
- runtime zip extraction
- process launch preflight
- Settings page runtime diagnostics

但当前本机实测暴露了两个问题：

- 已暂存的 RWKV Lightning artifact 是 CUDA/NVIDIA 包，不适合当前 AMD Radeon 780M 开发机。
- 真正适合首发用户路径的 runtime 方案、模型格式、硬件 backend、批量翻译 API 形态，需要先和 RWKV 模型工程师确认。

继续在 Rosetta 内实现模型下载、启动和进程管理，容易在 runtime 方案尚未定型时扩大错误方向的代码和 UI 面积。

## Decision

暂停 Rosetta 内置/托管 RWKV runtime 的继续开发。

当前开发阶段先跳过“一键本地运行 RWKV LLM”，改为连接 RWKV 工程师部署好的翻译模型 API，优先推进 Rosetta 的长文档翻译产品闭环：

- API 连接配置
- 外部 RWKV API 探测
- 翻译接口契约确认
- TXT/Markdown pipeline
- segment 调度
- 进度、失败重试和缓存
- 双语预览与导出

现有 runtime manager 代码和文档保留为实验性历史上下文，不删除、不作为当前功能依赖。

本决策不意味着 Rosetta 只能连接工程师临时部署的 API。长期产品形态可以支持多个由用户显式选择的 RWKV 后端：

- Rosetta 托管的本机 RWKV runtime
- 用户自己在本机或局域网部署的 RWKV LLM API
- 用户明确配置的远程 / 云端 RWKV API

这些后端都必须服务于 Rosetta 的窄目标：长文档翻译、结构保留和批量处理。它们不得把 Rosetta 扩展成聊天产品、通用 AI 助手、云同步服务或团队协作产品。

## Current Development Rule

在恢复本地 runtime 工作之前，后续功能开发应遵循：

- 不继续扩展 `start_rwkv_runtime`、下载器、runtime installer、模型 artifact 管理或 one-click launch UI。
- 不让翻译 pipeline 依赖 Rosetta 托管 runtime 的 readiness。
- 当前实际开发入口是外部 RWKV translation API connector，优先验证非流式 batch API。
- 翻译 connector 应面向“已存在的 RWKV 翻译 API base URL”，无论该 API 是本机、局域网、用户自部署远程服务，还是工程师临时部署环境。
- 远程 / 云端 API 必须是用户显式配置和选择的可选后端，不能成为默认路径。
- 如果未来支持云端 API，UI 必须清楚提示源文档和译文会离开本机，并保留本地 / 自部署 API 作为隐私优先路径。
- 不引入云上传、登录、同步、遥测或通用 AI 助手能力。
- 不把工程师部署 API 记录成 Rosetta 官方长期云服务方向；它只是开发阶段的外部模型 endpoint。
- 不把 API token、body password 或其它认证凭据写入仓库、文档、测试或 fixture。
- 不把源文档、译文、segment 文本或文档结构写入 runtime 诊断日志。

## Future Resume Conditions

只有在以下信息确认后，才恢复内置/托管 RWKV runtime 开发：

- 推荐 runtime 方案，例如 RWKV Lightning、AI00、llama.cpp RWKV 或其他 runtime。
- Windows AMD / Intel / NVIDIA 的首发 backend 策略。
- 模型格式和转换路径，例如 `.pth`、safetensors、量化包。
- 批量翻译 API 契约，包括请求、响应、错误、超时和并发语义。
- 是否需要 password/auth，以及如何保证只绑定本机或安全的本地访问。
- 分发、下载、校验、签名和升级策略。

恢复时应新增 runtime choice ADR，而不是直接把当前 skeleton 当作最终方案继续扩展。

## Consequences

- 当前 MVP 可以先验证 Rosetta 的文档翻译核心体验，不被本地模型分发问题阻塞。
- 现有 runtime skeleton 可能暂时留在 Settings 和 Tauri commands 中，但它是 parked/experimental，不代表当前产品路径。
- 后续开发者看到 runtime 文档时，应优先读本 ADR，再决定是否触碰 runtime manager。
- 最终本地一键运行目标没有取消，只是等待 RWKV 工程侧方案确认后再继续。
