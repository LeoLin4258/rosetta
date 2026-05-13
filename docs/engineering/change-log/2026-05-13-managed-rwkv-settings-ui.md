# 2026-05-13 Managed RWKV Settings UI + Jobs Readiness Gate (Phase 5)

## Context

Phase 0–4 把后端管道铺齐了：[Phase 0](../plans/2026-05-13-rwkv-mobile-macos-validation-notes.md) 验证 sidecar、[Phase 1](2026-05-13-rwkv-provider-adapter-split.md) 拆 provider 抽象、[Phase 2](2026-05-13-rwkv-sidecar-build-pipeline.md) 接 Tauri bundle、[Phase 3](2026-05-13-managed-rwkv-runtime-macos.md) 写 lifecycle 命令、[Phase 4](2026-05-13-managed-rwkv-model-install.md) 接模型下载。但所有这些用户看不见——`installManagedRwkvRuntime` / `startManagedRwkvRuntime` 等命令必须从 UI 里调。

Phase 5 接上"傻瓜式"那一面：Settings 页加一个 `LocalRwkvPanel`，AppShell 加一条克制的 onboarding banner，Jobs 页接 readiness gate 让翻译自动选 provider。

## Changes

### `useRosettaStore` 新增 `managedRuntime` slice

[`src/store/useRosettaStore.ts`](../../../rosetta-app/src/store/useRosettaStore.ts)：

- 新加 `ManagedRuntimeSlice` 类型：`status` / `progress` / `lastError` / `bannerDismissed`。
- 新增 4 个 setter：`setManagedRuntimeStatus` / `setManagedRuntimeProgress` / `setManagedRuntimeError` / `dismissManagedRuntimeBanner`。
- **不**写入 `partialize`/`merge`：每次 Rosetta 启动重新探测，避免持久化的"假就绪"骗到用户。`bannerDismissed` 也是 session 内有效——下次启动再次提醒。

### `useManagedRwkvRuntime` hook

[`src/lib/useManagedRwkvRuntime.ts`](../../../rosetta-app/src/lib/useManagedRwkvRuntime.ts)：

- mount 时拉一次 `get_managed_rwkv_runtime_status` + 订阅 `managed-rwkv://install-progress` 事件，全部写进 store。
- 暴露 5 个 action 包装：`install(options?)` / `cancelInstall()` / `start()` / `stop()` / `probe()` / `readLogs()`，每个都做错误捕获 + 安装类动作自动 `refreshStatus()`。
- 暴露 `isManagedRuntimeReady(status)` 选择器，给 Jobs 页就绪门控复用。

### `LocalRwkvPanel` 组件

[`src/features/settings/LocalRwkvPanel.tsx`](../../../rosetta-app/src/features/settings/LocalRwkvPanel.tsx)：

- 单一卡片，title/description 跟随 `ManagedRuntimeState`（`unsupported` / `not-installed` / `installed` / `starting` / `ready` / `failed` / `stopped` + 安装进行中状态）。
- header badge 直观显示状态（`运行中` / `安装中` / `未安装` / `仅支持 Apple Silicon` 等）。
- **单一主操作按钮**：根据状态切动词——`安装` / `启动` / `停止` / `修复并重试` / `取消下载` / `重新校验模型`。绝不一次摆出三个操作按钮。
- 安装进行中：自定义 Tailwind 进度条（百分比 + 速度 MB/s + 当前阶段文案），下方"取消下载"按钮 + "下次安装将自动从断点续传"提示。
- 模型信息行：filename + 大小 + SHA-256 前 16 位 + 模型/日志路径 + baseUrl + PID（已就绪时）。
- 日志摘要：默认收起的 `Collapsible`，展开调 `get_managed_rwkv_runtime_logs_summary` 读尾部 8 KB。
- 全 zh-CN 文案、shadcn/ui 组件、lucide-react icons，跟 SettingsPage 现有风格一致。

### `SettingsPage` 整合

[`src/features/settings/SettingsPage.tsx`](../../../rosetta-app/src/features/settings/SettingsPage.tsx)：

- 在"外部翻译 API"section 之前插入 `<LocalRwkvPanel />`，本地优先。
- 原"翻译服务"section 改名"外部翻译 API"，header 描述改为"远程 / 自部署 RWKV API，可作为本地翻译未就绪时的回落方案"，让两者关系清晰。
- 删掉旧的"本地模型 · 即将支持"占位 section + 不再用的 `ServerOff` icon import。

### AppShell onboarding banner

- 新建 [`src/features/settings/ManagedRwkvOnboardingBanner.tsx`](../../../rosetta-app/src/features/settings/ManagedRwkvOnboardingBanner.tsx)：单行、可关闭，仅在 `status?.state === "not-installed"` 且不在 `/settings` 路由时显示；带"去设置安装"链接（指向 `/settings#local-rwkv` 锚点）。
- [`src/app/AppShell.tsx`](../../../rosetta-app/src/app/AppShell.tsx)：mount 时拉一次 `getManagedRwkvRuntimeStatus` 写入 store（让 banner / Jobs 页都能从同一 source 读到状态），在 `<Outlet />` 上方挂 `<ManagedRwkvOnboardingBanner />`。

