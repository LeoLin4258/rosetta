# 2026-05-13 Managed RWKV Model Install — Download + Verify + Manifest (Phase 4)

## Context

[ADR 0003](../decisions/0003-macos-first-managed-rwkv-runtime.md) 计划"普通用户点一下就完成本地翻译引擎的安装"。Phase 3 已经把 sidecar 二进制 / 分词表 / managed runtime 框架打齐（[2026-05-13-managed-rwkv-runtime-macos.md](2026-05-13-managed-rwkv-runtime-macos.md)），但 `install_managed_rwkv_runtime` 当时只是个 stub——只确保目录存在，没真的下载模型。

Phase 4 接上模型下载这一关：流式下载 + Range 断点续传 + SHA256 校验 + 镜像 fallback + manifest 落盘 + 进度事件，所有逻辑都按 ADR 0003 的隐私约束（127.0.0.1 only、不写源/译文进日志）和分发约束（同公司项目无 license 风险）实现。

## Changes

### 新增 `managed_rwkv/install.rs`

完整的模型下载子系统：

- **流式下载**：`reqwest` 开 `stream` feature，`bytes_stream` 一边写 `.part` 一边喂 `Sha256` hasher，避免一次性占 1.3 GB 内存。
- **Range resume**：检测到既有 `.part` 时先 re-hash 已有前缀（M4 上 1 GB ~5 秒），再以 `Range: bytes=<size>-` 请求剩余部分；server 不接受 Range（HTTP 200 而非 206）时自动回退到从头下载。
- **镜像 fallback**：profile 里的 `model_download_urls` 数组从前到后试，单个镜像 HEAD/GET/stream 任一失败就清掉 `.part` 换下一个。Phase 4 首发镜像顺序：HuggingFace 原站（reqwest 自动读 `HTTPS_PROXY`，国内走 Clash 即可），hf-mirror.com 作为 fallback。ModelScope 上 `mollysama/rwkv-mobile-models` 当前没有 `.prefab` 文件，**不进入镜像列表**——profile 注释里说明了 2026-05-13 的探测结果。
- **SHA256 验证**：硬编码 `f6eeb1fff051bcba88539f641993d9a45e4f697f2db37e3bf0fcdd09bff2ef15`（Phase 0 验证通过的同一份文件）。Phase 0 已经端到端跑通这份模型，所以 SHA256 不匹配就是真坏了。匹配时 `.part` → `<filename>` 原子 rename；不匹配时 `.part` → `<filename>.part.broken` 让用户能看见证据。
- **既有文件检测**：start 时如果 `model_file` 已经在场，先 re-verify SHA256；通过就直接走 `installed: false, ready: true` short-circuit，写一次 manifest 后返回。这条路径让"已有模型 → 仅写 manifest"的恢复也走 install 命令。
- **取消**：`AtomicBool` 在每个 chunk 边界检查。取消时**保留 `.part`**，下一次 install 自动 resume。
- **进度事件**：节流到 ~10/秒发 Tauri `managed-rwkv://install-progress` 事件，payload 是 `InstallProgress { phase, bytes_done, bytes_total, source_url, speed_bytes_per_sec, started_at, message, last_error }`。轮询接口 `get_managed_rwkv_install_progress` 同时存在，前端两种姿势都能用。
- **修复**：`InstallOptions { repair: true }` 在开始下载前先清掉 `.part` / `.part.broken` / 已存在模型 / manifest，让"重新下载"按钮的语义明确。
- **Manifest**：成功安装后写 `models/<profile>/manifest.json`，包含 `schemaVersion / profileId / providerId / filename / sha256 / sizeBytes / sourceUrl / installedAt`。

### `managed_rwkv/profile.rs`

- 移除 `model_size_bytes_estimate`（仅是估算），改为 `model_size_bytes`（精确值，HEAD content-length 必须匹配，否则 fail fast）。
- 新增 `model_sha256: &'static str`：写入 Phase 0 验证文件的实际 hex。
- 新增 `model_download_urls: &'static [&'static str]`：按优先级排列的镜像。

### `managed_rwkv/mod.rs`

- 替换之前的 stub `install_managed_rwkv_runtime` 为真实实现，接受可选 `options: InstallOptions`（`repair: bool`），返回 `InstallResult`。
- 新增 `get_managed_rwkv_install_progress` 命令（拉取当前进度快照）。
- 新增 `cancel_managed_rwkv_install` 命令（请求取消）。
- 旧 `ManagedRuntimeInstallStubResult` 类型移除。

### `lib.rs`

- `.manage(managed_rwkv::InstallStateRegistry::default())` 注册安装注册表为 Tauri state。
- 注册 2 个新 commands：`cancel_managed_rwkv_install` / `get_managed_rwkv_install_progress`。

### `Cargo.toml`

- `reqwest` 增加 `"stream"` feature（启用 `bytes_stream()`）。
- 新增 `futures-util = { version = "0.3", default-features = false }`（流的 `StreamExt::next`）。

