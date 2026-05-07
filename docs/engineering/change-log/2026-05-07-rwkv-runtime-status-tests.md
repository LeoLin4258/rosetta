# 2026-05-07 RWKV Runtime Status Tests

## 范围

为本地 RWKV runtime 状态机增加 Rust 单元测试。

覆盖状态：

- app data layout 不存在时为 `not-installed`
- runtime/model/logs 目录存在但 manifest 缺失时为 `partial`
- runtime 和 model manifest 都有效时为 `installed`
- manifest JSON 损坏时为 `invalid`

## 原因

runtime manager 后续会继续接入下载、校验、启动和日志读取。状态判断如果只靠设置页手动观察，很容易在后续阶段引入回归。先把纯状态逻辑拆成可测路径，可以让后续实现更稳。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`

