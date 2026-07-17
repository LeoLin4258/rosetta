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
    pdf_source.json
    pdf_pages.<targetLang>.json
    pdf_run.<targetLang>.json
    translated-pages/
      <targetLang>/
        page-0001.pdf
    pdf-prepare-cache/
      v1/<prepare-key-sha256>/
        manifest.json
        layout.npz
    .tmp/pdf-runs/<runId>/
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
- PDF v2 的当前实现是视觉 PDF 翻译路径：导入阶段只创建 job-local `source.pdf` 和 PDF skeleton document，翻译阶段由 PDFMathTranslate fork 的 Rosetta-native engine contract 生成页级译文 PDF。旧的“PDF 转 RosettaBlock/Segment 后复用 TXT/Markdown 调度”和 v1 shim/replay 路径是历史背景，不再描述当前主路径。
- PDF importer 应输出 `RosettaDocument(format: "pdf")`、一个 `RosettaSourceFile(format: "pdf")`、空 `RosettaBlock[]` 和空 `Segment[]`。PDF 仍进入普通 job/workbench 模型，但页面翻译事实不保存在 `Segment[]` 中。
- PDF importer 遇到 image-only、加密或无法解析的文件时必须返回清晰错误，不能创建空任务。
- 系统文件选择和导出路径选择必须通过非阻塞 Tauri dialog command 完成，不能在 command 中调用 `blocking_pick_file` 或 `blocking_save_file`，避免 Windows 原生对话框打开时卡住应用窗口。
- 当前视觉 PDF 翻译路径把 PDF 作为版面保持型文档处理：导入阶段复制 `source.pdf`，翻译阶段使用 PDFMathTranslate fork 的 Rosetta-native engine contract 按 page window 生成页级译文 PDF，并把页状态保存到 `pdf_pages.<targetLang>.json`。这条路径不把 PDF 文本回填为普通 Rosetta text segments。
- 视觉 PDF 翻译路径中的文本单元由 Python PDF engine 抽取为 typed `TranslationUnit[]`，再由 Rust 调用本地 provider 翻译。Python worker 不调用 RWKV、OpenAI-compatible shim、Rosetta HTTP batch endpoint 或 translator service。PDF 翻译事实不经过普通文档的 `Segment[]` 调度。
- `pdf_source.json` 是 PDF source 元数据文件，记录 `pageCount`、`sourceFingerprint`、导入文件名、原始路径快照和时间戳。`sourceFingerprint` 只用于诊断和未来显式去重，不触发隐式共享状态。
- `pdf_pages.<targetLang>.json` 是 PDF 页级译文状态文件，记录源 PDF 页数、目标语言、每页状态、正式 `PageResult` 元数据和页级译文 PDF 相对路径。schema version 2 只持久化 `pending`、`translated`、`failed`。应用加载时遗留的 `queued` / `translating` 页必须恢复为可重试状态。
- `pdf_pages.<targetLang>.json` v2 中，`resultKind` 可为 `translated`、`no_text`、`failed`。`resultKind="no_text"` 表示该页完成但无可提取文本，不应伪造 `translatedPdfPath` 或译文字数。
- PDF v2 不迁移 beta v1 页状态。读取到 `schemaVersion < 2` 的 PDF page state 时，Rosetta 必须清理派生译文 artifacts 和旧 page-state 文件，保留 `source.pdf`，并返回空的 v2 pending state。
- `pdf_run.<targetLang>.json` 是当前或最近一次 PDF 翻译 run。`running` / `pausing` run 必须绑定 `ownerSessionId`；新 app session 看到旧 live run 时必须恢复为 `paused`。
- PDF 页级译文文件保存在 `translated-pages/<targetLang>/page-000N.pdf`。这些文件是 Rosetta 内部译文产物，不是用户导出文件。旧任务中的 `pdf-pages/<targetLang>/page-000N.pdf` 和 `pdf-pages/page-000N.pdf` 只作为兼容读取入口；repair 应迁移或复制到 `translated-pages/`。
- 旧任务中的 `pdf_page_translations.<targetLang>.json` 和 `pdf_page_translations.json` 只作为兼容读取入口，新写入必须使用 `pdf_pages.<targetLang>.json`。
- `.tmp/pdf-runs/<runId>/` 只保存当前批次输出。批次提交成功后应清理；启动/repair 可清理无 active lease 的旧临时目录。
- `pdf-prepare-cache/v1/<prepare-key-sha256>/` 是可删除的 PDF 派生缓存，不是翻译事实。`layout.npz` 只保存压缩后的 ONNX layout masks；`manifest.json` 保存 schema、source fingerprint、页选择、语言方向、engine/model signature 和时间戳。读取时任何身份或版本不匹配都必须视为 miss，不能复用旧数组。
- PDF prepare cache 必须先原子替换 `layout.npz`、最后原子替换 `manifest.json`；只有完整且通过校验的 manifest 才表示可用。每个 job 最多保留 12 个窗口且总量最多 256MB，按最近使用时间淘汰。删除 job 时随 job 目录一并删除，不跨 job 共享。

