# 2026-05-13 RWKV Sidecar Build Pipeline & macOS Bundle Wiring (Phase 2)

## Context

[ADR 0003](../decisions/0003-macos-first-managed-rwkv-runtime.md) 确定 Rosetta v1 在 macOS Apple Silicon 上以独立 sidecar 进程的形态分发本地 RWKV 翻译运行时。Phase 0 验证证明 `rwkv-mobile` + WebRWKV 后端能编、能跑（[2026-05-13-rwkv-mobile-macos-validation-notes.md](../plans/2026-05-13-rwkv-mobile-macos-validation-notes.md)），Phase 1 把 Rosetta 的翻译 connector 解耦成 provider 抽象（[2026-05-13-rwkv-provider-adapter-split.md](2026-05-13-rwkv-provider-adapter-split.md)）。

但 Rosetta 自己分发的 `.app` / `.dmg` 还没有 sidecar 二进制可装：rwkv-mobile 上游 CI 只发布 `librwkv_mobile.dylib`，不发布 `rwkv_server`。Phase 2 解决"哪儿来的 sidecar"——为 Rosetta 仓库自建 macOS arm64 CI workflow，从源码编译并打包 sidecar，让后续 Tauri bundle 步骤能直接消费。

## Changes

### 新增 CI workflow

- 新建 [`.github/workflows/build-rwkv-sidecar-macos.yml`](../../../.github/workflows/build-rwkv-sidecar-macos.yml)：
  - **触发**：`workflow_dispatch`（手动）+ push tag `sidecar-v*`（发版）。**不**在每次 push/PR 上跑——sidecar 升级是显式动作。
  - **runner**：`macos-15`（Apple Silicon）。`CMAKE_OSX_DEPLOYMENT_TARGET=13.0` 与 Tauri bundle 的 `minimumSystemVersion` 保持一致。
  - **pin commit**：环境变量 `RWKV_MOBILE_COMMIT=498ae7e1e2b7c3abc998250b17f57948dbdd1dc0`（Phase 0 验证通过的 commit）；workflow_dispatch 时可临时覆盖。
  - **build flags**：`-DENABLE_WEBRWKV_BACKEND=ON -DENABLE_MLX_BACKEND=OFF -DENABLE_NCNN_BACKEND=OFF -DENABLE_LLAMACPP_BACKEND=OFF -DENABLE_SERVER=ON -DHTTPLIB_USE_OPENSSL_IF_AVAILABLE=OFF`。
  - **依赖 sanity 校验**：编完用 `otool -L` 扫描，链了 `/opt/homebrew/`、`/usr/local/`、`/opt/local/` 任何路径都 fail，避免给用户发送一个在 clean Mac 上 dlopen 不到的二进制。
  - **打包**：`rwkv-sidecar-macos-arm64-<short-sha>.tar.gz`，内含 `rwkv-server-aarch64-apple-darwin`（按 Tauri externalBin 命名）+ `b_rwkv_vocab_v20230424.txt` + `MANIFEST.json`（commit、build flags、每个文件的 SHA256、buildTime）。
  - **产物**：workflow artifact（每次跑都上传）+ GitHub Release asset（仅 tag push）。
  - **codesign / notarize**：暂不在本 workflow 做，留 Phase 7 与主 app 签名一起整套验证。workflow 注释里写明了所需 secrets。

### Sidecar 暂存脚本

- 新建 [`rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh`](../../../rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh)：
  - `--tag sidecar-vX` — 从 GitHub Release 拉指定 tag 的 tarball，校验 SHA256（per-file via MANIFEST.json，可选 `--sha256` 提前指定 tarball 整体），解压并 `install` 到正确路径。
  - `--local <rwkv-mobile-dir>` — 本地开发快捷模式：直接从相邻的 rwkv-mobile 工作目录拷贝 `build/examples/rwkv_server` + `assets/b_rwkv_vocab_v20230424.txt`，绕过 GitHub Release。
  - `--repo owner/name` — 覆盖默认 `LeoLin4258/rosetta`。
  - 依赖：Bash + curl + shasum + Python3（用 Python 校验 MANIFEST.json 里 per-file SHA256），所有 macOS 系统自带。
- 新增 [`rosetta-app/src-tauri/binaries/.gitkeep`](../../../rosetta-app/src-tauri/binaries/.gitkeep) 与 [`rosetta-app/src-tauri/resources/rwkv-sidecar/.gitkeep`](../../../rosetta-app/src-tauri/resources/rwkv-sidecar/.gitkeep) 让目录在仓库里存在但不提交真实二进制。
- 更新 [`rosetta-app/src-tauri/.gitignore`](../../../rosetta-app/src-tauri/.gitignore) 忽略 `binaries/*` 和 `resources/rwkv-sidecar/*`（保留 .gitkeep）。

### Tauri 配置

