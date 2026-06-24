# Data Model Conventions

## Scope

本文档记录 Rosetta 核心数据模型约定。当前类型定义位于：

```txt
rosetta-app/src/types/rosetta.ts
```

## Core Model

Rosetta 的核心数据流是：

```txt
Document
  -> RosettaDocument
  -> RosettaBlock[]
  -> Segment[]
  -> translated Segment[]
  -> export result
```

## RosettaDocument

`RosettaDocument` 表示导入文档的统一中间格式。

约定：

- importer 负责把不同格式转换为 `RosettaDocument`。
- translator 不直接处理原始文件格式。
- preview 和 exporter 应尽量基于同一套 IR，避免预览和导出结果分叉。
- 文件夹项目使用 `files: RosettaSourceFile[]` 记录项目内每个源文件。单文件项目也应写入一个 `file-1` 条目，旧缓存缺失时读取方需要能回退到虚拟单文件。
- `RosettaSourceFile.relativePath` 使用 `/` 分隔的项目内相对路径，只能由 importer 从用户选择的根目录安全生成，不能接收前端拼接出的任意路径。
- `RosettaSourceFile.sourceLang` 和 `RosettaSourceFile.targetLang` 是文件级语言方向。旧缓存缺失这些字段时读取方回退到 `RosettaDocument.sourceLang` / `RosettaDocument.targetLang`。
- 任务工作台中的语言选择作用于当前文件，不应静默重置项目内其它文件的译文。项目级批量改语言如果后续恢复，必须是单独入口并明确提示影响范围。

## RosettaBlock

`RosettaBlock` 表示文档结构单元，例如标题、段落、列表项、表格单元格、代码块。

约定：

- `order` 必须保留原文档顺序。
- 多文件项目中 `fileId` 指向 `RosettaSourceFile.id`。没有 `fileId` 的旧 block 视为 `file-1`。
- `shouldTranslate` 决定是否进入翻译调度。
- 代码块、URL、文件路径、公式等内容应尽量标记为不翻译或在 segment 阶段保护。
- `style` 只记录结构和导出需要的信息，不放 UI 临时状态。

## Segment

`Segment` 是翻译调度的最小单位。

约定：

- 一个 block 可以拆成多个 segment。
- `blockId` 必须能追溯回原始 block。
- 多文件项目中 `fileId` 应与所属 block 的 `fileId` 一致，用于按文件导出和文档式预览分组。
- `order` 必须能恢复翻译前顺序。
- `preserveWhitespace` 用于提示合并和导出阶段保留空白。
- `Segment` 现在主要表示源文切分结果。新的译文文件工作流不应继续把多语言译文写回同一条 `Segment.translatedText` 作为唯一事实来源。
- 用户编辑后的译文状态应标记为 `edited`，后续重翻不能静默覆盖。
- `sourceLang` 和 `targetLang` 必须跟随所属 job/document 的语言方向。任务页修改语言方向时，需要更新所有 segments 的语言字段。
- `sourceLang` 和 `targetLang` 必须跟随所属文件的语言方向。任务页修改当前文件语言方向时，只更新该文件下的 segments。
- 如果当前文件语言方向发生变化，已有自动译文不再可信，应清空该文件 translatable segments 的 `translatedText` 和 `error`，并把状态重置为 `pending`。这样可以避免 UI 显示新语言配置下的旧译文。
- `translationHistory` 是旧缓存兼容字段。新的默认历史译文 UI 不再从 segment-level history 重组，而是读取文件级 `TranslationRevision`。
- 重新翻译当前文件表示启动一次新的完整文件翻译运行，不是只补翻缺失 segment。开始重翻前，当前文件内所有可翻译 segment 的当前译文应保存为文件级历史版本，然后清空当前译文并从 0 重新计算本次运行进度。
- 选中段落重翻时，用户选择的是 block；如果一个 block 被拆成多个 segments，重翻范围包含该 block 下所有可翻译 segments。开始局部重翻前同样保存一份当前文件完整译文版本。
- 用户查看历史记录时，应看到过去某一次翻译运行的完整文件译文版本，而不是零散 segment 记录。Segment 仍是调度和缓存单位，但历史查看使用 `TranslationRevision.segmentTranslations` 重建文件视图。

## Translation File

`RosettaTranslationFile` 表示一个源文件在某个目标语言下的内部译文文件。

约定：

