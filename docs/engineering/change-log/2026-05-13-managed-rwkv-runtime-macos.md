# 2026-05-13 Managed RWKV Runtime Skeleton — macOS-first (Phase 3)

## Context

[ADR 0003](../decisions/0003-macos-first-managed-rwkv-runtime.md) 把 macOS Apple Silicon 定为本地 RWKV 运行时的首发平台。Phase 1 把翻译 connector 解耦成 provider 抽象（[2026-05-13-rwkv-provider-adapter-split.md](2026-05-13-rwkv-provider-adapter-split.md)）；Phase 2 把 sidecar 二进制接进 Tauri bundle（[2026-05-13-rwkv-sidecar-build-pipeline.md](2026-05-13-rwkv-sidecar-build-pipeline.md)）。

Phase 3 把"Rosetta 自己启动这个 sidecar"这条路径补齐：新增 `managed_rwkv` 模块，提供平台兼容检测、profile 抽象、app data 布局、sidecar lifecycle（启/停/探活）和 7 个 Tauri commands，让 Phase 5 的 Settings UI 能直接拿来调。

旧的 [`rwkv_runtime.rs`](../../../rosetta-app/src-tauri/src/rwkv_runtime.rs) （Windows libtorch 骨架）继续 `#[allow(dead_code)]` 保留作为 Phase 8 重启 Windows 时的资料，**本次不改动**。新代码是完全新建的 `managed_rwkv/` 模块。

## Changes

### Rust side — 新增 `src-tauri/src/managed_rwkv/` 模块（5 个文件）