- 更新 [`rosetta-app/src-tauri/tauri.macos.conf.json`](../../../rosetta-app/src-tauri/tauri.macos.conf.json) 新增 `bundle` 节：
  - `targets: ["app", "dmg"]`（macOS 平台特定，不影响 Windows 的 `nsis` target）。
  - `externalBin: ["binaries/rwkv-server"]`（Tauri 自动追加 `-<target-triple>`，编译期校验文件存在）。
  - `resources: ["resources/rwkv-sidecar/*"]`（tokenizer + MANIFEST 进入 `Contents/Resources/_up_/resources/rwkv-sidecar/`）。
  - `macOS.minimumSystemVersion: "13.0"`（Ventura；与 sidecar `CMAKE_OSX_DEPLOYMENT_TARGET` 一致，覆盖所有 Apple Silicon Mac）。

## Compatibility

- Windows 构建路径**不受影响**：`tauri.macos.conf.json` 仅在 macOS target 时合并；`bundle.targets = ["nsis"]` 在 `tauri.conf.json` 不变。
- Rust / TypeScript 源码零改动。`cargo check`、`pnpm typecheck`、所有现有测试 pass。
- 既有外部 API 翻译流不变。
- 更新器（`createUpdaterArtifacts: true`）在 macOS 也会自动产 `.app.tar.gz + .sig`，但 endpoint 仍指向 GitHub `latest.json`；Phase 7 需要确认 macOS 包路径写进 `latest.json`。

## Developer Bootstrap

macOS 上 `cargo check` / `cargo build` / `pnpm tauri build` 第一次跑前**必须**先 stage sidecar 二进制，否则 tauri-build 编译期 externalBin 校验会失败：

```bash
# 方案 A：本地刚编完 rwkv-mobile 的开发者
bash rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh \
  --local /path/to/rwkv-mobile

# 方案 B：拉已发布的 sidecar tag
bash rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh \
  --tag sidecar-v0.1.0-498ae7e
```

第一个 `sidecar-v*` tag 发出之前，所有 macOS 开发者只能走方案 A（需要本地有 rwkv-mobile 源码 + 完成 Phase 0 验证步骤的本地构建）。该约束记入本次 change-log，Phase 5 之前再决定是否在 `pnpm tauri build` 前自动调用 fetch script。

## Known Boundary

- **codesign + notarize 不在本 workflow 内**：完整签名链路（含 hardened runtime、`com.apple.security.cs.disable-library-validation` 等 entitlements）放到 Phase 7 与主 app 签名一起跑，避免 Phase 2 引入未经验证的签名 step 后被 cargo cache 等问题掩盖。
- **CI 不会自动消费产物**：本 workflow 只产出 tarball；让 `pnpm tauri build`-side workflow（未来的 Phase 7 release workflow）显式调用 fetch script 拉指定 tag。Phase 2 不引入"sidecar 自动滚动升级"，每次 sidecar 版本变动都是显式 tag。
- **Tauri 编译期校验**：上面 Developer Bootstrap 一段提到的"先 stage 才能 cargo check"是 Tauri externalBin 默认行为。若未来 CI 上出现链式构建（不先 stage 就 cargo check），需要再调整。
- **Apple Silicon-only**：本 workflow 不产 macOS x86_64 binary；Intel Mac 用户在 Phase 5 的 UI 上会看到"仅 Apple Silicon"提示，回落到外部 API。
- **dylib 不在 bundle 内**：实测 rwkv_server 已静态链接 rwkv_mobile，无需单独 ship `librwkv_mobile.dylib`。本次 packaging 步骤特意不复制 dylib。

## Verification

- `python3 -c 'import yaml; yaml.safe_load(...)'` 验证 workflow YAML 语法 ✓
- `python3 -m json.tool` 验证 `tauri.macos.conf.json` / `tauri.conf.json` ✓
- `bash -n scripts/fetch-rwkv-sidecar.sh` 语法检查 ✓
- `cargo check` （在本机用 `--local` 模式 stage 后）：通过 ✓
- `cargo test --lib`：78 个测试全过（Phase 1 的 13 个新单测仍 pass）
- `pnpm typecheck` ✓
- 本机 `fetch-rwkv-sidecar.sh --local /Users/leolin/Documents/GitHub/rwkv-mobile` 成功 stage：
  - `src-tauri/binaries/rwkv-server-aarch64-apple-darwin` (23 MB)
  - `src-tauri/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt` (2.2 MB)

CI workflow 本身未在线上跑过——下一步推 `sidecar-v0.1.0-498ae7e` tag 触发首次发版，验证 macos-15 runner 的实际行为（特别是 Rust toolchain 安装时间和 ninja 并行度）。

## Next

Phase 3：把 `rwkv_runtime.rs` 从 "Windows libtorch 专用" 重构为按平台 profile 选择；macOS 路径的 sidecar 启动指向 bundle 里的 externalBin，端口走 ephemeral，HTTP 仅绑 127.0.0.1。
