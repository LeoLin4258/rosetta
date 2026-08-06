# Docling PDF-to-Markdown 实验

日期：2026-08-06  
Docling 仓库：`C:\Users\Leo\Documents\GitHub\docling`  
Docling commit：`9b454c9e88454d95fd04d538c552a3c07bc3c04d`  
实验平台：Windows、CPython 3.13.3、CPU、Docling 2.118.0、torch 2.13.0+cpu

## 结论摘要

- 普通文字型 PDF 在关闭 OCR 后可以转成 Markdown。布局、标题、段落、页码来源和表格结构均可得到；但 Markdown 不是无损中间格式，复杂表格的合并单元格会退化，部分 PDF 文本会出现词内误空格。
- 标准本地模型路径可以完全离线运行，但首次运行会从 Hugging Face 下载模型；必须将模型固定到应用自己的 artifacts 目录，并在生产配置中关闭 `torch.compile`，否则没有 MSVC `cl` 的普通 Windows 环境会失败。
- 体积明显超过 400 MB：只带 PDF 后端的修正后环境约 310 MB，但不能启动标准 PDF CLI；可用的本地布局环境约 979 MB，模型缓存约 530 MB，解压合计约 1.51 GB；`env-local + hf-cache-local` 压缩后仍为 792.17 MB。
- 技术上可以嵌入 Rosetta，但不适合作为主进程内库。推荐独立 Python worker/sidecar，通过本地 IPC 返回 Docling JSON；保留一个长期运行的 converter 复用模型，避免每份 PDF 重载约 1.4 GB 的进程内存。
- 当前版本不建议把 Docling 标准能力直接塞进 Rosetta 的安装包。若 400 MB 是硬上限，应继续使用现有轻量 PDF 路径，或把 Docling 作为可选下载组件，并单独评估其许可和升级策略。

## 测试语料

语料来自 Docling 仓库 `tests/data/pdf/sources`，均为带文字层的 PDF：

| 文件 | 页数 | 用途 |
| --- | ---: | --- |
| `normal_4pages.pdf` | 4 | 韩文双栏报告，含标题、摘要、脚注、图片和页眉页脚；主验收样本 |
| `amt_handbook_sample.pdf` | 1 | 手册型页面，含多个标题和图片 |
| `2305.03393v1-pg9.pdf` | 1 | 英文论文双栏页面，含复杂表格 |
| `code_and_formula.pdf` | 2 | 代码块和公式边界样本 |

页面已使用 Poppler `pdftoppm` 渲染到 `tmp/pdfs/docling-spike/renders/` 做视觉对照。

## 安装组合和体积

体积为目录实际占用，未计入系统已有 Python；MB 为十进制，MiB 为二进制。

| 环境 | 安装组合 | 依赖目录 | 模型缓存 | 结果 |
| --- | --- | ---: | ---: | --- |
| `env-slim` | `format-pdf,convert-core,cli` | 310.42 MB | 0 | CLI 导入标准 PDF pipeline 时缺少 `docling_ibm_models`，不可作为标准转换环境 |
| `env-local` | `format-pdf,convert-core,models-local,cli` | 978.51 MB | 530.01 MB | 布局模型可用；未额外装 OpenCV 时表格路径失败，关闭表格后可用 |
| `env-full` | `standard` | 1,144.95 MB | 530.01 MB | 默认能力可用，额外包含 OCR、Office、Web、chunking 等 Rosetta 不需要的依赖 |
| `env-local + hf-cache-local` ZIP | 可用本地模型组合 | - | - | 792,167,657 bytes（792.17 MB） |

模型缓存分解：`docling-layout-heron` 约 171.77 MB；`docling-models` 约 358.24 MB，后者包含 TableFormer 的 accurate/fast 权重。`torch` 单目录约 475.4 MB，是本地环境的最大依赖。

只删除 headers、`.lib`、`__pycache__` 等开发文件可以节省一部分空间，但无法把运行时压到 400 MB 以下；这不是可接受的首选发布策略，因为需要维护脆弱的白名单。

## 转换结果

### 无 OCR、无表格模型

命令核心参数：

```powershell
$env:HF_HOME = "...\\hf-cache-local"
$env:HF_HUB_OFFLINE = "1"
$env:TRANSFORMERS_OFFLINE = "1"
$env:DOCLING_INFERENCE_COMPILE_TORCH_MODELS = "false"
docling normal_4pages.pdf --from pdf --to md --to json `
  --image-export-mode placeholder --no-ocr --no-tables --device cpu --output OUTPUT
