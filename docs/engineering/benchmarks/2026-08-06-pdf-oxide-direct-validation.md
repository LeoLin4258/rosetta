# pdf_oxide 直接集成验证

日期：2026-08-06  
平台：Windows、Rust 1.92.0、CPU、`pdf_oxide 0.3.77`  
配置：`default-features = false`、无 OCR、无图片提取

## 结论

- `pdf_oxide` 的体积和性能通过：最小 release 可执行文件为 8.54 MiB，ZIP 为 3.50 MiB；49 页文字型 PDF 的热进程转换速度约为 9.7 ms/页。
- Rust 原生集成可行，不需要 Python、PDFium、模型文件、native sidecar 或运行时下载。本次独立二进制表明新增体积处于约 9 MiB 未压缩、3-5 MiB 安装包压缩增量的量级，但这不是 Tauri 增量上限；最终数字仍需在正式依赖决策时用 A/B release artifact 确认。
- 直接调用 `pdf_oxide` 的韩文输出明显好于上一轮 Xberg 输出：大部分词间空格存在，说明 Xberg 的韩文整段粘连并非 `pdf_oxide` 不可避免的底层限制。
- 直接 Markdown 质量仍不通过：韩文存在词内误空格和视觉行断裂，双栏脚注会穿插正文，复杂表格退化，代码没有 fenced block，公式只保留近似文本，RTL 样本严重错乱。
- `ConversionOptions::default()` 已经使用 `StructureTreeFirst`，未标记 PDF 会回退到 `ColumnAware`。在主要样本中，默认模式和显式 `ColumnAware` 输出完全相同，单独切换该选项不能改善质量。
- `strip_running_headers_footers` 在本次所有样本中没有改变输出；对 9-16 页完整论文却慢约 3-6 倍。源码会在每页转换时重新扫描整本文档，多页调用形成近似 O(page²) 的额外工作，不适合直接启用。
- 失败边界可控但需要 Rosetta 包装：纯图片页输出明确的 `[OCR REQUIRED - page 1]` 标记，非 PDF 输入返回 `InvalidXref`；密码 PDF 不报错但返回空 Markdown，调用方必须把空结果识别为加密/不可读错误。

后续 structured IR 验证已经完成。最终判断：**停止把 `pdf_oxide` 加小型自研规则层作为 Rosetta 通用 PDF -> Markdown 方案继续投入。** 它仍适合轻量 PDF 预检、纯文本提取或明确限制范围的简单页面，但不应成为 Rosetta 的权威 Markdown 结构恢复层。停止原因不是体积或速度，而是复杂 Figure 内文字与论文正文混排、标题语义缺失、韩文跨视觉行空格不可判定等问题已经要求页面对象识别和更完整的布局规则集。当前生产 `pdf2zh` 视觉页面管线不变。

## 实验方法

实验 harness 位于被 Git 忽略的 `tmp/pdf-oxide-spike`，未修改 Rosetta 生产代码。依赖配置：

```toml
[dependencies]
pdf_oxide = { version = "=0.3.77", default-features = false }
```

每份 PDF 分别执行三种配置：

1. `default`：`ConversionOptions::default()`。
2. `column`：显式设置 `ReadingOrderMode::ColumnAware`。
3. `column-stripped`：`ColumnAware` 加 `strip_running_headers_footers = true`。

所有配置都关闭图片包含和嵌入，调用 `PdfDocument::to_markdown_all()`，记录打开、转换、页数和 Markdown 字节数。

## 性能

### 主验收样本

下表为同一热进程中的默认模式结果：

| 样本 | 页数 | 转换时间 | 平均每页 | Markdown |
| --- | ---: | ---: | ---: | ---: |
| `normal_4pages.pdf` | 4 | 41.9 ms | 10.5 ms | 18.0 KiB |
| `2305.03393v1-pg9.pdf` | 1 | 7.3 ms | 7.3 ms | 2.4 KiB |
| `amt_handbook_sample.pdf` | 1 | 15.4 ms | 15.4 ms | 3.7 KiB |
| `code_and_formula.pdf` | 2 | 17.8 ms | 8.9 ms | 5.4 KiB |
| `right_to_left_01.pdf` | 1 | 6.5 ms | 6.5 ms | 2.5 KiB |
| `table_mislabeled_as_picture.pdf` | 1 | 8.3 ms | 8.3 ms | 3.5 KiB |

### 完整论文

