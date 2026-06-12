# 2026-06-12 Blank TXT Document

## Summary

新增“新建文件”作为空工作台入口，用于创建空白 TXT 文档；侧边栏“新建文档”从直接打开系统文件选择器改为打开主工作台空状态。

## Changes

- 新增 `create_blank_txt_document` Tauri command，创建普通 `format: "txt"` 单文件 job。
- 新增 `update_txt_source_file` Tauri command，保存原文编辑内容并按 TXT 规则重建 blocks 和 segments。
- `WorkspaceEmpty` 现在提供 `导入文件`、`导入文件夹`、`新建文件` 三个入口，不再显示最近文档模块。
- TXT 单文件工作台的左侧原文区域新增编辑模式，编辑时原文区域变为文本输入框。
- 保存原文后清空旧译文文件列表，避免显示与新原文不匹配的译文。

## Impact

空白新建文件不是新格式，仍按 TXT 导入、预览、翻译和导出路径处理。当前只支持编辑单文件 TXT 原文。

## Verification

已验证：

```bash
cd rosetta-app/src-tauri
cargo test blank_txt_bundle_uses_txt_format_and_starts_empty
cargo test txt_source_edit_rebuilds_blocks_and_segments
```
