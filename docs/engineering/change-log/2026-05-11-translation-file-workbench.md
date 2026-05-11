# 2026-05-11 Translation File Workbench

## Summary

Rosetta 工作台从“当前文件的一份译文”调整为“源文件生成多个内部译文文件”的结构。

## Changes

- 新增 `translation_files.json` 和 `translations/<translationFileId>.json`，用于保存每个源文件的多目标语言译文。
- 加载旧项目时，如果 `segments.json` 中已有 `translatedText`，会迁移为默认目标语言译文文件。
- 全局侧边栏只保留项目列表；项目内源文件、译文文件和批量选择移动到工作台内部。
- 导出入口绑定当前译文文件，默认文件名包含目标语言。

## Notes

- 本次仍使用 JSON 存储，没有迁移到 SQLite。
- v1 译文预览只读，人工编辑能力后续单独设计。
