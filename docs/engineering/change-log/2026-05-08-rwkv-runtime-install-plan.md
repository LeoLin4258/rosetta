# 2026-05-08 RWKV Runtime Install Plan Skeleton

## 范围

新增本地 RWKV 安装计划骨架。

新增 Tauri command：

```txt
get_rwkv_runtime_install_plan
```

该 command 不联网、不下载、不启动 runtime，只根据当前 app data 中的 manifest 和本地文件校验结果返回：

- runtime 是否缺失、就绪或无效
- model 是否缺失、就绪或无效
- 每项的目标目录
- manifest 路径
- artifact 路径，如果 manifest 已声明
- 简短状态信息

## 状态修正

状态判断现在会单独校验已存在的 manifest。此前如果只有 model manifest 存在但 runtime manifest 缺失，坏 model manifest 可能只表现为 `partial`。现在坏 model manifest 会直接返回 `invalid`。

## 前端

设置页的本地 RWKV 面板新增“安装计划”区，用于展示 runtime 和 model 两项的准备状态。这只是展示安装计划，不会触发下载。

## 测试

新增 Rust 测试覆盖：

- 无 manifest 时安装计划为 missing
- runtime/model 都有效时安装计划 ready
- 单项 manifest 错误时安装计划标记对应 item 为 invalid
- 只有坏 model manifest 时 runtime status 返回 invalid

runtime 测试总数从 11 个增加到 15 个。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