- [`profile.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/profile.rs)：`RuntimeProfile` 静态描述结构 + `MACOS_ARM64_WEBRWKV` 常量（v1 启用）+ `WINDOWS_AMD64_LIBTORCH`（`enabled: false`，Phase 8 复活）。`current_profile()` 按运行时 `OS+ARCH` 选；非 Apple Silicon 返回 `None` 让命令报 `unsupported`。`RuntimeProfileSummary` 是给前端的 camelCase 序列化形态。
- [`layout.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/layout.rs)：`RuntimeLayout` 解析 `<app-local-data>/managed-rwkv/` 下的所有路径——`models/<profile>/manifest.json + model file`、`runtime-state/active-runtime.json`、`logs/runtime.log`。计算与 `ensure_dirs()` 分开，dev box 不会被空目录污染。
- [`status.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/status.rs)：`ManagedRuntimeState`（unsupported / not-installed / installed / starting / ready / failed / stopped）+ install plan（sidecar / tokenizer / model 三项）+ sidecar/tokenizer 路径解析（bundle 模式找 `Contents/MacOS/...`，dev 模式找 `src-tauri/binaries/...`）。
- [`lifecycle.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs)：`ManagedRwkvRuntimeRegistry`（`tokio::sync::Mutex<RuntimeInner>` 共享状态）+ `start_sidecar`（ephemeral 端口、`tokio::process::Command spawn`、kill_on_drop、HTTP 等 `/health` 200，超时 45s）+ `stop_sidecar`（kill + wait 防止 zombie）+ `probe_sidecar`（GET `/health`，5s 超时）+ `read_log_tail`（最后 8KB）。每次访问 registry 前 `reap_exited_child()` 回收已退出的子进程，状态机不漂移。
- [`mod.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/mod.rs)：7 个 Tauri commands 的薄包装层 + 公共类型再导出。

### lib.rs

- 注册 `managed_rwkv` 模块。
- `.manage(managed_rwkv::Registry::default())` 把 lifecycle 状态接进 Tauri state plumbing。
- 注册 7 个新 commands：
  - `get_managed_rwkv_runtime_status`
  - `get_managed_rwkv_install_plan`
  - `install_managed_rwkv_runtime`（Phase 3 内是 stub，Phase 4 接下载）
  - `start_managed_rwkv_runtime`
  - `stop_managed_rwkv_runtime`
  - `probe_managed_rwkv_runtime`
  - `get_managed_rwkv_runtime_logs_summary`

### Cargo.toml

- `tokio` features 增加 `"process"` + `"io-util"`（spawn child + log file handle）。production code 之外没新增 crate。

### Frontend

- [`src/types/rosetta.ts`](../../../rosetta-app/src/types/rosetta.ts)：新增 11 个 TS 类型镜像 Rust 端的 managed runtime 输出（`ManagedRuntimeState`, `ManagedRuntimeStatus`, `ManagedRuntimeInstallPlan`, `ManagedRuntimeStartResult`, `ManagedRuntimeProbeResult`, `ManagedRuntimeLogsSummary` 等）。
- [`src/lib/rwkvRuntime.ts`](../../../rosetta-app/src/lib/rwkvRuntime.ts)：新增 7 个 `getManagedRwkv*` / `startManagedRwkv*` / `stopManagedRwkv*` / `probeManagedRwkv*` / `installManagedRwkv*` / `getManagedRwkvRuntimeLogsSummary` bridge 函数。**旧的 paused stubs 不动**，等 Phase 8 Windows 复活时再决定是否清理。

## Compatibility

- 现有翻译流（外部 API + Phase 1 provider 抽象）**行为不变**。所有 `rwkv_api::*` / `rwkv_providers::*` 命令一字未动。
- 旧 `rwkv_runtime.rs` 全部 36 个单测继续通过。
- 全套 89 个测试通过（旧 78 + 新 11）。
- TypeScript public 类型仅新增不修改。
- 在 Intel Mac / Windows / Linux 上运行：`get_managed_rwkv_runtime_status` 返回 `state: "unsupported"`，所有 lifecycle 命令返回友好错误，**不 panic**。

## Known Boundary

- `install_managed_rwkv_runtime` 当前**只确保目录存在**并返回 install plan 摘要——真正的模型下载是 Phase 4 的工作。命令现在就建好，是为了让 Phase 5 的 Settings UI 落在稳定 contract 上。
- 模型路径目前是空（除非用户手动放）：本机 e2e 测试需要把 `/Users/leolin/rwkv-test/models/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab` 软链到 `~/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`，Phase 4 会替换这一手动步骤。
- `active-runtime.json` / model `manifest.json` **当前不写盘**——runtime 状态只在内存 `RuntimeInner` 里。优点是 Rosetta 重启就清零，不会留下不一致的 pid 文件；缺点是无法在崩溃后自动恢复（这与 ADR 0002 的"运行时状态非持久化"约束一致）。Phase 4 会引入 model manifest 持久化。
- sidecar 取消行为：当前的 `stop_managed_rwkv_runtime` 直接 SIGKILL（`tokio::process::Child::kill`）；rwkv_server 没有公开的 graceful shutdown。这与 Phase 0 验证笔记里"取消测试待补"的开放项一致；Phase 6 翻译集成时再确认是否需要先关 HTTP 连接再 kill。
- 没有引入 `tauri-plugin-shell`：sidecar 路径手动从 `app.path().resource_dir()` + `Contents/MacOS/...`（bundle）或 `CARGO_MANIFEST_DIR/binaries/...`（dev）解析，避免新增依赖与 capability JSON 改动。

## Verification

- `cargo check`：通过。
- `cargo test --lib`：89 passed（旧 78 + 新 11，无 regression）。
- `cargo clippy --lib --all-targets`：managed_rwkv 部分零警告；5 个 pre-existing 警告（rosetta_jobs / 老 rwkv_api / rwkv_runtime）不变。
- `pnpm typecheck`：通过。
- Sidecar 二进制依然在 [`src-tauri/binaries/rwkv-server-aarch64-apple-darwin`](../../../rosetta-app/src-tauri/binaries/) （Phase 2 fetch script 已 stage）；分词表在 [`src-tauri/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt`](../../../rosetta-app/src-tauri/resources/rwkv-sidecar/) 。
- 端到端 spawn 验证延后到 Phase 5（UI 打通后从 Settings 点"启动"按钮触发）。Phase 3 unit 测试覆盖：profile 选择、布局解析、install plan 检测、端口分配、命令参数拼接、UTC ISO 时间格式。

## Next

Phase 4：模型下载实现。`install_managed_rwkv_runtime` 接 reqwest 流式下载（ModelScope 优先、HF 走代理为 fallback），SHA256 校验，写 `models/<profile>/manifest.json`，触发进度回调。
