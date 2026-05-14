# Phase 7.B Codesign / Notarize — Deferred

**状态**：阻塞中（2026-05-14 暂缓）

## 为什么暂缓

Phase 7.B（Developer ID Application 证书 + notarize + DMG + updater 私钥）需要
通过公司 Apple Developer Program 账号。该账号绑定的 2FA 设备 / 验证码归一位
同事所有，每次登录需要他实时配合接收 6 位短信验证码——人不在线时验证码反复过期，
等待成本远高于实际签名配置的工作量。

A3a 在另一台 Mac 上 AirDrop 后双击 .app 时遇到"文件已损坏"（macOS 对 ad-hoc 签名 +
quarantine 的 .app 的最强力拒绝路径），这确认了**对真实公开用户**没有 Developer ID
就无法分发。但**对当前阶段（0.1.0-beta.1，仅内部 dogfood）**这不是阻塞：

- 开发本机直接 build + 跑 `.app`：✅ 无影响。
- 团队内分发：✅ 一行 `xattr -dr com.apple.quarantine <path>` 绕过 Gatekeeper，
  接收方按指引执行一次即可。
- 真实公开 release：❌ 需要 Phase 7.B 完成。

代价用户化语言版："现在能内部 dogfood / 演示，不能挂到 GitHub Release 让陌生人
下载即用。"

## 暂缓的具体内容

- **不**为 Rosetta 申请 Developer ID Application 证书。
- **不**在 `tauri.conf.json` 添加 `bundle.macOS.signingIdentity` / `hardenedRuntime`
  / `entitlements`。
- **不**配置 App Store Connect API Key 或 App-Specific Password。
- **不**重生成 Tauri updater minisign 私钥（虽然 `tauri.conf.json` 里的 pubkey
  与本机私钥不匹配，但既然 updater 流程没启用，没有实际损害）。
- **不**启用 DMG bundle target（目前 `--bundles app` 已足够内部 dogfood）。

`tauri.conf.json` 当前的 updater pubkey 是历史遗留——配套的私钥已不可访问。
公开发版前如选择"用旧 pubkey 让老 beta 用户也能更新"，需要先找回老私钥；
更现实的方案是和老 beta 用户协调一次"重装"，新版本带新 pubkey。这部分留到
Phase 7.B 真正启动时再决定。

## 内部分发临时 workaround

把 `.app` 发给同事 / 测试者时，附上一行指令：

```bash
xattr -dr com.apple.quarantine /Applications/Rosetta.app
```

（如果 .app 放在别处，把路径替换掉即可。）

执行后双击就能开。**只需做一次**——macOS 不再标记这个 .app 为"来自外部"。

理由说明：我们的 .app 是 ad-hoc 签名（macOS 默认不信任的本机签名），AirDrop
/ 邮件 / 下载 等渠道传过去时会自动加 `com.apple.quarantine` 扩展属性，触发
Gatekeeper 最严格的拒绝路径——"已损坏"是它的措辞，**文件本身没问题**，纯粹是
不信任。`xattr -dr` 删除这个属性，等同告诉 macOS"这是我本机的 app，不是从
外部来的"。

## 恢复 Phase 7.B 的触发条件

下面任一条件成立时再开 7.B：

- 公司 2FA 同事腾出连续 1–2 小时配合（创建证书 + 拿 API Key + 第一次 notarize
  调通）。
- Rosetta 需要走公开 release（GitHub Release / 官网 / 应用市场任一）。
- 或者切到个人 Apple Developer 账号（$99/年，无 2FA 阻塞但 Gatekeeper 上显示
  个人名而非公司名——不够正式）。

恢复时按本 plan 顶部 Phase 7.B 描述执行，依赖：

1. Developer ID Application 证书（Keychain Access 生成 CSR → developer.apple.com
   申请 → 下载 .cer → 双击装进 login keychain）
2. App Store Connect API Key 或 App-Specific Password（用于 notarytool）
3. 新的 minisign keypair（`pnpm tauri signer generate -w`），替换
   `tauri.conf.json` 的 pubkey

`security find-identity -v -p codesigning` 看到"Developer ID Application:
&lt;公司名&gt; (TEAM_ID)"那一行就是开干起点；把那一整行贴回会话，剩下 tauri.conf.json
+ entitlements + notarize 配置可以 1 小时内做完。