PDF prepare cache manifest:

```json
{
  "schemaVersion": 1,
  "engineVersion": "rosetta-pdf-engine-v2.1",
  "cacheKey": "{...}",
  "sourceFingerprint": "...",
  "pages": [1, 2, 3],
  "model": {
    "filename": "doclayout_yolo_docstructbench_imgsz1024.onnx",
    "bytes": 123456789,
    "modifiedNs": 1784104077254000000
  },
  "layoutFile": "layout.npz",
  "createdAt": 1784104077254,
  "updatedAt": 1784104077254
}
```

PDF source metadata:

```json
{
  "schemaVersion": 1,
  "pageCount": 436,
  "sourceFingerprint": "sha256:...",
  "filename": "source.pdf",
  "originalPath": "C:/Users/...",
  "importedAt": "1782369534004",
  "updatedAt": "1782369534004"
}
```

PDF page state:

```json
{
  "schemaVersion": 2,
  "sourcePageCount": 436,
  "targetLang": "zh-CN",
  "pages": [
    {
      "pageNumber": 1,
      "status": "translated",
      "resultKind": "translated",
      "translatedPdfPath": "translated-pages/zh-CN/page-0001.pdf",
      "sourceUnitCount": 8,
      "translatedUnitCount": 8,
      "sourceChars": 1788,
      "translatedChars": 551,
      "artifactVersion": "1782369534004",
      "artifactCompression": "fast",
      "artifactBytes": 123456,
      "error": null,
      "updatedAt": "1782369534004",
      "lastRunId": "pdf-run-1782369534004"
    }
  ]
}
```

PDF run state:

```json
{
  "schemaVersion": 1,
  "runId": "pdf-run-1782369534004",
  "jobId": "job-1782369534004-document",
  "targetLang": "zh-CN",
  "state": "running",
  "mode": "continue",
  "requestedPages": [1, 2, 3],
  "completedPages": [1],
  "failedPages": [],
  "currentChunk": [2, 3],
  "ownerSessionId": "session-1234-1782369534000",
  "leaseUpdatedAt": "1782369535000",
  "cancelRequested": false,
  "startedAt": "1782369534004",
  "updatedAt": "1782369535000",
  "lastError": null
}
```

PDF cleanup task:

```json
{
  "jobId": "job-1782369534004-document",
  "path": "AppData/Rosetta/jobs/.trash/job-1782369534004-document-1782369535000",
  "createdAt": "1782369535000",
  "lastError": "The process cannot access the file..."
}
```

导出约定：

