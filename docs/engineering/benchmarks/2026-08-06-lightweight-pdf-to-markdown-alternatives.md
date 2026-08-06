# 轻量 PDF-to-Markdown 替代方案实验

日期：2026-08-06  
平台：Windows、CPython 3.13.3、CPU  
样本：Docling 仓库 `tests/data/pdf/sources` 中的 4 份文字型 PDF，关闭 OCR

> 后续验证：已绕过 Xberg 直接测试 `pdf_oxide 0.3.77`，并完成 structured spans -> 实验性 Rosetta IR 的恢复层。体积、性能、常规段落合并和精确重复页眉去除通过，但复杂 Figure 内文字会穿插论文正文，韩文跨行空格也无法用全局规则稳定决定。最终结论是停止通用 PDF -> Markdown 自研恢复层，不接入生产管线。详见 [`2026-08-06-pdf-oxide-direct-validation.md`](2026-08-06-pdf-oxide-direct-validation.md)。

## 结论摘要

- Docling 不是唯一可行路线。无模型方案在本机可达到 `0.02-0.21 秒/页`，比当前 Docling 长驻 worker 的约 `1.5 秒/页` 快约 7-87 倍。
- 复用现有 PDFium/轻量 Rust parser 的自研路线在体积和速度上最优，但后续 structured IR spike 已证明：一旦要求复杂 Figure、caption、footnote 和多栏内容不穿插，就会继续实现对象级布局引擎。它不再是推荐默认路线。
- Xberg 1.0.14 是目前最值得关注的 MIT/Rust 候选，4 页样本仅需 0.069 秒，峰值内存约 54 MiB；但本次韩文样本出现大量词间空格丢失，复杂表格也明显退化，当前质量不能直接作为默认转换器。
- pdftext 0.7.1 的文字和韩文空格质量好于 Xberg，4 页约 0.085 秒；但它只输出文字/JSON，不直接生成结构化 Markdown，双栏页的块顺序也会交错。其实现适合作为自研 PDFium 规则的参考，不适合作为最终产品能力直接包装。
- PyMuPDF4LLM 的旧无模型路径和当前 ONNX 布局路径速度、Markdown 质量、体积均明显优于 Docling。当前布局路径 4 页约 0.84 秒，隔离依赖压缩约 89 MiB；但是 PyMuPDF 为 AGPL-3.0/商业双许可，`pymupdf-layout` 为 PolyForm Noncommercial/商业双许可，与 MIT Rosetta 的默认商业分发不兼容，除非购买并确认商业许可。
- Marker、MinerU、Surya 等模型型方案依赖 Torch、Transformers、OCR/布局权重，仍会回到大包体、大内存和较慢启动，不值得作为当前轻量默认路径继续实测。

## 测试对象

| 候选 | 版本 | 核心实现 | 模型 | 许可结论 |
| --- | --- | --- | --- | --- |
| Xberg | 1.0.14 | Rust、`pdf_oxide`、lopdf、Markdown renderer | 本次关闭布局/OCR模型 | MIT，可用于 Rosetta |
| pdftext | 0.7.1 | Python、pypdfium2/PDFium | 无 | Apache-2.0，可用；需核对传递依赖 notices |
| PyMuPDF4LLM legacy | 0.2.0 + PyMuPDF 1.28.0 | MuPDF、启发式布局/表格 | 无 | AGPL-3.0 或商业许可，默认不可用 |
| PyMuPDF4LLM layout | 1.28.0 | MuPDF、ONNX 布局/表格 | wheel 内置小模型 | 非商业/商业双许可，默认不可用 |
| Docling 对照 | 2.118.0 | Python、Torch、布局与表格模型 | 约 530 MB | 技术可用，默认包体不通过 |

官方来源：

- Xberg: <https://github.com/xberg-io/xberg>
- pdftext: <https://github.com/datalab-to/pdftext>
- PyMuPDF4LLM: <https://github.com/pymupdf/pymupdf4llm>
- Marker: <https://github.com/datalab-to/marker>
- MinerU: <https://github.com/opendatalab/MinerU>

## 性能结果

所有候选均在一个进程内按相同顺序处理：复杂表格 1 页、韩文双栏 4 页、复杂表格 1 页、手册 1 页、代码公式 2 页。下表的峰值内存覆盖整个进程，速度为端到端 SDK 调用，不包含安装时间。

| 候选 | 导入耗时 | 4 页双栏 | 平均每页 | 1 页复杂表格首次/再次 | 峰值 RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Xberg | 0.080 s | 0.069 s | 0.017 s | 0.022 / 0.015 s | 54.2 MiB |
| pdftext | 0.304 s | 0.085 s | 0.021 s | 0.021 / 0.021 s | 60.1 MiB |
| PyMuPDF4LLM legacy | 0.952 s | 0.584 s | 0.146 s | 0.277 / 0.271 s | 78.9 MiB |
| PyMuPDF4LLM layout | 0.802 s | 0.841 s | 0.210 s | 0.305 / 0.242 s | 309.3 MiB |
| Docling 长驻 worker | - | 6.002 s | 约 1.50 s | 9.486 / 4.275 s | 约 1,396 MiB |

短文档结果包含固定启动、解析和渲染成本，不能按页数简单外推到长文档。不过无模型候选和 Docling 的差距足够大，不影响路线判断。

## 分发体积

隔离环境使用 `uv` 创建并压缩。Windows venv 复用了系统 Python，因此压缩值表示候选依赖 payload，不包含可独立分发的 Python runtime。若走 Python sidecar，还需增加并维护一个嵌入式 Python；直接 Rust 集成则不需要。