| 样本 | 页数 | 默认模式 | 平均每页 | 开启页眉页脚去重 |
| --- | ---: | ---: | ---: | ---: |
| `2203.01017v2.pdf` | 16 | 155.4 ms | 9.7 ms | 1,001.6 ms |
| `2206.01062.pdf` | 9 | 134.7 ms | 15.0 ms | 389.7 ms |
| `2305.03393v1.pdf` | 14 | 87.4 ms | 6.2 ms | 458.6 ms |

三个完整论文共 39 页，默认模式总转换时间约 377 ms，约 9.7 ms/页。该结果是已启动进程内的纯转换时间，不包含首次安装和 Rust 编译时间。

## 分发体积

| 产物 | 大小 |
| --- | ---: |
| 最小 release EXE | 8.54 MiB |
| Windows ZIP | 3.50 MiB |

这个可执行文件已经包含 `pdf_oxide` 实际转换路径及其 Rust 依赖，不需要外部 DLL 或模型。它比 Xberg Python binding 的 42.44 MiB ZIP 小一个数量级，也不需要额外分发 Python runtime。

`pdf_oxide` 会给 Cargo 增加较多源码依赖，但静态链接和 dead-code elimination 后实际交付体积很小。不能用 Cargo registry、`target` 缓存或依赖数量推断用户下载大小。

## Markdown 质量

### 韩文与双栏

通过点：

- 文字基本完整，大部分韩文词间空格存在。
- 标题、部分章节和粗体能生成 Markdown 语义。
- 主要正文大致按栏阅读，而不是逐行左右交错。

不通过点：

- 存在词内误空格，例如 `돌 입하였다고`、`이 에 대하여`。
- 大量视觉行被输出为独立段落；`감염` / `증-19` 等词会跨 Markdown 段落断开，不能直接用于翻译分段。
- 第一页脚注和另一栏正文发生穿插。
- 页眉、页脚、页码和发行信息仍保留。
- 4 页样本共有 178 个非空输出行，其中 136 行短于 35 个字符，说明视觉行合并不足，而不是自然段恢复。

因此，韩文结果比 Xberg 包装层有实质改善，但仍未达到“可直接翻译的 Markdown”门槛。

### 表格

复杂英文表格生成了 Markdown 管道表，但列标题、行标签和部分数值被拆到表外。原表多级表头和合并单元格无法可靠恢复。输出中出现 9 行 Markdown table，并不代表表格语义正确。

结论：简单规则表格可能可用；复杂表格必须保留为低置信度结构或线性文本，不能宣称完整 Markdown table 支持。

### 代码与公式

- 代码没有 fenced code block。
- JavaScript 标点和调用结构丢失，例如 `console.log(add(3, 5));` 退化为近似 `console log add (3 , 5)`。
- 公式只保留线性近似文本，上标退化，例如 `a² + 8 = 12` 近似为 `a2 + 8 = 12`。

结论：适合“尽量保留可读文字”的安全降级，不适合结构化代码或公式承诺。

### RTL

阿拉伯语样本出现正常片段、反向片段和粘连片段混合，同一段内方向顺序不稳定。当前版本的 RTL Markdown 不可作为 Rosetta 默认支持范围。

### 页眉页脚去重

`column-stripped` 与 `column` 在全部九个样本上的输出哈希一致，没有删除可见重复内容。完整论文转换时间分别增加约 6.4 倍、2.7 倍和 5.3 倍。

源码在每页 `to_markdown` 中调用一次整本文档的 repeated header/footer 扫描，因此 `to_markdown_all` 会重复执行跨页扫描。Rosetta 不应直接开启该选项；如果采用 `pdf_oxide`，应在 IR 层一次性统计并去重。

### 失败边界

额外生成了一页只有图像 XObject、`pypdf.extract_text()` 为空的 PDF。Poppler 渲染确认页面视觉内容正常，`pdf_oxide` 输出：

```markdown
> [OCR REQUIRED - page 1]
> This page is a scanned/rasterised image with no extractable text layer; run OCR to recover its content.
```

这适合映射为 Rosetta 现有的“扫描件或纯图片，当前不做 OCR”错误，不能把提示文本当成待翻译正文。

使用普通 Markdown 文件伪装输入时，`PdfDocument::open()` 返回 `InvalidXref`。使用密码加密且未提供密码的 PDF 时，打开成功但三个转换模式都返回 0 字节 Markdown。Rosetta 包装层必须同时检查解析错误、OCR marker 和全空结果，避免创建空任务。

## 决策门槛

