# RWKV Runtime Progress Snapshot

## Date

2026-05-08

## Summary

RWKV runtime 模块已经完成本地托管运行时的管理骨架，当前没有接入真实下载、安装 Python、启动 RWKV Lightning 或模型推理。

当前重点应从继续堆 skeleton 转向 Stage 0 实测：

- 确认 RWKV Lightning 启动命令和最小依赖
- 确认 `/translate/v1/batch-translate` 的真实响应格式、错误格式和稳定 batch size
- 确认 Windows 上 CPU/GPU 可用性和首发 runtime 包策略

## Implemented In App

### Tauri Commands

当前已有 commands：

```txt
get_rwkv_runtime_status
initialize_rwkv_runtime_layout
get_rwkv_runtime_install_plan
get_rwkv_runtime_install_progress
prepare_rwkv_runtime_install
get_rwkv_runtime_artifact_catalog
scan_rwkv_runtime_artifacts
extract_rwkv_runtime_artifact
```

这些 commands 均为窄接口，不暴露任意 shell 执行。

### Local Layout

当前固定使用 Tauri app data 下的托管目录：

```txt
runtime/rwkv-lightning/
models/rwkv-v7-g1-translate/1.5b/
logs/
```

### Status Model

runtime status 支持：

```txt
not-installed
partial
installed
invalid
```

### Manifest Validation

已实现校验：

- runtime manifest id 必须以 `rwkv-lightning-` 开头
- model manifest id 必须是 `rwkv-v7-g1-translate-1.5b`
- model `contextTokens` 必须是 `4096`
- model `supportedDirections` 必须包含 `en-zh` 和 `zh-en`
- `sha256` 如果存在，必须是 64 位小写十六进制
- model manifest 必须提供 `filename` 和 `sha256`
- model artifact 必须在 Rosetta 管理目录内
- 如果 `sizeBytes` 存在，则必须匹配本地文件大小
- 本地 artifact SHA-256 必须匹配 manifest
- artifact filename 不允许绝对路径或 `..` 逃逸

普通状态刷新和安装计划只做轻量校验，不重新读取大文件计算 SHA-256，避免设置页在已放入 1.3GB runtime zip 和 3GB model 后卡住。完整 SHA-256 校验只在用户显式点击扫描文件或解压运行时时执行。

Manifest reader 已兼容 UTF-8 BOM，避免 Windows 手动写入 manifest 时导致 `expected value at line 1 column 1`。

### Install Plan And Progress

已实现：

- 安装计划：runtime/model 分别返回 `missing`、`ready`、`invalid`
- 安装进度 skeleton：整体返回 `queued`、`ready`、`blocked`
- progress item 返回 `pending`、`ready`、`blocked`

当前进度仍然由本地状态派生，不进行网络下载。

### Artifact Catalog

已实现 artifact catalog skeleton：

- `RWKV Lightning runtime`
- `RWKV v7 G1 Translate 1.5B model`

当前 catalog 中 model item 和 Windows amd64 runtime item 均已通过 ModelScope metadata 确认，catalog 可用于后续下载实现。

已确认 artifact：

- `rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip`
- `RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth`

### Manual Artifact Scan

已实现离线/手动下载文件扫描：

- 扫描固定管理目录，不接受任意前端文件路径
- 只识别固定的 expected filenames
- 校验文件大小
- 校验 SHA-256
- 通过校验后写入 runtime/model manifest
- 扫描结果返回已写入 manifest 路径和错误列表

当前扫描目标：

```txt
runtime/rwkv-lightning/rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
models/rwkv-v7-g1-translate/1.5b/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
```

这一步不下载大文件，只消费用户已经放入管理目录的文件。

### Runtime Extraction

已实现受限 runtime zip 解压：

- 解压前快速校验 runtime manifest 或 runtime zip size
- 只解压 Rosetta 管理目录内的固定 runtime artifact
- 解压目标固定为 `runtime/rwkv-lightning/runtime-bundle/`
- 拒绝 unsafe zip entry path
- 解压完成后必须存在 `rwkv_lightning.exe`
- 如果 `runtime-bundle/rwkv_lightning.exe` 已存在，则直接返回，不重复解压

解压动作不再隐式扫描 model，也不重复读取大文件计算 SHA-256。完整 SHA-256 校验由用户显式点击扫描文件触发。

本地真实 zip 检查确认该包包含 Windows 可执行文件和 DLL，不需要用户安装 Python：

```txt
rwkv_lightning.exe
rwkv_vocab_v20230424.txt
torch_cpu.dll
torch_cuda.dll
cudnn64_9.dll
```

`rwkv_lightning.exe --help` 和 missing-model 启动探针均以 exit code 1 退出且无 CLI 输出。后续启动管理不能依赖 help text 或 stderr，需要使用进程状态、端口 readiness 和 HTTP probe。

### Settings UI

设置页已展示：

- 本地 RWKV 状态
- app data 路径
- runtime bundle 路径
- `rwkv_lightning.exe` 路径
- 安装计划
- 安装进度
- artifact catalog
- 手动扫描已放入管理目录的 RWKV 文件
- 解压已校验的 RWKV runtime
- 手动 RWKV API 配置

## Tests

当前 Rust runtime 相关测试：26 个。

覆盖：

