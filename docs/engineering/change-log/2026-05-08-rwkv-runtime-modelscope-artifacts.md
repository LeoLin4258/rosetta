# 2026-05-08 RWKV Runtime ModelScope Artifacts

## 范围

通过 ModelScope repo files API 确认 runtime 和 1.5B model artifact metadata，并回填 artifact catalog。

已确认 model artifact：

```txt
filename: RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
sizeBytes: 3055445546
sha256: b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa
downloadUrl: https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth
```

已确认 Windows amd64 runtime artifact：

```txt
filename: rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
sizeBytes: 1321825122
sha256: e4957c0dc771ea949d24f1d15123848dc2243546db62f4928c695c799c99e881
downloadUrl: https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/rwkv_lightning_libtorch2.10.0+cu132_sm75-120_Windows_amd64.zip
```

本次没有下载模型权重或 runtime zip。

## 代码变更

artifact catalog 中 runtime 和 model item 均从 `metadata-pending` 改为 `ready`。Catalog 现在可用于后续下载实现。

安装进度中的 missing item 现在可以带上预期下载大小：

- runtime: `1321825122`
- model: `3055445546`

## 仍待确认

- runtime zip 内部结构
- 解压后启动命令
- 是否支持无 password 本地运行
- 是否能绑定 `127.0.0.1`
- 首发是否只支持 Windows amd64 CUDA runtime

## 验证

已执行：

- `cargo fmt`
- `cargo test rwkv_runtime`
- `corepack pnpm typecheck`
- `cargo check`

按要求不运行：

- `corepack pnpm dev`
- `corepack pnpm build`
