# Rosetta 本地长文本翻译 App 项目说明与计划

## 1. 项目概述

Rosetta 是一款面向敏感文档和超长文本场景的本地 AI 翻译桌面端应用。

它使用本地运行的 RWKV LLM 翻译模型，利用 RWKV 的 big batch / batch translate 能力，对论文、书籍、商业计划书、内部资料等长文本进行高速批量翻译，并尽量保留原文档结构与排版。

Rosetta 不定位为通用 AI 助手，也不提供聊天、总结、改写等泛 AI 功能。它的核心目标是：

> 不上传、不聊天、不打扰，只在本机快速翻译长文本。

---

## 2. 项目要解决的问题

现有云端 AI 翻译工具在很多场景下存在明显限制：

1. **隐私与安全问题**  
   商业计划、合同草稿、内部资料、未发表论文、客户文件等敏感内容不适合上传到云端 AI 服务。

2. **超长文本处理不稳定**  
   论文、书籍、技术文档等内容篇幅长，直接复制到翻译工具中容易超出上下文限制，也难以保持结构。

3. **翻译速度不足**  
   逐段调用传统翻译接口或通用大模型，速度慢、成本高，不适合批量处理大量段落。

4. **排版容易丢失**  
   很多翻译工具输出的是纯文本，标题、列表、表格、代码块、引用、脚注等结构容易被破坏。

5. **结果难以检查和修正**  
   对于长文档，用户需要知道哪一段翻译成功、哪一段失败、哪一段结构异常，并能够局部重翻或编辑。

---

## 3. 核心定位

Rosetta 的定位是：

> 一个本地优先的长文本批量翻译工作台。

核心关键词：

- 本地翻译
- 隐私保护
- 长文本处理
- 高速批量翻译
- 文档结构保留
- 极简桌面端体验

不做：

- 云端 AI 对话
- 文档问答
- 自动总结
- 润色改写
- 在线同步
- 团队协作
- 浏览器划词翻译
- 通用 AI 助手

---

## 4. 目标用户与典型场景

### 4.1 目标用户

1. **研究人员 / 学生**  
   需要翻译论文、教材、学术资料，但不希望上传未公开内容。

2. **企业用户 / 创业者**  
   需要翻译商业计划书、内部文档、客户资料、会议材料等敏感内容。

3. **开发者 / 技术人员**  
   需要翻译技术文档、README、API 文档、架构文档等结构化内容。

4. **小说 / 书籍阅读者**  
   需要翻译长篇小说、EPUB、Markdown 书稿等长文本内容。

### 4.2 典型使用场景

- 翻译一篇 30 页英文论文为中文
- 翻译一本英文小说或 EPUB 电子书
- 翻译公司内部商业计划书
- 翻译 Markdown 技术文档并保留标题、代码块、表格
- 翻译长篇资料并导出双语对照版本

---

## 5. 核心卖点

### 5.1 本地可信

文件在本机解析、切分、翻译和导出。默认不上传原文、译文或文档结构。

可在产品中明确展示：

- 文件不会上传
- 翻译在本机完成
- 无需登录
- 无需联网，除非下载模型或检查更新

### 5.2 高速批量翻译

利用 RWKV LLM 翻译模型的 batch / big batch 能力，将长文档切分为多个可翻译文本块，并批量发送给本地模型。

与逐段翻译相比，Rosetta 的目标是显著提升长文本处理速度。

### 5.3 面向长文本

Rosetta 不是短句翻译工具，而是围绕长文档设计：

- 任务进度
- 分段缓存
- 失败重试
- 暂停与继续
- 局部重翻
- 双语对照预览
- 导出完整文档

### 5.4 排版保真

Rosetta 的重要卖点之一是尽量保留原文档结构：

- 标题
- 段落
- 列表
- 表格
- 引用
- 代码块
- 链接
- 图片占位
- 脚注与注释，视格式能力而定

原则：

> 结构由程序保留，文本由模型翻译。

不依赖模型“猜测并重建格式”。

---

## 6. 技术栈建议

### 6.1 桌面端

推荐：

- Tauri
- React
- TypeScript
- Tailwind CSS
- Zustand

原因：

- 体积较小
- 更符合极简工具型 App 的产品气质
- 适合本地文件访问、侧车进程管理和系统集成
- 前端开发体验与现有 React 技术栈一致

备选：

- Electron

Electron 实现成本较低，但体积较大，产品气质偏重。

### 6.2 本地翻译服务

Rosetta 连接本地 RWKV LLM 翻译模型服务。

可能使用的接口：

