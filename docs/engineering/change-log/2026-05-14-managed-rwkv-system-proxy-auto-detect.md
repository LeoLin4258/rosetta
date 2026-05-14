# 2026-05-14 Managed RWKV macOS System Proxy Auto-Detect (B 方案)

## Context

A3b / A3a 现场观察：
- 用户首次进 Rosetta，Settings 里**已经放了**"下载代理"输入框，但实际感受是
  "点了安装失败之后才发现"（A3b user words）。
- AirDrop 到另一台 Mac 之后，必须先手填代理 → 才能下载 1.3 GB 模型。

技术上根因是 reqwest **只读 `HTTPS_PROXY` 环境变量**，不读 macOS 系统代理设置。
但 Clash Verge / Surge / 大多数 GUI 代理客户端的"系统代理"开关其实是把代理写
进 macOS 的 NetworkSettings，可以通过 `scutil --proxy` 读出来——这就是 macOS
所有原生 GUI app（Safari、Mail、App Store）跟代理对接的方式。

本次落地"B 方案"：reqwest 在 install 路径不再傻乎乎依赖 env，而是**主动检测**
macOS 系统代理作为兜底。99% 中国 Clash 用户的体验从"必须先去 Settings 填一个
框"变成"系统代理开着就直接装"。

未动 UI——按用户要求，UI/UX 整体待统一打磨。

## Changes

### `managed_rwkv/install.rs`

- 新增 `detect_system_proxy() -> Option<String>`（cfg = `target_os = "macos"`）：
  shell out `scutil --proxy` → 拿 stdout → 交给 `parse_scutil_proxy_output`
  纯函数解析。
- 新增 `parse_scutil_proxy_output(&str) -> Option<String>`（纯 fn，可单测）：
  - 按行 `split_once(':')` 提 key/value
  - 显式匹配 `HTTPS*` / `HTTP*` / `SOCKS*` 三组，避免 starts_with 前缀冲突
  - 优先级 **HTTPS > HTTP > SOCKS**
  - 输出格式：HTTPS / HTTP → `http://<host>:<port>`（reqwest::Proxy::all 用 CONNECT
    隧道处理两者）；SOCKS → `socks5://<host>:<port>`
- 7 个单测覆盖：真实 Clash 输出（HTTPS/HTTP/SOCKS 同端口）、全部 disabled、空输入、
  HTTPS off 时降级 HTTP、HTTP off 时降级 SOCKS、Enable 但缺 host/port 跳过、
  额外空白容忍。
- 非 macOS 平台返回 `None`，Linux / Windows 用户继续走"Settings 输入 + env 兜底"
  的旧路径，将来 Phase 8 再补。
- `install_model` 里把 `effective_proxy_url()` 改成有三层 fallback：
  1. Settings 输入（用户显式填）
  2. 系统代理（auto-detect）
  3. None（reqwest 默认行为：直连或 env 代理）
- 每条路径加 `[rwkv-install] proxy source: ...` 诊断行，dev 终端能直接看到这次
  install 走的哪条。

### 0 行 UI 改动

LocalRwkvPanel / store / hook / TS 类型完全不动。用户的 store-side `downloadProxy.url`
仍然走老优先级（显式填 → 覆盖自动检测）。Settings 输入框的存在本身就是 auto-detect
失败 / 误检测时的逃生通道。

## Compatibility

- `cargo check` / `cargo clippy --lib --all-targets`：零新增警告。
- `cargo test --lib`：**114 passed**（旧 107 + 新 7）。
- 行为兼容：
  - 已填代理的老用户 → 行为不变。
  - 空代理 + 没开系统代理的用户 → 行为不变（继续 fallback 到 env / direct）。
  - 空代理 + Clash 系统代理开着的用户 → **新行为**：自动用上系统代理，下载直接成功。
- 不依赖任何新 crate，纯 `std::process::Command` 调 `scutil`。

## Known Boundary

- **仅 macOS**：Linux / Windows 用户仍然要手填或 export env。Phase 8 真正开始
  Windows 路径时再补对应的检测（Windows 走注册表 / PAC，Linux 走 GNOME settings
  或 KDE config）。
- **检测发生在 install 调用时**：用户在 Rosetta 跑着的时候切换 Clash 模式 → 下一次
  install 才会重新检测。stop 重启 sidecar 不重新探测系统代理（loopback 不需要）。
- **HTTPS proxy URL 用 `http://` 方案**：reqwest::Proxy::all 接受 `http://`
  作为 HTTPS CONNECT 隧道的标准 URL，**这不是 bug**。`https://` 在 reqwest 里指
  "代理本身也用 HTTPS 加密"，Clash / Surge / 其他常见客户端的 HTTP proxy 端口
  不支持这个。
- **不检测 PAC / AutoConfig**：`ProxyAutoConfigEnable: 1` + `ProxyAutoConfigURLString`
  这条路径目前忽略。中国 Clash 用户极少用 PAC，遇到 PAC 用户就让他在 Settings
  手填即可。
- **诊断行用 eprintln**：和 `[rwkv-cancel]` / `[rwkv-install]` 同套路。发版前
  统一考虑迁到 `tracing::debug!` + RUST_LOG 控制。
- **没动 UI**：包括"何时 highlight 输入框"这条之前讨论的 UX 改进——按用户要求
  整体 UI/UX 一起规划再动。

## Verification

- 静态：`cargo check` / `cargo test --lib` / `cargo clippy --lib --all-targets`
  全过；零新警告。
- 实机预期（用户运行 dev 即可验证）：
  1. 不动 Settings 输入框（保持空 / 删掉之前填的）
  2. `unset https_proxy http_proxy all_proxy` 确认 env 干净
  3. Clash 系统代理开着
  4. 点安装 → dev 终端应该看到 `[rwkv-install] proxy source: macOS system proxy
     (http://127.0.0.1:7897)` 一行，下载正常
- 反向测试：把 Clash 系统代理关掉 → 重启 dev → 点安装 → dev 终端看到
  `[rwkv-install] proxy source: none (...)` → 下载失败（预期）

## Memory Touchpoints

未新增 memory。已有的 [project-rwkv-mobile-cn-build-gotchas](../../../.claude/projects/-Users-leolin-Documents-GitHub-rosetta/memory/project_rwkv_mobile_cn_build_gotchas.md)
讲的是 build 阶段的代理配置；运行时代理的策略（loopback never via proxy /
remote auto-detect macOS system proxy）等本轮 UI 整改时再统一记一条。
