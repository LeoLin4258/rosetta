# macOS Apple Silicon 一键本地 RWKV 翻译运行时 — 实施计划

> **当前状态（2026-05-13 晚）**
>
> | Phase | 内容 | 状态 |
> | --- | --- | --- |
> | 0 | 上游契约 + Phase 0 验证（M4 mini 实测） | 🟢 [验证笔记](2026-05-13-rwkv-mobile-macos-validation-notes.md) |
> | 1 | provider 抽象拆分（lightning-contents / mobile-batch-chat） | 🟢 [Phase 1 change-log](../change-log/2026-05-13-rwkv-provider-adapter-split.md) |
> | 2 | macOS Sidecar CI + Tauri bundle | 🟢 [Phase 2 change-log](../change-log/2026-05-13-rwkv-sidecar-build-pipeline.md) |
> | 3 | `managed_rwkv` Tauri 模块（lifecycle + 7 commands） | 🟢 [Phase 3 change-log](../change-log/2026-05-13-managed-rwkv-runtime-macos.md) |
> | 4 | 模型下载 + SHA256 + manifest + 进度事件 | 🟢 [Phase 4 change-log](../change-log/2026-05-13-managed-rwkv-model-install.md) |
> | 5 | UI 一键安装 + Jobs 就绪门控 + batch size clamp | 🟢 [Phase 5 change-log](../change-log/2026-05-13-managed-rwkv-settings-ui.md) + [batch clamp fix](../change-log/2026-05-13-managed-rwkv-batch-size-clamp.md)；**M4 mini 实机端到端通过** |
> | 6 | 动态 batch / 取消语义 / 长文档回归 | ⬜ 下一站 |
> | 7.A1 | dev 从零安装下载 | 🟢 [change-log](../change-log/2026-05-14-managed-rwkv-a1-fixes-loopback-no-proxy.md) — 修 lastError 可见性 + loopback `.no_proxy()` |
> | 7.A2 | bundled `.app` 本机 | 🟢 [change-log](../change-log/2026-05-14-managed-rwkv-bundle-path-resolution.md) — 修 sidecar 文件名 trim + resources 路径 |
> | 7.A.C | Settings 代理输入框 | 🟢 [change-log](../change-log/2026-05-14-managed-rwkv-download-proxy-setting.md) — store 持久化 + 透传到 install reqwest |
> | 7.A3b | 同 Mac 重置状态 Finder 启动 | 🟢 双击 .app → 填代理 → 下载 → 启动 → 翻译 → 导出 全链路通过 |
> | 7.A3a | 全新 Mac / 新用户 | ⬜ — 用户另一台 Mac，主要验 Gatekeeper / Developer ID signing |
> | 7.B | codesign + notarize + DMG + updater 私钥 | ⬜ **阻塞中** — 公司 Apple ID 2FA 需要管理员配合，暂缓 |
> | 8 | Windows / Intel Mac | ⬜ 不在本里程碑 |
>
> **下一步建议**：7.A3a（第二台 Mac 验 Gatekeeper）或 7.B（codesign）。Phase 6 全部完成；Phase 7.A 核心交付物已稳定通过自身机 + bundled 双重验证；剩下的差距是真实 Mac 跨机 + Apple Developer 签名链。
>
> **本机当前实机状态**（Mac mini M4）：
> - Sidecar 二进制已暂存到 `rosetta-app/src-tauri/binaries/`，分词表在 `rosetta-app/src-tauri/resources/rwkv-sidecar/`。
> - 模型软链 `~/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/RWKV_v7_G1c_..._nf4.prefab` → `/Users/leolin/rwkv-test/models/<同名>`。
> - 用 `pnpm tauri dev` 启动后，Settings → "本地 RWKV 翻译" → "启动本地翻译" → Jobs 页可以跑实际翻译。

## Context

Rosetta v1 的目标是让 macOS Apple Silicon 用户**不开终端、不装 Python、不点 GitHub**，从 UI 一键安装本地 RWKV 翻译引擎并跑长文档翻译。

之前的计划 [`2026-05-12-macos-apple-silicon-rwkv-runtime.md`](2026-05-12-macos-apple-silicon-rwkv-runtime.md) 假设可以直接用 `rwkv-mobile` 的 MLX 后端 + 上游预编译 server 二进制。**2026-05-13 调研发现该假设不成立**：