- `/translate/v1/batch-translate`
- `/big_batch/completions`

最终以实际模型服务提供的稳定接口为准。

### 6.3 本地编排层

建议在 UI 和 RWKV 服务之间增加 Local Translation Orchestrator。

职责：

- 文件解析
- 文档结构转换
- 文本切分
- 批量调度
- 翻译进度管理
- 失败重试
- 本地缓存
- 译文合并
- 导出文档

结构：

```txt
React UI
  ↓
Local Translation Orchestrator
  ↓
RWKV Local API
```

---

## 7. 总体架构

```txt
Desktop App
  ├── UI Layer
  │   ├── 导入界面
  │   ├── 任务进度
  │   ├── 双语预览
  │   ├── 结构切分预览
  │   └── 设置页
  │
  ├── Document Pipeline
  │   ├── Importer
  │   ├── RosettaDocument IR
  │   ├── Segmenter
  │   ├── Translator Scheduler
  │   ├── Merger
  │   └── Exporter
  │
  ├── Local Job Store
  │   ├── 任务状态
  │   ├── 分段缓存
  │   ├── 翻译结果
  │   └── 失败记录
  │
  └── RWKV Runtime Connector
      ├── API 检测
      ├── batch translate
      ├── big batch completions
      └── 错误处理
```

---

## 8. 中间格式设计

为了让不同文件格式共用同一套翻译、预览和导出逻辑，需要设计统一的中间格式。

### 8.1 RosettaDocument

```ts
type RosettaDocument = {
  id: string
  filename: string
  format: "txt" | "markdown" | "docx" | "pdf" | "epub" | "html"
  sourceLang?: string
  targetLang: string
  blocks: RosettaBlock[]
}
```

### 8.2 RosettaBlock

```ts
type RosettaBlock = {
  id: string
  type:
    | "heading"
    | "paragraph"
    | "list_item"
    | "table_cell"
    | "blockquote"
    | "code"
    | "caption"
    | "footnote"
    | "metadata"

  sourceText: string
  translatedText?: string

  shouldTranslate: boolean
  order: number
  path?: string
  style?: Record<string, unknown>

  status:
    | "pending"
    | "translating"
    | "done"
    | "failed"
    | "skipped"
    | "edited"
}
```

### 8.3 Segment

```ts
type Segment = {
  id: string
  blockId: string
  order: number
  sourceText: string
  translatedText?: string
  sourceLang?: string
  targetLang: string
  kind: RosettaBlock["type"]
  preserveWhitespace: boolean
  status: "pending" | "translating" | "done" | "failed" | "skipped"
}
```

---

## 9. 文本切分策略

文本切分是 Rosetta 的核心技术功能之一。

错误方式：

```txt
每 1000 字切一次
```

正确方式：

```txt
Document
  → Block
    → Paragraph
      → Sentence
        → Token-budget Chunk
```

### 9.1 切分原则

1. 优先保留自然段落
2. 不在句子中间强行切断
3. 不破坏 Markdown / DOCX / EPUB 的结构
4. 不翻译代码块、URL、文件路径、公式等内容
5. 超长段落按句子进一步切分
6. 根据模型上下文和 batch 性能控制单段长度

### 9.2 按长度分桶

为了提升 batch 效率，可以按文本长度分桶：

```txt
small: 0 - 120 chars
medium: 120 - 800 chars
large: 800 - 2000 chars
huge: 2000+ chars，需要继续切分
```

不同桶使用不同 batch size：

```txt
small: 128 - 300 条
medium: 32 - 128 条
large: 8 - 32 条
```

这样可以避免短句被长段落拖慢。

---

## 10. 翻译模式设计

由于 batch 翻译速度快，但上下文可能较弱，可以提供三种模式：

### 10.1 极速模式

- 最大 batch size
- 最少上下文
- 适合技术文档、表格、短段落
- 速度最快

### 10.2 平衡模式

- 中等 batch size
- 可携带少量前文上下文
- 适合大多数论文和普通文档
- 默认推荐

### 10.3 连贯模式

- 更小 batch size
- 更重视上下文连续性
- 适合小说、书籍、叙事性长文本
- 速度相对较慢

---

## 11. 文件预览设计

文件预览不是附属功能，而是 Rosetta 的核心模块之一。

它需要帮助用户确认：

1. 原文是否正确解析
2. 文档结构是否正确识别
3. 哪些内容会被翻译
4. 哪些内容会被跳过
5. 译文是否与原文结构对应
6. 导出结果是否可信

### 11.1 预览模式

建议提供以下模式：