```

`normal_4pages.pdf`：首次模型已缓存后离线转换成功，端到端约 15.6 秒，Docling 内部处理约 9.4 秒；其余三个样本也全部成功，单文件约 11.7–12.6 秒。输出同时包含 Markdown 和 JSON。

质量观察：

- 双栏阅读顺序基本正确，标题和章节可识别，页码来源信息保存在 JSON `prov` 中（`page_no`、bbox、charspan）。
- `normal_4pages.pdf` 的 Markdown 能保留正文、脚注和图片占位，但源 PDF 中的字符间距会造成 `돌 입하였다고`、`감염 증-19`、`확 진자`、`보 니` 一类词内误空格；这会影响后续翻译分段，不能把 Markdown 当作唯一权威文本。
- 默认 Markdown 图片模式是 base64 embedded；`normal_4pages.pdf` 输出约 63.9 KB，其中大部分是图片。使用 `--image-export-mode placeholder` 后约 18.7 KB，Rosetta 应禁用 embedded，图片另行管理。

### 表格开启

补齐完整 `env-full` 后，`2305.03393v1-pg9.pdf` 在 `--no-ocr --tables` 下成功，输出 1 个表格（JSON 约 69.3 KB，Markdown 约 2.8 KB）。表格数值和列结构基本可读，但 Markdown 将原表格中的 OTSL/HTML 成对行压进同一单元格（例如 `OTSL HTML`）；这是 Markdown 表达合并/跨行结构的限制。Rosetta 若需要可编辑、可翻译表格，应使用 Docling JSON 的 table grid/cell 数据，不应从 Markdown 反解析。

### 代码和公式

`code_and_formula.pdf` 成功识别代码 fenced block；公式输出包含 `<!-- formula-not-decoded -->`，没有得到 LaTeX。公式识别需要额外 enrichment 模型，不应纳入普通 PDF v1 的默认路径。

## 嵌入实验

使用 SDK 创建一个 `DocumentConverter`，在同一进程连续转换 `2305.03393v1-pg9.pdf` 和 `normal_4pages.pdf`：

```text
两份均 success；总耗时 15.006 秒
单份耗时 9.183 秒、5.823 秒（第二份复用已加载模型）
进程峰值 RSS 约 1,401.8 MiB
```

这证明 SDK 可以作为本地 worker 嵌入，但不适合在 Tauri Rust 主进程中直接承载。建议边界：

1. Rosetta Tauri 只负责启动、停止和监控 Python worker；worker 通过 stdin/stdout 或 localhost loopback IPC 接收本地文件路径和配置。
2. worker 返回 Docling JSON、Markdown（仅导出用途）以及诊断信息；Rosetta 自己把 JSON 映射为 block/segment，`source.pdf` 仍是权威文件。
3. worker 启动时设置 `DOCLING_INFERENCE_COMPILE_TORCH_MODELS=false`、固定 `HF_HOME/artifacts_path`，并拒绝远程服务和第三方插件。
4. 使用单 worker 长驻复用模型；不要每个文档创建一个进程。

### ONNX 方向

Docling 当前有 `docling-layout-heron-onnx` 的布局导出，但实际测试发现：即使安装 `models-onnxruntime`，导入 PDF pipeline 仍需要 `docling-ibm-models`；该包又拉入 torch/torchvision。因此在当前 2.118.0 代码路径下，ONNX extra 没有消除 PyTorch，不能作为本次发布的体积解法。若未来 Docling 将 `docling_ibm_models` 拆成无 torch 的公共解析包，应重新评估。

## 风险和后续建议

- 默认 `compile_torch_models=True` 在本机没有 `cl` 时失败；这是 Windows 发布必须覆盖的启动回归测试。
- 首次运行会访问 Hugging Face；离线部署必须预置并校验 artifacts，不能让用户在转换时临时下载。
- 模型权重许可需要按实际使用的 Hugging Face 仓库单独核验，不能只看 Docling 源码的 MIT 声明。
- 1.4 GB 峰值 RSS 和 1.5 GB 解压占用会明显影响 Rosetta 的启动、内存和安装体验。
- 应加入真实中文/英文长文、扫描 PDF（OCR 开关）、密码 PDF、损坏 PDF、超长 PDF 和取消/超时测试；本实验只覆盖文字层 PDF。

## 推荐决策

当前结论：**功能上可行，作为 Rosetta 默认内置能力不通过；作为可选本地 Docling worker/sidecar 有条件通过。**

下一步若继续做，应先做一个不影响主安装包的可选 worker POC，并定义 JSON 到 Rosetta block/segment 的映射和取消协议；在此之前不应把 `env-full` 或完整模型缓存纳入主安装包。