- `MollySophia/rwkv-mobile` CI 只发布 `librwkv_mobile.dylib`（库），**没有 `rwkv_server` 预编译产物**，需要我们自己 CI 编译。
- HuggingFace 上 `mollysama/rwkv-mobile-models` 的 `mlx/` 目录已改名为 `coreml/`，**当前没有 MLX 格式的翻译权重**。
- 翻译模型仅支持 **EN↔ZH 双向**。

注：rwkv-mobile 与 Rosetta 属同一家公司同事项目，分发授权不是阻塞项，但 LICENSE 仓库可见性问题建议内部走个流程补上（不在本计划范围）。

本计划采用：
- **后端：从源码自建 rwkv-mobile + WebRWKV（基于 wgpu/Metal）**。绕开 MLX 的 Swift 工具链依赖与缺权重问题，仍复用上游 `/v1/batch/chat`、`/v1/chat/roles`、`/v1/batch/supported_batch_sizes` 端点。
- **模型：`RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`**（实测 1.3 GB nf4 量化，WebRWKV 兼容，Apache-2.0；最初估算"~600MB"是 Phase 0 前的猜测，实际值已写入 `profile.rs::MACOS_ARM64_WEBRWKV.model_size_bytes`）。
- **形态：独立 sidecar 进程**，Tauri 通过 stdout/health 监督，HTTP 仅绑定 `127.0.0.1`。

预期成果：M1 8GB 起步的 Apple Silicon Mac，安装 Rosetta、点一次"安装本地翻译"、等一次 ~1.3 GB 下载、看到"就绪"、即可翻译。

## Strategic Risks & Gates

| 风险 | 应对 | 阻塞阶段 |
| --- | --- | --- |
| WebRWKV Metal 后端的批量翻译稳定性未验证 | Phase 0 在 M1/M2/M3 上跑批量基准 | Phase 0 退出条件 |
| `/v1/chat/roles` 是全局状态 | v1 单运行时单方向；切换方向先 stop 再 start，或 issue 新 roles 调用 | 写入适配器约定 |
| 上游 rwkv-mobile master 推进可能引入 API breaking | sidecar 构建固定到验证通过的 commit，升级走内部沟通而非自动拉 master | Phase 2 写入 workflow 注释 |
| macOS Gatekeeper / 公证 | sidecar 与 dylib 必须 codesign + notarize；clean Mac 测试 | Phase 7 退出条件 |
| Intel Mac 不支持 | UI 明确显示"仅 Apple Silicon"，回落外部 API | Phase 5 设计 |

## Architecture

```
Rosetta UI (React)
  └─ TranslationRunner (lib/translationRunner.ts) — segment 调度 + 取消
      └─ ProviderRouter (新) — 选 rwkv-lightning-contents / rwkv-mobile-batch-chat / custom-rwkv-api
          ├─ rwkv-api.ts (现有 contents[] connector，保留)
          └─ rwkv-mobile-adapter.ts (新)
              └─ Tauri command
                  └─ RwkvRuntime (src-tauri/src/rwkv_runtime.rs 改造)
                      ├─ artifact install (复用现有 SHA256/manifest 框架)
                      ├─ sidecar lifecycle (复用现有 spawn/probe)
                      └─ provider profile (新：macOS-arm64-webrwkv vs windows-amd64-libtorch)
                          └─ sidecar process: rwkv_server --backend webrwkv ...
                              └─ 127.0.0.1:<ephemeral port>
                                  /health
                                  /v1/batch/chat
                                  /v1/chat/roles
                                  /v1/batch/supported_batch_sizes
```

三个边界严格分开：document pipeline ↔ provider adapter ↔ managed runtime。

## Implementation Phases

### Phase 0 — Upstream Contract Validation （阻塞所有后续） 🟢

**目标**：在写产品代码前，把"能不能跑、跑得多快、日志安不安全"这几件事钉死。

任务：
1. 在内部 macOS arm64 机器上手动跑通完整链路：
   ```bash
   git clone https://github.com/MollySophia/rwkv-mobile.git --depth 1
   cd rwkv-mobile
   mkdir build && cd build
   cmake .. -DENABLE_WEBRWKV_BACKEND=ON -DENABLE_MLX_BACKEND=OFF \
            -DBUILD_EXAMPLES=ON -DENABLE_SERVER=ON \
            -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_POLICY_VERSION_MINIMUM=3.5
   cmake --build . -j 8
   # 验证 build/examples/rwkv_server 存在
   ./examples/rwkv_server \
     --model /path/to/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab \
     --tokenizer ../assets/b_rwkv_vocab_v20230424.txt \
     --backend webrwkv --host 127.0.0.1 --port 8765
   ```