- `translation` 导出纯译文。
- `bilingual` 导出双语对照。
- 未完成或失败 segment 导出时使用原文占位，避免输出断裂。
- Markdown 导出只承诺保留基础 marker，不承诺完整 CommonMark AST 级别还原。
- 任务工作台的导出最小单位是当前选中的译文文件，而不是整个项目。项目是文件集合与共享设置容器，不能让用户在当前文件视图里误触发整项目导出。
- 当前译文文件必须完成翻译后才能导出；`done`、`edited` 和 `skipped` 视为已处理，`pending`、`translating`、`failed` 或空译文不能导出。
- PDF 导出例外：视觉 PDF 导出始终生成完整 PDF，已翻译页使用 `translated-pages/` 中的页级译文 PDF，未翻译页或失败页保留源 PDF 对应页面。因此 PDF 不要求所有页面完成后才能导出。
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

## PDF v3 PageGraph

PDF v3 的 `PageGraph` 是独立于当前 PDF v2 page-state 的 native 页面 IR。
它目前仍处于隔离开发阶段，尚未写入正式 job cache，也不需要迁移 v1/v2
派生 artifact。

约定：

- `PageGraph.schemaVersion` 当前为 `4`。
- 一个 `PageAtom` 只在 reconciliation 全部对象级检查通过后，才能从
  `pdfium-unverified` 变为 `pdfium-verified` 或 `to-unicode-corrected`。
- atom 的低层来源使用稳定的 mapping ID、text-show ID、operand ID、operand
  index、可选 `TJ` array index、encoded byte range 和 source-unit character
  index 表达；不把原始 encoded bytes 复制进 PageGraph。
- 一个源编码单元可以对应多个 Unicode atom，例如 ligature。相关 atom 共享
  encoded byte range，并使用 `sourceUnitCharIndex` / `sourceUnitCharCount`
  区分单元内字符。
- PDFium 根据几何生成但不属于 encoded operand 的空白标记为
  `pdfium-synthetic-whitespace`，不得伪造 operand provenance，也不得进入翻译。
  `isGenerated` 空白即使能关联到 PDFium 文本对象，也仍按 synthetic 处理，除非
  后续 source reconciliation 证明它对应真实 encoded unit。
- ToUnicode 中存在但 PDFium 没有几何 atom 的空白只计入
  `unrepresentedSourceWhitespaceCount`；原操作数字节保持不变。
- 映射失败的文本对象必须整对象保持 `preserved-unmapped`，不能留下部分 atom
  已修正、部分 atom 未修正的状态。
- 页面 reconciliation 状态只能是 `unreconciled`、`complete`、`partial` 或
  `preserved`。字体不匹配、atom 覆盖缺失或 decoder 缺失必须让页面保持
  `partial` / `preserved` 并记录 typed fallback reason。
- Form XObject 按 `Do` 操作顺序递归；Form 自有资源优先，缺失资源从调用上下文
  回退。text-show ID 包含稳定 invocation path，operand ID 仍指向底层唯一 stream
  字节位置。
- schema v3 的 `formInvocationPath` 必须是结构化 `FormInvocationStep[]`，每一跳包含
  parent stream object/generation、`Do` operation index 和 child Form stream
  object/generation。renderer 不得从 text-show ID hash 或展示字符串反解析调用路径。
- schema v4 的 atom source provenance 还必须包含 stream object/generation、operation
  index、unqualified source font resource、source `Tf` size 和 source `Tz` horizontal
  scaling。renderer 必须重新解析 operation 前的文本状态并逐项校验，不能只信持久化值。
- 同一 Form stream 的多次视觉调用不能伪装成独立 operand。`SharedContentStream`
  只有在 decode、font identity 和 atom coverage 等基础 gate 全部通过后才可作为
  可映射能力标记，不能覆盖更具体的 typed fallback。reconciliation 可以保留完整
  invocation provenance；renderer 必须通过 invocation-local copy-on-write 隔离实际
  视觉调用。直接 Form stream、引用环、超过 32 层和 PDFium/source 调用数不一致仍
  必须产生 typed fallback。
