# RWKV 本地运行时一键启动计划

## 状态

Draft

## 背景

Rosetta 需要让非技术用户也能使用本地 RWKV 翻译模型。目标不是“允许用户配置一个本地 API 地址”，而是用户安装 Rosetta 后，可以在应用内启用本地翻译模型，并在不打开终端、不手动安装 Python 的情况下完成长文档翻译。

当前推荐模型：

- ModelScope：`AlicLi/RWKV_v7_G1_Translate`
- Hugging Face 镜像：<https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate>
- 第一目标模型：1.5B

当前推荐推理项目：

- RWKV Lightning 教程：<https://www.rwkv.cn/tutorials/intermediate/rwkv_lightning>
- RWKV Lightning 仓库：<https://github.com/RWKV-Vibe/rwkv_lightning>

本文档是计划文档，不是 ADR。最终运行时选择需要在 Stage 0 和 Stage 1 完成本地实测后，单独记录到 `docs/engineering/decisions/`。

## 产品边界

这项工作必须保持 Rosetta 的窄产品定位：

- 本地文档翻译
- 隐私敏感文件
- 长文本与文档结构保留
- 通过本地模型 API 做批量翻译

不得引入：

- 云端上传
- 账号或登录
- 遥测
- 聊天界面
- 通用 AI 助手能力
- 文档问答、改写、总结

只有用户明确执行下载模型或下载运行时等动作时，才允许联网。源文档、译文、segment 文本和文档结构不得上传。

## 当前调研结论

### 模型

`AlicLi/RWKV_v7_G1_Translate` 是面向翻译的 RWKV-V7 模型，不是通用聊天模型。公开信息显示：

- 模型规模包括约 0.4B 和 1.5B
- 主要语言方向包括英译中和中译英
- 上下文约 4096 tokens
- License 为 Apache-2.0

需要本地验证的问题：

- ModelScope 上 1.5B 模型的准确文件名、大小和校验值
- ModelScope 与 Hugging Face 文件是否一致
- 1.5B 在 CPU、NVIDIA GPU、核显机器上的实际内存和显存占用
- 是否已有可用量化格式，或是否需要 Rosetta 自己准备量化包

### RWKV Lightning

RWKV Lightning 是第一阶段最合适的运行时候选，因为它已经暴露翻译相关的 batch API：

- `/translate/v1/batch-translate`
- `/big_batch/completions`
- OpenAI-compatible chat-style endpoints

MVP 默认应使用 `/translate/v1/batch-translate`。它接受结构化翻译输入：

```json
{
  "source_lang": "en",
  "target_lang": "zh-CN",
  "text_list": ["Text to translate"]
}
```

这个接口能直接映射 Rosetta 的 `Segment[] -> text_list -> translated Segment[]`，不需要 Rosetta 在 MVP 阶段拼 prompt 或解析 completion。

`/big_batch/completions` 可以作为后续极速模式候选，但不应作为第一默认接口。它可能更快，但 Rosetta 需要负责 prompt 模板、停止词、输出解析和异常恢复。

### 运行时包装风险

RWKV Lightning 当前是 Python 项目，不是面向桌面用户的一键二进制运行时。教程默认用户有 Python 环境，并能安装 PyTorch 等依赖。这个体验不适合作为 Rosetta 的用户路径。

因此 Rosetta 需要托管运行时策略：

- 用户不应手动安装 Python
- 用户不应为了 Rosetta 手动安装 CUDA
- 用户不应运行 `pip`、`conda` 或 `python app.py`
- Rosetta 负责检测、下载、校验、启动、health check、日志和停止

## 推荐架构

```txt
React UI
  -> Tauri commands
      -> RWKV Runtime Manager
          -> managed runtime package
          -> model files
          -> rwkv_lightning API server
          -> logs and health checks
  -> Translation Connector
      -> http://127.0.0.1:{port}/translate/v1/batch-translate
```

职责划分：

- React 只展示状态和用户动作。
- Zustand 保存用户可见的运行时设置和状态。
- Tauri commands 暴露窄运行时操作。
- Rust 管理本地路径、子进程、端口、日志和文件校验。
- 翻译调度继续面向本地 HTTP API。

前端不得暴露任意命令执行。Rust 命令边界应保持窄接口：

```txt
get_rwkv_runtime_status
install_rwkv_runtime
start_rwkv_runtime
stop_rwkv_runtime
get_rwkv_runtime_logs
```

后续可选命令：

```txt
delete_rwkv_runtime
delete_rwkv_model
open_rwkv_runtime_folder
run_rwkv_runtime_diagnostics
```

## 运行时分发方案

### 方案 A：托管 Python Runtime 包

Rosetta 首次启用本地模型时，下载预构建 runtime 压缩包到 app data 目录。压缩包包含私有 Python runtime、固定版本依赖、RWKV Lightning 代码和启动器。

示例目录：

```txt
Rosetta/
  runtime/
    rwkv-lightning/
      python/
      app/
      site-packages/
      start-server.exe
      runtime-manifest.json
  models/
    rwkv-v7-g1-translate/
      1.5b/
        model.pth
        model-manifest.json
  logs/
    rwkv-runtime.log
```