2. 手动调验 4 个端点：
   - `GET /health` 返回 200
   - `POST /v1/chat/roles` 设置 `{"user_role":"English","assistant_role":"Chinese"}`
   - `GET /v1/batch/supported_batch_sizes` 返回数组
   - `POST /v1/batch/chat` 用 8 条短句跑出正确顺序 + 中文译文 + `choices[].index`
3. 取消测试：发起 batch 后 SIGINT / 关 HTTP 连接，观察进程是否清理。
4. 日志审计：默认日志级别下确认**不打印** prompt 文本和译文（同事项目可直接改源码，不必走 issue）。
5. 基准测试矩阵（M1 8GB、M3 16GB 至少各一台）：
   - segment 长度桶 100/300/600/1000/1500/2000 字符
   - batch size 扫描 `/v1/batch/supported_batch_sizes` 报告的值
   - 记录：成功率、超时率、空译文率、单批延迟、字符/秒。

**退出条件**：
- 1.5B nf4 模型在 M1 8GB 上能起来且不 OOM。
- batch chat 端到端跑通，choices index 与请求顺序一一对应。
- 日志默认无敏感文本（或锁定需要打的 patch）。
- 基准结果落表，作为 Phase 6 batch size 默认值的依据。

**交付物**：[`2026-05-13-rwkv-mobile-macos-validation-notes.md`](2026-05-13-rwkv-mobile-macos-validation-notes.md)（验证结果与基准数字）。

### Phase 1 — Provider Adapter Split 🟢

**目标**：解耦 `rwkv-lightning-contents`（现 Windows 外部 API 路径）和 `rwkv-mobile-batch-chat`（macOS 新路径），让 runner 写一次、跑任意 provider。

修改：
- 新建 [`rosetta-app/src-tauri/src/rwkv_providers/`](../../../rosetta-app/src-tauri/src/) 模块：
  - `mod.rs` — provider trait `RwkvTranslationProvider` 定义 probe/translate/cancel。
  - `lightning_contents.rs` — 把 [`rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs) 当前的 `/v1/chat/completions` + `contents[]` 逻辑搬过来，作为 `rwkv-lightning-contents` 实现。
  - `mobile_batch_chat.rs` — 新 `rwkv-mobile-batch-chat` 实现，映射 Rosetta segment → `conversations[].messages[]`，从 response `choices[].index` 还原顺序。
- 缩窄 [`rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs) 至 dispatcher 层，对前端命令保持向后兼容。
- 前端：
  - 新建 `rosetta-app/src/lib/providers/`，提供 `selectProvider(runtimeStatus, config)` → providerId。
  - [`rosetta-app/src/lib/translationRunner.ts`](../../../rosetta-app/src/lib/translationRunner.ts) 改为接收 `provider: ProviderHandle`，不再硬编码 endpoint。
  - [`rosetta-app/src/lib/rwkvApi.ts`](../../../rosetta-app/src/lib/rwkvApi.ts) 保持现 API，新增 `translateRwkvBatchChat()` 给 mobile-batch-chat 用。
- TypeScript 类型：[`rosetta-app/src/types/rosetta.ts`](../../../rosetta-app/src/types/rosetta.ts) 增 `RwkvProviderId = "rwkv-lightning-contents" | "rwkv-mobile-batch-chat" | "custom-rwkv-api"`。

**退出条件**：
- 现有外部 API 流仍工作（旧 connector 行为不变）。
- 手动指向本地 Phase 0 的 sidecar，能通过新 mobile-batch-chat 适配器跑通 probe + 8 条 batch。
- 单测覆盖 batch 响应乱序、缺失 index、空 content。

### Phase 2 — Sidecar Build Pipeline & Bundling 🟢

**目标**：让我们的 macOS CI 能产出可签名的 `rwkv_server` + `librwkv_mobile.dylib` + tokenizer。