- 一个 `RosettaSourceFile` 可以对应多个 `RosettaTranslationFile`，例如同一章同时有 `zh-CN` 和 `ja` 译文。
- `translation_files.json` 保存译文文件列表和状态统计；每个译文文件的正文保存在 `translations/<translationFileId>.json`。
- `TranslationSegment.sourceSegmentId` 指向源 `Segment.id`，译文状态和文本不再与源 segment 混在一起。
- 工作台和导出必须以当前选中的 `translationFileId` 为译文事实来源。
- 旧项目如果只有 `segments.json.translatedText`，加载时迁移成默认目标语言译文文件；旧字段暂不删除。
- 译文文件是 Rosetta 内部管理对象，不自动写入用户磁盘路径；用户点击导出时才生成外部文件。
- `TranslationSegment.status === "translating"` 只表示一次前端翻译运行已把该批次交给模型请求，不能视为跨应用重启仍然存在的真实后台任务。工作台加载项目时必须把遗留的 `translating` segments 恢复为 `pending`，并重建 `RosettaTranslationFile.status`，避免异常退出或模型卡住后项目永久停在“翻译中”。
- 用户手动停止翻译时，当前已经持久化为 `translating` 的批次应恢复为 `pending`，已完成的 `done` / `edited` segments 保留，失败的 segments 保留为 `failed`。停止不是失败，不应把用户主动停止写成模型错误。

## TranslationRevision

`TranslationRevision` 表示某个文件在一次重翻前保存下来的完整译文快照。

约定：

- 历史版本是文件级，不是项目级，也不是单个 segment 级。
- `translation_revisions.json` 保存在 job 目录下，缺失时读取方必须按空数组处理，保证旧任务缓存可继续打开。
- `segmentTranslations` 使用 `Record<segmentId, translatedText>` 保存当时该文件所有可翻译 segment 的非空译文。
- 如果当前文件没有任何有效译文，不创建 revision。
- `reason` 记录触发原因：`file-retranslation`、`selection-retranslation` 或 `language-change`。
- `scopeBlockIds` 只记录局部重翻触发时用户选中的 blocks，历史版本本身仍然是完整文件译文快照。
- 导出始终使用当前 `segments`，不使用历史版本。历史版本当前只用于查看，后续如需“恢复为当前译文”或“导出历史版本”需要单独设计入口。

## Job

`RosettaJob` 表示一个本地翻译任务。

约定：

- Job 状态变化应可恢复，不能只存在内存中。
- Job 的语言方向由 `RosettaDocument.sourceLang`、`RosettaDocument.targetLang` 和 `RosettaJobSummary.targetLang` 共同持久化。`sourceLang` 当前只在完整 bundle 中读取，job summary 暂不重复存储。
- `RosettaJobSummary.filename` 是用户看到的项目名。导入时默认使用源文件名或文件夹名，之后可以由用户重命名；`sourceFilename` 保留原始导入名。
- `RosettaJobSummary.sourceFiles` 保存侧边栏等轻量 UI 所需的文件列表。完整文件结构仍以 `RosettaDocument.files` 为准。
- `RosettaSourceFile.translationStatus` 和对应的 segment 统计字段用于文件树等轻量 UI 表达文件级翻译状态。它们由后端根据 `segments.json` 派生并同步，旧缓存缺失时按未翻译/0 处理；调度和导出仍以 `Segment.status` 为准。
- `RosettaJobSummary.targetLang` 只作为项目列表兼容字段。多文件项目可能存在不同目标语言，当前文件语言必须读取 `RosettaSourceFile.targetLang` 或其 document fallback。
- MVP 阶段任务缓存使用 JSON 文件，根目录固定在 app data 的 `jobs/` 下。
- Job store 的持久化文件必须带 `schemaVersion: 1`，后续格式变化需要迁移路径。
- `RosettaJobBundle` 是前端加载项目的最小完整单位，包含 `job`、`document`、`segments`、`translationFiles`、`translationRevisions`。
- `index.json` 只保存 `RosettaJobSummary[]`，完整文档和 segments 分别保存在项目目录下。
- 删除项目只删除 Rosetta 自己的 job cache，不删除用户原始文件，也不删除已经导出的文件。
- 后续如果引入 SQLite，需要新增 ADR 说明原因和迁移策略。

当前 JSON 布局：

```txt
AppData/Rosetta/jobs/
  index.json
  <jobId>/
    source.txt 或 source.md
    source.pdf
    sources/<relative-path>  # 文件夹项目
    document.json
    segments.json
    translation_files.json
    translations/<translationFileId>.json
    pdf_page_translations.<targetLang>.json
    pdf_page_translations.json  # 旧任务兼容读取
    pdf-pages/
      <targetLang>/
        page-0001.pdf
    translation_revisions.json
    exports/
```

