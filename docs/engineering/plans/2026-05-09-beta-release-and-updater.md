# Rosetta Internal Beta Release and Updater Plan

## Summary

Rosetta 的第一轮团队测试版使用正式发版骨架，而不是临时拷贝可执行文件。目标是在内部测试阶段同时验证：

- Windows x64 安装包分发
- SemVer beta 版本号
- GitHub Release 托管
- Tauri v2 updater 手动检查更新
- 更新后重启和本地缓存保留

当前默认 release endpoint：

```txt
https://github.com/LeoLin4258/rosetta/releases/latest/download/latest.json
```

如果源码仓库保持 private，应改用一个专门的 public release repo 托管安装包、updater artifact、签名和 `latest.json`，避免 updater 需要 GitHub 登录态。

## Versioning

内部 beta 使用 SemVer prerelease：

```txt
0.1.0-beta.1
0.1.0-beta.2
0.1.0-beta.3
```

每次发布需要同步更新：

- `rosetta-app/package.json`
- `rosetta-app/src-tauri/Cargo.toml`
- `rosetta-app/src-tauri/tauri.conf.json`

GitHub Release tag 使用：

```txt
v0.1.0-beta.1
```

Release title 使用：

```txt
Rosetta 0.1.0 Beta 1
```

## Signing

Windows 代码签名和 Tauri updater 签名是两件事：

- Windows 代码签名：第一轮内部 beta 暂不做，因此 SmartScreen 或“未知发布者”提示是预期现象。
- Tauri updater 签名：必须做。updater 的签名校验不能关闭。

当前 updater public key 已写入 `tauri.conf.json`。

首次生成的本机私钥路径：

```txt
C:\Users\Leo\.rosetta-release\rosetta-beta.key
```

私钥绝不能提交仓库。丢失私钥后，已安装版本将无法验证后续更新包，需要重新安装新的安装包。

发布时使用环境变量：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH="C:\Users\Leo\.rosetta-release\rosetta-beta.key"
corepack pnpm build:tauri
```

如果后续为私钥增加密码，还需要设置：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<password>"
```

## Build Artifacts

Tauri 配置开启：

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  }
}
```

每次 beta release 至少上传：

- Windows installer
- Windows updater artifact
- updater artifact 对应的 `.sig`
- `latest.json`

`latest.json` 必须和安装包放在同一个 GitHub Release 附件中，供已安装 App 的设置页手动检查更新。

## latest.json

使用 Tauri static JSON 格式。示例：

```json
{
  "version": "0.1.0-beta.2",
  "notes": "修复任务工作台状态刷新问题，优化设置页更新入口。",
  "pub_date": "2026-05-09T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<paste .sig file content here>",
      "url": "https://github.com/LeoLin4258/rosetta/releases/download/v0.1.0-beta.2/<windows-updater-artifact>"
    }
  }
}
```

注意：

- `signature` 是 `.sig` 文件内容，不是 `.sig` 文件 URL。
- `url` 指向 updater artifact，不是普通说明文档。
- `pub_date` 使用 RFC3339。
- `notes` 写给测试成员看，不写内部实现流水账。

## Release Procedure

1. 更新版本号。
2. 运行验证：

```powershell
cd rosetta-app
corepack pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

3. 设置 updater 私钥环境变量：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH="C:\Users\Leo\.rosetta-release\rosetta-beta.key"
```

4. 打包：

```powershell
cd rosetta-app
corepack pnpm build:tauri
```

5. 收集 Windows installer、updater artifact 和 `.sig`。
6. 手动生成 `latest.json`。
7. 创建 GitHub Release 并上传所有产物。
8. 在上一版已安装 App 中进入设置页，点击“检查更新”。
9. 验证下载、安装、重启后版本号更新。

## Test Plan

首次安装测试：

- 在干净 Windows 测试机安装 `0.1.0-beta.1`。
- 记录 Windows 未签名安装包提示。
- 配置 RWKV API。
- 完成导入、翻译、查看译文、重翻和导出。

更新链路测试：

- 发布 `0.1.0-beta.2`。
- 在 `0.1.0-beta.1` 中手动检查更新。
- 确认显示新版本号和 release notes。
- 确认下载、安装、重启成功。
- 确认重启后版本号为 `0.1.0-beta.2`。
- 确认本地 job cache 和 RWKV API 设置仍保留。

失败场景测试：

- `latest.json` 不存在时显示清晰错误。
- 当前已是最新时显示“已是最新版本”。
- 签名不匹配时更新失败，并提示用户更新包校验失败。
- 网络不可用时更新失败，但不影响翻译主流程。
- 未配置 RWKV API 时仍能检查更新。

## Assumptions

- 第一阶段只支持 Windows x64 更新。
- 不搭建动态更新服务器。
- 不做启动时自动更新弹窗。
- 不加入登录、云同步、遥测或聊天能力。
- 正式公开发布前，需要单独处理 Windows 代码签名、自动发布 CI、下载页、正式 release notes 和迁移策略。
