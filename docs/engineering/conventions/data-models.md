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
- 当前视觉 PDF 生产主路径是受管 `pdf2zh` page-artifact pipeline：导入阶段创建 job-local `source.pdf` 和 PDF skeleton document；Rust 持久化 `pdf_pages.<targetLang>.json`、`pdf_run.<targetLang>.json` 与逐页译文 PDF，Python worker 只负责 prepare、translation-unit collection 和 page render。
- PDF importer 应输出 `RosettaDocument(format: "pdf")`、一个 `RosettaSourceFile(format: "pdf")`、空 `RosettaBlock[]` 和空 `Segment[]`。PDF 仍进入普通 job/workbench 模型，但页面翻译事实不保存在 `Segment[]` 中。
- PDF importer 遇到 image-only、加密或无法解析的文件时必须返回清晰错误，不能创建空任务。
- 系统文件选择和导出路径选择必须通过非阻塞 Tauri dialog command 完成，不能在 command 中调用 `blocking_pick_file` 或 `blocking_save_file`，避免 Windows 原生对话框打开时卡住应用窗口。
- PDF 页面翻译不经过普通文档 `Segment[]` 调度，也不投影成前端 segment translation run；工作台通过 PDF 专用 page/run commands 读取进度、预览和导出事实。
- `pdf_source.json` 是 PDF source 元数据文件，记录 `pageCount`、`sourceFingerprint`、导入文件名、原始路径快照和时间戳。`sourceFingerprint` 由 production `source_state` 直接计算 canonical `sha256:<64 lowercase hex>`；CP9 删除 native v3 后不再依赖共享 `DocumentHandle` identity primitive。
- `pdf_pages.<targetLang>.json` schema v2 是当前 durable 页状态；`translated-pages/<targetLang>/page-000N.pdf` 是对应页的正式译文 artifact。
- `pdf_pages.<targetLang>.json` v2 中，`resultKind` 可为 `translated`、`partial`、`no_text`、`failed`。`resultKind="partial"` 表示页面 artifact 已成功生成，但至少一个已知渲染单元因译文为空、缺失或占位符损坏而保留原文；`translatedUnitCount + fallbackUnitCount` 必须等于 `sourceUnitCount`。`resultKind="no_text"` 表示该页完成但无可提取文本，不应伪造 `translatedPdfPath` 或译文字数。
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
      "resultKind": "partial",
      "translatedPdfPath": "translated-pages/zh-CN/page-0001.pdf",
      "sourceUnitCount": 8,
      "translatedUnitCount": 7,
      "fallbackUnitCount": 1,
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
- 当前 PDF 导出由 production exporter 从 `source.pdf` 与已提交的 `translated-pages/<targetLang>/page-000N.pdf` 组装完整 PDF；已归档的 native v3 export coordinator 和 command surface 已删除。
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

## Archived PDF v3 Contracts (Non-Production)

本节及其后的 PDF v3 数据模型只记录 2026-07-16 至 2026-07-20 native
rewrite 的历史 contract，供源代码历史取证使用。相关实现、Cargo feature、命令和测试已在
2026-08-02 的 CP9 第二步删除；下文出现的 command、schema、artifact 和“当前”措辞都只是
历史记录，不表示仍受支持或可恢复。当前 production authority 以本文前面的 PDF
page-artifact 约定、`docs/engineering/pdf-pipeline.md`、ADR 0077 和当前代码为准。

这些 beta v3 artifacts 不迁移到 production page state。若未来重新启用任何 v3 contract，
必须先建立新的 active plan/ADR、定义持久化迁移或 reset 边界，并重新通过真实文档和三平台验收。

## PDF v3 PageGraph

PDF v3 的 `PageGraph` 是独立于当前 PDF v2 page-state 的 native 页面 IR。
它目前仍处于隔离开发阶段，尚未写入正式 job cache，也不需要迁移 v1/v2
派生 artifact。

约定：

- PDF v3 工作台只持有 newest target-language run 的可重建投影：一个 bounded control status，以及最多四个 64-record visible page windows。不得读取完整长文档 page state、持久化 current-run pointer，或把前端缓存升级成 authority。
- run discovery 未完成或失败时必须 fail closed，不能创建并发 run、显示 legacy 译文或开放 legacy PDF 导出。nonterminal run 的页面选择必须锁定。
- 顶栏状态与 pause/resume/cancel/recover/retry 只由 native run status 和 owner eligibility 驱动。`completed + preserved` 是完成页计数；failed page retry 只在 `retryable=true` 且当前 session 拥有 run 时开放。

- `PageGraph.schemaVersion` 当前为 `6`。schema v6 是 beta reset：旧提取 artifact
  不迁移并由当前 source authority 逐页重建。
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
- schema v5 的 atom source provenance 还必须保存 exact text-show operator 和完整 text
  operand SHA-256。`TranslationPatch` renderer 只能从这份已 reconciliation 的 provenance
  构造低层 request，不得重新按 Unicode 文本搜索 content stream。
- schema v6 在 reconciliation 后派生 page-local `line -> paragraph -> flow-container`
  hierarchy。每个 group 保存稳定 ID、按阅读顺序排列的 atom ID、PDF page-space
  `[left, bottom, right, top]` bounds 与有限 `0..=1` confidence；不得复制 source text。
- visual hierarchy 只接纳 finite、近水平且已经 `pdfium-verified`、
  `to-unicode-corrected` 或 synthetic whitespace 的 atom。每个 atom 在同一结构层最多归属
  一个 group，line/paragraph/flow-container 三层覆盖集合必须完全一致。
- 分组必须逐页、确定性且有硬上限：最多 200,000 个 eligible atoms、50,000 条 line、
  25,000 个 paragraph 和 10,000 个 flow container。超过 atom 或派生 group 上限时整页不写
  部分 hierarchy，只记录不含正文的 typed warning。
- visual line 先按 baseline 与水平 gutter 拆分，paragraph 再按垂直节奏、水平重叠、缩进和
  emphasis transition 组合；flow container 要求稳定列边界，不能因错开的段落或页尾横向
  文本跨越双栏 gutter。低置信度区域在 region renderer 开放后必须整容器 preserve。
- 同一 Form stream 的多次视觉调用不能伪装成独立 operand。`SharedContentStream`
  只有在 decode、font identity 和 atom coverage 等基础 gate 全部通过后才可作为
  可映射能力标记，不能覆盖更具体的 typed fallback。reconciliation 可以保留完整
  invocation provenance；renderer 必须通过 invocation-local copy-on-write 隔离实际
  视觉调用。直接 Form stream、引用环、超过 32 层和 PDFium/source 调用数不一致仍
  必须产生 typed fallback。
- `PageGraph` source text 是本地派生数据，不能进入遥测或普通诊断日志。
- 正式持久化 PageGraph 前必须再确定压缩格式、source/engine/schema identity、
  原子写入和可删除重建规则。

## PDF v3 Translation Plan

PDF v3 `TranslationPagePlan` 是单页、进程内的 provider-neutral 翻译计划，不是磁盘
authority，也不能进入 job cache、scheduler shard 或前端持久状态。每次处理页面时都从
当前 reconciled PageGraph 重建，PageGraph 释放时计划也必须释放。

约定：

- `TranslationPagePlan.schemaVersion` 当前为 `1`；计划绑定 exact page number 与
  `sourcePageHash`。
- 当前一个 unit 必须完整覆盖一个 source text object 的 provenance-bearing atoms，且
  atom 必须共享一个 style 与一个 text-show identity。无法满足时记录稳定 preserved
  reason，不能拆成猜测性的部分回填。
- `unitId` 由 plan contract version、source page hash、page number 与 source-order atom
  IDs 计算；不得使用 source text、数组位置或 provider 输出位置作为 correspondence key。
- provider input 中 protected span 使用不会与该 source unit 冲突的 `{vN}` token。provider
  result 必须包含每个 token 恰好一次、顺序不变且不得包含未知 token；reassembler 恢复
  `exactText` 并记录 UTF-8 byte placement。
- provider results 按 exact `unitId` 组成完整集合。结果可乱序，但 missing、duplicate、
  unknown 或 extra unit 必须使本页失败，不能按数组位置或文本相似度补配。