- `PageGraph` source text 是本地派生数据，不能进入遥测或普通诊断日志。
- 正式持久化 PageGraph 前必须再确定压缩格式、source/engine/schema identity、
  原子写入和可删除重建规则。

## PDF v3 TranslationPatch

PDF v3 的 `TranslationPatch` 是页级译文的持久化权威数据，不是 PDF 文件，也不同于
renderer 内部瞬时使用的 `ContentOperandRangePatch`。当前逻辑 schema 为 `1`；它仍在
隔离 native 模块中，尚未接入正式 job store，因此不迁移 v1/v2 PDF 派生 artifact。

约定：

- patch 必须绑定 1-based page number、`sourcePageHash`、目标语言、正数
  `translationRevision`、provider/model identity 和 renderer version。
- `entryId` 由 source page hash 与按 PageGraph 顺序排列的 atom ID 确定；`patchId` 是
  清空自身 ID 字段后完整 canonical patch 的 SHA-256。translation metadata 和 renderer
  decision 都属于 patch identity。
- 每个 entry 必须保存 ordered atom ID 及完整 source `PageAtom` 的 SHA-256，但不得重复
  保存普通 source text。一个 atom 在一份 page patch 中最多属于一个 entry；
  `preserved-unmapped` atom 不得进入译文 entry。
- 一个 entry 当前必须解析到唯一非空 `styleId`。mixed-style translation 要先拆成可验证
  entry，不能在持久化后靠字符串启发式恢复样式。
- protected span 必须用固定宽度 `u32` 保存 span ID/kind/exact value 之外的 translated
  UTF-8 byte start/length，不能把平台相关的 `usize` 写进持久化 schema。
  entry 必须完整覆盖 span 的所有 atom，placement bytes 必须逐字节等于 exact value，且
  多个 placement 不得重叠。
- PageGraph protected span 本身必须由唯一、存在、严格 source-order 的 atom 组成，且
  atom source text 拼接结果必须等于 `exactText`。引用、URL、数字等区域需要在 PageGraph
  中拆为明确 atom，不能在 renderer 中用 substring 猜测。
- renderer decision 只能是 `pending`、带显式 fit strategy 和有限 `0..=1` scale 的
  `fitted`，或带稳定 reason code 的 `preserved`。renderer 写回 decision 后必须重算
  `patchId`。
- decode 必须针对当前 PageGraph 重建 canonical patch，拒绝 page/hash/schema、atom hash、
  entry order/content、protected placement 或 patch identity 不一致的内容。
- 初始 encoding 为 compact JSON。build、encode 和 decode 都执行 16 MiB page-patch
  上限；单 entry translated text 上限为 8 MiB，单 patch entry count 上限为 100,000。
- compact JSON patch 本身不负责文件所有权；原子 revisioned 文件与索引由下述
  TranslationPatch Store 管理。压缩容器、render-cache quota 和 streaming export 仍属于
  后续 Phase 4。

## PDF v3 TranslationPatch Store

PDF v3 patch store 是 source document + target language 隔离的页级译文磁盘权威。它不
复用 v1/v2 PDF page state，也不保存完整译文页 PDF。

约定：

- target-language 目录使用 exact language identity 的 SHA-256，不允许用户输入直接成为
  相对路径；store root 必须是 native orchestrator 提供的绝对路径。
- 根 `manifest.json` 只保存 schema、source fingerprint、exact target language、固定
  `pagesPerShard = 64` 和 deterministic manifest ID，不随每一页提交增长。
- 页索引使用 `shard-XXXXXXXX.json`。每个 shard 最多覆盖 64 个连续页号，并保存独立
  generation、deterministic shard ID 和 source-ordered page entries。
- 64 页是内部 index bound，不是 PageSet、translation batch、scheduler window 或 UI chunk。
  任意页仍可独立读取、提交和重试。
- page entry 必须保存 page/source hash、positive translation revision、patch ID、immutable
  patch filename 和 byte count。filename 必须由 page/revision/patch ID 确定，不能接受
  manifest 中的任意相对路径。
