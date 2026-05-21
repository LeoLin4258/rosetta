# macOS Developer ID Signing Setup

## Summary

Configured the macOS bundle for Developer ID signing and notarization readiness.

## Changes

- Added `Developer ID Application: Shenzhen Yuanshi Intelligence Co., Ltd. (3FTQ9PH6TL)` as the macOS signing identity in `tauri.macos.conf.json`.
- Enabled hardened runtime for macOS release bundles.
- Added `Entitlements.plist` with `com.apple.security.cs.disable-library-validation` so the notarized bundle can load Rosetta's bundled native runtime libraries.

## Local Secret Material

The App Store Connect API key is not stored in the repository. Notarization credentials were saved locally in Keychain under the profile name `rosetta-notary`.

## Validation

- `security find-identity -v -p codesigning` found one valid Developer ID Application identity.
- `xcrun notarytool store-credentials rosetta-notary ...` validated and saved the notarization credentials.

Release build, notarization submission, stapling, and Gatekeeper validation remain to be run as the next release-hardening step.

## Follow-up

Added `src-tauri/scripts/release-macos.sh` and `docs/engineering/release/macos-release.md` after the first successful notarized DMG. The script uses `pnpm tauri build --bundles app --no-sign`, copies the `.app` with `ditto --norsrc`, signs the clean bundle manually, notarizes and staples the app, then creates/signs/notarizes/staples the final DMG.

Validated the script end to end on 2026-05-21. Apple notarization accepted both the app archive and the DMG, and the final DMG passed Gatekeeper assessment with `source=Notarized Developer ID`.