- reassemble 前必须从当前 PageGraph 重建 canonical plan 并与传入 plan 完整比较，拒绝 stale
  page/hash/atom/style/provenance 或被篡改的计划。
- 计划最多 100,000 units、每 unit 最多 1 MiB source UTF-8 bytes、每页 accepted source
  最多 16 MiB；不得跨页持有 PageGraph 或 source text。
- 没有 safe unit 的页面必须由 translation worker 显式提交 `Preserved`，不得生成空 patch
  表示翻译成功。
- 计划中的 source/provider text 只存在于当前处理内存。持久化 TranslationPatch 仍不得复制
  普通 source text。
- local-provider bridge 只消费 plan 的 `providerText`，并通过 provider-owned generic unit
  执行 batch/chunk/retry。PDF v3 不得构造或持有 legacy pdf2zh worker unit。
- `{vN}` protected token 必须在 provider chunk planner 中从 model input 移除，并在 provider
  output 重建时保持原 token；随后仍由 Translation Plan reassembler 验证 exact set/order 并
  恢复 `exactText`。
- provider bridge failure 只能暴露稳定 kind、retryability 和无文本 message；raw response、
  source text 或 translated text 不得进入 scheduler shard、普通诊断或前端状态。
- provider/model identity 不由 plan 或 bridge 猜测。renderer-owning page processor 必须从选中
  runtime/component manifest 获取并写入 `TranslationPatchDraftMetadata`。

## PDF v3 Legacy Text-Show TranslationPatch

text-show `TranslationPatch` schema `1` 是 PDF v3 旧 renderer 的页级译文模型。它保留给
低层回归和 beta 基线，不再是新建生产 run 的 authority；新 run 使用下一节的 region patch。
它不是 PDF 文件，也不同于 renderer 内部瞬时使用的 `ContentOperandRangePatch`。

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
- `pending` patch 只是 translation planner 与 renderer 之间的进程内草稿。renderer 必须
  一次为每个 entry 生成唯一非 pending decision 并重算 `patchId`；只有完整 resolved patch
  才能进入 TranslationPatch Store。App 崩溃在 store commit 前时，该页 revision 重新规划和
  渲染，不能恢复或猜测一个半完成的 pending draft。
- decode 必须针对当前 PageGraph 重建 canonical patch，拒绝 page/hash/schema、atom hash、
  entry order/content、protected placement 或 patch identity 不一致的内容。
- 逻辑 encoding 为 compact JSON。build、encode 和 decode 都执行 16 MiB page-patch
  上限；单 entry translated text 上限为 8 MiB，单 patch entry count 上限为 100,000。
  TranslationPatch Store 将这个 canonical JSON 以 gzip 容器保存为磁盘 authority；压缩
  只改变存储表示，不改变 patch ID、entry identity、renderer validation 或恢复语义。
- compact JSON patch 本身不负责文件所有权；原子 revisioned 文件与索引由下述
  TranslationPatch Store 管理。render-cache quota 和 streaming export 仍属于后续 Phase 4。

## PDF v3 Region TranslationPatch

生产 PDF v3 页级译文 authority 是 `RegionTranslationPatch` schema `2`。translation unit 是
完整视觉段落，render unit 是完整 flow container；不得按字符数把段落译文拆回 source
text-show。

约定：

- patch 必须绑定 page/source hash、目标语言、positive revision、provider/model identity 和
  exact `rosetta-pdf-v3-region-translation-renderer/2`。
- container 必须保存 flow-container group ID、ordered source atom identity/hash、ordered
  paragraph results、protected-span placement 和唯一 renderer decision。普通 source text、
  shaped glyph、layout line 和 PDF object delta 不得进入 durable patch。
- renderer decision 只允许 `pending`、带 finite fit scale/line count 的 `reflowed`，或带稳定
  reason code 的 `preserved`。只有所有 container resolved 的 patch 可以写入 store。
- cache/restart/export replay 必须重新验证 PageGraph、source provenance、布局和 stored decision；
  任何 line count、fit scale、preservation 或 patch identity 漂移必须 fail closed。
- 一个 unsafe container 必须整体保留。不得只清除其中部分 source show，也不得用不透明矩形
  遮盖原文。

## PDF v3 TranslationPatch Store

PDF v3 patch store 是 source document + target language 隔离的页级译文磁盘权威。它不
复用 v1/v2 PDF page state，也不保存完整译文页 PDF。

约定：

- target-language 目录 identity 必须同时包含 patch-store schema version 和 exact language
  identity 的 SHA-256，当前格式为 `language-v3-<sha256>`。schema 变化不得复用旧目录；beta
  旧 `language-<sha256>` store 不迁移、不读取，也不允许用户输入直接成为相对路径。store root
  必须是 native orchestrator 提供的绝对路径。
- 根 `manifest.json` 只保存 schema、source fingerprint、exact target language、固定
  `pagesPerShard = 64` 和 deterministic manifest ID，不随每一页提交增长。
- 页索引使用 `shard-XXXXXXXX.json`。每个 shard 最多覆盖 64 个连续页号，并保存独立
  generation、deterministic shard ID 和 source-ordered page entries。
- 64 页是内部 index bound，不是 PageSet、translation batch、scheduler window 或 UI chunk。
  任意页仍可独立读取、提交和重试。
- page entry 必须保存 page/source hash、positive translation revision、patch ID、immutable
  gzip patch filename 和压缩后 byte count。filename 必须由 page/revision/patch ID 确定，不能
  接受 manifest 中的任意相对路径；读取时必须先限制压缩输入和解压输出，再验证 canonical
  patch identity。
- patch file 先通过 unique temp + `sync_all` + rename 提交；owning shard 再通过
  temp + backup + rename 原子替换。新 shard durable 后才能删除被替换的旧 revision。
- 同 revision + 同 patch ID 是幂等写；同 revision + 不同内容是 conflict；低 revision
  必须拒绝。source page hash 在同一 store/page 内变化也必须拒绝。
- gzip body 使用 envelope schema `1`，payload kind 必须显式为 `text-show` 或 `region`。
  production commit/load 使用 region typed API；kind 不匹配必须拒绝，不能交叉解码。
- commit、load 和 repair 都必须拒绝含 `pending` entry/container 的 patch。store 不提供同
  revision pending-to-resolved 更新，也不保存第二套 draft authority。
- 每个进程/store 首次访问执行完整 repair。canonical/temp/backup shard 选择最高有效
  generation，generation 相同优先 canonical。普通后续提交只读取 owning shard，不得
  重新读取所有历史 page patch。
- missing/corrupt patch 只移除对应 page entry；其他页必须保持可读。结构损坏或 filename /
  internal shard index 不一致的 shard 作为可重建派生状态丢弃。
- repair 必须删除 incomplete patch temp、unreferenced patch、superseded revision 和失效
  index sidecar。正常提交也应立即删除本页已替换 revision，避免长任务磁盘持续增长。
- 同一 absolute language directory 的进程内写入由共享 coordinator 串行化。当前不允许
  多 Rosetta 进程并发拥有同一个 job store。

## PDF v3 Render Cache

PDF v3 render cache 保存按需生成的 preview PNG 或 complete translated page PDF。它是
可删除派生数据，不是翻译权威；source PDF + 当前 `TranslationPatch` 必须足以重建全部
cache miss。它不复用 v1/v2 PDF page cache。

约定：

- cache 位于 native orchestrator 提供的 absolute `render-cache/v1` 目录。root 下只允许
  canonical manifest/index 和 SHA-256 addressed artifact 使用固定文件名；source、language、
  patch 或 renderer 字符串不得直接成为路径组件。
- key 必须绑定 source fingerprint、1-based page number、patch ID、positive translation
  revision、renderer version、output kind，以及 preview 的 pixel width / fixed-point scale。
  任一维度变化都必须 cache miss。
- output kind 当前只允许 `previewPng` 与 `translatedPagePdf`。preview 必须至少指定 width
  或 scale；完整 page PDF 不接受 raster size 参数。`translatedPagePdf` 必须只包含目标页，
  删除其他 page tree entries、document-level navigation 和不可达对象后重新编号、压缩并
  验证为 exactly one page；不得把完整源文档复制到每个 page artifact。
