# 2026-05-07 RWKV Runtime Artifact Verification

## 范围

为本地 RWKV runtime manifest 增加真实文件校验。

新增行为：

- model manifest 必须提供 `filename` 和 `sha256`
- model artifact 必须位于 Rosetta 管理的 model 目录内
- 如果 manifest 提供 `sizeBytes`，必须与本地文件大小一致
- 本地文件的 SHA-256 必须与 manifest `sha256` 一致
- runtime manifest 如果提供 `filename`、`sha256` 或 `sizeBytes`，也会执行同样的文件校验

本阶段仍不下载模型、不安装 Python、不启动 runtime。

## 安全边界

artifact `filename` 不允许：

- 绝对路径
- `..` 路径逃逸
- 任何非普通路径组件

这样后续下载或安装阶段即使 manifest 出错，也不会让校验逻辑读取 Rosetta 管理目录之外的文件。

## 测试

新增 Rust 测试覆盖：

- model artifact 缺失时返回 `invalid`
- model artifact size 不匹配时返回 `invalid`
- model artifact hash 不匹配时返回 `invalid`
- model filename 试图逃逸管理目录时返回 `invalid`

runtime 状态测试总数从 7 个增加到 11 个。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
