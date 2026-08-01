# Rosetta Engineering Notes

这个目录记录 Rosetta 的工程决策、较大改动和长期约定。它不是替代 Git 历史，而是补充 Git 历史中缺少的上下文：为什么这样做、哪些边界不能随意破坏、未来做新功能时应该遵循什么。

## 当前活跃计划

- PDF 稳定化、Linux 包体与可维护性治理：[`plans/2026-07-27-pdf-stabilization-governance.md`](plans/2026-07-27-pdf-stabilization-governance.md)

同一领域只能有一份活跃 handoff authority。历史 plan、closeout、ADR 和 change-log 可能保留当时的事实，但下一位 agent 应从这里列出的活跃计划进入，并以当前代码和实际验证为准。

## 目录

```txt
engineering/
  decisions/     技术决策记录，记录不可轻易改变的方向
  change-log/    大改动记录，记录每次较大项目改动的范围和验证方式
  conventions/   开发约定，指导后续功能如何保持一致
  benchmarks/    性能实测记录，记录可复查的测试环境、方法、结果和结论
  plans/         较大功能或架构工作的执行计划，记录尚未定案的推进路径
  archive/       历史文档索引，只负责状态和当前权威路由
  pdf-pipeline.md 当前 PDF 翻译管线的文件布局、状态机、恢复和删除规则
```

## 什么时候更新

需要更新工程文档的情况：

- 引入新的技术栈、库、插件或运行时依赖
- 改变 Tauri/Rust 与 React/TypeScript 的边界
- 改变 RosettaDocument、RosettaBlock、Segment、Job 等核心数据结构
- 改变本地任务缓存、导入、导出或翻译调度策略
- 完成一个会影响后续开发方式的大功能
- 修复一个暴露设计问题的 bug，并形成新的约定
- 制定会影响多个阶段的大功能计划

不需要更新工程文档的情况：

- 小的样式修正
- 文案调整
- 不改变行为的局部重构
- 单个明显 bug 的直接修复，且没有形成新约定

## 文档类型

### ADR

ADR 放在 `decisions/`。它回答“为什么选择这个方案”。文件名使用递增编号：

```txt
0001-app-stack.md
0002-document-ir.md
```

### Change Log

大改动记录放在 `change-log/`。它回答“这次改了什么，怎么验证”。文件名使用日期加简短主题：

```txt
2026-05-07-initial-infra.md
```

### Conventions

约定文档放在 `conventions/`。它回答“以后类似功能应该怎么写”。约定不是越多越好，只记录已经影响实际开发的规则。

### Benchmarks

性能实测记录放在 `benchmarks/`。它回答“某个性能判断是怎么测出来的”。记录应包含测试对象、环境、数据来源、执行命令、关键结果、限制和后续动作，避免只留下一个孤立数字。

### Plans

计划文档放在 `plans/`。它回答“还没有完全定案的大功能准备怎么推进”。计划可以包含待验证假设、阶段目标、风险和退出标准。方案被验证并形成长期约束后，应新增 ADR 或更新对应约定，而不是让计划文档承担最终决策记录。

较长实施应拆成可独立交接的 checkpoint，并在同一活跃计划内维护状态、验证结果和下一步动作。不要为每个 agent 或每个 checkpoint 新建 handoff 文档。

## 文档治理

- 同一领域只能有一个 active handoff；checkpoint 开始、验证结果、blocker 和下一步动作都回写这份 active plan。
- 历史 plan、closeout、benchmark、ADR 和 change-log 不因过时而删除，但必须通过归档 banner 或短索引指向当前事实来源。
- 一个 release 最多维护一份 aggregate change-log。checkpoint 过程记录属于 active plan，不应为每个 agent 或小任务继续新增 change-log。
- ADR 只记录会长期约束实现、数据或发布边界且不应被普通重构静默改变的决定。实验步骤、临时 handoff 和可逆实现细节留在 plan 或 change-log。
- 当前行为说明必须优先引用代码、conventions 和领域 current-pipeline 文档；历史文件中的 `current`、`production`、`authority` 只代表其写作时点。

## 写作原则

- 记录决策依据，不写泛泛总结。
- 明确影响范围，方便未来排查回归。
- 明确验证方式，避免只留下主观判断。
- 如果约定还不稳定，标记为 draft。
- 如果未来推翻旧决策，新增 ADR，不直接覆盖历史原因。