- 原文预览
- 译文预览
- 双语对照预览
- 结构切分预览

### 11.2 双语对照预览

核心视图：

```txt
Original                        Translation
------------------------------------------------
Introduction                    引言

This paper proposes...          本文提出了……
```

支持：

- 点击段落
- 高亮对应原文和译文
- 查看 segment 状态
- 复制原文
- 复制译文
- 编辑译文
- 单段重翻
- 标记为不翻译

### 11.3 结构切分预览

用于展示 Rosetta 解析出的文档结构：

```txt
[heading] Introduction
[paragraph] This paper proposes a new method...
[list_item] Fast
[list_item] Local
[code] skipped
```

这对调试和建立用户信任都很重要。

### 11.4 性能要求

超长文档预览必须使用虚拟滚动。

不能一次性渲染全部 block：

```tsx
// 不推荐
{blocks.map(block => <BlockPreview block={block} />)}
```

应该使用虚拟列表，只渲染当前屏幕附近的内容。

---

## 12. 文件格式支持计划

### 12.1 Phase 1

优先支持：

- TXT
- Markdown
- 文字型 PDF
- 基础 DOCX

导出：

- TXT
- Markdown
- PDF 来源文档的 TXT / Markdown 风格译文或双语导出
- DOCX 基础版
- 双语 Markdown

说明：

- 由于 v1 上层要求，PDF 从原 Phase 3 前移到 Phase 1。
- v1 的 PDF 基线承诺是文字型 PDF 的文本提取、结构化翻译和文本式导出。
- PDF 格式高保真还原是 nice to have。如果 PDF 工程师能在不破坏主 pipeline 的前提下实现接近原版式的还原，可以作为增强路径推进。
- v1 不把 OCR、任意 PDF 的原版式复刻或默认写回源 PDF 作为基线验收。

### 12.2 Phase 2

增加：

- EPUB
- HTML

适合书籍、小说和结构化网页文档。

### 12.3 Phase 3

增加：

- PDF 论文结构增强
- 多栏阅读顺序优化
- 表格、脚注、图片说明等更细结构提取

注意：PDF 不承诺完美排版复刻。

### 12.4 Phase 4

视需求增加：

- PPTX
- XLSX

这两类格式排版与结构复杂，暂不作为 MVP 重点。

---

## 13. 排版保真策略

Rosetta 的排版保真原则：

> 程序负责结构，模型只负责文本。

### 13.1 Markdown

规则：

- 标题标记保留
- 列表标记保留
- 代码块不翻译
- inline code 不翻译
- URL 不翻译
- 图片路径不翻译
- 表格结构不改，只翻译 cell 内容
- frontmatter 默认不翻译

### 13.2 DOCX

第一版目标：

- 保留段落顺序
- 保留标题层级
- 保留基础列表
- 保留基础表格
- 尽量保留加粗、斜体、链接

暂不承诺：

- 完美分页
- 完美页眉页脚
- 完美批注
- 完美脚注
- 复杂 Word 样式 100% 还原

### 13.3 PDF

PDF 分两类处理：

1. 文字型 PDF  
   提取文本，生成结构化翻译结果。

2. 扫描型 PDF  
   需要 OCR，暂不作为 MVP 核心。

PDF 第一版的基线不承诺原版式 100% 复刻。接近原版式的格式还原是 nice to have，可由熟悉 PDF 的工程师作为增强能力探索，但必须能回退到文本型 PDF 主路径。

---

## 14. 任务系统与缓存

长文翻译必须支持任务级管理。

### 14.1 任务状态

```ts
type JobStatus =
  | "created"
  | "parsing"
  | "ready"
  | "translating"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled"
```

### 14.2 Segment 状态

```ts
type SegmentStatus =
  | "pending"
  | "translating"
  | "done"
  | "failed"
  | "skipped"
  | "edited"
```

### 14.3 本地缓存目录

示例：

```txt
.rosetta/
  jobs/
    job-id/
      source.json
      document-ir.json
      segments.json
      translations.json
      export/
```

### 14.4 必须支持

- 暂停
- 继续
- 取消
- 失败重试
- 单段重翻
- 修改后重新导出
- App 关闭后恢复任务

---

## 15. 质量检查

由于模型只负责翻译，可以增加规则型质量检查。

检查项：

- 译文为空
- 译文明显过短
- 译文明显过长
- 数字丢失
- URL 丢失
- Markdown 链接损坏
- 表格列数变化
- 括号不匹配
- 引号不匹配
- 术语疑似不一致

这些检查不需要通用 LLM，也能显著提升专业感。

