# macOS Release Procedure

## 发版 Checklist（快速参考）

每次发版按顺序执行：

**1. 改版本号（三个文件必须一致）**

```bash
# 将三处 0.1.0-beta.X 改为新版本号
rosetta-app/package.json
rosetta-app/src-tauri/tauri.conf.json
rosetta-app/src-tauri/Cargo.toml
```

**2. 提交版本号变更**

```bash
git add rosetta-app/package.json rosetta-app/src-tauri/tauri.conf.json rosetta-app/src-tauri/Cargo.toml
git commit -m "Bump version to <new-version>"
```

**3. 设置密钥（每次终端会话需要设置一次）**

```bash
export TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/rosetta/updater.key
export SUPABASE_SERVICE_ROLE_KEY=<从 Supabase Project Settings → API 复制 service_role key>
```

**4. 构建 + 公证 + 签名**

```bash
cd /path/to/rosetta
bash rosetta-app/src-tauri/scripts/release-macos.sh
```

约需 5–10 分钟（含 Apple 公证等待）。完成后产出：

```
dist/release/Rosetta-<version>-macos-arm64.dmg
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz.sig
```

**5. 上传到 Supabase（未发布状态）**

```bash
bash rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

上传 DMG + updater artifact，写入未发布的 metadata 行。

**6. 测试更新流程**

用已安装旧版本的设备，在 Settings 里点检查更新，走一遍完整更新流程。

**7. 正式发布**

测试通过后，运行脚本末尾打印的 `PATCH` 命令（或直接复制粘贴）：

```bash
curl --fail-with-body \
  --request PATCH \
  --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
  --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
  --header "Content-Type: application/json" \
  --data '{"is_published":true}' \
  "https://bdujdewqopcgwijhfbcz.supabase.co/rest/v1/app_releases?app=eq.rosetta&version=eq.<version>&target=eq.darwin&arch=eq.aarch64"
```

**8. 验证**

```bash
# 应返回新版本的 JSON
curl -s 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=0.0.0'

# 应返回 204（已是最新版）
curl -s -o /dev/null -w "%{http_code}" \
  'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=<new-version>'
```

---

This document describes the local macOS release path for Rosetta.

## Current Status

Rosetta can produce a macOS Apple Silicon DMG that is:

- signed with Developer ID Application;
- submitted to Apple notarization;
- stapled with the notarization ticket;
- accepted by Gatekeeper via `spctl`.

The release script also creates a Tauri updater artifact from the signed and stapled app bundle. This updater artifact is separate from the public DMG and is the only artifact that the Supabase updater publish script uploads.

The current release script is intentionally local-first. It uses credentials in the developer's macOS Keychain and does not store Apple API key material in the repository.

## Prerequisites

Run on macOS arm64 with:

- Xcode command line tools;
- Node, pnpm, Rust, and the Tauri CLI dependencies already working;
- the Developer ID Application certificate installed in the login keychain;
- notarization credentials stored with `notarytool`.

The expected signing identity is:

```txt
Developer ID Application: Shenzhen Yuanshi Intelligence Co., Ltd. (3FTQ9PH6TL)
```

The expected notary profile is:

```txt
rosetta-notary
```

To confirm the local machine is ready:

```bash
security find-identity -v -p codesigning | grep "Developer ID Application"
xcrun notarytool history --keychain-profile rosetta-notary
```

## Build Command

From the repository root:

```bash
bash rosetta-app/src-tauri/scripts/release-macos.sh
```

From `rosetta-app/`:

```bash
bash src-tauri/scripts/release-macos.sh
```

The script reads the app version from `rosetta-app/package.json` and writes:

```txt
dist/release/Rosetta-<version>-macos-arm64.dmg
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz.sig
```

## Why The Script Does Not Let Tauri Sign Directly

On this workstation, `pnpm tauri build` reached the signing phase but failed with:

```txt
resource fork, Finder information, or similar detritus not allowed
```

The generated bundle inherited macOS/FileProvider metadata such as Finder information. `codesign` rejects that metadata when signing an app bundle.

The release script avoids this by:

1. running `pnpm tauri build --bundles app --no-sign`;
2. copying the app bundle with `ditto --norsrc` to remove resource forks and Finder metadata;
3. signing Mach-O files and the app bundle manually;
4. notarizing and stapling the `.app`;
5. creating and signing the updater `.app.tar.gz` from that signed and stapled app;
6. creating, signing, notarizing, and stapling the final DMG.

## Verification Performed By The Script

The script verifies the app and DMG with:

```bash
codesign --verify --deep --strict --verbose=4 <Rosetta.app>
spctl --assess --type execute --verbose=4 <Rosetta.app>
codesign --verify --verbose=4 <Rosetta.dmg>
spctl --assess --type open --context context:primary-signature --verbose=4 <Rosetta.dmg>
```

After the stapled app passes Gatekeeper, the updater artifact is created from that exact staged app bundle:

```txt
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz.sig
```

A successful final DMG check should include:

```txt
accepted
source=Notarized Developer ID
```

## First Successful Manual Release

The first successful manual notarized artifact was produced on 2026-05-21:

```txt
dist/release/Rosetta-0.1.0-beta.1-macos-arm64.dmg
```

Both the app zip and DMG submissions returned `Accepted` from Apple notarization, and the final DMG passed `spctl`.

## First Successful Scripted Release

The first end-to-end scripted release also passed on 2026-05-21:

```bash
bash rosetta-app/src-tauri/scripts/release-macos.sh
```

Apple notarization returned `Accepted` for both submissions:

```txt
App submission: 6d60d21f-ea4e-4624-bf8b-89d85f78c517
DMG submission: f0d3a280-8136-47e2-bf7c-9fe037b65d22
```

The final artifact was:

```txt
dist/release/Rosetta-0.1.0-beta.1-macos-arm64.dmg
```

Gatekeeper accepted it as:

```txt
accepted
source=Notarized Developer ID
```

## First Successful Supabase Updater Release

The first release that includes a signed updater artifact uploaded to Supabase was produced on 2026-05-22 for version `0.1.0-beta.2`.

Apple notarization returned `Accepted` for both submissions:

```txt
App submission: e1a7d2cc-7a72-49b0-a765-38bbbd7d461e
DMG submission: 560f5841-f7ab-4dbd-9211-be172917ed94
```

Artifacts written to `dist/release/`:

```txt
Rosetta-0.1.0-beta.2-macos-arm64.dmg           (25 MB)
Rosetta-0.1.0-beta.2-macos-arm64.app.tar.gz    (22 MB)
Rosetta-0.1.0-beta.2-macos-arm64.app.tar.gz.sig (440 B)
```

Gatekeeper accepted both as:

```txt
accepted
source=Notarized Developer ID
```

The updater artifact and metadata were published to Supabase. The endpoint was confirmed to return the correct Tauri updater JSON for older current versions and `204 No Content` for the current or newer version:

```bash
# returns 200 + JSON when a newer version is published
curl -s 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=0.0.0'

