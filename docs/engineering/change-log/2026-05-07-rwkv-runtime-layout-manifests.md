# 2026-05-07 RWKV Runtime Layout and Manifests

## 范围

推进本地 RWKV runtime 的第二段实现：

- 新增 `initialize_rwkv_runtime_layout` Tauri command
- 创建 app data 下的 runtime、model、logs 目录
- 定义并读取 runtime/model manifest 摘要
- 扩展 runtime status，区分 `not-installed`、`partial`、`installed`、`invalid`
- 设置页增加“准备目录”动作

本次改动仍不下载模型、不安装 Python、不启动 RWKV Lightning，也不创建假的 installed manifest。

## Runtime 边界

新增 command：

```txt
initialize_rwkv_runtime_layout
```

该 command 只允许创建 Rosetta 管理的固定目录：

```txt
AppData/Rosetta/runtime/rwkv-lightning
AppData/Rosetta/models/rwkv-v7-g1-translate/1.5b
AppData/Rosetta/logs
```

它不接受任意路径，不执行 shell 命令，不启动进程。

## Manifest 行为

状态查询会尝试读取：

```txt
runtime-manifest.json
model-manifest.json
```

如果 manifest 缺失但目录存在，状态为 `partial`。如果 manifest 存在但 JSON 无法解析，状态为 `invalid`，并把错误信息返回给设置页。

## 验证

已执行：

- `cargo fmt`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