修改：
- 新建 `.github/workflows/build-rwkv-sidecar-macos.yml`：
  - runner `macos-15`（arm64）。
  - 检出 `MollySophia/rwkv-mobile` **固定到 Phase 0 验证通过的 commit**（写入 workflow 注释）。
  - 装 cmake + Rust 工具链（WebRWKV 依赖 Rust）。
  - 构建 `-DENABLE_WEBRWKV_BACKEND=ON -DENABLE_MLX_BACKEND=OFF -DENABLE_SERVER=ON`。
  - 产出 tarball：`rwkv-sidecar-macos-arm64-<commit>.tar.gz` 内含 `rwkv_server`、`librwkv_mobile.dylib`、`b_rwkv_vocab_v20230424.txt`、`MANIFEST.json`（commit、build flags、SHA256）。
  - 上传到本仓库 release assets。
- 修改 [`rosetta-app/src-tauri/tauri.conf.json`](../../../rosetta-app/src-tauri/tauri.conf.json)：
  - `bundle.targets` 加入 `"app", "dmg"`（macOS）。
  - `bundle.externalBin` 指向 `binaries/rwkv-server-aarch64-apple-darwin`（Tauri sidecar 命名约定）。
  - `bundle.resources` 加入 dylib 与 tokenizer 路径。
  - `bundle.macOS.minimumSystemVersion` 设为 `"13.0"`。
- 新建 `rosetta-app/src-tauri/binaries/.gitkeep` + `scripts/fetch-rwkv-sidecar.sh`：在 `pnpm tauri build` 前从 release assets 拉取并校验 SHA256，放到 `binaries/`。
- macOS codesign + notarize 配置（写入 workflow 而非 conf）。

**退出条件**：
- `pnpm tauri build` 在 clean macOS arm64 上能产出 `.app` / `.dmg`。
- 解压 `.app` 后 `Contents/Resources/_up_/binaries/rwkv-server-aarch64-apple-darwin` 可执行且签名有效（`codesign --verify --deep --strict --verbose=4`）。
- 在 clean Mac 上 Gatekeeper 不拦截。

### Phase 3 — Tauri Runtime Module 改造 🟢

**目标**：把 [`rwkv_runtime.rs`](../../../rosetta-app/src-tauri/src/rwkv_runtime.rs) 从"Windows libtorch 专用"改成"按 profile 选平台"。

修改：
- 将 [`rwkv_runtime.rs`](../../../rosetta-app/src-tauri/src/rwkv_runtime.rs) 拆成：
  - `rwkv_runtime/mod.rs` — 顶层命令与公共类型（`RwkvRuntimeStatus`、`RwkvRuntimeState`、`RwkvArtifactManifest` 等保留）。
  - `rwkv_runtime/profiles.rs` — 定义 `RuntimeProfile` 结构（id、平台、binary 文件名、模型 URL/SHA256/大小、启动参数、provider id、健康端点）。
  - `rwkv_runtime/profiles/macos_arm64_webrwkv.rs` — 新 profile：sidecar 走 `bundle.externalBin`（无需下载，已随 app 打包），模型走 HuggingFace 下载。
  - `rwkv_runtime/profiles/windows_amd64_libtorch.rs` — 保留现有常量但置 `enabled: false`（Phase 8 再恢复）。
  - `rwkv_runtime/install.rs` — 现有 download/SHA256/extract 逻辑参数化为接收 profile。
  - `rwkv_runtime/process.rs` — 现有 spawn/pid/probe 逻辑参数化。
- 端口选择：用 `std::net::TcpListener::bind("127.0.0.1:0")` 抢一个 ephemeral 端口再让出，写入 `runtime-state/active-runtime.json`。
- 兼容性检查：`#[cfg(target_os = "macos")]` + 运行时 `std::env::consts::ARCH == "aarch64"`，否则 status 返回 `unsupported`。
- 在 [`lib.rs`](../../../rosetta-app/src-tauri/src/lib.rs) 的 `invoke_handler!` 中注册：
  - `rwkv_runtime::get_managed_rwkv_runtime_status`
  - `rwkv_runtime::get_managed_rwkv_install_plan`
  - `rwkv_runtime::install_managed_rwkv_runtime`（下载模型）
  - `rwkv_runtime::start_managed_rwkv_runtime`
  - `rwkv_runtime::stop_managed_rwkv_runtime`
  - `rwkv_runtime::probe_managed_rwkv_runtime`
  - `rwkv_runtime::get_managed_rwkv_runtime_logs_summary`
