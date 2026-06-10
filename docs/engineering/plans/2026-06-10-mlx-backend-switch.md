# MLX 后端切换（WebRWKV → MLX + 0.4B 模型）

日期：2026-06-10

## 背景

原方案为 WebRWKV（wgpu/Metal）+ 1.5B nf4 模型（1.26 GB）。RWKV 工程师验证后，macOS M4 上使用 MLX 后端 + 0.4B 模型（~360 MB zip）速度更快，体积更小，翻译质量足够。

## 本次改动

### `profile.rs`
- 新增 `MACOS_ARM64_MLX` 常量（`backend: "mlx"`，0.4B 模型，`model_is_zip: true`）
- `MACOS_ARM64_WEBRWKV` 改为 `enabled: false`
- `ALL_PROFILES` 将 MLX profile 排第一

模型信息：
- 文件名：`rwkv7-0.4B-g1d-translate-20260607-ctx4096-mlx-6bit.zip`
- 大小：377,343,557 字节
- SHA256：`ae1109105ce91627406972c25d618da2922f74331f773b18975c7e4e290bc226`

### `layout.rs`
- `RuntimeLayout` 新增 `model_extracted_dir: Option<PathBuf>` 字段
- zip profile 解压后目标目录 = `model_dir/<zip stem>`（去掉 `.zip` 后缀）

### `install.rs`
- 新增 `extract_zip` 函数，下载验证通过后解压 zip 到 `model_dir`，解压完成后删除 zip

### `lifecycle.rs`
- 模型路径校验从 `is_file()` 改为 `exists()`，兼容目录型模型（MLX 格式）

### `mod.rs`（start 命令）
- 传给 `start_sidecar` 的 `model` 路径：zip profile 用 `model_extracted_dir`，否则用 `model_file`

### `status.rs`（install plan）
- 模型就绪检测：zip profile 优先检查 `model_extracted_dir.exists()`，否则检查 `model_file.is_file()`

### CI（`.github/workflows/build-rwkv-sidecar-macos.yml`）
- `RWKV_MOBILE_COMMIT` 更新为 `9c0780d4eeeb71ff8d5b6b8a0e8588f843427cbf`（2026-06-10 验证）
- CMake 参数：`ENABLE_WEBRWKV_BACKEND=OFF`，`ENABLE_MLX_BACKEND=ON`
- 产物新增 `default.metallib`（MLX 运行必需）

## 模型下载地址
- HuggingFace：`mollysama/rwkv-mobile-models/resolve/main/mlx/rwkv7-0.4B-g1d-translate-20260607-ctx4096-mlx-6bit.zip`
- hf-mirror：同路径备用

## 回滚方法
`MACOS_ARM64_WEBRWKV.enabled` 改回 `true`，`MACOS_ARM64_MLX.enabled` 改为 `false`，还原测试断言。