优点：

- 不要求系统 Python
- 不污染全局 Python 或 PATH
- 比塞进安装包更容易更新
- 适合由 Tauri/Rust 管理进程生命周期

缺点：

- 首次安装可能需要下载数 GB
- PyTorch GPU 包体积大，平台差异明显
- 我们需要构建、签名、版本化和托管 runtime 包

结论：

作为 MVP 主路径。

### 方案 B：PyInstaller Sidecar

把 RWKV Lightning server 打成包含 Python 和依赖的可执行 sidecar。

优点：

- 用户感知最简单，Rosetta 启动一个可执行文件
- 与 Tauri sidecar 模型匹配
- 不要求系统 Python

缺点：

- PyTorch 打包产物可能非常大
- CUDA 变体会让 release matrix 复杂
- 更新时需要替换大体积可执行文件
- Python 依赖问题调试可能更困难

结论：

作为备选 spike。如果托管 Python runtime 包过于脆弱，再评估。

### 方案 C：AI00 / Vulkan Runtime

调研 AI00 或其他非 Python RWKV runtime，作为中长期替代方案。

优点：

- 避免 Python 和 PyTorch 分发问题
- Vulkan 可能更好覆盖 NVIDIA、AMD、Intel GPU
- 如果质量一致，更适合 consumer 桌面分发

缺点：

- 需要验证模型格式兼容性
- 需要验证 batch 翻译 API
- 可能需要 Rosetta 自己做 prompt 和输出解析
- 翻译质量和性能可能与 RWKV Lightning 不一致

结论：

并行 spike，但不阻塞 RWKV Lightning MVP。

## 用户体验

设置页应提供一个本地 RWKV 状态区：

```txt
未安装
正在下载运行时
正在下载模型
已安装
正在启动
已就绪
启动失败
已停止
```

主动作保持简单：

```txt
启动本地 RWKV
```

如果缺少必要文件，同一入口可以引导安装：

```txt
安装本地 RWKV
```

UI 应显示：

- 模型名称和大小
- runtime 状态
- 启动后的本地 API 地址
- 最近一次 health check 时间
- 简短失败原因
- 面向高级用户的诊断日志入口

UI 不应显示：

- 大段 Python traceback 作为主错误
- prompt 模板
- CUDA 调参作为一级控件
- 聊天式控件

## 本地存储

使用 Tauri app data 目录，不使用仓库相对路径，也不默认要求用户选择任意系统目录。

建议结构：

```txt
AppData/Rosetta/
  runtime/
    rwkv-lightning/
  models/
    rwkv-v7-g1-translate/
  jobs/
  logs/
```

每个下载产物都应有 manifest：

```json
{
  "id": "rwkv-lightning-windows-x64-cpu",
  "version": "2026.05.07",
  "source": "https://...",
  "sha256": "...",
  "installedAt": "2026-05-07T00:00:00Z"
}
```

模型 manifest 记录：

```json
{
  "id": "rwkv-v7-g1-translate-1.5b",
  "source": "modelscope",
  "filename": "...",
  "sha256": "...",
  "sizeBytes": 0,
  "contextTokens": 4096,
  "supportedDirections": ["en-zh", "zh-en"]
}
```

## 安全与隐私

运行时命令必须窄接口、参数校验。

要求：

- 推理服务绑定到 `127.0.0.1`，不得绑定 `0.0.0.0`
- 默认端口被占用时自动选择本地端口
- 日志写入 app data
- 日志不得记录源文档文本或译文
- 使用前校验下载产物 hash
- release build 优先使用签名 runtime 包
- 不给前端开放宽泛文件系统权限
- 不允许前端运行任意二进制或 shell 命令

## Translation Connector 合约

MVP 目标接口：

```txt
POST /translate/v1/batch-translate
```

Rosetta 映射：

```txt
sourceLang -> source_lang
targetLang -> target_lang
segments.map(sourceText) -> text_list
```

返回结果必须按 segment 顺序回填。如果 runtime 支持单条错误，Rosetta 只标记对应 segment 失败。如果整个请求失败，scheduler 应先缩小 batch 重试，再把 segment 标记为失败。

初始语言范围：

```txt
English -> Chinese
Chinese -> English
```

其他语言方向必须经过模型验证后再加入。

## 性能验证

Stage 0 必须先收集真实数据，再定 UI 默认值。

需要测量：

- 冷启动时间
- 热启动时间
- 首次请求延迟
- tokens 或字符吞吐
- 峰值内存
- 显存占用
- CPU fallback 是否可用
- 不同长度 bucket 的稳定 batch size
- 长输入失败模式

建议 bucket：

```txt
small: 0-120 chars
medium: 120-800 chars
large: 800-2000 chars
huge: 翻译前继续切分
```

初始保守默认值：

```txt
stable: small 32, medium 8, large 2
balanced: small 64, medium 16, large 4
fast: small 128, medium 32, large 8
```

这些数值只是占位，必须用 1.5B 模型实测后修正。