| 候选环境 | 安装后 | ZIP | 分发解释 |
| --- | ---: | ---: | --- |
| Xberg Python binding | 119.74 MiB | 42.44 MiB | wheel 自带 Rust native extension；另需 Python runtime |
| pdftext | 60.99 MiB | 20.50 MiB | 主要为 PDFium、NumPy/Pydantic；另需 Python runtime |
| PyMuPDF4LLM legacy | 54.10 MiB | 20.67 MiB | MuPDF native library；另需 Python runtime |
| PyMuPDF4LLM layout | 193.87 MiB | 89.42 MiB | MuPDF、ONNX Runtime、小型布局/表格模型；另需 Python runtime |
| Docling | 约 1.51 GB | 755.47 MiB | 运行环境和模型合并包 |

Rosetta 当前已经包含 PDFium 资源，目录实测约 22.0 MB。若直接复用现有 PDFium，新的 PDF-to-Markdown 解析能力不需要再分发第二份 PDF 引擎。

Xberg Rust crate 支持关闭默认 feature 并只开启 `pdf` 与 `tokio-runtime`。本次最小 release spike 的依赖树仍有约 601 行，首次 release 编译在 10 分钟实验窗口内未完成，因此没有可靠的最终 Tauri 二进制增量数字。Python wheel 的 42.44 MiB ZIP 可以作为“全功能预编译 binding”的上界参考，但不能当成 Rosetta 最终增量。

## Markdown 质量

### Xberg

优点：

- 标题和部分表格能直接输出为 Markdown。
- 速度极快，内存最低，MIT 许可且可直接嵌入 Rust。
- 无 Python、Torch 和运行时模型下载要求。

问题：

- 韩文样本大量丢失词间空格，例如标题和正文被粘连，会直接降低后续翻译分段质量。
- 双栏页的脚注、页眉和正文顺序出现穿插。
- 复杂表格把多行 OTSL/HTML 数据合并进同一单元格，列头也丢失。
- 代码没有 fenced code block，公式只保留线性化的近似文本。

当前判断：性能通过，许可通过，质量不通过。可跟踪或做上游修复，不应直接成为默认实现。

### pdftext

优点：

- 韩文空格和原文字面质量是本次轻量候选中最稳定的。
- 复杂表格的所有数值均能按阅读顺序提取，速度和内存非常好。
- Apache-2.0，底层 PDFium 与 Rosetta 现有引擎一致。

问题：

- 只提供 plain text/JSON，不负责标题、表格、列表、代码等 Markdown 语义。
- 双栏 `sort=True` 仍可能按页面纵坐标交错左右栏，而不是完整读完左栏再读右栏。
- 表格只能得到线性文字，需额外的网格/列对齐规则才能生成可靠 Markdown table。

当前判断：适合作为算法参考或短期 sidecar POC，不适合作为最终产品边界。

### PyMuPDF4LLM legacy

优点：

- 韩文空格、双栏顺序、复杂表格和 fenced code block 整体较好。
- 依赖 payload 仅约 21 MiB ZIP，峰值内存约 79 MiB。

问题：

- 韩文首页的蓝底摘要块在输出中缺失，说明无模型规则仍可能漏掉背景上的正文。
- Markdown 有较多空行、字体强调噪声和页眉页脚。
- 公式只得到近似文本。
- AGPL-3.0/商业双许可是硬阻塞。

当前判断：技术表现很有吸引力，但没有合适商业许可时不应集成。

### PyMuPDF4LLM layout

优点：

- 本次普通文字型样本中综合 Markdown 质量最好。
- 标题层级、双栏、脚注、韩文表格和复杂英文表格均明显优于 Xberg/pdftext。
- 4 页不足 1 秒，体积和内存都远低于 Docling。

问题：

- 仍存在词内误空格、页眉页脚噪声和表头字符错误。
- 代码被输出为行内 `<mark>`，公式页的公式内容反而缺失。
- 当前依赖许可不能用于 MIT Rosetta 的默认商业分发。

当前判断：如果未来愿意采购并确认 Artifex 商业许可，这是最强的现成技术方案；否则不进入实现计划。

## 推荐方案

### 默认路线：不新增通用 PDF -> Markdown

- 保持 Rosetta 当前 `pdf2zh` 视觉页面翻译路径，不改变生产 PDF 权威状态。
- 不把 `pdf_oxide`、PDFium 字符坐标或实验性 block IR 包装成“通用 Markdown 转换器”。体积和性能通过不代表 reading order 与对象边界通过。
- 若产品可以接受严格受限能力，可另行定义“简单文字 PDF”并在复杂 Figure、Form XObject、table、RTL 或栏位不确定时明确拒绝；不能静默输出看似正确的 Markdown。

### 后续候选

- PyMuPDF4LLM layout：只有在愿意采购并确认 Artifex 商业许可时，才值得做正式 A/B release artifact 和更大语料验收。
- 其他商业/许可可接受的成熟 layout engine：下一次调研应优先验证 Figure/caption/object grouping，而不是再次比较纯文字提取速度。
- Xberg / `pdf_oxide`：保留为快速预检、页数/文字层检测或上游观察对象，不继续扩 Rosetta 自有布局规则。
- Docling：只保留为离线研究质量基线，不进入默认安装包。

## 验证闭环

原计划的小型 spans/lines -> block IR spike 已完成。它证明普通正文和常规双栏可以用有限规则明显改善，但 `2206.01062.pdf` 首页复杂 Figure 的内部文字会与论文摘要混排；继续修复需要 Form XObject、绘制层级、Figure/caption 和统一 reading-order graph。该结果已触发停止条件，因此不再安排 30-50 份 PDF 的阈值调参阶段。