- default artifact quota 为 384 MiB，default entry limit 为 4,096；创建 cache 时可以缩小或
  调整。单 artifact 大于完整 quota 必须在写盘前拒绝。实现绝对上限为 16 GiB / 16,384
  entries，避免异常配置解除 metadata bound。
- artifact payload 总和必须始终不超过 quota。manifest 与固定 64 个最多 1 MiB 的 hash
  index shard 是额外但有界的 metadata overhead。64 shard 不是页范围、PageSet、scheduler
  window 或 UI chunk。
- entry 保存 canonical key、key SHA-256、artifact filename/bytes/content SHA-256 和 logical
  last access。LRU 只读取 bounded metadata，不得为淘汰加载 PNG/PDF body。
- artifact 通过 unique temp + `sync_all` + rename 写入；index shard 通过 temp + backup +
  rename 原子替换。content-addressed artifact 不得原地覆盖。
- open 必须返回 active lease。leased key 不得替换或淘汰；消费 bytes 时必须验证 exact
  length、content SHA-256 和 output signature。失败只删除该 entry。
- cache bridge 只接受 resolved patch，并从 exact source fingerprint、resolved patch ID /
  revision 和当前 renderer contract version 构造 `translatedPagePdf` key。checksum/signature
  corruption 在 bridge 层表现为 miss，调用方从 source + resolved patch 重建。
- page artifact 在 render 输入时绑定 source fingerprint；cache insert 只能使用 artifact 自带
  identity，不得再次接受一个可能不一致的 source fingerprint。
- `previewPng` bridge 只接受已 resolved 的 current-renderer patch 与已验证的 exactly-one-page
  artifact。当前 width contract 为 200..=1,800 pixels；请求不得静默 clamp，因为 cache key
  中的 width 必须等于实际 raster width。
- preview key 的 renderer identity 必须同时绑定 translation patch renderer 与 preview
  rasterizer contract。当前组合为
  `rosetta-pdf-v3-translation-patch-renderer/1+rosetta-pdf-v3-preview-rasterizer/1`；PDFium
  render config、PNG encoding 或 bundled raster engine 发生影响输出的变化时必须升级 preview
  contract，不能复用旧 PNG。
- preview artifact 在 render 时封装完整 cache key，insert 不接受第二套 source / patch / width
  identity。PNG 必须验证 signature、非零高度与 exact requested width；checksum/signature
  corruption 在 bridge 层表现为 miss。
- renderer 返回 resolved patch/page bytes 与 cache insert 是两个独立步骤。cache quota、I/O
  或 lease failure 不得丢弃 resolved patch，也不得阻止 patch store 成为 durable authority。
- 同一 absolute cache directory 的进程内操作由共享 coordinator 串行化，并要求一致
  config。当前不允许多 Rosetta 进程同时写同一个 cache。
- 首次访问 repair 只读取 index 和 artifact metadata，不读取 artifact body。missing artifact
  只删除自己的 entry，invalid shard 只丢弃该 shard；orphan artifact、artifact temp 和 index
  sidecar 必须清理。quota/config 缩小时按 persisted LRU 淘汰到两个上限以内。
- explicit repair 在 active lease 存在时必须拒绝。删除整个 render cache 不得影响 patch
  store、source PDF 或已导出的文件。

## PDF v3 Legacy TranslationPatch Renderer

`TranslationPatch` renderer 是 durable page translation 与低层 content-stream renderer
之间的唯一桥接层。它接收 unchanged source document、reconciled PageGraph、全 pending
或全 resolved patch、prepared unified font faces 和显式 fit policy；mixed lifecycle state
必须在 document mutation 前拒绝。

约定：

- 一个 entry 当前必须完整覆盖一个 source text object，并由 PageGraph v6 provenance 解析到
  唯一 stream、Form invocation path、operation、operator、operand hash 和 `BT/ET` target。
  不完整、跨 object 或 provenance 不足的 entry 使用稳定 reason code 保留原文。
- entry 先按 physical stream/path 和 logical `BT/ET` target 聚合。全部 target 必须针对同一
  unchanged source document 完成 identity、style、font、anchor、geometry、fit 和 encoding
  preflight；preflight 不得修改 document。
- renderer 必须先为 patch 中每个 entry 生成 fitted/preserved decision 并计算 resolved
  `patchId`，再调用一次现有 page-level atomic batch。batch 失败时不得返回可持久化 patch，
  也不得改变 document objects 或 `max_id`。
- 首次 render 接受全 pending patch 并产生 resolved identity。cache miss/restart 重建接受全
  resolved patch，但必须重新执行相同 preflight，逐 entry 精确匹配 stored decision；任何
  fit strategy、scale 或 preservation reason 漂移都 typed 拒绝且零修改。
- 当前 renderer contract version 为
  `rosetta-pdf-v3-translation-patch-renderer/1`。render 与 cache address 都必须验证 patch
  版本完全一致；旧版本 patch 不允许由新实现静默重放或进入当前 cache namespace。
- source operator、operand hash、operation 或 paint/style identity 变化属于 stale source，
  是整次 render 的 typed fatal error。无法验证的 text-object boundary、anchor、fit、font face
  或支持范围属于 entry/group preservation，不得让安全 entry 一并退回原文。
- 默认 readability floor 为 `0.9`。低于 floor 的译文保留原文并记录
  `translation-overflow`；不得无限水平压缩。
- render 成功返回 resolved patch 与可选 batch diagnostic。全 preserved 页不修改 PDF，
  batch 可以为空，但 resolved patch 仍是该 revision 的完整 durable authority。
- page PDF serializer 消费一个显式 working document ownership，完成 replacement 后只保留
  selected page 并 prune/renumber/compress。API 不得为了方便而隐式 clone 整个长 PDF；
  future scheduler/working-document strategy 可以替换输入来源而不改变 artifact/cache contract。
- 完整文档导出必须先根据全部 resolved patches 收集每个 face 的完整字符集，再原子创建
  一个 `DocumentTranslationFontRegistry`。registry 按 weight 确定性排序，每个实际使用的
  face 只提交一套 6-object Type0 subset。
- registry binding 必须同时匹配 weight、asset ID、source font fingerprint 和 deterministic
  subset name，并验证 Type0 object 仍存在且 `/BaseFont` 身份一致。不同 subset 不得复用
  相同 resource binding。
- registry-aware page render 只能把已有 Type0 object ID 挂到 page/Form effective resources，
  `stagedFontObjectCount` 必须为 0。单页 cache artifact 路径继续独立 staging，以保持既有
  page-PDF byte contract；两种路径不得隐式共享可变 registry。
- registry 只解决 document-wide font reuse 和输出体积。font allocation、registry binding、
  selected page dictionary、inherited resources 与 target content-stream reads 已接收 lazy
  `PdfObjectView`。Form invocation validation、effective resource materialization 与 COW clone
  staging 也已消费 lazy source/accumulated views。跨页 stream/Form ownership 由可复用的
  `PdfStreamOwnershipIndex` 从同一个 lazy source view 解析；production registry render 不得
  接收完整 `lopdf::Document`。

## PDF v3 Region Translation Renderer

生产 renderer contract 是 `rosetta-pdf-v3-region-translation-renderer/2`。它消费 resolved
region patch，以 flow container 为原子边界清除 owned source text shows，并使用统一
Regular/Bold 字体一次绘制完整 translated layout。

约定：

- pending render 可以生成 container decisions；durable resolved replay 只能复现相同 decision
  和 patch identity，不得重新解释后静默改写。
- source neutralization、Form/top-level stream COW、page resource/contents 更新、opacity state、
  font objects 和 translated overlay 必须合并为一个 object delta。任何 ownership 或 provenance
  不完整都整体 preserve 或 typed fail。
- preview 单页 PDF/cache key 必须绑定 source fingerprint、page、region patch ID/revision 和
  region renderer version；旧 text-show artifact 不得命中。