---

## 16. 术语表

长文本翻译需要术语一致性。

### 16.1 MVP 方案

用户可以手动提供术语表：

```txt
embedding = 嵌入
retrieval = 检索
alignment = 对齐
state = 状态
```

### 16.2 使用方式

两种可能方案：

1. 翻译前注入简短术语提示  
   需要测试 RWKV 翻译模型是否稳定遵循。

2. 翻译后做术语一致性检查  
   更稳定，适合 MVP。

MVP 建议优先做术语检查，而不是强制自动替换。

---

## 17. 隐私与安全设计

Rosetta 的隐私承诺必须由技术实现支撑。

### 17.1 默认行为

- 不上传文档
- 不上传翻译结果
- 不上传结构化文本
- 不自动加载远程资源
- 不执行文档中的脚本
- 不要求登录

### 17.2 文件预览安全

对于 HTML / EPUB / Markdown 预览，需要注意：

- 禁止执行 script
- 禁止自动加载远程资源
- 链接点击前提示
- iframe sandbox
- HTML sanitize
- 限制预览内容访问本地文件系统

### 17.3 可选数据

如果未来需要崩溃报告或匿名统计，必须默认关闭，并清楚说明不会包含原文或译文。

---

## 18. MVP 范围

### 18.1 MVP 输入格式

- TXT
- Markdown
- 文字型 PDF
- DOCX 基础版

### 18.2 MVP 输出格式

- TXT
- Markdown
- PDF 来源文档的 TXT / Markdown 风格译文和双语导出
- DOCX 基础版
- 双语 Markdown

### 18.3 MVP 核心功能

- 本地 RWKV API 连接
- 文件导入
- 文档解析为 IR
- 文本切分
- batch 翻译调度
- 进度显示
- 本地缓存
- 暂停 / 继续
- 失败重试
- 双语对照预览
- 结构切分预览
- 手动编辑译文
- 单段重翻
- 导出译文文档

### 18.4 MVP 不做

- PDF 完美排版
- OCR
- PPTX
- XLSX
- 云端翻译
- AI 问答
- 总结
- 改写
- 账号系统
- 团队协作
- 在线同步

---

## 19. 开发阶段计划

### Stage 0：接口验证

目标：确认 RWKV 翻译模型的真实能力边界。

任务：

- 确认最终使用接口
- 测试最大单条文本长度
- 测试稳定 batch size
- 测试不同语言方向
- 测试流式与非流式模式
- 测试错误返回格式

验收：

- 可以稳定批量翻译 text list
- 可以处理长文档切分后的多个 segment
- 可以获得可用的错误信息和重试策略

### Stage 1：纯文本 Pipeline

目标：跑通最小翻译闭环。

流程：

```txt
TXT 输入
  → 段落切分
  → batch 翻译
  → 合并
  → TXT 输出
```

验收：

- 可以翻译 2 万字以上文本
- 可以显示进度
- 可以失败重试
- 翻译结果顺序正确

### Stage 2：Markdown Pipeline

目标：验证结构保真。

流程：

```txt
Markdown
  → AST
  → 提取文本节点
  → 翻译
  → 回填
  → 导出 Markdown
```

验收：

- 标题保留
- 列表保留
- 表格保留
- 代码块不翻译
- 链接不损坏
- 导出后 Markdown 可正常阅读

### Stage 3：任务系统与预览

目标：形成工作台雏形。

任务：

- Job store
- Segment status
- 本地缓存
- 双语预览
- 虚拟滚动
- 段落选择
- 单段重翻
- 手动编辑译文

验收：

- 大文档滚动不卡
- 翻译过程中译文渐进出现
- 失败段落可单独重试
- 用户编辑结果可保留并导出

### Stage 4：Tauri 桌面端整合

目标：形成可安装桌面 App。

任务：

- 文件拖拽导入
- 本地文件读写
- 设置页
- RWKV API 地址配置
- 本地任务目录
- 导出文件选择

验收：

- 可在桌面端完成完整流程
- 不依赖浏览器
- 不上传文件

### Stage 5：DOCX 基础支持

目标：进入办公文档场景。

任务：

- DOCX 导入
- 段落解析
- 标题解析
- 列表解析
- 表格解析
- 基础样式保留
- DOCX 导出

验收：

- 普通 Word 文档可翻译
- 段落顺序正确
- 标题和表格结构基本保留

### Stage 6：质量检查与术语表

目标：提升专业可用性。

任务：

- 数字检查
- URL 检查
- 空译文检查
- 长度异常检查
- Markdown 格式检查
- 术语表导入
- 术语一致性提示

