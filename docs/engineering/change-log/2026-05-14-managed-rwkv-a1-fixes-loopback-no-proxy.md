# 2026-05-14 Managed RWKV A1 Fixes: Loopback `.no_proxy()` + lastError Visibility

## Context

Phase 7.A 第一关验证（[plan](../plans/2026-05-13-macos-rwkv-one-click-implementation.md)
里的"从零状态走完整安装流程"）。用户在 dev 环境删掉模型软链、点"安装本地翻译模型"，
触发真实 HF 下载 → SHA256 校验 → manifest 写入 → 启动 sidecar → 翻译。

这次 dogfood 是从 Phase 4/5 落地以来**第一次让 Phase 4 的下载路径在 live 环境跑**
（之前我们一直用软链跳过下载）。结果暴露了两个真实 bug——都不在测试覆盖里，
都是真用户场景必踩。

## Bugs Found & Fixed

### 1. `lastError` 被 `refreshStatus` 自动清空，UI 吞掉错误

之前 [store 实现](../../../rosetta-app/src/store/useRosettaStore.ts) `setManagedRuntimeStatus`
里有一行 `lastError: status ? null : state.managedRuntime.lastError` —— 意思是
status 每次成功刷新就把 lastError 清掉。

但 install 失败的流程恰好是：
1. `useManagedRwkvRuntime.install()` 的 catch 抓到 Err → `setError(message)`
2. 然后立刻调 `await refreshStatus()` 更新 status
3. `refreshStatus` 成功 → `setManagedRuntimeStatus(status)` → **lastError 被清**
4. UI 红 banner 永远显示不出来，用户只看到按钮闪一下回到"安装"

**修复**：[store](../../../rosetta-app/src/store/useRosettaStore.ts) `setManagedRuntimeStatus`
不再触碰 `lastError`。错误的生命周期由 action handler 负责（每个 install/start/stop
开始时 `setError(null)` 一次，失败时 `setError(message)`）。

### 2. Loopback reqwest 走代理 → 启动卡 `/health` 45 秒超时

更隐蔽的 bug。reqwest 默认读 `HTTPS_PROXY` / `HTTP_PROXY` 环境变量。国内用户为了
让 HuggingFace 下载走 Clash 代理（[memory: project-rwkv-mobile-cn-build-gotchas](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_cn_build_gotchas.md)）
会 `export https_proxy=http://127.0.0.1:7897`。然后**所有** Rosetta 发出的 HTTP
请求都被 reqwest 试图走 Clash 代理——包括 sidecar 本机 `http://127.0.0.1:<ephemeral>/health`。
Clash 拒绝 / hang loopback 请求，`wait_for_health` 45 秒超时，UI 报"/health 在
45 秒内未就绪。"

sidecar 本身已经成功启动并打印 `ModelInfo`，只是 Rosetta 永远连不上它。

**修复**：所有打**本机 sidecar**的 reqwest builder 加 `.no_proxy()`：

- [`managed_rwkv/lifecycle.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs)
  `wait_for_health`、`probe_sidecar`。
- [`rwkv_providers/mobile_batch_chat.rs`](../../../rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs)
  `query_supported_batch_sizes`、`set_chat_roles_for_pair`、`translate_batch`。
  抽出 `loopback_client(timeout_ms)` helper 消除三处重复 + 注释解释为什么必须
  `.no_proxy()`。

**保留**走代理的：[`install.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/install.rs)
`download_from_mirror` 仍然继承 env 代理——下载 HF 是远程请求，必须走 Clash 才能
跨 GFW。

### 3. Install 路径加 `[rwkv-install]` mirror trace

辅助诊断。`download_from_mirror` 每次尝试镜像 / 成功 / 失败 / fatal / 镜像耗尽
都打一行带前缀的 stderr。和 Phase 6 的 `[rwkv-cancel]` 同套路。后续任何下载相
关 bug 直接看 dev 终端就能定位。

## Verification

- `cargo check` / `cargo test --lib` / `pnpm typecheck`：全过（107 测试无 regression）。
- 实机 A1 端到端：删模型软链 → 重启 dev → 点安装 → 1.3 GB 下载（~1 分钟 @ 25 MB/s
  via Clash @127.0.0.1:7897）→ SHA256 校验通过 → manifest 写入 → 状态 flip 成"已安装" →
  点启动 → sidecar 加载（~10 秒）→ 状态变"运行中" → 跑 Markdown 翻译通过。

## Known Boundary

- **`HTTPS_PROXY` 还是要靠 shell export** —— 当前唯一靠 env 把代理传给 reqwest。
  Bundled `.app` 双击启动**不继承 shell 环境**，会重蹈"HF 直连超时"覆辙。
  这是 Phase 7.A2/A3 阶段的下一个必修 bug。可能方案：
  - **B**. Rust 端通过 `scutil --proxy` 或 `system-configuration` crate 检测
    macOS 系统代理，自动透传给 reqwest（仅给 install 路径用）
  - **C**. Settings 加代理输入框 + 持久化到 store + 透传 reqwest builder
  - **B + C**：默认自动检测，输入框作为覆盖
- **"修复并重试"按钮目前总是重下** —— `repair: true` 会 cleanup_artifacts。其实
  本次 A1 的失败是启动卡 /health 而不是模型坏，用户其实想要"重启 sidecar"，应
  该点"直接重启"。失败态 UI 给两个按钮容易让人误点。下一轮 UX 收尾可考虑：
  - 把"直接重启"做成主按钮、"修复并重试"做成次要 menu item
  - 或者 "修复并重试" 先 SHA256 再下载，匹配就跳过下载只重启 sidecar

## Memory Updates

无新增；已有 [project-rwkv-mobile-cn-build-gotchas](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_cn_build_gotchas.md)
里已记录 Clash 端口 7897 与各组件如何接代理的事实。后续如果决定 B/C 方案，再
追加"loopback never via proxy"这条单独的 memory 行。