- 文档导出先收集全部 reflowed container 字符，创建一份 document-wide font registry，再逐页
  replay region delta。每个实际使用的 Regular/Bold subset 只允许一套 6-object Type0 资源，
  禁止按页重复嵌入字体。
- 全 preserved patch 不需要 prepared font，也不得产生空字体对象；最终导出可退化为 byte-exact
  verified source copy。

## PDF v3 Incremental Export Delta

PDF v3 最终导出的写出边界由 immutable source PDF、`IncrementalExportBase` 和本次
delta objects 组成。它是一次 export session 的进程内模型，不是新的持久化翻译权威；
source PDF + resolved `TranslationPatch` 仍是可重建依据。

约定：

- `IncrementalExportBase` 只保存 source SHA-256、source byte count、latest xref offset、
  maximum object number 和 trailer dictionary。writer 不得要求 source `Vec<u8>` 或完整
  previous object graph。
- delta 只包含本次新增或覆盖的 indirect objects，以 exact object/generation 排序；object 0、
  generation 65535、同一 object number 的多个 generation 和空 delta 必须拒绝。
- writer 以固定 64 KiB buffer 从原始 source PDF 流式复制，同时重新计算 byte count 和
  SHA-256。任一 identity 不匹配时不得替换已有目标文件。
- incremental xref 的 `/Prev` 必须指向 source latest xref；trailer 保留 `/Root`、`/Info`、
  `/ID` 等 source identity，并移除只属于 xref stream 的字段。当前 writer 使用 classic xref
  delta，并显式拒绝超过 `u32` offset 的文件。
- stream object 写出时必须根据实际 serialized content 重算 direct `/Length`。对象值必须走
  结构化 PDF serializer，不能用字符串搜索或替换生成 indirect object。
