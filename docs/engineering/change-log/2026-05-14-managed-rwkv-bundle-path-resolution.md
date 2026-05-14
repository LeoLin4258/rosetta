# 2026-05-14 Managed RWKV Bundle Path Resolution (Phase 7.A2)

## Context

Phase 7.A2 第一次让 Rosetta 以 **bundled `.app`** 形态跑（不是 `pnpm tauri dev`）。
`pnpm tauri build --bundles app` 产物在 `target/release/bundle/macos/Rosetta.app`。
从 dev → bundle 模式转换暴露了两个 Tauri 2 实际包装行为与我们 Phase 3 `locate_*`
代码假设不符的地方。

## Bugs Found & Fixed

### 1. Tauri 2 strip 掉 sidecar 文件名的 target-triple 后缀

- **Dev**：`src-tauri/binaries/rwkv-server-aarch64-apple-darwin`（保留 triple 后缀，
  这样多平台 binaries/ 目录可以并存）
- **Bundle**：`Contents/MacOS/rwkv-server`（**去掉** triple；每个 `.app` 已经是
  架构专属，后缀冗余）

我们 [status.rs](../../../rosetta-app/src-tauri/src/managed_rwkv/status.rs) 的
`locate_sidecar` 只查带后缀的版本，bundle 模式直接返回 `None`，导致启动时
"sidecar 文件不存在" fail。

**修复**：加 `strip_target_triple_suffix` 辅助函数，识别 4 个常见 triple
（`aarch64-apple-darwin` / `x86_64-apple-darwin` / Windows MSVC/GNU）；
`locate_sidecar` 同时探测带后缀和不带后缀两个名字。这样同一份代码 dev 和 bundle
都能找到二进制。

### 2. Tauri 2 把 `bundle.resources` 放在 `Contents/Resources/`，不再有 `_up_/`

- **Tauri 1 时代**：`bundle.resources` 里的相对路径会落到 `Contents/Resources/_up_/<rel>`。
- **Tauri 2 时代**：直接 `Contents/Resources/<rel>`。

我们 `locate_tokenizer` 写的是 Tauri 1 路径，bundle 找不到分词表 → 启动失败。

**修复**：同时探测 Tauri 2 路径 (`Contents/Resources/resources/rwkv-sidecar/<name>`)
和 Tauri 1 路径（防止 Tauri 未来回退）。两条都试，谁存在用谁。

## Verification

- `cargo check` / `cargo test --lib managed_rwkv`：17 个测试全过。
- 实机 A2：
  - `pnpm tauri build --bundles app` 产出 `Rosetta.app`（21 MB 主程序 + 23 MB sidecar
    + 2.2 MB 分词表 + 资源）。
  - `open Rosetta.app` 启动后 macOS 没弹 Gatekeeper（ad-hoc 签名在 dev 机本机可通；
    其他 Mac 仍会拦——见 Known Boundary）。
  - Settings → "本地 RWKV 翻译" panel 自动显示"已安装 · 未启动"——证明 app data
    目录（`~/Library/Application Support/com.rosetta.desktop/`）在 dev 与 bundle
    之间共用，A1 下载的模型继承下来不用重下。
  - 点"启动本地翻译" → 5–15 秒变"运行中"——证明：
    - bundle 模式下 sidecar 路径 (`Contents/MacOS/rwkv-server`) 被 `locate_sidecar`
      正确识别。
    - tokenizer 路径 (`Contents/Resources/resources/rwkv-sidecar/b_rwkv_vocab_v20230424.txt`)
      被 `locate_tokenizer` 正确识别。
    - `.no_proxy()` 在 release bundle 下也正确隔离 loopback 流量（A1 修过的 bug
      在 release build 不退步）。
  - Jobs 跑一遍 Markdown 翻译：通过。
- `codesign -dv` 报 `Signature=adhoc`，TeamIdentifier `not set`——意料之中，本机
  dev 机不带 Apple Developer 证书；走 A3 / 公开分发前需要正经签名。

## Tauri Build Artifact Layout (reference)

实测 Tauri 2.11 在 macOS arm64 下的 .app 结构：

```
Rosetta.app/
  Contents/
    Info.plist
    MacOS/
      rosetta-app                 ← 主可执行（21 MB stripped）
      rwkv-server                 ← sidecar，target triple 被 trim
    Resources/
      icon.icns
      resources/                  ← bundle.resources 起点（Tauri 1 时代是 _up_/）
        rwkv-sidecar/
          .gitkeep
          b_rwkv_vocab_v20230424.txt
```

不再有 `_up_/` 中间目录。`bundle.resources` 直接落 `Contents/Resources/<rel>`。

## Known Boundary

- **Updater 签名错误**：`pnpm tauri build` 末尾报 `A public key has been found,
  but no private key. Make sure to set TAURI_SIGNING_PRIVATE_KEY environment variable.`
  这是 Tauri Updater plugin 需要私钥签 `.app.tar.gz` 更新包；dev 机不带这把密钥。
  .app 本身已经正确产出，只是 updater 增量包没签。Phase 7 公开发版时通过 GitHub
  Actions secret 注入 `TAURI_SIGNING_PRIVATE_KEY` 即可。
- **A3 仍被代理问题阻塞**：本次 A2 用 `open Rosetta.app` 从 terminal 启动，
  继承 shell 的 `HTTPS_PROXY` env。Finder/Dock 双击启动**不会继承 shell env**，
  下次让真用户在新 Mac 上点 .app 时 HF 下载会立刻 connection-reset。下一步必须
  做代理自动检测（macOS `scutil --proxy` / `system-configuration` crate）或者
  Settings 加代理输入框。
- **ad-hoc 签名只在本机能开**：放到别的 Mac 上 Gatekeeper 会拒，提示"无法验证
  开发者"。公开发版需要 Developer ID Application 证书 + notarization。
- **DMG 这次没产**：用了 `--bundles app` 跳过 DMG 加速 A2 验证。Phase 7 公开
  发版前补 `--bundles app,dmg` 跑一次完整流程。