`pdf_oxide` 进入下一阶段的理由：

- 许可、体积、速度、本地运行和 Rust 集成都明显通过。
- 相比从 PDFium 字符坐标开始，已经提供字体解码、spans、words、lines、XY-Cut、标题和表格框架。
- Xberg 的韩文粘连问题不是必须接受的底层结果。

下一阶段必须证明以下三点，否则停止继续投入：

1. 使用 spans/lines API，在不维护大型规则集的前提下稳定合并视觉行和自然段。
2. 单次跨页统计能够可靠去除重复页眉页脚，且不误删正文。
3. 对双栏脚注、复杂表格、代码、公式和 RTL 能检测低置信度并安全降级，不静默产生看似正确的错误结构。

下述 structured IR 实验已经执行这些门槛验证；本节保留为当时的进入条件，最终结果见文末决策。

## Structured IR 恢复层验证

### 范围与实现

本轮继续使用 Git 忽略目录 `tmp/pdf-oxide-spike`，没有修改 Rosetta 生产代码、PDF 权威状态或 `pdf2zh` 页面产物架构。实验层直接调用：

- `PdfDocument::extract_structured(page)` 获取 regions、spans、bbox、`column_index` 和 heading role。
- `PdfDocument::extract_tables(page)` 只用于标记表格区域，不生成 Markdown table。
- `extract_structured_with_column_mode(page, ColumnMode::Two)` 仅在 Auto 没有给出任何栏位、同时 region 横坐标呈明确左右双峰时启用。

实验输出三份独立产物：原始 `*.structured.raw.json`、实验性 `*.blocks.json` 和 `*.recovered.md`。恢复层包含几何行聚类、页面全宽 band 与栏位重排、行距/缩进段落合并、一次性跨页 chrome 统计，以及 table/code/formula/form/RTL/旋转文字的低置信度标记。低置信度内容保留线性文字并输出提示，不伪造 fenced code、公式或 Markdown table。

实验 harness 最终为 856 行 Rust（含 CLI、JSON schema、Markdown renderer 和 4 个单元测试）。虽然其中不全是布局规则，但仅一轮验证已明显超过“很小的胶水层”，后续再加入 Figure/Form XObject、caption、footnote 和 heading 规则会继续快速增长。

### 性能与体积

| 范围 | 页数 | structured 提取 | 恢复层（含 table detector） | 合计 | 平均每页 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 6 份主边界样本 | 10 | 49.5 ms | 60.7 ms | 110.2 ms | 11.0 ms |
| 3 份完整论文 | 39 | 124.2 ms | 326.4 ms | 450.6 ms | 11.6 ms |

这组时间不含文件写入和进程启动。恢复层加入表格检测后，完整论文仍处于约 7.2-19.0 ms/页，性能通过。

| 最小实验产物 | 大小 |
| --- | ---: |
| release EXE | 6.50 MiB |
| ZIP | 2.71 MiB |

因此 structured API、`serde_json` 和本轮规则层没有带来不可接受的包体积；体积继续通过。

### 段落恢复结果

| 样本 | 视觉行 | 输出 blocks | 减少 | 低置信度 blocks |
| --- | ---: | ---: | ---: | ---: |
| `normal_4pages.pdf` | 295 | 92 | 68.8% | 1 |
| `amt_handbook_sample.pdf` | 75 | 24 | 68.0% | 0 |
| `code_and_formula.pdf` | 85 | 15 | 82.4% | 4 |
| `right_to_left_01.pdf` | 16 | 1 | 93.8% | 1 |
| `2305.03393v1-pg9.pdf` | 46 | 19 | 58.7% | 9 |
| `table_mislabeled_as_picture.pdf` | 70 | 23 | 67.1% | 3 |
| `2203.01017v2.pdf` | 1,099 | 356 | 67.6% | 29 |
| `2206.01062.pdf` | 723 | 350 | 51.6% | 123 |
| `2305.03393v1.pdf` | 688 | 257 | 62.6% | 50 |

通过点：

- 修正 `pdf_oxide` 实际使用的 bottom-left Y 坐标方向后，region 内视觉行可以稳定恢复为 top-to-bottom。
- 韩文首页通过全宽 band 加 `column_index` 重排后，顺序恢复为页首信息、标题/摘要、左栏、右栏；不再逐行左右交错。
- Auto 漏检航空手册双栏时，受严格双峰条件约束的 `ColumnMode::Two` 能把 75 条视觉行恢复为 24 个 blocks，主要正文段落基本正确。
- 论文第 9 页的表格样本中，表格行全部输出为 `table_like` 低置信度块；代码、公式、RTL 和下划线表单样本也能显式降级。

