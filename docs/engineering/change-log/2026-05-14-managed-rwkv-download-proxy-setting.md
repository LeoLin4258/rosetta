# 2026-05-14 Managed RWKV Download Proxy Setting (Phase 7.A — pre-A3)

## Context

A1 / A2 都靠 shell `export HTTPS_PROXY=http://127.0.0.1:7897` 让 reqwest 透过
Clash 访问 HuggingFace。**双击启动的 `.app` 不继承 shell env**——A3 阶段（干净
用户账号 / 新 Mac）下，真用户不会先打开终端 export，模型下载会立刻 GFW reset。
ADR 0003 / Phase 7 计划承诺的"一键安装"在国内场景会卡在第一步。

本次落地最小可用的方案 C：Settings 里加一个代理输入框，存进 zustand 持久化
state，install 命令把它透传给 Rust 的 reqwest。

未做（Phase 7.B 再考虑）：自动检测 macOS 系统代理（B 方案）—— 涉及 `scutil --proxy`
或 `system-configuration` crate 适配，复杂度更高，C 已能 cover 99% 国内用户场景，
等 A3 真实反馈再决定是否值得。

## Changes

### Store

[`useRosettaStore.ts`](../../../rosetta-app/src/store/useRosettaStore.ts)：

- 新类型 `DownloadProxyConfig { url: string }`，注释说明这是**仅用于远程下载**的
  代理（loopback 走 `mobile_batch_chat::loopback_client` 永远绕开）。
- 顶层 state 增 `downloadProxy: DownloadProxyConfig`，默认 `{ url: "" }`。
- setter `setDownloadProxyUrl(url)`（trim 后写入）。
- 加进 `partialize` 白名单 + `merge` 合并逻辑——代理 URL 跨重启持久化，但用户可
  随时清空（这是私有 state，跟翻译 token / password 一起在 `rosetta-app-settings`
  localStorage 键里）。

### TS types

[`types/rosetta.ts`](../../../rosetta-app/src/types/rosetta.ts)：

`ManagedRuntimeInstallOptions` 增 `proxyUrl?: string | null`。注释明确这只影响
远程下载，不影响 loopback sidecar。

### Hook

[`useManagedRwkvRuntime.ts`](../../../rosetta-app/src/lib/useManagedRwkvRuntime.ts)：

`install(options?)` action 读 store 里的 `downloadProxy.url`，合并到 install
options 后再调 Tauri。caller 显式传 `proxyUrl` 仍然覆盖（留给将来一次性 override
的可能）。

### UI

[`LocalRwkvPanel.tsx`](../../../rosetta-app/src/features/settings/LocalRwkvPanel.tsx)：

- 新 `DownloadProxyField` 组件：`<Input>` 接 store proxy + setter，placeholder
  `http://127.0.0.1:7897`，caption 解释"仅用于下载模型，不影响本地翻译"。
- `showProxyInput(state, isInstallActive)`：只在 `not-installed` / `failed`
  / 正在 install 时展示；`ready` / `installed` 状态隐藏（避免无意义的 UI 噪音）。
- 安装进行中输入框 disabled——避免用户改一半，install 已经读了旧值。

### Rust

[`managed_rwkv/install.rs`](../../../rosetta-app/src-tauri/src/managed_rwkv/install.rs)：

- `InstallOptions` 增 `proxy_url: Option<String>` 字段 + `effective_proxy_url()`
  helper（trim + 空字符串视为 None）。
- `install_inner` 签名增 `proxy_url: Option<&str>`。
- `download_from_mirror` 同上。建 reqwest client 时若有 proxy URL，调用
  `reqwest::Proxy::all(url)` 加上去；坏 URL 走 `DownloadError::Fatal`（不重试下
  一个镜像——URL 本身错误，重试无意义）。
- 加 `[rwkv-install] mirror via proxy: <url>` trace 行，便于排查"proxy 设了但
  下载还是失败"的场景。

## Compatibility

- `cargo check` / `cargo test --lib`：107 测试全过。
- `pnpm typecheck`：通过。
- 现有用户：`downloadProxy.url` 默认空字符串 → `effective_proxy_url()` 返回 None
  → reqwest builder 不调 `.proxy()` → 行为完全等同 Phase 7.A 之前（仍然继承
  `HTTPS_PROXY` env 作为兜底）。
- 持久化：新字段加入 `partialize` 白名单。已有 zustand persisted 用户的 localStorage
  里没有 `downloadProxy` 字段 → `merge` 用 `current.downloadProxy` 默认（`{ url: "" }`）。
- Loopback 通信（`/health`、`/v1/batch/chat` 等）继续走 `loopback_client` 的
  `.no_proxy()`，**不受这个设置影响**。

## Verification

- Frontend / Rust 全部静态验证通过。
- 实机：Rosetta dev 或 bundled `.app` 都可——Settings → 本地 RWKV → 代理输入框
  填 `http://127.0.0.1:7897` → 删模型软链 / 点修复 → install。Rust 端应该看到
  `[rwkv-install] mirror via proxy: ...` 行，下载正常。

## Known Boundary

- **代理只用于 install**：当前只透传到 `download_from_mirror`。Future-proof：
  如果以后增加其它远程出口（telemetry / update server / 翻译 API），需要明确
  哪些走代理、哪些不走。loopback 的 `.no_proxy()` 已经把"绝不走代理"的边界钉死。
- **没做自动检测**：方案 B 还没做。Clash Verge 用户开了系统代理但没填这里，
  install 还是失败。这是 A3 阶段我们要看真实用户卡多少次再决定的优先级。
- **没做 URL 验证 / 健康探测**：UI 不主动测代理是否能通；用户输入完直接点安装
  才能知道。考虑过加个"测试代理"按钮但 over-engineered，先看 A3 反馈。
- **不支持代理认证**（`http://user:pass@host:port`）：reqwest::Proxy 本身支持
  basic auth URL；但 zustand 持久化会把密码明文存进 localStorage。Phase 7
  公开发版前如果有需求，再考虑 keychain 集成 + 分字段输入。

## Memory Update

考虑加一条 memory 记录"loopback never via proxy, remote always opt-in via store
field"作为后续 reqwest 改动的指引。本次先不加——等 A3 验证完整通过后一并整理。