- 文件提交必须在 destination 同目录创建唯一 temp，完成 `flush` 和 `sync_all` 后再原子
  replace。Windows 使用 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`；失败或取消必须
  删除 temp 并保留旧 destination。
- cancellation 至少在 source copy chunk、delta object 和 commit 前检查。commit 成功后结果
  才可报告完成。
- export 必须始终从 immutable job source 生成，不能把旧译文 export 当作新 source 继续追加，
  以免 revision 次数导致文件持续膨胀。
- 当前 font registry 与 page staging 已从 accumulated source/overlay view 读取
  maximum/object identity；selected page number、page object ID 与 top-level `/Contents`
  identity 由 immutable source 上的 `PdfPageIndex` 提供。page dictionary、materialized
  inherited resources、Form resource path 与 target content streams 由独立 immutable source
  view 提供；COW clone ID 从 accumulated view maximum 之后分配。跨页 ownership index 也只
  消费 immutable source view；production final export 不得创建或维护 legacy traversal
  `Document`。

`PdfObjectDelta` 是 renderer 与 incremental writer 之间唯一允许的 indirect-object 变更集合。

- delta 保存按 object/generation 排序的 `lopdf::Object` map 和本次分配后的 maximum object
  number，不保存 source bytes、PageGraph、TranslationPatch 或用户可见文本副本。
- font registry staging 接收 immutable `&dyn PdfObjectView` 并返回 delta；兼容 mutation API
  只允许在 staging 成功后 apply。TranslationPatch production page staging 接收四个职责分离的
  read contracts：immutable `&dyn PdfObjectView` source objects、独立 `&dyn PdfObjectView`
  accumulated identity、selected `PdfPageIndex` 与 reusable `PdfStreamOwnershipIndex`。source
  view 负责 selected page dictionary、inherited/Form resources、invocation validation 与 source
  streams；accumulated view 必须包含本次 export 已合并的 font/page delta，并负责 registry
  lookup 与新对象号预留。
  预检、fit、copy-on-write 和 object allocation 全部成功前不得修改 source document。
- 多页 export accumulator 先合并 font delta，再按页合并 page delta。同一 object ID 的完全
  相同值可幂等合并；不同值、同一 object number 的多个 generation、无效 ID 或低于实际
  object ID 的 maximum 必须在 accumulator mutation 前拒绝。
- 旧单页 render API 可以在 staging 成功后显式把 delta apply 到 owned working document，
  以维持 page cache byte contract；final export writer 必须直接消费 `PdfObjectDelta`，不能再
  比较 source/working complete object graph 推导变化。
- delta 是 export session 的瞬时敏感内存，不写入 patch store 或普通诊断。结果只报告 object
  count、maximum object number 和已有 renderer 统计。
- 当前多页 proof 直接基于 lazy source 分配 document font objects，并复用同一个 selected
  `PdfPageIndex`。每页通过 immutable source view 读取 page dictionary、materialized inherited
  resources、Form invocation tree 与 target streams，通过 `source + accumulated delta` overlay
  读取 registry identity 与 allocation maximum；同一个 `PdfStreamOwnershipIndex` 在各页间复用。
  font/page delta 不 apply 到完整 document，production renderer 已无完整 document ownership。
- 同一 export revision 的每个 page 只能 staging 一次。accumulated overlay 当前只为 page
  renderer 提供 maximum 与 registry object identity，不让同一页的后续 staging 隐式读取前次
  page dictionary/content delta；重复 page/object mutation 必须由 delta merge conflict 拒绝。

PDF v3 source object working set 使用独立的 transient `PdfSourceObjectStore`，不进入 durable
job schema。

- source PDF 通过 read-only memory map 打开；xref table、xref stream、object stream 和 trailer
  由窄化的 `pdf-rs` reader 解析，不复制完整 source bytes 到 Rust heap。
- reader 只按 exact object/generation 解析请求对象，并立即转换为 renderer 内部使用的
  `lopdf::Object`。`pdf-rs` primitive 不允许越过 source-object module 边界。
- 默认 object LRU 上限为 16 MiB / 512 项，单对象最多缓存 4 MiB。超过单对象上限的 stream
  可以作为当前操作的 transient owned object 返回，但不得进入常驻 cache。
- `PdfObjectOverlay` 先读取显式 `PdfObjectDelta`，未命中时才读取 immutable source store；
  maximum object number 取 source/delta 最大值。overlay 不修改 mmap、source file 或 delta。
- cache diagnostics 只允许 source load/hit、resident entry/byte count，不记录 object value、
  stream bytes、原文或译文。
- source store 保存原始 trailer、latest xref offset、page count 和 maximum object number，供
  incremental export base 使用。最终 writer 在 commit 前仍须重新核对 source length 与
  SHA-256，memory map 不是 source identity authority。
- extraction/mapping `DocumentHandle` 只组合该 bounded source store、PDFium document 与
  source identity/page count；不得重新持有完整 `lopdf::Document` 或无条件 all-page object
  ID vector。每页 mapping 通过 transient one-page index/context 读取资源、内容流、字体与
  Form，并在页结束时释放 parsed content cache。
- `PdfObjectView` 也由 `lopdf::Document` 实现，供迁移期 compatibility wrapper 使用；它不应
  重新成为新 renderer API 的 concrete source contract。
- transient `PdfPageIndex` 从 trailer `/Root` 与 catalog `/Pages` 开始，只为明确 `PageSet`
  解析 1-based page number、page object ID、ancestor `/Pages` IDs 与直接 `/Contents`
  references。它使用可信页树节点的 `/Count` 跳过不相交子树，并在最大 selected page 后停止；
  不得枚举或缓存所有页作为默认行为。
- page index 必须在 encountered selected paths 上拒绝错误 root/catalog、非 dictionary node、
  非法 `/Type`/`Count`/`Kids`、non-reference kid、cycle、repeated ownership、超过 64 层、
  页码溢出、缺失 selected page 与非 reference `/Contents`。索引是 export session 的 transient
  导航数据，不进入 durable job schema，也不包含 page content bytes。
- compatibility renderer API 可以从 owned `Document` 构造单页 index；multi-page export 必须
  从 immutable lazy source 为整个明确 `PageSet` 构造一次 index，并在各页 staging 间复用。
  index、PageGraph、temporary traversal document 与 accumulated overlay 必须属于同一个 immutable
  source identity，不允许把不同 PDF 的 object IDs 组合成一次 staging。
- transient `PdfStreamOwnershipIndex` 从 trailer/catalog page tree 流式扫描所有 page dictionary，
  但只为 caller 提供的 exact stream target set 保存 `unreferenced`、`uniqueToPage` 或
  `sharedAcrossPages` 三态。它不得保存 page dictionary、content bytes 或每个 target 的完整 page
  number set；状态内存随 target 数量而不是 page/reference 数量增长。
- ordinary target ownership 只检查直接 `/Contents`，不得加载或解压其他 page content stream。
  只有 target stream 自身声明 `/Subtype /Form` 时，才保守扫描 page effective XObjects 与 nested
  Form-local XObjects。page tree 最多 64 层，Form resource graph 最多 32 层且每页最多 4,096 次
  Form visit；direct Form、cycle、错误 count/type/kids/resources 必须返回 typed failure。
- multi-page export 必须在 page staging 前用 selected content roots 构造一次 ownership index 并
  在全部 page 间复用；不得按 page 重复全页树扫描。legacy operand-patch compatibility API 的
  `Document` ownership helpers 不属于 production TranslationPatch renderer。
- transient `PdfPageObjectContext` 只从 immutable source view 与一个 `PdfIndexedPage` 解析 exact
  page dictionary 和 effective inherited `/Resources`。它从 page 向 ancestor chain 查找直接或
  indirect resources，最远 ancestor 先合并、最近 scope 后覆盖，并把 resource category 中的
  indirect dictionary materialize 成 owned dictionary。reference chain 最多 64 层；cycle、错误
  page type、非 dictionary resources 与冲突 category 都必须返回 typed failure。
- `PdfPageObjectContext` 是一次 page staging 的 owned snapshot，不进入 durable job schema，也不
  持有 source store borrow。non-COW page font binding 必须在该 snapshot 上 clone page/resources，
  不能重新调用 `Document` inherited-resource helper。
- owned `PdfResourceContext` 从 page context 开始，按 invocation path 为每个 Form 把自身
  `/Resources` 叠加到 parent effective resources。Form scope 优先，缺失 category/name 回退到
  parent；direct/indirect resource dictionary、category dictionary 与 XObject stream 都通过
  source view 解析。reference chain 最多 64 层，cycle、错误 resource shape 与不可解析 XObject
  必须在 clone allocation 前 typed 拒绝。
- production Form COW staging 只接收 immutable source view、accumulated view、`PdfIndexedPage`
  与 `PdfPageObjectContext`。root `/Contents` identity 来自 page index，source Form/root stream
  来自 source view，page clone 来自 page context，clone IDs 从 accumulated maximum 后连续分配。
  `Document` compatibility wrapper 只能即时构造相同 index/context 后调用该窄接口。
- production font registry staging、binding 与 page clone allocation 已消费
  `PdfObjectView`，selected page identity 已消费 `PdfPageIndex`，selected page resource/content
  reads 与 Form COW traversal 已消费 `PdfPageObjectContext`/source view。新 Type0 object 可完全
  从 delta overlay 校验，Form/page clone ID 从 accumulated maximum 之后分配。cross-page
  ownership 也已消费 bounded lazy index；renderer staging working set 已由 source object LRU、
  explicit targets、selected page context、active Form paths 与 accumulated delta 共同界定。

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
- production clone-tree validation/staging 必须从 immutable `PdfObjectView` 读取 root/Form
  streams 和 effective resources，从 accumulated view maximum 之后分配 clone ID，并返回 staged
  objects/page dictionary；不得重新进入完整 `Document` 读取 invocation path。兼容 mutation API
  只允许在完整 staging 成功后 apply。
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

## PDF v3 PageGraph Artifact Store

PDF v3 `PageGraph` durable authority 独立于 scheduler metadata、TranslationPatch
Store 和 Render Cache。beta 阶段不迁移旧 PDF extraction artifacts。

约定：

- store manifest 必须绑定 source fingerprint、source page count、native engine
  version、PageGraph schema version 和固定 64-page shard width；任一 identity 不符
  不得打开或复用该 store。
- 每页只有一个 immutable `*.pagegraph.json.gz` artifact。gzip 必须使用固定 mtime=0
  和受控 compression level；artifact ID 是完整 compressed bytes 的 SHA-256，filename
  必须包含 exact 1-based page number 和 digest。
- JSON 必须通过有 64 MiB 上限的 writer 直接流入 gzip，不能先保留 document-wide
  PageGraph/JSON buffer。compressed artifact 也必须独立限制为 64 MiB；解压读取必须使用
  `take(limit + 1)` 防止 compression bomb。
- durable load 必须验证 compressed digest、gzip integrity、decompressed size、PageGraph
  schema/page、由 source fingerprint 推导的 source-page hash、reconciled status、atom/style
  identity 和 shard metadata。任一失败的 artifact 不得进入 recovery inventory。
- artifact file 是 extraction content authority；64-page shard 只是 bounded rebuildable
  index。repair 必须逐个读取 artifact 并重建 shard，不得同时保留多个 PageGraph。
- worker construction 必须验证 scheduler binding、DocumentHandle 和 PageGraphStore 的
  source/page count/engine/schema identity，并为 scheduler exact PageSet 构造一次 reusable
  mapping index。
- sequential worker 每次只 claim 一个 extraction lease；只有 PageGraph artifact 原子提交
  成功后才能 commit scheduler extraction authority，然后才可 claim 下一页。artifact 已提交但
  scheduler commit 中断时，stale-owner recovery 必须从 validated inventory 提升该页。
- deterministic reconciliation failure 进入 non-retryable extraction failure；store/I/O
  failure 进入 retryable failure。reason code 不得包含 source text、translated text 或路径。

## PDF v3 Translation Worker and Recovery Inventory

PDF v3 translation orchestration connects independently durable PageGraph,
TranslationPatch and scheduler state. It remains isolated from legacy PDF page
artifacts and ordinary Rosetta text segments.

约定：

- scheduler translation binding 必须固定 source fingerprint/page count、exact
  PageSet、source/target language、engine、PageGraph/TranslationPatch schema 和
  renderer version。worker 在 claim 前必须验证两个 store 与该 binding 一致。
- sequential worker 每次只能 claim 一个 translation lease，并只保留该页一个
  PageGraph。provider 跨页 batching 不得改变 page ownership 或让已完成 PageGraph
  常驻内存。
- page processor 只返回 fully resolved `TranslationPatch`、带稳定 reason code 的
  explicit preservation，或带明确 retryability 的稳定失败。pending renderer draft
  不得越过 worker/store 边界。
- worker 必须逐项验证 claim extraction authority、PageGraph artifact authority、
  patch source/page/atom identity、target language、schema 和 renderer version。
- resolved patch 必须先 durable commit 到 TranslationPatch Store，之后才能提交
  scheduler completed authority。两次提交之间崩溃时 recovery inventory 必须提升
  已验证 patch，不能重复翻译。
- preserved 页面不创建空白译文 PDF 或伪 patch；scheduler 持久化 extraction authority
  与稳定 preservation reason，导出时保留源页面。
- PageGraph authority 不可用和瞬时 patch-store I/O/lock failure 是 retryable；invalid
  patch 与 revision/content conflict 是 non-retryable。失败 reason 不得包含 source text、
  translated text、provider response 或本地路径。
- complete recovery inventory 必须先验证 scheduler/store binding，再逐页验证
  PageGraph artifact 与对应 patch。只能纳入 scheduler exact PageSet，且不得同时保留
  多页 PageGraph/patch body。
- missing/corrupt patch 是 page-local invalid authority，可以 repair/drop；真实 filesystem
  I/O failure 必须上抛，不能作为坏 patch 静默删除。

## PDF v3 Durable Scheduler

PDF v3 长文档调度状态独立于 legacy `PdfTranslationRun`、TranslationPatch Store 和
Render Cache。beta 阶段不迁移 v1/v2 PDF 派生状态。

约定：

- 一个 run directory 只包含一个 versioned `manifest.json` 和按 page number 分配的
  `shard-XXXXXXXX.json`。每个 shard 最多 64 个 requested page record；64 只是磁盘
  metadata bound，不是 batch、queue window、PageSet 或 UI chunk。
- manifest 固定绑定 run/source fingerprint、source page count、canonical requested
  PageSet、source/target language、engine、PageGraph schema、TranslationPatch schema 和
  renderer version。容量、typed run/cancellation state、owner session lease、claim cursor
  和 rebuildable summary 也保存在 manifest。
- page shard 是状态权威。`open` 必须按 requested PageSet 推导全部 expected shard，验证
  page coverage 恰好相等且无重复，再流式重建 summary 和 completed state。不得信任可能
  落后于 shard commit 的 manifest summary。
- durable page state 只能是 `pending`、带 exact extraction artifact/source-page identity
  的 `extracted`、再带 exact patch ID/revision 的 `completed`、带 stable reason code 的
  `preserved`，或带 stage/retryability/resume authority 的 `failed`。
- rendered PNG/page PDF 不得作为完成权威。`completed` 必须引用已经由 TranslationPatch
  Store 原子提交并重新验证的 patch authority；`extracted` 必须引用已经由 PageGraph/IR
  store 验证的 extraction authority。
- page lease 必须保存 unique lease ID、owner session、`extraction` / `translation` stage
  和 lease timestamp。只有 pending 可以 claim extraction，只有 extracted 可以 claim
  translation；commit/fail 必须同时匹配 page、owner、lease ID 和 stage。
- owner heartbeat 是 run-level lease。不同 session 只有在旧 owner 明确 stale 后才能接管。
  stale-owner recovery 必须接收完整、已验证的 extraction/patch inventory：可以提升在
  scheduler update 前已经落盘的 authority；completed patch 无效时退回有效 extracted，
  extraction 也无效时退回 pending。不得仅凭旧 scheduler state 猜测 artifact 有效。
- backpressure 分别限制 extracting、extracted waiting 和 translating page count。claim
  结果不得超过调用者 limit 或剩余容量；不得把固定 10 页写入持久化或公开调度语义。
- pause/cancelling 阻止新 claim，但允许已租赁工作显式 commit/fail。取消只有在 active
  page lease 清零后才能进入 `cancelled`。failed page 只有 `retryable=true` 才能恢复。
- run state 包含 `running`、`paused`、`cancelling`、`cancelled`、`failed` 和 `completed`。
  当全部 requested pages 都收敛到 completed/preserved/failed 且至少一页 failed 时，run 必须
  原子收敛到 `failed`；worker、owner heartbeat 和前端 active polling 必须停止。打开旧的全失败
  `running` manifest 时必须从 shard summary 自动归一化。重试一个 retryable failed page 会把
  run 重新切回 `running`；non-retryable page、pause/resume/cancel 和 export 不得把 failed run
  伪装成成功。跨 session 的 failed run 可以在 owner lease 过期后接管，但接管本身不启动 worker。
- status API 默认使用 page-number ordered window，单次最多 256 records；不得默认返回
  整个长文档页状态数组。
- manifest 和 shard 使用 unique temp、file `sync_all`、backup/rename 替换和支持平台上的
  parent-directory sync。读取必须验证 canonical/temp/backup 候选，提升最高有效 generation
  并清理 sidecar。首次创建必须在 unique sibling staging directory 完整写入所有 shard 和
  manifest，再通过一次 directory rename 暴露 canonical run。一个进程内同 run handles
  必须共享 coordinator lock。

## PDF v3 Typed Run Control Plane

PDF v3 run control 是 durable scheduler 和 Tauri/UI 之间的窄接口，不是第二份任务状态。

约定：

- 公开命令只接受 `jobId`、safe `runId` 和分页参数。job directory 必须从 Tauri app data
  内部解析，前端不得传入任意 filesystem path 或 owner session ID。
- run owner 使用 native process session identity。状态只返回当前 session 是否为 owner，
  不返回可被前端重放的 owner session ID。page lease 同样只投影 stage、timestamp 和
  `ownedByCurrentSession`，不得返回内部 lease ID 或 owner session ID。
- 状态固定返回 exact canonical PageSet、run/cancellation state、rebuildable summary、
  immutable runtime/component/provider/model/font identity 和 page-number ordered records。
- page records 默认最多 64 条，调用者可请求的硬上限为 256 条；`nextStartAfter` 和
  `hasMore` 是唯一分页游标语义。不得为长文档返回默认全量页数组。
- 状态不得返回原文、译文、provider raw response、endpoint、credential 或 font path。
- pause/resume/cancel 必须由 scheduler owner gate 执行。取消先进入 `cancelling`，只有
  extraction/translation active leases 都清零后才能进入 `cancelled`。cancel command
  必须幂等，允许 active lease settle 后重试并完成状态收敛。
- exact failed-page retry 公开输入只能增加 `pageNumber`，owner/timestamp/runtime identity
  仍由 native 提供。只允许当前 owner 在 `running` 或 `paused` 中恢复
  `retryable=true` 的 requested page；extraction failure 回到 `pending`，保留有效
  extraction authority 的 translation failure 回到 `extracted`。不得清零 attempt 或删除
  durable artifact authority。
- retry status 复用 schema 4，并从被重试页开始返回默认最多 64 条 ordered records；不得为
  单页重试返回完整长文档状态。inactive worker 必须先重新解析并精确验证 source、runtime
  manifest、live component/provider/model 和统一字体 binding，之后才能改变页状态和幂等注册
  一个 supervisor。普通 status polling 不得触发该路径。
- runtime status 必须先验证 `runtime-manifest.json` 与 scheduler translation binding；
  identity drift 或缺失 manifest 是硬错误，不能降级成未知字符串或继续运行。
- status schema `rosetta-pdf-v3-run-control-status/4` 必须返回
  `ownerRecoveryEligibleAtMs` 和 bounded `ownerHeartbeat` 健康投影。heartbeat 只能包含
  `active`、`intervalMs`、`lastSuccessAtMs` 和 `consecutiveFailures`，不得包含 session ID、
  run path 或 raw error。当前 native takeover 下限固定为 owner lease 最后更新时间后
  5 分钟，前端不得传入或缩短 stale cutoff。
- native lifecycle 每个 active run 最多持有一个 10 秒周期 heartbeat；scheduler 必须在
  coordinator lock 内同时验证 owner 和 nonterminal state。terminal、owner mismatch 和 app
  exit 必须卸载 heartbeat。heartbeat 不得自动接管其他 owner，接管只能走 validated stale
  recovery；前端轮询不得成为 owner lease authority。
- status 还必须返回 bounded `worker` 健康投影，只允许 `active`、当前 `stage`、
  `lastProgressAtMs` 和 `consecutiveFailures`。不得返回 run/source path、owner ID、provider
  endpoint、raw error、原文或译文。worker inactive 时不得仅因 status polling 重新挂接
  heartbeat。
- 工作台只通过 bounded run list 为当前 target language 选择最新 revision；该选择是可重建的
  frontend projection，不持久化第二份 current-run ID，也不覆盖 scheduler authority。没有 v3 run
  时才允许回退到 legacy PDF 页状态和译文预览。
- 工作台页状态按当前虚拟可见页对齐到 64-record window 获取。frontend 最多保留 4 个 window，
  稀疏 PageSet 造成重叠时按最近一次 fetch 合并；run 进入 terminal state 后，每个旧 window 必须
  至少刷新一次 terminal projection。不得为了页码滚动预取完整 PageSet 状态。
- recovery 必须在 scheduler coordinator lock 内拒绝当前 session 仍有 active page lease
  的自接管，并拒绝未过期的其他 session owner。过期后只能用 validated
  PageGraph/TranslationPatch inventory 调用 scheduler
  recovery；patch revision 必须等于当前 runtime manifest revision。不得信任旧 shard
  authority，也不得恢复 pending patch、prepared font 或 PDF delta。
- recovery inventory 校验可能遍历完整 PageSet，Tauri 命令必须在 blocking worker 中执行。
  source identity 与 live component/runtime/font binding 也必须在改变 owner 前完成精确验证。
  接管释放旧 lease 后，如果 run 已处于 `cancelling` 且 active lease 清零，必须立即收敛到
  `cancelled`。

## PDF v3 Native Worker Supervisor

PDF v3 native worker supervisor 是 process-local execution owner，不是 durable page state、
frontend timer 或第二份 scheduler。

约定：

- 每个 canonical run directory 最多注册一个 supervisor。它内部可以分别持有 extraction 与
  translation blocking loop，但 registry identity、stop/cancel signal、health 和 completion
  wait 必须以 run 为单位。
- supervisor 只能接收由 run creation 或 stale recovery blocking verifier 产生的
  `VerifiedDocumentIdentity`，并精确匹配 scheduler binding、immutable runtime manifest、live
  component/provider/model 和统一字体 bytes。source bytes 在该边界只允许 hash 一次；PDFium
  打开必须消费同一 verified identity，不能再次全文件 hash。任何 drift 必须 fail closed，不能
  先执行 provider I/O 或写 patch。
- extraction loop 可以在同一 blocking thread 内复用一个 `DocumentHandle` 与 mapping index；
  translation loop 可以复用一个 lazy source-object view 与 ownership index。每个 stage 一次只
  处理一个 active page/PageGraph，claim 总量仍由 scheduler `2 / 4 / 1` capacity 控制。
- durable scheduler、PageGraph store、patch store 与 PDFium 操作不得在 async executor thread 上
  直接执行。translation provider future 可以通过当前 Tokio runtime 驱动，但其 page-bounded
  filesystem 前后处理仍留在 blocking worker。
- 同一 process 的 PDFium open/extract/render/drop 必须共享一个 operation lock。extraction 只按
  bounded worker batch 持锁，不能为完整长文档独占 PDFium；translation 不经过该锁。
- `paused` 阻止新 claim 并让 loop 有界休眠；已有 lease 可以完成。`cancelling` 必须立即设置
  level-triggered provider cancel flag，等待当前 extraction/translation lease commit/fail 后由
  supervisor 收敛到 `cancelled`。不得丢弃仍在磁盘 scheduler 中的 lease。
- terminal state、owner mismatch 或 stop signal 必须卸载 supervisor。App exit 与 local-data
  reset 必须先 signal stop/cancel，等待已注册 supervisor 完成，再停止 owner heartbeat 和
  managed translation runtime；reset 只有在这些步骤完成后才能删除 jobs/model directories。
- 删除单个 job 必须先停止并等待该 job `pdf-v3/runs/` 下的全部 supervisor；不得在 worker
  仍持有 source mmap、PDFium handle、scheduler lease 或 provider request 时删除 job tree。
- stale recovery 在改变 owner 前必须重新解析 live trusted component；成功接管 nonterminal run
  后才能注册新 supervisor。frontend polling、pause/resume 和 heartbeat 不得创建第二个 worker。

## PDF v3 Translation Runtime Manifest

PDF v3 translation runtime manifest 是每个 scheduler run 的不可变 runtime/component
身份绑定，不是 provider 连接配置、任务进度或翻译内容 authority。beta 阶段不迁移此前
隔离开发产生的 PDF v3 artifacts。

约定：

- run directory 中固定使用 `runtime-manifest.json`；schema 当前为 `1`，完整 JSON 不得超过
  64 KiB，decoder 必须拒绝 unknown fields 并重算 content-derived manifest ID。
- manifest 必须绑定 scheduler 的 source fingerprint/page count、canonical exact PageSet、
  source/target language、engine、PageGraph/TranslationPatch schema 和 renderer version。
- manifest 还必须固定 positive translation revision、exact renderer fit policy、component
  ID/version/manifest ID/build SHA-256、platform/architecture、provider ID、model ID/model
  SHA-256，以及 Regular/optional Bold font 的 asset ID、weight、face index、byte count 和
  complete-file SHA-256。
- manifest 是 immutable authority：首次写入使用 unique temp + file `sync_all` + rename；
  exact same content 重交是幂等，任何不同 identity 必须 conflict，不能覆盖或自动升级。
- manifest 不得保存 provider endpoint、API token、body password、font path、source text、
  translated text 或 provider raw response。
- live runtime 必须在 provider I/O 前匹配 provider kind、当前 platform/architecture 和 exact
  font descriptors。page processor production config 只能从 validated live binding 构造。
- component manager 负责验证 model/font 安装 artifact、license/component manifest 和实际
  provider process health；provider response 不得声明或改写 model identity。
- manifest 大小与页数无关。它不能保存 prepared font subset、pending patch、PageGraph、
  page object delta 或 export delta。

## PDF v3 Trusted Translation Component Status

PDF v3 translation component status 是 native component resolver 对当前可用 provider、model
与统一译文字体 identity 的隐私安全投影，不是 install manifest、provider 配置或 run authority。

约定：

- resolver 输入只能包含目标语言；component/provider/model/font identity 必须由 native 当前
  platform profile、已安装 manifest、受管 sidecar/font bytes 和 live process state 推导，前端不得
  提供或覆盖。模型完整 bytes 只在 install/update/repair 时校验，正常 App 启动复用安装 manifest
  中的 SHA-256，并仅检查文件类型与 byte count。
- managed runtime 必须为 `Ready`、匹配当前 OS/architecture、install plan 完整且 health probe
  成功。profile、PID 与 loopback base URL 在 probe 前后以及 blocking artifact hash 完成后必须
  完全一致；发生切换必须 fail closed。
- model/runtime install manifest 必须精确匹配 compile-time profile。sidecar 必须 hash 实际文件；
  model identity 使用安装时已经完整校验的 manifest SHA-256，正常启动不得重新读取完整 GGUF。
- 译文统一字体由目标语言选择，只允许受管 Source Han Sans CN Regular/Bold 或 Go Noto
  Kurrent Regular；完整 font bytes 必须匹配固定 SHA-256。缓存后的 immutable bytes 是 live
  renderer binding，不能在 run 内重新从 mutable path 推导 identity。
- component manifest ID 必须由 component version、runtime profile/release、实际 sidecar/model、
  provider/model profile、PDF asset release 与实际 Regular/optional Bold font identity 共同导出。
- status schema 当前为 `rosetta-pdf-v3-component-status/1`，只允许返回 `ready`、verification
  timestamp、component/build/platform、runtime release、provider/model 与 font byte descriptors。
  不得返回 path、base URL、endpoint、PID、token、password、credential、raw error 或文档文本。
- native v3 只复用 legacy PDF pack 的固定 font files 与 release manifest；不得把 Python worker、
  doclayout model 或 legacy process readiness 作为 native component resolver 的依赖。
- trusted run creation 必须直接消费 resolved live binding 并生成 immutable runtime manifest；不得
  接受前端声明的 component/provider/model/font identity。

## PDF v3 Trusted Run Creation

PDF v3 run creation 是 scheduler 与 immutable runtime manifest 的唯一生产创建边界，不是
frontend 可组装的通用 manifest API。

约定：

- 公开输入只允许 `jobId`、optional exact `requestedPageSet`、`targetLanguage` 与 optional
  `preferredPageNumber`。省略 PageSet 表示全部源页；空集合、越界页或 reversed range 必须拒绝。
  preferred page 只允许是 exact PageSet 的成员，并且只在原子创建期间旋转已有 extraction/
  translation cursor；它不得改变 PageSet、capacity、page authority 或恢复规则。前端不得提供
  run ID、source path、fingerprint/page count、source language、revision、capacity、render policy、
  owner 或任何 component/provider/model/font identity。
- source path 必须从 app-data job root 派生，实际 source bytes 必须重新计算 canonical
  `sha256:` fingerprint 并与 `pdf_source.json` authority 一致。page count 来自该 source
  metadata；缺失、zero page 或 identity drift 必须在创建前拒绝。
- source language 必须从持久化 document/file metadata 解析；读取不得加载 blocks、segments 或
  translation history。缺失 source language 时，只能按当前双语 profile 确定性选择 target 的反向
  language，并再次验证 live profile supported direction。
- run ID 与 positive translation revision 只能由 native 分配。当前同进程 creation lock 下扫描
  committed immutable runtime manifests，revision 使用 `max + 1`；已提交 run identity 无效时不得
  跳过后复用 revision。
- scheduler manifest/shards 与 `runtime-manifest.json` 必须先在 hidden sibling directory 中完整
  durable commit，并用 staged scheduler binding 重建/验证 bounded status。只有全部成功后才能通过
  one directory rename 暴露 final run；pre-rename failure 必须清除 staging，partial run 不得可见。
- production engine identity 当前为 `rosetta-pdf-v3-native-engine/1`；PageGraph/Patch/renderer version
  必须使用当前 native constants。默认 render policy 与 independent scheduler capacities 由 native
  固定，前端不得把 capacity 当 batch/chunk 配置。
- 当前默认 capacity 为 extracting `2`、extracted-waiting `4`、translating `1`。它只限制内存与
  backpressure，不限制 PageSet 大小，也不产生用户可见的十页切分。
- final rename 后必须立即注册当前 process-native worker supervisor，并由 active worker 挂接
  owner heartbeat。worker 在执行前仍须重新验证 source/runtime/component binding；注册失败或
  worker inactive 时 status 不得声称 run 正在本机推进。
- 创建结果复用 bounded run-control status，不得返回 source path、document text、endpoint、PID、
  owner/session ID、credential 或 raw storage/component error。

## PDF v3 Bounded Run Enumeration

PDF v3 run list 是 committed scheduler/runtime authority 的只读、分页投影，不是新的持久化
run index，也不是 lifecycle 或 worker 状态权威。

约定：

- 公开输入只允许 `jobId`、optional `targetLanguage`、optional exclusive
  `beforeRevision` 和 optional `limit`。job root 与当前 native session 必须由 Tauri 内部解析；
  前端不得传 path、owner/session ID 或 timestamp。
- list schema 固定为 `rosetta-pdf-v3-run-list/1`，按 positive translation revision 降序。
  默认最多 16 个 run，硬上限 64；`nextBeforeRevision` 只在仍有更旧结果时返回当前页最后一个
  revision，下一页必须使用严格小于该 revision 的语义。
- 扫描期间只能保留 requested top-K 候选，不得让 response working set 随 run history 增长。
  当前目录扫描和 authority validation 仍与历史 run 数量线性相关；未来如引入 durable index，
  必须由 native 原子维护并新增 ADR，不能把前端缓存升级为权威。
- hidden creation staging directory 必须忽略；每个 visible committed run 必须重新打开 scheduler、
  流式重建 summary，并验证 immutable runtime manifest binding。任何 visible run 无效都必须 fail
  closed，不得静默跳过后复用 revision 或给出不完整历史。
- target filter 使用与 trusted creation 相同的 primary-language normalization；返回 item 仍保留
  scheduler 中的 exact source/target language identity。
- item 只允许返回 run ID、revision、state、source page count、exact PageSet、source/target
  language、summary、`ownedByCurrentSession` 和固定 native recovery eligibility timestamp。
  不得返回 page records、source fingerprint、runtime/component identity、owner/lease ID、path、
  endpoint、credential、raw error、原文或译文。
- enumeration 必须是纯观察操作：不得同步 lifecycle、注册/停止 worker、挂接 heartbeat、更新
  owner lease 或执行 stale recovery。选中 run 后的页级状态必须另行读取 bounded run-control
  window。

## PDF v3 Lazy Translated-Page Preview

PDF v3 translated preview 是 durable scheduler、PageGraph 和 region TranslationPatch authority
的按需二进制投影，不是完整译文 PDF、第二份 page state 或导出权威。

约定：

- 公开命令只接受 `jobId`、safe `runId`、positive `pageNumber` 和 bounded
  `targetWidth`。job/source path、source fingerprint、target language、revision、font、
  provider 与 renderer policy 必须从 native authority 推导，前端不得覆盖。
- 只有无 active lease 的 exact `completed` page 可以生成译文预览。scheduler 中的
  extraction artifact/source-page hash 和 patch ID/revision 必须分别匹配当前 PageGraph store、
  TranslationPatch store 与 immutable runtime manifest。pending、extracted、preserved、failed、
  leased、non-requested 或 superseded authority 必须 fail closed。
- 缓存读取顺序固定为 exact-width PNG、single-page translated PDF、lazy source replay。
  PNG hit 可以直接返回 raw IPC bytes，不得 hash source 或加载 font；page-PDF hit 必须验证
  source identity，但不得解析 font；只有 full miss 才允许加载 run manifest 绑定的统一译文字体。
- source identity cache 最多保留 32 个 absolute source entries，以 byte count 和 modified
  nanoseconds 作为失效 stamp。stamp 未变化时可复用已计算 SHA-256；stamp 变化必须重新 hash
  完整 source。stamp 本身不得替代 source fingerprint authority。
- full miss 必须通过 bounded lazy source-object view 只物化 selected page 可达对象及其继承的
  resources/page geometry，不得加载完整 source `lopdf::Document`。单页物化必须拒绝跨页 page-tree
  references，并限制 65,536 reachable objects 与 128 traversal depth。
- preview font resolver 只能读取受管统一 Regular/optional Bold bytes，并与 immutable runtime
  manifest 精确匹配；它不得要求 translation provider/model process 仍健康，也不得读取 endpoint
  或 credential。统一译文字体是 renderer identity，不要求复用原文字体。
- PDFium rasterization 必须在 process-wide operation lock 下执行并返回
  `tauri::ipc::Response` raw PNG bytes。公开 response/error 不得包含 source/translated text、path、
  fingerprint、font path、provider response 或 credential。
- page PDF 与 PNG 都是 render cache 中的 disposable derivatives。插入失败可以 best effort 忽略；
  cache miss/corruption 必须从 durable patch 重建，不能改变 scheduler completed state。preserved page
  没有 patch，UI 必须根据 typed page state 复用 source preview。
- virtualized workbench 只有在 selected run 的 bounded page state 为 exact `completed` 时才调用 v3
  translated PNG command；`preserved` 复用同页 source PNG，pending/extracted/leased/failed、未请求页
  和尚未加载的 status window 都保持占位。preview cache identity 必须包含 run ID、patch ID、
  translation revision 和 page update identity，不能复用 legacy translated-PDF path 作为 v3 authority。

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
  所有 target 必须基于未修改的 source view 完成 hash/style/fit/encoding/path 校验，再统一
  staging 和 commit。production replacement staging 不得读取或持有完整 source document。
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
- per-show diagnostic schema 当前为 `rosetta-pdf-v3-text-show-replacement/7`，transaction
  schema 为 `rosetta-pdf-v3-text-show-replacement-transaction/3`。transaction 必须报告
  确定性排序的 `translationFontWeights`、`formInvocationDepth`、`clonedStreamCount` 和
  `pageContentRewired`，不能压缩成单一 weight。只允许报告 page/stream、count、style
  ID、weight/face、normalized color/opacity、render mode、geometry/fit、staged/cloned
  object count、stable text-show ID 和 timing，不得包含 source/translated text。
- page batch diagnostic schema 当前为
  `rosetta-pdf-v3-text-show-replacement-batch/1`，target schema 为
  `rosetta-pdf-v3-text-show-replacement-batch-target/1`。batch 负责全局 target/replacement、
  font object、clone、page rewiring 和 timing 计数；target 只记录自身 stream/path depth、
  replacement count、weights 与 per-show diagnostics，不得记录文本 payload。
## PDF v3 Native Export Result

The public PDF v3 export command is `export_rosetta_pdf_v3_run` and accepts
only `jobId`, safe `runId`, and a user-selected `targetPath`. Source identity,
target language, requested PageSet, runtime revision, renderer policy, provider,
model and font bindings are derived from native durable authority.

Its result schema is `rosetta-pdf-v3-run-export/1` and contains the run ID,
target language, requested/completed/preserved page counts and
`rosetta-pdf-v3-region-translation-export/2` container/line/font/byte/object metrics. It does not contain
source or destination paths, document text, credentials, endpoints or owner
session IDs. Nonterminal runs, active leases and any scheduler, runtime,
PageGraph, patch or source identity mismatch are rejected before atomic
destination replacement. Preserved pages remain source content; a run with no
translated pages uses verified atomic source copy.
