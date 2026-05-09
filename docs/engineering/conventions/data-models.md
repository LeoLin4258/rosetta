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
- 用户编辑后的译文状态应标记为 `edited`，后续重翻不能静默覆盖。
- `sourceLang` 和 `targetLang` 必须跟随所属 job/document 的语言方向。任务页修改语言方向时，需要更新所有 segments 的语言字段。
- 如果任务语言方向发生变化，已有自动译文不再可信，应清空 translatable segments 的 `translatedText` 和 `error`，并把状态重置为 `pending`。这样可以避免 UI 显示新语言配置下的旧译文。
- `translationHistory` 是旧缓存兼容字段。新的默认历史译文 UI 不再从 segment-level history 重组，而是读取文件级 `TranslationRevision`。
- 重新翻译当前文件表示启动一次新的完整文件翻译运行，不是只补翻缺失 segment。开始重翻前，当前文件内所有可翻译 segment 的当前译文应保存为文件级历史版本，然后清空当前译文并从 0 重新计算本次运行进度。
- 选中段落重翻时，用户选择的是 block；如果一个 block 被拆成多个 segments，重翻范围包含该 block 下所有可翻译 segments。开始局部重翻前同样保存一份当前文件完整译文版本。
- 用户查看历史记录时，应看到过去某一次翻译运行的完整文件译文版本，而不是零散 segment 记录。Segment 仍是调度和缓存单位，但历史查看使用 `TranslationRevision.segmentTranslations` 重建文件视图。

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
- MVP 阶段任务缓存使用 JSON 文件，根目录固定在 app data 的 `jobs/` 下。
- Job store 的持久化文件必须带 `schemaVersion: 1`，后续格式变化需要迁移路径。
- `RosettaJobBundle` 是前端加载项目的最小完整单位，包含 `job`、`document`、`segments`、`translationRevisions`。
- `index.json` 只保存 `RosettaJobSummary[]`，完整文档和 segments 分别保存在项目目录下。
- 删除项目只删除 Rosetta 自己的 job cache，不删除用户原始文件，也不删除已经导出的文件。
- 后续如果引入 SQLite，需要新增 ADR 说明原因和迁移策略。

当前 JSON 布局：

```txt
AppData/Rosetta/jobs/
  index.json
  <jobId>/
    source.txt 或 source.md
    sources/<relative-path>  # 文件夹项目
    document.json
    segments.json
    translation_revisions.json
    exports/
```

导入约定：

- 当前只支持 TXT、Markdown。
- 文件由 Tauri command 读取，前端不直接获得宽泛文件系统权限。
- “新项目”可以导入单个文件，也可以导入文件夹。文件夹导入递归收集 `.txt`、`.md`、`.markdown`，跳过隐藏目录，并限制项目文件数量，避免原型阶段一次塞入过大项目。
- 文件夹项目的 `RosettaJobSummary.sourceKind` 为 `directory`，`fileCount` 记录导入文件数。`format` 仍保持 `txt | markdown`，混合项目只用它作为总体显示和兼容字段，每个文件的真实格式以 `RosettaSourceFile.format` 为准。
- TXT 按空行切分为段落。
- Markdown 使用轻量 block parser，首版只保留标题、段落、列表、引用、代码块和空行等基础结构。
- fenced code block、纯 URL 行和空白行默认 `skipped`。
- 系统文件选择和导出路径选择必须通过非阻塞 Tauri dialog command 完成，不能在 command 中调用 `blocking_pick_file` 或 `blocking_save_file`，避免 Windows 原生对话框打开时卡住应用窗口。

导出约定：

- `translation` 导出纯译文。
- `bilingual` 导出双语对照。
- 未完成或失败 segment 导出时使用原文占位，避免输出断裂。
- Markdown 导出只承诺保留基础 marker，不承诺完整 CommonMark AST 级别还原。
- 任务工作台的导出最小单位是当前选中的文件，而不是整个项目。项目是文件集合与共享设置容器，不能让用户在当前文件视图里误触发整项目导出。
- 当前文件必须完成翻译后才能导出；`done`、`edited` 和 `skipped` 视为已处理，`pending`、`translating`、`failed` 或空译文不能导出。
- 当前文件导出到用户选择的具体文件路径，输出文件名默认来自当前文件名，例如 `chapter.zh.md` 或 `chapter.bilingual.md`。
- 多文件项目的批量导出如果后续恢复，应作为单独的项目级入口，并明确提示会导出项目内所有文件。删除项目只删除 Rosetta job cache，不删除用户原始文件或已导出目录。

## Preview

文档预览应优先呈现“源文件视图”，而不是直接暴露 segment 列表。

约定：

- 双语预览左侧渲染原文结构，右侧渲染译文结构。
- 多文件项目的默认预览范围是“当前选中的一个文件”，不是把项目内所有文件连续渲染在同一个预览面板里。当前文件由前端 UI state `activeFileId` 控制。
- Markdown 预览使用 Markdown renderer，并启用 GFM 等常见语法支持；不要执行原文中的 HTML/script。
- 原文和译文滚动应同步，hover 某个 block 时两侧对应 block 同步高亮。
- 滚动期间应暂停 hover 高亮更新，避免鼠标停在文本上时 hover state 与滚动同步互相触发重渲染。
- 未翻译的 translatable block 在译文侧显示为空，不回退显示原文；`skipped` 内容如代码块仍可按原文保留。
- Segment 仍是调度和缓存单位，但不应作为普通用户默认看到的主要阅读结构。后续如果恢复结构切分调试视图，应作为单独的高级/诊断视图。

## RWKV API Config

`RwkvConnectionConfig` 表示当前翻译后端连接设置。托管本地 runtime 暂停期间，它应面向一个已存在的 RWKV 翻译 API，而不是 Rosetta 管理的 runtime 状态。

约定：

- `baseUrl` 和 `endpoint` 共同组成请求地址。
- API token、body password 等凭据只能保存在用户本机设置中，不能写入仓库、文档、测试或 fixture。
- 远程或云端 API 必须是用户显式配置的 opt-in 后端。
- 翻译 pipeline 不能依赖 `start_rwkv_runtime` 或 managed runtime readiness。
- 翻译请求使用当前任务的语言方向生成 prompt，格式为 `<SourceLabel>: ...\n\n<TargetLabel>:`。当前 RWKV 工程师确认的主路径仍是 English -> Chinese，其他语言方向属于 UI 和数据模型已支持、模型效果待验证的扩展能力。
- 如果未来恢复 Rosetta 托管本地 runtime，应新增 runtime choice ADR，再决定是否扩展该配置模型。

## Compatibility

核心类型一旦被任务缓存使用，就视为持久化格式的一部分。修改字段时需要考虑：

- 是否需要版本号
- 旧任务是否还能读取
- 是否需要迁移脚本
- 导出结果是否受影响
