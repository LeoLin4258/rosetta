# 0004 PDF 翻译管线技术债务

## Status

Deferred

## Context

PDF 翻译功能在迭代过程中暴露了一系列互相关联的 bug：

- 取消按钮无效（`cancelRef.current` 在 PDF 路径下始终为 null）
- 取消后预览区仍显示"正在翻译"（本地 `generating` state 未重置）
- 鬼魂翻译（取消后自动触发了新一轮 PDF 生成）
- 顶部进度条显示无意义的 `0/1 · 0%`

根本原因：`PdfDocumentPreview` 同时承担了展示和翻译触发两个职责。每次发现问题都通过补丁修复，导致逻辑分散在多个组件中。

## Decision

2026-05-20 做了一次局部结构改善：将 `PdfDocumentPreview` 改为纯展示组件，所有 PDF 生成逻辑上移到 `WorkspacePage`。这修复了鬼魂翻译的直接成因（自动触发 effect），但 `WorkspacePage` 本身仍存在以下技术债：

1. **三条翻译路径逻辑重复**：`handleTranslate` / `handleRetranslateAll` / `handleRetranslateSelected` 各自管理 `runId`、`cancelRef`、`startTranslationRun` / `finishTranslationRun`，代码高度重复，PDF 和文本路径用 `if (format === "pdf")` 分支混在一起。

2. **取消机制脆弱**：`cancelRef` 存一个可能为 null 的函数，依赖调用方在正确时机设置和清除，容易遗漏（如 `handleRetranslateAll` PDF 分支直到本次修复前都没有设置 `cancelRef`）。

3. **翻译状态靠多个变量拼凑**：`isTranslating = !!activeTranslationRun`、`runId`、`cancelRef`、`pdfError`、`pageError` 分散维护，没有统一的状态机。

4. **`WorkspacePage` 体积过大**：~700 行，UI 渲染与业务逻辑混在同一文件。

## Future Work

完整重构方向（待排期）：

- 将 PDF 翻译逻辑提取为独立的 `usePdfTranslation` hook
- 将文本翻译逻辑提取为独立的 `useTextTranslation` hook
- 用统一的取消机制（如 `AbortController` 或单一 cancel token）替代 `cancelRef`
- 考虑用状态机（XState 或简单枚举）管理翻译生命周期

## Consequences

当前实现可用，已知缺陷在可控范围内。下次 PDF 翻译路径出现新 bug 时，应优先评估是否触发完整重构，而非继续打补丁。