导入约定：

- v1 目标支持 TXT、Markdown 和文字型 PDF。PDF 支持必须作为 importer 接入同一套 Rosetta IR，不应另起独立任务模型或预览模型。
- 文件由 Tauri command 读取，前端不直接获得宽泛文件系统权限。
- “新项目”可以导入单个文件，也可以导入文件夹。文件夹导入递归收集受支持格式，跳过隐藏目录，并限制项目文件数量，避免原型阶段一次塞入过大项目。
- 空白 TXT 文件由窄 Tauri command 在 Rosetta 本地缓存中创建，持久化为普通 `format: "txt"` 单文件 job，不设置 `sourcePath`。后续原文编辑仍按 TXT 规则重新切分 blocks 和 segments。
- 文件夹项目的 `RosettaJobSummary.sourceKind` 为 `directory`，`fileCount` 记录导入文件数。`format` 是轻量 summary 字段，混合项目只用它作为总体显示和兼容字段，每个文件的真实格式以 `RosettaSourceFile.format` 为准。
- TXT 按空行切分为段落。
- Markdown 使用轻量 block parser，首版只保留标题、段落、列表、引用、代码块和空行等基础结构。
- fenced code block、纯 URL 行和空白行默认 `skipped`。
- PDF v1 的基线支持是可提取文本的 PDF。扫描 PDF 和 OCR 不属于 v1 基线范围。PDF 格式高保真还原是 nice to have，可以探索，但不能阻塞文本提取、翻译、预览和文本式导出的主路径。
- PDF importer 应输出 `RosettaDocument(format: "pdf")`、`RosettaSourceFile(format: "pdf")`、`RosettaBlock[]` 和 `Segment[]`，后续翻译、预览、译文文件和导出继续复用现有流程。
- PDF 页码、页内顺序等来源信息先放入 `RosettaBlock.style.pdf`，例如 `{ page: 1, orderOnPage: 12 }`。高保真还原需要的 bbox、字体、列检测结果等布局信息也应先放在 `style.pdf` 中探索；不要在没有 ADR 的情况下把这些字段提升到核心模型顶层。
- PDF importer 遇到 image-only、加密或无法解析的文件时必须返回清晰错误，不能创建空任务。
- 系统文件选择和导出路径选择必须通过非阻塞 Tauri dialog command 完成，不能在 command 中调用 `blocking_pick_file` 或 `blocking_save_file`，避免 Windows 原生对话框打开时卡住应用窗口。
- 当前视觉 PDF 翻译路径把 PDF 作为版面保持型文档处理：导入阶段缓存 `source.pdf`，翻译阶段使用 `pdf2zh --pages` 生成页级译文 PDF，并把页状态保存到 `pdf_page_translations.<targetLang>.json`。这条路径不把 PDF 文本回填为普通 Rosetta text segments。
- 视觉 PDF 翻译路径中的文本块由 pdf2zh 抽取并通过本地 OpenAI shim 调用 RWKV，不经过普通文档的 `Segment[]` 调度。shim 必须在转发给 RWKV 前切分过长文本块，避免小上下文模型被 pdf2zh 合并出的长段落卡住。PDF shim 的默认策略是句子优先切分、短句按 token budget 合并，参考文献按 `[N]` 条目优先切分，图表 caption 保留编号和后续句子，常见 PDF 断词在切分前修复。
- `pdf_page_translations.<targetLang>.json` 是 PDF 页级译文状态文件，记录源 PDF 页数、目标语言、每页状态和页级译文 PDF 相对路径。应用加载时遗留的 `queued` / `translating` 页必须恢复为可重试状态。旧任务中的 `pdf_page_translations.json` 只作为兼容读取入口，读取后应写回语言专属状态文件。
- PDF 页级译文文件保存在 `pdf-pages/<targetLang>/page-000N.pdf`。这些文件是 Rosetta 内部缓存，不是用户导出文件。旧任务中的 `pdf-pages/page-000N.pdf` 只允许在目标语言可信时兼容读取，不能让不同目标语言静默复用同一页缓存。

导出约定：