# returns 204 when already on the latest version
curl -s -o /dev/null -w "%{http_code}" \
  'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=0.1.0-beta.2'
```

The in-app updater smoke test (install → check → download → relaunch → verify Gatekeeper) is pending. It requires a device with a version lower than `0.1.0-beta.2` installed, and will be completed on the next release.

## Troubleshooting

### `resource fork, Finder information, or similar detritus not allowed`

Do not keep retrying direct Tauri signing. Use the release script, which copies the app with `ditto --norsrc` before signing.

### `source=Unnotarized Developer ID`

The app is signed but not notarized or not stapled. Re-run the release script and check the `notarytool submit` output.

### `notarytool` authentication failure

Refresh the local Keychain profile:

```bash
xcrun notarytool store-credentials rosetta-notary \
  --key /path/to/AuthKey_<KEY_ID>.p8 \
  --key-id <KEY_ID> \
  --issuer <ISSUER_ID>
```

Do not commit the `.p8` file or print its contents in logs.

### Tauri updater artifacts

The public DMG and Tauri updater artifact are separate release outputs:

- the DMG is for manual installation;
- the Tauri updater artifact is delivered through Supabase for in-app updates.

`release-macos.sh` writes the public DMG plus the signed updater artifact to `dist/release/`:

```txt
dist/release/Rosetta-<version>-macos-arm64.dmg
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz.sig
```

`publish-macos-updater.sh` only publishes the `dist/release/Rosetta-<version>-macos-arm64.app.tar.gz` updater artifact and its `.sig`. It does not publish the unsigned Tauri target bundle under `rosetta-app/src-tauri/target/release/bundle/`, and it does not publish the public DMG.

The app checks this Supabase Edge Function endpoint for updates:

```txt
https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target={{target}}&arch={{arch}}&current_version={{current_version}}
```

The first updater release only supports:

```txt
darwin-aarch64
```

Before publishing an updater release, set the local secrets:

```bash
export SUPABASE_SERVICE_ROLE_KEY="$(security find-generic-password -a rosetta -s supabase-service-role-key -w)"
export TAURI_SIGNING_PRIVATE_KEY_PATH="$HOME/.tauri/rosetta/updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -a rosetta -s tauri-updater-key-password -w)"
```

Only set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when the updater private key requires it. If a CI or local environment stores the updater private key as a string instead of a file path, set `TAURI_SIGNING_PRIVATE_KEY` instead of `TAURI_SIGNING_PRIVATE_KEY_PATH`.

From the repository root, build the signed and notarized public DMG and signed updater artifact first:

```bash
bash rosetta-app/src-tauri/scripts/release-macos.sh
```

Then publish the updater artifact:

```bash
bash rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

The publish script uploads the updater artifact to the private `rosetta-releases` bucket and writes an unpublished `app_releases` row. After testing the unpublished release, use the `PATCH` command printed by the script to publish it.

To hide a bad release, set the version and mark it unpublished:

```bash
export ROSETTA_RELEASE_VERSION="$(node -p "require('./rosetta-app/package.json').version")"

curl -X PATCH \
  "https://bdujdewqopcgwijhfbcz.supabase.co/rest/v1/app_releases?app=eq.rosetta&version=eq.${ROSETTA_RELEASE_VERSION}&target=eq.darwin&arch=eq.aarch64" \
  -H "apikey: ${SUPABASE_SERVICE_ROLE_KEY}" \
  -H "Authorization: Bearer ${SUPABASE_SERVICE_ROLE_KEY}" \
  -H "Content-Type: application/json" \
  -H "Prefer: return=representation" \
  --data '{"is_published":false}'
```

Do not upload user documents, translations, job caches, prompts, or runtime logs to Supabase. Supabase release storage is only for updater artifacts, DMG installers, and release metadata.

## Official Website Download API

The `rosetta-latest-dmg` Edge Function provides the download entry point for the official website.

**Endpoint:**

```txt
GET https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-latest-dmg
```

No authentication required. Returns the latest published `darwin-aarch64` release that has a DMG uploaded.

**Response (200):**

```json
{
  "version": "0.1.0-beta.2",
  "pub_date": "2026-05-22T05:38:30.631422+00:00",
  "url": "<signed_download_url_valid_for_1_hour>"
}
```

**Response (404):** No published release available.

**Website integration example:**

```js
const res = await fetch(
  "https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-latest-dmg"
);
const { version, url } = await res.json();
// display version, redirect user to url on click
```

The signed URL expires after 1 hour. Fetch it fresh on each page load or button click.
