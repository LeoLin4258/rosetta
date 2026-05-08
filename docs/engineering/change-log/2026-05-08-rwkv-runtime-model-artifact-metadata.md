# 2026-05-08 RWKV Runtime Model Artifact Metadata

## 范围

确认并回填 1.5B 翻译模型的 Hugging Face mirror artifact metadata。

已确认：

```txt
filename: RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
sizeBytes: 3055445546
sha256: b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa
downloadUrl: https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate/resolve/main/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
```

数据来源：

- Hugging Face model API
- Hugging Face tree API with `expand=true`

本次没有下载模型权重。

## 代码变更

artifact catalog 中的 model item 从 `metadata-pending` 改为 `ready`。

runtime item 仍为 `metadata-pending`，因为 runtime 打包方式、文件名、大小和 hash 尚未确认。因此整体 catalog 仍不会被标记为可下载。

## 仍待确认

- ModelScope 上同一 1.5B 文件的 metadata 是否与 Hugging Face mirror 一致
- 首发下载源是否使用 ModelScope、Hugging Face mirror，还是允许用户手动选择本地文件
- runtime package metadata

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求不运行：

- `corepack pnpm dev`
- `corepack pnpm build`