- layout 缺失
- partial layout
- valid installed manifests
- invalid JSON
- invalid model id
- invalid directions
- malformed sha256
- artifact 缺失
- artifact size mismatch
- artifact hash mismatch
- artifact path escape
- install plan missing/ready/invalid
- install progress queued/ready/blocked
- artifact catalog metadata pending
- catalog target directory contract
- 手动扫描空目录
- 扫描有效 artifact 后写 manifest
- 扫描 artifact hash mismatch
- runtime zip 解压成功路径
- runtime zip 缺少 executable
- zip entry path escape 拒绝
- 设置页状态刷新不 hash 大模型文件
- 已解压 runtime 直接快速返回
- runtime zip size mismatch 在解压前拒绝
- UTF-8 BOM manifest 读取

最近验证命令：

```txt
cargo fmt
cargo test rwkv_runtime
corepack pnpm typecheck
cargo check
```

按项目要求没有运行：

```txt
corepack pnpm dev
corepack pnpm build
```

## External Findings

### RWKV Lightning

RWKV Lightning 是 RWKV 批量推理后端，教程说明它基于 Albatross 和 Robyn，支持批量推理 API，并提供 `/translate/v1/batch-translate`。

已确认的启动形态：

```txt
python app.py --model-path <your model path> --port <your port number> --password <password>
```

已确认的翻译接口形态：

```txt
POST http://localhost:8000/translate/v1/batch-translate
```

示例请求：

```json
{
  "source_lang": "en",
  "target_lang": "zh-CN",
  "text_list": ["Hello world!", "Good morning"]
}
```

中译英也有对应示例：

```json
{
  "source_lang": "zh-CN",
  "target_lang": "en",
  "text_list": ["你好世界", "早上好"]
}
```

Source:

- <https://www.rwkv.cn/tutorials/intermediate/rwkv_lightning>
- <https://github.com/RWKV-Vibe/rwkv_lightning>

### Translation Model

`Alic-Li/RWKV_v7_G1_Translate` 模型卡显示：

- RWKV-V7 翻译模型
- 约 0.4B 和 1.5B 参数规模
- 基于 `BlinkDL/rwkv7-g1` 微调
- 支持英中和中英
- ctx 4096
- Apache-2.0

Source:

- <https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate>

已确认的 HF mirror 1.5B artifact metadata：

```txt
filename: RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
sizeBytes: 3055445546
sha256: b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa
downloadUrl: https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate/resolve/main/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
```

该 metadata 来自 Hugging Face model API / tree API，不涉及权重下载。

ModelScope API 确认同一 1.5B 文件 metadata 一致：

```txt
filename: RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
sizeBytes: 3055445546
sha256: b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa
downloadUrl: https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
```

ModelScope API 同时确认了 Windows amd64 runtime zip：

```txt
filename: rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
sizeBytes: 1321825122
sha256: e4957c0dc771ea949d24f1d15123848dc2243546db62f4928c695c799c99e881
downloadUrl: https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
```

该 metadata 来自 ModelScope repo files API，不涉及权重或 runtime zip 下载。

### Demo Script

模型 demo 使用：

```txt
torch
rwkv
gradio
RWKV_V7_ON=1
RWKV_JIT_ON=1
RWKV_CUDA_ON=1
strategy='cuda fp16'
rwkv_vocab_v20230424
```

当前 main 分支 demo 中可见的模型路径是 0.4B 示例，不是 1.5B 的最终 Rosetta 目标：

```txt
RWKV_v7_G1a_0.4B_Translate_ctx4096_20250914_95%.pth
```

Source:

- <https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate/blob/main/webui_new.py>

## Current Gaps

### Must Confirm Before Real Download

- 是否首发只支持 Windows amd64 + CUDA 13.2 / sm75-120 runtime 包
- 是否同时提供 HF mirror fallback 下载源
- 是否需要用户选择本地文件作为离线安装路径

### Must Confirm Before Runtime Launch

- RWKV Lightning 是否可以绑定 `127.0.0.1`
- 是否支持无 password 本地运行
- health check endpoint 是否存在；如果不存在，使用哪个 endpoint 做 ready check
- stdout/stderr 是否可能泄露源文本或译文
- Windows 上 `python app.py --model-path ...` 的启动耗时和失败格式

### Must Confirm Before Translation Connector

- `/translate/v1/batch-translate` 的真实响应 JSON shape
- 单条失败与整批失败如何返回
- 超时行为
- batch size 上限
- source/target lang 取值是否只支持 `en`、`zh-CN`

## Recommended Next Step

当前已经具备不由 App 下载大文件的离线扫描路径。下一步应进入 Stage 0 runtime spike：

1. 通过 App command 或设置页完成扫描与解压。
2. 设计并实现受控启动 command。
3. 启动后检查进程状态和 `127.0.0.1:8000` 端口 readiness。
4. 用最小 text list 调 `/translate/v1/batch-translate`。
5. 记录响应格式、错误格式和性能指标。
6. 形成 runtime launch ADR。

## Stage Status

```txt
Plan document                          Done
Runtime status skeleton                Done
Layout initialization                  Done
Manifest read/validation               Done
Artifact file validation               Done
Install plan skeleton                  Done
Install progress skeleton              Done
Artifact catalog                       Done, ModelScope model/runtime metadata confirmed
Manual artifact scan                   Done
Runtime zip extraction                 Done
Settings extracted-state display       Done
Runtime ADR                            Pending Stage 0
Local model/runtime files              Done on current workstation
RWKV Lightning launch                  Pending process/port probe
Translation connector                  Pending runtime validation
One-click install                      Pending download strategy
One-click start                        Pending runtime launch validation
```