## 失败处理

预期失败：

- 模型缺失
- runtime 缺失
- 下载中断
- hash 校验失败
- 端口占用
- server health check 失败
- 内存或显存不足
- GPU 或驱动不支持
- 请求超时
- runtime 返回格式异常

用户主界面错误应短而可操作：

```txt
本地 RWKV 启动失败：模型文件不完整。
```

详细诊断写入：

```txt
logs/rwkv-runtime.log
```

## 实施阶段

### Stage 0：Runtime Spike

目标：

证明 1.5B 模型能通过 RWKV Lightning 在本机稳定批量翻译。

任务：

- 下载 1.5B 模型
- 手动运行 RWKV Lightning
- 测试 `/translate/v1/batch-translate`
- 测试英译中和中译英
- 测试 segment 长度和 batch size
- 记录内存、显存、启动耗时、延迟和错误行为

退出标准：

- 有可复现的手动启动命令
- Rosetta 能发送 batch 请求并收到有序译文
- 初始 scheduler 限制有实测依据
- 集成前的 blocker 被记录清楚

### Stage 1：Runtime ADR

目标：

把最终运行时方向记录为 ADR。

任务：

- 新增 `docs/engineering/decisions/0002-rwkv-runtime.md`
- 说明为什么选择该 runtime
- 说明被拒绝或推迟的方案
- 说明 app data 布局和 Tauri 命令边界
- 说明未来如果切换 AI00 的迁移路径

退出标准：

- runtime 方向被接受，或明确标记为 experimental
- 后续实现有稳定架构依据

### Stage 2：Runtime Manager Skeleton

目标：

增加 Tauri/Rust 侧 runtime 状态和进程生命周期基础能力。

任务：

- 新增 Rust runtime manager 模块
- 实现 status command
- 实现本地端口选择
- 实现 health check
- 实现进程启动和停止
- 实现日志路径和日志读取

退出标准：

- 前端可以展示 runtime 状态
- Rust 可以通过同一生命周期路径启动/停止一个本地测试服务
- 没有暴露任意 shell 执行

### Stage 3：托管 Runtime 安装

目标：

让 Rosetta 在没有系统 Python 的机器上安装或定位 runtime 和模型。

任务：

- 定义 runtime manifest
- 定义 model manifest
- 实现下载进度
- 实现 hash 校验
- 实现可重试下载
- 解压到 app data
- 处理部分安装和失败清理

退出标准：

- 干净 Windows 机器可从 Rosetta UI 安装 runtime 资产
- 中断安装可以重试
- 启动前所有资产都经过校验

### Stage 4：Translation Connector 集成

目标：

把 Rosetta segment scheduler 接到本地 RWKV runtime。

任务：

- 新增 `/translate/v1/batch-translate` connector
- 映射 `Segment[]` 到 `text_list`
- 按顺序回填结果
- 处理请求级失败和单项失败
- batch 失败后缩小 batch 重试
- 避免日志记录源文和译文

退出标准：

- 一组文本 segment 可以通过本地 runtime 翻译
- 失败 batch 能产生可恢复 segment 状态
- connector 有针对性测试覆盖

### Stage 5：UI 集成

目标：

让 runtime 成为 Rosetta 工作台的一部分。

任务：

- 设置页增加本地 RWKV 状态区
- 增加安装、启动、停止动作
- 显示简短失败状态
- 显示当前模型和本地 API 地址
- 提供诊断入口，但不作为主要 UI
- 翻译流程能感知 runtime readiness

退出标准：

- 非技术用户可以在 Rosetta 内安装、启动和使用模型
- 不需要终端步骤
- UI 仍然是桌面翻译工作台

### Stage 6：Runtime 替代方案 Spike

目标：

判断非 Python runtime 是否能替代或补充 RWKV Lightning。

任务：

- 测试 AI00 或其他 Vulkan/native RWKV runtime
- 验证模型格式兼容性
- 验证翻译质量
- 验证 batch 吞吐
- 比较分发复杂度

退出标准：

- 给出 keep、switch 或 defer 建议
- 如果决定切换，新增 ADR，不静默替换 runtime

## 需要同步更新的文档

开始实现后：

- 新增 runtime choice ADR
- 每个大阶段完成后更新 `docs/engineering/change-log/`
- 如果 runtime 状态或模型 metadata 进入持久化任务数据，更新 `docs/engineering/conventions/data-models.md`
- 如果引入新的前端状态或 UI 约定，更新 frontend conventions

## 开放问题

- ModelScope 上 1.5B 的准确文件名和 checksum 是什么？
- 第一版 runtime 包应该 CPU-only、CUDA-only，还是按 backend 拆分？
- Rosetta 应该声明的最低 Windows 版本和 GPU 驱动要求是什么？
- 1.5B 在 CPU-only 机器上是否可用？
- `/translate/v1/batch-translate` 是单项错误还是整请求错误？
- RWKV Lightning 是否能稳定避免在日志里输出源文本？
- 是否提供 0.4B 作为低内存机器 fallback？
- AI00 是否足够兼容，能否成为长期默认 runtime？