- `translation` 导出纯译文。
- `bilingual` 导出双语对照。
- 未完成或失败 segment 导出时使用原文占位，避免输出断裂。
- Markdown 导出只承诺保留基础 marker，不承诺完整 CommonMark AST 级别还原。
- 任务工作台的导出最小单位是当前选中的译文文件，而不是整个项目。项目是文件集合与共享设置容器，不能让用户在当前文件视图里误触发整项目导出。
- 当前译文文件必须完成翻译后才能导出；`done`、`edited` 和 `skipped` 视为已处理，`pending`、`translating`、`failed` 或空译文不能导出。
- PDF 导出例外：视觉 PDF 导出始终生成完整 PDF，已翻译页使用 `pdf-pages/` 中的页级译文 PDF，未翻译页或失败页保留源 PDF 对应页面。因此 PDF 不要求所有页面完成后才能导出。
- 当前译文文件导出到用户选择的具体文件路径，输出文件名默认来自源文件名和目标语言，例如 `chapter.zh-CN.md` 或 `chapter.zh-CN.bilingual.md`。
- 多文件项目的批量导出如果后续恢复，应作为单独的项目级入口，并明确提示会导出项目内所有文件。删除项目只删除 Rosetta job cache，不删除用户原始文件或已导出目录。

## Preview

文档预览应优先呈现“源文件视图”，而不是直接暴露 segment 列表。

约定：

- 双语预览左侧渲染原文结构，右侧渲染当前选中译文文件的译文结构。
- 原文预览窗口只渲染当前源文件结构，不显示空译文栏。
- 主工作台不渲染双语预览，避免源文件切换时加载和测量大文档内容导致卡顿。双语预览放在独立窗口中按需加载。
- 多文件项目的默认预览范围是“当前选中的一个文件”，不是把项目内所有文件连续渲染在同一个预览面板里。当前文件由前端 UI state `activeFileId` 控制。
- 当前源文件由 `/jobs/:jobId/files/:fileId` 路由表达。当前译文文件在主工作台内由 `activeTranslationFileId` 表达；独立原文预览窗口使用 `/preview/:jobId/sources/:sourceFileId` 直接加载源文件，独立译文预览窗口使用 `/preview/:jobId/translations/:translationFileId` 深链接直接加载译文文件。
- 后台保存、导出刷新、翻译批次完成等异步结果不能无条件改变 active job/file。只有用户显式打开或导入项目时才允许设置 active bundle；后台结果应只刷新 job list，且仅在当前 active job 仍匹配时刷新已加载 bundle。
- Markdown 预览使用 Markdown renderer，并启用 GFM 等常见语法支持；不要执行原文中的 HTML/script。
- 原文和译文滚动应同步，hover 某个 block 时两侧对应 block 同步高亮。
- 滚动期间应暂停 hover 高亮更新，避免鼠标停在文本上时 hover state 与滚动同步互相触发重渲染。
- 独立预览窗口允许点击 block 选择局部重翻范围；选择单位是 block，保存单位仍是该 block 下的 `TranslationSegment[]`。
- 未翻译的 translatable block 在译文侧显示为空，不回退显示原文；`skipped` 内容如代码块仍可按原文保留。
- Segment 仍是调度和缓存单位，但不应作为普通用户默认看到的主要阅读结构。后续如果恢复结构切分调试视图，应作为单独的高级/诊断视图。
- 预览必须使用 block 级虚拟滚动，避免长文档一次性渲染全部 Markdown blocks。

## RWKV API Config

`RwkvConnectionConfig` 表示当前翻译后端连接设置，包括用户选择使用 Rosetta 管理的本地模型还是远程 API。

约定：

- `providerPreference` 是用户手动选择的翻译后端，当前支持 `local` 和 `remote-api`。翻译调度必须尊重该选择，不应仅因为本地 runtime 已 ready 就自动改用本地模型，也不应因为远程 API 已配置就自动改用远程。
- `baseUrl` 和 `endpoint` 共同组成请求地址。
- API token、body password 等凭据只能保存在用户本机设置中，不能写入仓库、文档、测试或 fixture。
- 远程或云端 API 必须是用户显式配置的 opt-in 后端。
- 选择 `local` 时，翻译 pipeline 使用 Rosetta 管理的本地 RWKV runtime；选择 `remote-api` 时，使用远程 API 配置。设置页必须清楚展示当前选择和该后端是否可用。
- 翻译请求使用当前任务的语言方向生成 prompt，格式为 `<SourceLabel>: ...\n\n<TargetLabel>:`。当前请求体使用 `contents[]` batch、`stream: true` 和模型后端指定的采样参数；响应解析需兼容普通 JSON 与 SSE `data:` chunk。当前 RWKV 工程师确认的主路径仍是 English -> Chinese，其他语言方向属于 UI 和数据模型已支持、模型效果待验证的扩展能力。

## Compatibility

核心类型一旦被任务缓存使用，就视为持久化格式的一部分。修改字段时需要考虑：

- 是否需要版本号
- 旧任务是否还能读取
- 是否需要迁移脚本
- 导出结果是否受影响