### Frontend

- [`src/types/rosetta.ts`](../../../rosetta-app/src/types/rosetta.ts)：删除 `ManagedRuntimeInstallStubResult`；新增 `ManagedRuntimeInstallPhase` / `ManagedRuntimeInstallProgress` / `ManagedRuntimeInstallOptions` / `ManagedRuntimeInstallResult` / `ManagedRuntimeCancelInstallResult` 5 个类型。
- [`src/lib/rwkvRuntime.ts`](../../../rosetta-app/src/lib/rwkvRuntime.ts)：
  - `installManagedRwkvRuntime(options?)` 重写，返回 `ManagedRuntimeInstallResult`。
  - 新增 `getManagedRwkvInstallProgress()` 拉快照。
  - 新增 `cancelManagedRwkvInstall()`。
  - 新增 `subscribeManagedRwkvInstallProgress(handler)` —— 用 `@tauri-apps/api/event` 的 `listen`，监听 `managed-rwkv://install-progress` 事件，返回 unlisten 函数（React effect cleanup 用）。

## Compatibility

- 现有翻译流（外部 API + Phase 1 provider 抽象 + Phase 3 lifecycle）**行为不变**。
- 旧 `rwkv_runtime.rs` 36 个测试 + Phase 1.A / Phase 3 测试全数通过。
- 全套 95 个测试通过（旧 89 + 新 6）。
- TypeScript public 类型仅增、改 1 个（删除 stub 类型 + 新增 5 个），无破坏性改动。
- 进度事件接入是新增（前端不监听也不影响命令本身），UI 在 Phase 5 接上后才会真用。

## Known Boundary

- **网络环境依赖**：默认下载源是 huggingface.co（CloudFront CDN）；国内必须有可用代理（reqwest 读 `HTTPS_PROXY`/`HTTP_PROXY` 环境变量，Tauri app 会继承）。hf-mirror.com 作为 fallback 但 2026-05-13 实测它的 LFS 不稳定。Phase 5 的 Settings UI 需要给用户提供代理配置入口或文案说明。
- **ModelScope 不在镜像列表**：`mollysama/rwkv-mobile-models` 在 ModelScope 上没有 `.prefab` 文件，`MollySophia` 命名空间下也没找到（API 返回空 Files 列表）。如果上游后续发布到 ModelScope，profile 加一行 URL 即可。
- **取消语义**：当前在 chunk 边界检查；最坏情况要等当前 chunk 写完才退出，实际延迟 < 1 秒（reqwest 默认 chunk size）。已下字节通过 `.part` 持久化，下次 install 自动 resume。
- **首次 install 无进度恢复**：取消后 `.part` 存在，但 `InstallProgress` 内存状态会回到 Idle（Rosetta 重启）。下次 install 命令仍然会 resume，但 UI 不会自动告诉用户"你之前停在 X%"。Phase 5 决定要不要把 progress 状态也持久化。
- **manifest 不参与 install 决策**：当前 install 命令只看 `model_file + SHA256` 决定是否短路；不依赖 manifest 存在。manifest 是给后续审计 / UI 显示用。Phase 6 / Phase 7 可以扩到"manifest 缺失/过旧时强制重 verify"。
- **未在 live runtime 中验证 end-to-end download**：unit tests 覆盖 hex / manifest 序列化 / artifact cleanup / progress idle / suffix path / iso_now。HTTP 链路通过手工 curl 验证：HEAD 返回正确 Content-Length、`Accept-Ranges: bytes`、`Range: bytes=N-M` 返回 206 + 精确字节数。完整的"从 0 字节开始流式下载到完成"的端到端路径会在 Phase 5 UI 接入后从 Settings 触发验证。

## Verification

- `cargo check`：通过。
- `cargo test --lib`：95 passed（旧 89 + 新 6，无 regression）。
- `cargo clippy --lib --all-targets`：managed_rwkv 零警告；5 个 pre-existing 警告不变。
- `pnpm typecheck`：通过。
- 网络层手工验证（HF + Clash 代理）：
  - HEAD `https://huggingface.co/.../RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab` → 200 + `Content-Length: 1355373863` + `Accept-Ranges: bytes`
  - Range GET `bytes=1000000-1000099` → HTTP 206 + 100 字节
- 本地模型 SHA256 验证（Phase 0 文件）：
  - `shasum -a 256 /Users/leolin/rwkv-test/models/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`
  - = `f6eeb1fff051bcba88539f641993d9a45e4f697f2db37e3bf0fcdd09bff2ef15` ✓ 与 profile.rs 完全一致

## Next

Phase 5：UI 一键安装与就绪门控。Settings 面板接 `installManagedRwkvRuntime()` + `subscribeManagedRwkvInstallProgress()` + `startManagedRwkvRuntime()` 串起来；首次启动 onboarding banner；Jobs 页就绪门控；macOS-only 检测和回落 UI。
