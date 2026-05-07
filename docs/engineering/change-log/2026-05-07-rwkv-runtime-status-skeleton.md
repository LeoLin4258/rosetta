# 2026-05-07 RWKV Runtime Status Skeleton

## 范围

完成本地 RWKV runtime 计划的第一段实现：

- Tauri command skeleton，用于读取托管 RWKV runtime 状态
- 前端 helper，用于调用 runtime status command
- 设置页状态区，用于展示 Rosetta 托管的本地 RWKV 状态
- runtime status 的共享 TypeScript 类型

本次改动不安装、下载、启动或停止模型 runtime，只建立本地 runtime 状态查询的 Rust/React 边界。

## Runtime 边界

新增 command 保持窄接口：

```txt
get_rwkv_runtime_status
```

它解析 Tauri app data 目录，并返回以下预期路径：

- runtime manifest
- model manifest
- runtime log file
- 默认本地 API URL

它不创建目录、不启动进程、不执行 shell 命令，也不读取文档内容。

## 用户体验

设置页现在区分：

- Rosetta 托管的本地 RWKV runtime 状态
- 可手动配置的 RWKV API 连接设置

这保留了当前外部 API 工作流，同时为后续一键托管 runtime 做准备。

## 验证

已执行：

- `cargo fmt`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
