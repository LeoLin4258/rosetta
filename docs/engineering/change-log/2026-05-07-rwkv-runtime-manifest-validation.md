# 2026-05-07 RWKV Runtime Manifest Validation

## 范围

加强本地 RWKV runtime manifest 校验。

新增校验：

- runtime manifest id 必须以 `rwkv-lightning-` 开头
- model manifest id 必须为 `rwkv-v7-g1-translate-1.5b`
- model manifest `contextTokens` 必须为 `4096`
- model manifest `supportedDirections` 必须包含 `en-zh` 和 `zh-en`
- runtime/model manifest 中如果提供 `sha256`，必须是 64 位小写十六进制字符串

只有 runtime 和 model manifest 都通过校验时，状态才会进入 `installed`。

## 原因

后续会接入下载、解压和校验。仅凭 JSON 能解析就显示已安装，会让错误文件、错误模型或损坏 metadata 被误判为可用。这个阶段先把 metadata 合约固定下来，下一阶段再接 artifact 校验和下载状态。

## 测试

新增 Rust 测试覆盖：

- 模型 id 不匹配时返回 `invalid`
- 模型缺少 `zh-en` 方向时返回 `invalid`
- `sha256` 格式错误时返回 `invalid`

runtime 状态测试总数从 4 个增加到 7 个。

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`