- App data 布局（在 `app_local_data_dir()` 下）：
  ```
  Rosetta/
    runtimes/      ← 当前 macOS 路径下不下载，sidecar 来自 bundle resources
    models/
      rwkv-translate-1.5b-nf4/
        manifest.json
        model.prefab
        SHA256
    runtime-state/
      active-runtime.json
      runtime.pid
    logs/
      runtime.log
  ```
- 取消 [`lib.rs`](../../../rosetta-app/src-tauri/src/lib.rs) 上的 `#[allow(dead_code)]`。

**退出条件**：
- `cargo check` 通过；`cargo test rosetta_jobs` 不退步。
- 命令在 Intel Mac / Windows 调用返回 `unsupported` 状态，不 panic。
- `start_managed_rwkv_runtime` 能起 sidecar、`probe` 通过、`stop` 干净退出（无僵尸进程）。

### Phase 4 — 模型下载与首次安装流程 🟢

**目标**：把现有 Windows 下载/校验流程改造为 macOS 模型专用，从 HuggingFace 拉 nf4 prefab。

修改：
- `rwkv_runtime/install.rs`：
  - 复用现有 reqwest 流式下载 + 进度回调。
  - 模型 URL 写入 `macos_arm64_webrwkv.rs`：`https://huggingface.co/mollysama/rwkv-mobile-models/resolve/main/<actual path>/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`（Phase 0 验证后写入精确路径与 SHA256）。
  - 镜像选择：保留 `mirror_priority: ["huggingface", "modelscope"]`，运行时探测哪个可达（HuggingFace 在国内常超时；现有 Windows 代码已用 ModelScope，我们可以保持此偏好）。
- 失败处理：partial download 写到 `model.prefab.part`，校验失败保留 `.part.broken` 并显式提示用户。
- manifest 写入完成时间、源 URL、SHA256、文件大小、用于的 provider id。

**退出条件**：
- 模拟掉线后 resume 能继续。
- SHA256 不匹配时 status 走 `failed: artifact-corrupted` 并允许 repair（删后重下）。

### Phase 5 — UI 一键安装与就绪门控 🟢

**目标**：用户只点一个按钮、看一条进度条，主工作台不被模型管理噪音污染。

修改：
- 把 [`rosetta-app/src/lib/rwkvRuntime.ts`](../../../rosetta-app/src/lib/rwkvRuntime.ts) 的所有 stub 换成真实 `invoke()` 调用，并新增 `installManagedRwkvRuntime()`。
- 新建 `rosetta-app/src/features/settings/LocalRwkvPanel.tsx`：
  - 状态卡片：`未安装 / 下载中 X% / 安装中 / 就绪 / 已停止 / 失败 / 不支持`。
  - 主操作按钮：单一动词（安装 / 启动 / 停止 / 修复）。
  - 模型信息行：模型版本、大小、所在目录、最后探测延迟。
  - "查看日志摘要"折叠（默认收起，显示 redacted tail）。
- [`rosetta-app/src/features/settings/SettingsPage.tsx`](../../../rosetta-app/src/features/settings/SettingsPage.tsx)：
  - macOS arm64：`LocalRwkvPanel` 置顶为主路径。
  - 其他平台 / 未就绪用户：保持外部 API 配置可见。
- 首次启动检测：在 [`rosetta-app/src/app/AppShell.tsx`](../../../rosetta-app/src/app/AppShell.tsx) 加一个**克制的**onboarding banner（不是欢迎页），仅在 `unsupported=false && state=not-installed` 时显示，链接到 Settings。
- Jobs 页就绪门控：在 `rosetta-app/src/features/jobs/` 翻译入口处，若本地 runtime 已就绪则直接走本地，否则提示去 Settings 安装或切外部 API。
- store 改造：[`rosetta-app/src/store/useRosettaStore.ts`](../../../rosetta-app/src/store/useRosettaStore.ts) 加 `managedRuntime: { status, progress, lastError }` 切片（**不**写入 `persist()` 白名单，每次启动重新探测）。

**退出条件**：
- 非技术用户从全新 Mac 开始：开 Rosetta → Settings → 安装 → 进度条 → 就绪。全过程不开终端。
- 失败态有可读文案 + 修复按钮。
- 主文档工作台无运行时管理控件。

### Phase 6 — 翻译集成 ⬜ 下一站

**目标**：把"能跑"升级为"跑得稳跑得快跑得规矩"。Phase 5 已经端到端跑通；Phase 6 是把临时常量替换成 dynamic 行为 + 加回归 fixture + 收口取消/重试语义。