### Jobs 页就绪门控

[`src/features/jobs/JobsPage.tsx`](../../../rosetta-app/src/features/jobs/JobsPage.tsx)：

- 从 store 读 `managedRuntime.status`，计算 `managedRuntimeReady`。
- `rwkvConfigReady = managedRuntimeReady || isRwkvConfigReady(rwkv)`：本地就绪或外部 API 配置就绪都能解锁翻译按钮。
- 每次翻译开始前调 `selectProvider(...)`：本地就绪走 `rwkv-mobile-batch-chat` 指向 sidecar baseUrl；否则走 `rwkv-lightning-contents`。
- `runTranslationBatches({ provider, ... })`：Phase 1 已加的 `provider` optional 参数现在被真正用上了。
- **没有**在 Jobs 页加任何运行时管理控件——Phase 5 退出条件要求"主文档工作台无运行时管理控件"。引导都走 Settings + AppShell banner。

## Compatibility

- `cargo test --lib`：95 个测试全过（Phase 5 没改 Rust）。
- `pnpm typecheck`：通过。
- 旧用户：本地未就绪时翻译完全走外部 API（与 Phase 4 之前行为一致）；本地就绪后 `selectProvider` 自动切到 mobile-batch-chat，但 UI 体验保持原样。
- 持久化：`managedRuntime` 没进 `partialize`，**不**污染 `rosetta-app-settings` localStorage 键。已有用户不会因为升级而看到陌生状态。
- 没有 i18n 改动，文案沿用 zh-CN。

## Known Boundary

- **Banner / Settings 状态分别拉取**：AppShell 在 mount 时拉一次 status；用户进 Settings 时 `useManagedRwkvRuntime` 再拉一次 + 订阅事件。结果是同一个事件可能被两个 listener 处理（AppShell 自己不订阅事件，只在 mount 拉初始 status；进入 Settings 后才有事件订阅）。无 leak，但 Settings 关闭后 banner 不会通过事件刷新——需要靠 AppShell 重新 mount 或下一次显式刷新。Phase 6 / Phase 7 可以把全局事件订阅提到 AppShell 层。
- **Banner dismiss 不持久化**：刻意的——下次启动如果还是 not-installed，仍然提示。如果用户彻底不想用本地翻译，最好的"屏蔽"方式是把外部 API 配置好（自然 banner 仍然在，但用户已经能用翻译）。Phase 6 可考虑把"我永久不想看到这条提示"做成 `persist()` 项。
- **就绪门控只在每次 run 重算一次**：意味着翻译运行**中途**如果本地 runtime 崩了，当前批次仍然继续往 sidecar 发请求，直到失败回退。这个行为与外部 API 中途断网完全一致，不是 Phase 5 引入的新问题。Phase 6 翻译集成时会再确认。
- **打开 Settings 之前不会显示模型信息行**：AppShell 拉的 status 是 backend 已知信息，但 LocalRwkvPanel 自己挂载时 `useManagedRwkvRuntime` 会再拉一次（fresh）。无副作用，只是首次 render 比理论上慢一个 RTT。
- **未做 i18n**：UI 文案直接写在组件里。Rosetta 当前全 zh-CN，没有 i18n 框架；引入要等更大范围的多语言支持。
- **没有 Phase 5 Rust 改动**：所有命令在 Phase 3 / Phase 4 已经就位；Phase 5 只是消费它们。`cargo test` 仍然 95 通过。

## Verification

- `pnpm typecheck`：通过。
- 完整 Rust 测试 95 个仍过；`cargo check` clean。
- 手工 walk-through（在源码层级 review；live e2e 由用户在 Apple Silicon 上 `pnpm tauri dev` 试运行验证）：
  1. Phase 0 sidecar binary 已通过 fetch script stage 到 `src-tauri/binaries/`，本机模型在 `~/rwkv-test/models/` —— 用户可手动 `ln -s` 到 `~/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv-translate-1.5b-nf4/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab` 让 install plan 直接报"已就绪"，验证启动 → 探活 → 翻译完整链路。
  2. Settings → "本地 RWKV 翻译" panel：状态、进度、操作按钮、日志摘要逐项检查。
  3. AppShell banner：not-installed 时显示；点"去设置安装"跳 `/settings#local-rwkv`；点关闭按钮 session 内不再显示。
  4. Jobs 页：本地就绪后 Translate 按钮直接用本地（去观察 Tauri 后台进程的 stdout/network），外部 API 字段空填仍能工作。

## Next

Phase 6 — 翻译集成与端到端验证。重点：
- batch size 调度按 segment 长度桶动态选取，调 `/v1/batch/supported_batch_sizes` 缓存。
- run 开始时调一次 `/v1/chat/roles`，整个 run 内同方向。
- 取消语义：当前 sidecar 是 SIGKILL，需要确认是否要先关 HTTP 连接再 kill 以让 in-flight 翻译干净退出。
- 用 50 段 Markdown + 50 段 TXT 跑端到端回归。