- patch file 先通过 unique temp + `sync_all` + rename 提交；owning shard 再通过
  temp + backup + rename 原子替换。新 shard durable 后才能删除被替换的旧 revision。
- 同 revision + 同 patch ID 是幂等写；同 revision + 不同内容是 conflict；低 revision
  必须拒绝。source page hash 在同一 store/page 内变化也必须拒绝。
- 每个进程/store 首次访问执行完整 repair。canonical/temp/backup shard 选择最高有效
  generation，generation 相同优先 canonical。普通后续提交只读取 owning shard，不得
  重新读取所有历史 page patch。
- missing/corrupt patch 只移除对应 page entry；其他页必须保持可读。结构损坏或 filename /
  internal shard index 不一致的 shard 作为可重建派生状态丢弃。
- repair 必须删除 incomplete patch temp、unreferenced patch、superseded revision 和失效
  index sidecar。正常提交也应立即删除本页已替换 revision，避免长任务磁盘持续增长。
- 同一 absolute language directory 的进程内写入由共享 coordinator 串行化。当前不允许
  多 Rosetta 进程并发拥有同一个 job store。

## PDF v3 Content Operand Patch

PDF v3 的 `ContentOperandRangePatch` 是隔离 native renderer 的瞬时低层写入模型，
不是持久化 `TranslationPatch`，也尚未进入 job cache 或前端协议。

约定：

- patch 必须包含 1-based page、stream object/generation、operation index、operand
  index、可选 `TJ` array index、encoded byte start/length、完整源 operand byte
  count、完整源 operand SHA-256、replacement bytes 和结构化 Form invocation path。
- 同一批 patch 必须属于同一页，目标 stream 必须能从选中页到达；执行时不得按
  Unicode 文本搜索或猜测源位置。
- 写入前必须校验完整 operand 长度和 hash。针对同一 operand 的多个 patch 必须
  对源 identity 达成一致、全部在边界内且互不重叠。
- replacement 按 byte offset 倒序应用。所有受影响 stream 必须先在 clone 上完成
  decode、patch、encode 和 compress，只有全部成功后才能一次提交到 document；
  任一失败不得留下部分 stream 已修改。
- 被多个页面直接引用的 page content stream 不得原地修改。Form 在选中页有多个
  实际 invocation，或从多个页面 `/Resources/XObject` 图可达时，也不得原地修改。
  当前 executor 可对同一 selected page 的多个 logical target 执行 invocation-local
  copy-on-write：以 root page stream 和结构化 invocation path 前缀构建 clone tree，
  共同祖先只克隆一次，多个 root `/Contents` 引用一次性重定向。进入同一 COW root 的
  其他 staged target 必须合并进 clone tree，不能再原地修改 source stream。
- multi-target clone tree 必须先验证全部 invocation path、operand identity、resource
  binding 和 stream encoding，再按 leaf-to-root 的确定性顺序分配对象并一次 commit。
  任一 target 失败不得留下已克隆的 sibling target 或改变 `max_id`。
- 跨页 Form 判断允许保守拒绝未实际执行的资源声明，但不得为追求精确而解压解析
  所有未选中页面内容流。直接 Form、引用环和超过 32 层的资源图必须返回 typed
  ownership failure。
- replacement bytes 和 source operand bytes 不得进入普通诊断、日志或序列化结果；
  结果只报告 count、stream ID、hash/identity 错误和耗时。

## PDF v3 Unified Translation Font

PDF v3 译文不要求复用或匹配源 PDF 字体家族。源字体只负责 extraction/style
provenance；译文编码使用 Rosetta 管理的统一字体家族。

约定：

- 简体中文默认 `SourceHanSansCN-Regular.ttf`；只有存在已验证 bold style span 时才
  加载 `SourceHanSansCN-Bold.ttf`。其他语言的广覆盖候选为
  `GoNotoKurrent-Regular.ttf`。
