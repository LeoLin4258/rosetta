# 2026-05-08 RWKV Runtime Install Progress Skeleton

## 范围

新增本地 RWKV 安装进度 skeleton。

新增 Tauri commands：

```txt
prepare_rwkv_runtime_install
get_rwkv_runtime_install_progress
```

这两个 command 不联网、不下载、不启动 runtime。当前行为：

- `prepare_rwkv_runtime_install` 创建固定 app data 目录并返回派生安装进度
- `get_rwkv_runtime_install_progress` 根据当前安装计划派生进度状态
- 进度项支持 `pending`、`ready`、`blocked`
- 整体进度支持 `queued`、`ready`、`blocked`

## 前端

设置页本地 RWKV 面板新增：

- “准备安装”按钮
- 安装进度区

该按钮当前只准备目录和进度状态，不触发真实下载。

## 测试

新增 Rust 测试覆盖：

- 缺失项派生为 `queued`
- 已就绪计划派生为 `ready`
- invalid 项派生为 `blocked`

runtime 测试总数从 15 个增加到 18 个。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
