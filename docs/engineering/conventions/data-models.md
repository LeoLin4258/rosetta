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

## RosettaBlock

`RosettaBlock` 表示文档结构单元，例如标题、段落、列表项、表格单元格、代码块。

约定：

- `order` 必须保留原文档顺序。
- `shouldTranslate` 决定是否进入翻译调度。
- 代码块、URL、文件路径、公式等内容应尽量标记为不翻译或在 segment 阶段保护。
- `style` 只记录结构和导出需要的信息，不放 UI 临时状态。

## Segment

`Segment` 是翻译调度的最小单位。

约定：

- 一个 block 可以拆成多个 segment。
- `blockId` 必须能追溯回原始 block。
- `order` 必须能恢复翻译前顺序。
- `preserveWhitespace` 用于提示合并和导出阶段保留空白。
- 用户编辑后的译文状态应标记为 `edited`，后续重翻不能静默覆盖。

## Job

`RosettaJob` 表示一个本地翻译任务。

约定：

- Job 状态变化应可恢复，不能只存在内存中。
- MVP 阶段任务缓存使用 JSON 文件，根目录固定在 app data 的 `jobs/` 下。
- Job store 的持久化文件必须带 `schemaVersion: 1`，后续格式变化需要迁移路径。
- `RosettaJobBundle` 是前端加载项目的最小完整单位，包含 `job`、`document`、`segments`。
- `index.json` 只保存 `RosettaJobSummary[]`，完整文档和 segments 分别保存在项目目录下。
- 删除项目只删除 Rosetta 自己的 job cache，不删除用户原始文件，也不删除已经导出的文件。
- 后续如果引入 SQLite，需要新增 ADR 说明原因和迁移策略。

当前 JSON 布局：

```txt
AppData/Rosetta/jobs/
  index.json
  <jobId>/
    source.txt 或 source.md
    document.json
    segments.json
    exports/
```

导入约定：

- 当前只支持 TXT、Markdown。
- 文件由 Tauri command 读取，前端不直接获得宽泛文件系统权限。
- TXT 按空行切分为段落。
- Markdown 使用轻量 block parser，首版只保留标题、段落、列表、引用、代码块和空行等基础结构。
- fenced code block、纯 URL 行和空白行默认 `skipped`。

导出约定：

- `translation` 导出纯译文。
- `bilingual` 导出双语对照。
- 未完成或失败 segment 导出时使用原文占位，避免输出断裂。
- Markdown 导出只承诺保留基础 marker，不承诺完整 CommonMark AST 级别还原。

## RWKV API Config

`RwkvConnectionConfig` 表示当前翻译后端连接设置。托管本地 runtime 暂停期间，它应面向一个已存在的 RWKV 翻译 API，而不是 Rosetta 管理的 runtime 状态。

约定：

- `baseUrl` 和 `endpoint` 共同组成请求地址。
- API token、body password 等凭据只能保存在用户本机设置中，不能写入仓库、文档、测试或 fixture。
- 远程或云端 API 必须是用户显式配置的 opt-in 后端。
- 翻译 pipeline 不能依赖 `start_rwkv_runtime` 或 managed runtime readiness。
- 如果未来恢复 Rosetta 托管本地 runtime，应新增 runtime choice ADR，再决定是否扩展该配置模型。

## Compatibility

核心类型一旦被任务缓存使用，就视为持久化格式的一部分。修改字段时需要考虑：

- 是否需要版本号
- 旧任务是否还能读取
- 是否需要迁移脚本
- 导出结果是否受影响