- font asset、cache key 和 prepared subset 必须携带显式 `regular` / `bold` face
  intent。renderer 只能使用 PageGraph style plan 选定的 face；weight 不匹配必须在
  document mutation 前 typed 拒绝。
- bold intent 不能只依赖 PDFium 数值。当前分类为 weight >= 600，或去掉 subset
  prefix 后 font name 含受控 bold marker；未知/缺失 weight 必须 preserve，不能猜测。
- 生产 renderer 不得从 Windows/macOS/Linux 系统字体目录动态解析字体。字体必须是
  component manifest 管理的离线资产，并带 version、SHA-256、size 和 license 元数据。
- 字体文件必须在进程资产缓存中只读取/解析一次；page task 不得重复读取 10-15 MB
  font file。
- 每个 document/face 先收集完整 Unicode scalar set，再按 codepoint 排序生成一个
  deterministic subset。相同 font fingerprint + face index + glyph set 必须得到相同
  subset name、CID assignment 和 subset bytes。
- subset 前必须验证 outline embedding permission、subsetting permission、当前支持的
  TrueType `glyf` outline 和完整 glyph coverage。缺字不得静默调用系统 fallback；该
  region 必须进入 typed preservation/fallback。
- PDF 字体使用共享 Type0/CIDFontType2、Identity-H、显式 CID-to-GID、widths 和
  ToUnicode。CID 与 subset GID 分离，保证 translated text 可搜索、复制和重新提取。
- 同一 font subset object 可以挂到任意页资源字典；不得按页重复嵌入 font bytes。
- 当前 direct-cmap 写入只证明简体中文与 Latin。Arabic、Indic 等复杂脚本即使字体有
  glyph coverage，也必须等待 shaping engine，不能逐 Unicode scalar 直接写入。
- 普通诊断只记录 asset ID、font fingerprint、glyph/subset byte count、coverage 状态和
  耗时，不记录 translated text 或完整 codepoint 列表。

## PDF v3 Text-Show Replacement Transactions and Batches

当前 PDF v3 回填开放同一 selected page、底层 stream、结构化 invocation path 和
`BT/ET` 内的保守事务路径；single text-show 是一项事务的包装。

约定：

- request 必须引用选中页可达的 page/Form content stream、完整结构化 invocation path、
  operation index、expected operator、完整 text operand hash、source font
  resource/size/scaling、translated text、reconciled PageGraph 和 minimum fit scale。
- 选中页重复引用同一 top-level content stream、目标 path 不可验证、目标不在
  `BT/ET`、font state 不一致，或 source hash/operator 变化时必须 typed 拒绝。被其他
  页引用的 top-level stream 通过 selected-page copy-on-write 隔离，不得原地修改。
- renderer 在原 operation 位置临时切换统一 font `Tf` 与 fitted `Tz`，写入 translated
  CID show，然后恢复 source `Tf/Tz`。不得修改共享上游 `Tf`，不得使用背景矩形遮盖。
- `TJ` 的 source kerning 数字不得用于不同语言译文；当前转换为单个 `Tj`。
- 若目标后同一 text object 仍有 show，下一 show 前必须存在可验证的 position anchor：
  finite 且 operand shape 正确的 `Tm/Td/TD/T*`，或合法 quote show。无 anchor 的连续
  `Tj/TJ` 必须 preserve，不能假定译文 advance 与 source 相同。
- 一个 transaction 必须属于同一 page、底层 stream、完整 invocation path 和同一
  `BT/ET`，且 operation index 不重复。每个 entry 只能使用其 PageGraph style 选定的
  prepared Regular/Bold face；缺失或重复 face weight 必须 typed 拒绝。所有 entry
  必须基于未修改的 source content 完成校验，再按 operation index 倒序 splice，最后
  一次 commit。
