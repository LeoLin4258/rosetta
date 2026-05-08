# 2026-05-08 RWKV Runtime Artifact Catalog

## 范围

新增本地 RWKV artifact catalog skeleton。

新增 Tauri command：

```txt
get_rwkv_runtime_artifact_catalog
```

当前 catalog 不联网、不下载，只描述后续需要准备的 artifact：

- RWKV Lightning runtime
- RWKV v7 G1 Translate 1.5B model

每个 catalog item 返回：

- artifact id
- kind
- 状态
- 目标目录
- manifest 路径
- 来源页面
- 预期下载 URL、文件名、大小、hash，如果已知

当前 runtime 和 model 都标记为 `metadata-pending`，因为 Stage 0 尚未确认真实打包文件名、大小和 hash。catalog 因此不会被标记为可下载。

## 前端

设置页本地 RWKV 面板新增 `Artifact Catalog` 区，用于展示后续下载器会使用的来源和目标路径。

## 测试

新增 Rust 测试覆盖：

- metadata 未确认时 catalog 不可下载
- catalog item 指向 Rosetta 管理的 runtime/model 目标目录

runtime 测试总数从 18 个增加到 20 个。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`