验收：

- 可以定位疑似问题段落
- 可以辅助用户检查长文档翻译质量

---

## 20. 主要技术风险

### 20.1 PDF 排版风险

PDF 不是结构化文档，而是页面绘制结果。文本顺序、双栏、脚注、页眉页脚、公式和表格都可能解析错误。

应对：

- MVP 不承诺 PDF 完美排版
- v1 先支持文字型 PDF 的可解释文本提取
- 接近原版式的格式还原作为 nice to have 探索，不作为阻塞主流程的验收项
- 扫描 PDF 和 OCR 不作为第一阶段重点

### 20.2 上下文一致性风险

batch 翻译速度快，但段落之间上下文弱。

应对：

- 提供极速 / 平衡 / 连贯模式
- 对小说类文本降低 batch size
- 支持术语表
- 支持单段编辑和重翻

### 20.3 模型部署门槛风险

普通用户不一定能安装和启动本地 RWKV 模型服务。

应对：

- MVP 先支持连接已有本地 RWKV API
- 后续再做模型下载与本地运行管理
- 面向开发者和内部用户先验证核心 pipeline

### 20.4 DOCX 复杂样式风险

Word 文档样式复杂，很难 100% 还原。

应对：

- 第一版只承诺基础结构保留
- 预览和导出使用同一套 IR
- 复杂样式后续逐步增强

### 20.5 大文件性能风险

超长文档可能造成 UI 卡顿、内存过高或任务中断。

应对：

- 虚拟滚动
- segment-level 缓存
- 分批调度
- 后台任务执行
- 增量写入缓存

---

## 21. 产品体验原则

### 21.1 极简

界面不堆说明，不做 AI 助手式聊天界面。

主流程应该清楚：

```txt
导入 → 分析 → 翻译 → 预览 → 导出
```

### 21.2 可信

用户需要随时知道：

- 文件在哪里
- 翻译在哪里运行
- 当前翻译到哪里
- 哪些段落失败
- 哪些内容被跳过

### 21.3 可控

用户应该可以：

- 暂停
- 继续
- 取消
- 编辑译文
- 单段重翻
- 跳过某段
- 导出不同格式

### 21.4 不打扰

不做多余弹窗，不强制登录，不默认上传数据，不强推云端服务。

---

## 22. 第一版原型验收标准

第一版原型建议以 Markdown 长文档为主要验证对象。

目标：

> 导入一个 20,000 字英文 Markdown 文档，在本地 RWKV 翻译模型上批量翻译为中文，保留标题、列表、代码块和表格结构，支持双语预览和导出 Markdown。

验收标准：

1. 文件不上传云端
2. 可以连接本地 RWKV 翻译 API
3. 可以自动切分长文档
4. 可以批量翻译 segments
5. 可以显示翻译进度
6. 可以失败重试
7. 可以双语对照预览
8. 可以编辑单段译文
9. 可以导出 Markdown
10. 导出结果结构基本正确

---

## 23. 后续扩展方向

在 MVP 验证完成后，可以逐步增加：

- EPUB 翻译
- PDF 文本型翻译
- DOCX 高保真导出
- 术语库管理
- 项目级翻译记忆
- 多语言方向
- 模型下载和启动管理
- GPU / CPU 状态检测
- 翻译质量报告
- 双语电子书导出

---

## 24. 当前最需要确认的问题

项目启动前，需要优先确认：

1. RWKV 翻译模型最终使用哪个 API 端点？
2. 模型输入是否需要固定 prompt，还是直接传 text list？
3. 单条文本最大建议长度是多少？
4. 一批最多多少条最稳定？
5. 是否支持流式 batch 返回？
6. 支持哪些语言方向？
7. 首发平台是 Windows only，还是 Windows + macOS？
8. MVP 是否优先以 Markdown 为验证格式？
9. DOCX 第一版需要做到什么程度？
10. 是否需要内置模型管理，还是先连接外部本地 API？

---

## 25. 总结

Rosetta 的核心价值不是“又一个 AI 翻译工具”，而是：

> 在用户不信任云端 AI 的情况下，仍然可以在本机高速处理超长文本，并尽量保留原文档结构与排版。

第一阶段应该聚焦：

```txt
本地 RWKV 批量翻译
+ 长文本切分
+ 结构化双语预览
+ Markdown / TXT / 文字型 PDF
+ 任务缓存与失败重试
+ 排版保真导出
```

产品方向应保持克制：

> 不聊天，不上传，不堆功能，只做高质量本地长文本翻译。