**已知踩坑**（继承自前期实测，**Phase 6 必须解决**）：

- Phase 5 端到端验证踩到 `chat_batch: batch size 16 is not supported`，临时 clamp 到 [JobsPage](../../../rosetta-app/src/features/jobs/JobsPage.tsx) `LOCAL_RUNTIME_BATCH_SIZE = 8` 与 [rwkv_api.rs](../../../rosetta-app/src-tauri/src/rwkv_api.rs) `SIDECAR_MAX_BATCH_SIZE = 12`，详见 [batch-size-clamp change-log](../change-log/2026-05-13-managed-rwkv-batch-size-clamp.md) 与 [memory: project-rwkv-mobile-batch-size-limit](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_batch_size_limit.md)。两处常量在 Phase 6 dynamic batch 落地后可删除。
- 响应解析按 `assistant_role` 动态切分 `<原文>\n\n<lang>:` 前缀已在 Phase 1.A 落地（[mobile_batch_chat.rs](../../../rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs) `strip_response_prefix` + 单测），ZH→EN 实测已 work。Phase 6 不需要再动。

**Phase 6 修改清单**：

- [`rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs`](../../../rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs) 或新文件 — `query_supported_batch_sizes(config) -> Vec<u32>`，进程级缓存（重启 sidecar 时清）。
- [`rosetta-app/src-tauri/src/rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs) `start_mobile_batch_chat_run`：
  - 启动时调一次 `query_supported_batch_sizes`，取数组最大值（或按 segment 长度桶选小一些）。
  - 删掉 `SIDECAR_MAX_BATCH_SIZE = 12` 硬编码（dynamic 接管后冗余）。
  - 单运行时单方向：start run 时调一次 `/v1/chat/roles`，整个 run 不重复设置（当前每个 batch 都重设，浪费）。
- [`rosetta-app/src/features/jobs/JobsPage.tsx`](../../../rosetta-app/src/features/jobs/JobsPage.tsx)：
  - 删掉 `LOCAL_RUNTIME_BATCH_SIZE = 8` 临时常量，把决策权交给 Rust。
- 取消语义重审：当前 `stop_managed_rwkv_runtime` 是 SIGKILL；翻译 run 取消是 abort HTTP。两者衔接确认（特别是 stop 时是否要先取消 in-flight run）。
- 回归 fixture：固化一篇 ~50 段 Markdown（中英混排 + 列表 + 代码块）+ ~20 段 TXT 进 `docs/engineering/fixtures/`（或 `rosetta-app/src-tauri/tests/`），每次大改后人工跑一遍。
- 性能可观测性：sidecar log 可视化 segment 完成数 + 平均延迟 + 失败率（已在 LocalRwkvPanel 模型信息行预留位置）。
- Phase 0 未完成的三项（cancel 测试、日志审计、M1/M2 基准）至少补完前两项作为 release gate。

**验证**：翻译 ~50 段 Markdown + ~20 段 TXT，全程进度更新、中途取消、再续跑、导出与现有 pipeline 一致。

**退出条件**：
- 长文档 EN↔ZH 双向端到端通过。
- segment 顺序在批量乱序下仍稳定（用 `choices[].index` 还原 — Phase 1 已实现，回归验证不退步）。
- 取消 → 续跑 → 导出无段落丢失或错位。
- preview / export 不需要任何运行时特殊分支。
- `BATCH_SIZE` / `LOCAL_RUNTIME_BATCH_SIZE` / `SIDECAR_MAX_BATCH_SIZE` 三个临时常量删除或注释清楚为何保留。

### Phase 7 — 发布加固 ⬜

**目标**：clean Mac 上装得上、跑得动、卸得掉。

任务：
- macOS codesign 整链路（app + sidecar + dylib）。
- notarize + staple，Gatekeeper 在 SIP 全开机器上不弹拦截。
- 升级路径：从 0.1.x → 0.2.x，已下载的模型保留、runtime 自动指向新 sidecar。
- 损坏修复：手动删除 `model.prefab` 后 status 自动回退到"未安装"。
- 卸载语义：删除 .app 后 `~/Library/Application Support/Rosetta/` 是否清理（默认保留，UI 提供"清理本地模型"按钮，未来加）。
- 隐私自审：grep 所有日志路径，确认 segment 文本不进 `runtime.log` 或 manifest。

**退出条件**：
- clean macOS Apple Silicon Mac 装 dmg、点安装、跑长文翻译，全程不出现命令行或权限弹窗。
- 日志中不含源/译文。
- 升级 + 卸载行为符合预期。

### Phase 8 — （非本次范围）Windows / Intel Mac

- Windows libtorch 路径（现 `windows_amd64_libtorch.rs` 留作恢复入口）。
- Intel Mac：等待 web-rwkv 或 llama.cpp Metal-x86 路径成熟。
- 文档：留作未来 ADR/plan，不在本次 PR 范围。

## 现有可复用资产

| 用途 | 现有位置 | 复用方式 |
| --- | --- | --- |
| SHA256 / zip 解压 / manifest | [`rwkv_runtime.rs`](../../../rosetta-app/src-tauri/src/rwkv_runtime.rs) | 参数化为接收 profile，不重写 |
| 进程 spawn / pid 管理 / 日志尾读 | [`rwkv_runtime.rs`](../../../rosetta-app/src-tauri/src/rwkv_runtime.rs) | 抽到 `process.rs` 复用 |
| 流式下载 + 进度 | reqwest 已配置 | 直接复用 |
| 翻译 run 调度 / 取消 / 状态轮询 | [`rwkv_api.rs`](../../../rosetta-app/src-tauri/src/rwkv_api.rs) + [`translationRunner.ts`](../../../rosetta-app/src/lib/translationRunner.ts) | 拆成 provider-neutral runner + 适配器，保留外层 API |
| 前端 runtime 命令桥 | [`lib/rwkvRuntime.ts`](../../../rosetta-app/src/lib/rwkvRuntime.ts) | 直接换实现，签名稳定 |
| 设置页 RWKV 配置区 | [`SettingsPage.tsx`](../../../rosetta-app/src/features/settings/SettingsPage.tsx) | 嵌入新 `LocalRwkvPanel`，旧外部 API 配置保留为 fallback |
| Zustand 持久化 store | [`useRosettaStore.ts`](../../../rosetta-app/src/store/useRosettaStore.ts) | 加 managedRuntime 切片（非持久化） |
| 数据模型（segment/translation file/revision） | [`rosetta_jobs/model.rs`](../../../rosetta-app/src-tauri/src/rosetta_jobs/model.rs) | 不动 |

## ADR 与文档跟进

- 新建 [`0003-macos-first-managed-rwkv-runtime.md`](../decisions/0003-macos-first-managed-rwkv-runtime.md)：记录 macOS-first、WebRWKV 后端、sidecar 模型、provider 抽象、Windows 延后等决策。
- 在 [`0002-pause-managed-rwkv-runtime.md`](../decisions/0002-pause-managed-rwkv-runtime.md) 末尾加 supersession note 指向 0003，**不删历史**。
- 每个 Phase 落地时写一条 change-log，路径 `docs/engineering/change-log/2026-05-XX-rwkv-macos-phase-N-*.md`。
- Phase 0 验证笔记单独成文：[`2026-05-13-rwkv-mobile-macos-validation-notes.md`](2026-05-13-rwkv-mobile-macos-validation-notes.md)。

## Verification

每个 Phase 完成时跑：

```bash
cd rosetta-app
pnpm typecheck
pnpm lint
cd src-tauri
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```

集成验证（人工 + 半自动）：

1. **Phase 0**：手动 README 中命令链路，记录基准 CSV。
2. **Phase 1**：在 Settings 手填本地 sidecar URL（绕过 managed），跑 8 段 batch；外部 API 路径保持工作。
3. **Phase 3–4**：调用 `start_managed_rwkv_runtime` → `probe_managed_rwkv_runtime` → `stop_managed_rwkv_runtime`，确认进程清理。
4. **Phase 5**：人工首次安装走查；模拟掉线、SHA256 损坏、不支持平台三个分支。
5. **Phase 6**：固定一篇 50 段中文 / 50 段英文 Markdown 作为回归 fixture，每次跑端到端翻译；diff 输出。
6. **Phase 7**：clean Mac VM（或新建用户账户）装 dmg → 完整跑通。Gatekeeper / Quarantine 全程不弹拦截。

不在每次 PR 跑：`pnpm tauri build`（仅 release workflow）、长文档大模型翻译（仅 release candidate）。