但“视觉行减少”不等于 Markdown 已经正确：

- 韩文跨行边界没有足够信息判断是词内断行还是正常词间空格。连接能修复 `감염` + `증-19`，同时也会产生 `판매했거나향후`、`돌입한와중`；保留空格则重新出现词内误空格。这不是再调一个全局阈值能解决的问题。
- `StructuralHeading` 在主要未标记样本中经常没有出现，标题、作者、caption 和小节标题仍是普通正文。若自行按字体和位置补齐，会再引入一套 heading/caption 规则。
- 航空手册正文恢复明显改善，但图内 label、caption、分数和右栏正文仍会在局部错位；要修复需要图片区、caption 与正文的对象级分离。

### 页眉页脚

跨页统计只执行一次，复杂度为 O(页数 × 行数)，没有上一轮 `strip_running_headers_footers` 的近似 O(page²) 问题。

- `normal_4pages.pdf` 去除 4 行 chrome，`이슈와논점` 最终只保留一次。
- 三份完整论文分别去除 22、5、20 行；`2305.03393v1.pdf` 中在 7 页重复的 `M. Lysak, et al.` 被去除。
- 规则只处理 PDF artifact role、顶部/底部页码和至少半数页面重复的短行，因此本批样本未观察到正文误删。

它仍只解决“精确或数字归一化后重复”的 running chrome。单双页不同标题、位置漂移、章节变化和靠近正文的页眉仍需更多规则，所以这一项只能算窄范围通过。

### 文字守恒

将原始 spans 与输出 blocks 的非空白字符做 multiset 对比，9 份样本缺失比例为 0.00%-0.55%，没有新增字符。完整论文分别为 0.45%、0.09%、0.55%，与主动删除的页码、artifact 和重复页眉量级一致。

这说明恢复层没有大段静默丢文，但字符守恒不能证明阅读顺序正确。下面的复杂 Figure 反例几乎不丢字符，却生成了错误 Markdown。

### 决定性反例：Figure 内文字穿插正文

`2206.01062.pdf` 首页右栏的 Figure 1 包含四个复杂文档缩略图。`pdf_oxide` 把缩略图内部的矢量文字作为普通 page spans 提取，实验层随后将这些文字插到论文标题与摘要之间，包括显微镜说明、交通标志、航空图表等大量无关段落。

该页字符几乎全部保留，table detector 也给出大量 `table_like` 标记，但无法回答这些 spans 属于 Figure、caption 还是论文正文。要安全处理至少需要：

1. Form XObject、image、clip path 与绘制层级的区域识别。
2. Figure/table/caption 的对象级分组和正文遮罩。
3. 对嵌套文档截图、矢量图中文字和真实正文的冲突决策。
4. 跨页、多栏、footnote 与 full-width object 的统一 reading-order graph。

这已经不是小型 Markdown renderer，而是在继续实现文档布局引擎。它正是本轮设定的停止条件。

## 最终决策

本轮三个硬门槛的结果：

| 门槛 | 结果 | 说明 |
| --- | --- | --- |
| 视觉行稳定合并为自然段 | 部分通过 | 简单正文和常规双栏明显改善；韩文空格、caption/footnote 仍不稳定 |
| 单次跨页页眉页脚去除 | 窄范围通过 | 性能正确，精确重复有效；变化型 chrome 未解决 |
| 复杂内容安全降级 | 不通过 | table/code/formula/RTL 可标记，但复杂 Figure 内文字仍穿插正文 |

结论：**停止当前自研恢复层，不接入 Rosetta。** 不建议用更多 PDF 样本继续调这些局部阈值，因为当前主要失败已经从“样本覆盖不足”升级为“缺少对象级页面模型”。增加样本会发现更多规则分支，不会让现有小型规则自然收敛。

后续选择应限定为：

- 保持当前 `pdf2zh` 视觉页面翻译路径，不把 PDF 强制转换为 Markdown。
- 若 PDF -> Markdown 仍是战略需求，下一次验证应直接比较具备成熟 layout/Figure 模型的商业或许可可接受引擎，而不是继续扩展这份规则层。
- `pdf_oxide` 可保留为未来的快速 PDF 预检、页数/文本层检查或“仅简单文本 PDF”的受限能力候选，但必须有明确拒绝条件，不能静默回退成通用 Markdown。