- page-level batch 可以包含多个 transaction target，但所有 target 必须属于同一
  selected page。logical target identity 是 stream、invocation path 与 source `BT/ET`
  bounds 的组合，同一 identity 不得重复；同一 physical stream/path 可以包含多个不同
  文本对象。
  所有 target 必须基于未修改的 source document 完成 hash/style/fit/encoding/path 校验，
  再统一 staging 和 commit。
- logical target 校验完成后，renderer 必须按 `stream + invocation path` 聚合 physical
  target。同一 physical target 的 replacement operation 必须按 source operation index
  倒序合并，只允许一次 decode/encode/compress 和一次 ownership commit。unique top-level
  stream 只重写一次；同一 Form invocation 只生成一个 leaf clone。
- batch 需要的 Regular/Bold face 必须先取并集，每个 weight 只能生成一套 document-level
  subset。任一 target 需要 copy-on-write 时，整批 target 都必须进入同一 clone forest；
  包括本可原地更新的 unique top-level root，避免同一原子批次同时修改 source 与 staged
  page。多个 `/Contents` root 必须通过一个 page dictionary 一次重连。
- fit scale 低于 readability floor 时必须 preservation/overflow，不得无限压缩。
- 多个 face 的 staged font objects 必须按确定性 weight 顺序连续预留不重叠 object ID；
  clone object 必须从字体预留区间之后继续分配。unique top-level target 将字体挂到
  selected page resources；Form target 必须物化 leaf 的有效继承资源后挂载字体，并始终
  执行 invocation-local copy-on-write；跨页共享 top-level target 只克隆 stream 并重连
  selected page。字体、rewritten/cloned streams 和 page dictionary 只有全部校验成功后
  才能一起 commit；失败不得改变 `max_id` 或任何 document object。
- single text-show request 不得接受上层猜测的 max advance。renderer 必须通过
  text-show ID、stream/operation 和 source `Tf`/`Tz` provenance，从同一 source
  object 的 PageGraph origin、loose bounds 和 character transform 确定性计算
  page-space advance，再按 baseline matrix scale 转为 text-space fit bounds。
- 当前 PDFium 只提供 axis-aligned character bounds，因此 geometry fit 只开放页面轴
  对齐 baseline，包括正反向水平/垂直和缩放。任意角度 baseline 必须 typed preserve，
  不能投影 AABB 后高估可用宽度。
- 同一 source object 的 atom 必须解析到唯一 PageStyle。当前只允许非 italic、
  `FilledUnstroked`、有效 fill color/opacity 和可分类 weight；单个 show 内存在 mixed
  style、stroked、clipping 或缺失状态必须 preserve。不同 show 的已验证 Regular/Bold
  style 可以在同一 transaction 原子回填。
- renderer 必须从 content stream 起点重放目标前的 `q/Q`、`Tf/Tz`、`Tr` 和
  DeviceGray/DeviceRGB/DeviceCMYK paint state，并与 PageGraph style 核对。
  `cs/CS/sc/SC/scn/SCN/gs` 在完整 interpreter 实现前必须 typed preserve。
- per-show diagnostic schema 当前为 `rosetta-pdf-v3-text-show-replacement/6`，transaction
  schema 为 `rosetta-pdf-v3-text-show-replacement-transaction/3`。transaction 必须报告
  确定性排序的 `translationFontWeights`、`formInvocationDepth`、`clonedStreamCount` 和
  `pageContentRewired`，不能压缩成单一 weight。只允许报告 page/stream、count、style
  ID、weight/face、normalized color/opacity、render mode、geometry/fit、staged/cloned
  object count 和 timing，不得包含 source/translated text。
- page batch diagnostic schema 当前为
  `rosetta-pdf-v3-text-show-replacement-batch/1`，target schema 为
  `rosetta-pdf-v3-text-show-replacement-batch-target/1`。batch 负责全局 target/replacement、
  font object、clone、page rewiring 和 timing 计数；target 只记录自身 stream/path depth、
  replacement count、weights 与 per-show diagnostics，不得记录文本 payload。
