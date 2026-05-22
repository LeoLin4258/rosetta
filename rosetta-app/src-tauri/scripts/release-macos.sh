#!/usr/bin/env bash
# Build, sign, notarize, staple, and package the Rosetta macOS arm64 release.
#
# Usage from repo root:
#   bash rosetta-app/src-tauri/scripts/release-macos.sh
#
# Usage from rosetta-app/:
#   bash src-tauri/scripts/release-macos.sh
#
# Required local setup:
#   - Developer ID Application certificate installed in the login keychain.
#   - notarytool credentials stored under the keychain profile below.

set -euo pipefail

APP_NAME="${APP_NAME:-Rosetta}"
APP_IDENTIFIER="${APP_IDENTIFIER:-com.rosetta.desktop}"
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Shenzhen Yuanshi Intelligence Co., Ltd. (3FTQ9PH6TL)}"
NOTARY_PROFILE="${NOTARY_PROFILE:-rosetta-notary}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$(cd "$TAURI_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"

ENTITLEMENTS="$TAURI_DIR/Entitlements.plist"
BUILT_APP="$TAURI_DIR/target/release/bundle/macos/$APP_NAME.app"
DIST_DIR="$REPO_ROOT/dist/release"
STAGE_ROOT="$(mktemp -d)"
SIGNED_APP="$STAGE_ROOT/$APP_NAME.app"
APP_ZIP="$STAGE_ROOT/$APP_NAME.zip"
UPDATER_ARTIFACT_PATH=""
UPDATER_SIGNATURE_PATH=""

cleanup() {
  rm -rf "$STAGE_ROOT"
}
trap cleanup EXIT

log() {
  printf '[macos-release] %s\n' "$*" >&2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

version() {
  node -p "require('./package.json').version"
}

sign_file() {
  local path="$1"
  local identifier="$2"

  codesign --remove-signature "$path" >/dev/null 2>&1 || true
  codesign \
    --force \
    --timestamp \
    --options runtime \
    --sign "$SIGNING_IDENTITY" \
    --identifier "$identifier" \
    "$path"
}

sign_macho_files() {
  local main_executable="$SIGNED_APP/Contents/MacOS/rosetta-app"

  log "signing Mach-O files inside $SIGNED_APP"
  while IFS= read -r -d '' path; do
    if ! file "$path" | grep -Eq 'Mach-O|dynamically linked shared library'; then
      continue
    fi

    if [[ "$path" == "$main_executable" ]]; then
      codesign --remove-signature "$path" >/dev/null 2>&1 || true
      codesign \
        --force \
        --timestamp \
        --options runtime \
        --entitlements "$ENTITLEMENTS" \
        --sign "$SIGNING_IDENTITY" \
        --identifier "$APP_IDENTIFIER" \
        "$path"
    else
      local name
      name="$(basename "$path" | tr -c '[:alnum:].-' '-')"
      sign_file "$path" "$APP_IDENTIFIER.$name"
    fi
  done < <(find "$SIGNED_APP" -type f -print0)
}

notarize_and_staple_app() {
  log "zipping app for notarization"
  ditto -c -k --keepParent "$SIGNED_APP" "$APP_ZIP"

  log "submitting app notarization"
  xcrun notarytool submit "$APP_ZIP" --keychain-profile "$NOTARY_PROFILE" --wait

  log "stapling app ticket"
  xcrun stapler staple "$SIGNED_APP"
  xcrun stapler validate "$SIGNED_APP"
}

create_sign_notarize_dmg() {
  local app_version="$1"
  local dmg_path="$DIST_DIR/$APP_NAME-$app_version-macos-arm64.dmg"
  local tmp_dmg="$STAGE_ROOT/$APP_NAME.dmg"
  local dmg_source="$STAGE_ROOT/dmg-source"

  mkdir -p "$DIST_DIR"
  rm -f "$dmg_path" "$tmp_dmg"
  rm -rf "$dmg_source"
  mkdir -p "$dmg_source"

  log "staging DMG contents with Applications shortcut"
  ditto --norsrc "$SIGNED_APP" "$dmg_source/$APP_NAME.app"
  ln -s /Applications "$dmg_source/Applications"

  log "creating DMG"
  hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$dmg_source" \
    -ov \
    -format UDZO \
    "$tmp_dmg"

  log "signing DMG"
  codesign --force --timestamp --sign "$SIGNING_IDENTITY" "$tmp_dmg"
  codesign --verify --verbose=4 "$tmp_dmg"

  log "submitting DMG notarization"
  xcrun notarytool submit "$tmp_dmg" --keychain-profile "$NOTARY_PROFILE" --wait

  log "stapling DMG ticket"
  xcrun stapler staple "$tmp_dmg"
  xcrun stapler validate "$tmp_dmg"

  log "copying final DMG to $dmg_path"
  ditto --norsrc "$tmp_dmg" "$dmg_path"

  log "verifying final DMG with Gatekeeper"
  spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_path"

  ls -lh "$dmg_path"
}

create_sign_updater_artifact() {
  local app_version="$1"
  local artifact_path="$DIST_DIR/$APP_NAME-$app_version-macos-arm64.app.tar.gz"
  local sig_path="$artifact_path.sig"
  local signer_args=(tauri signer sign)

  if [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    signer_args+=(-f "$TAURI_SIGNING_PRIVATE_KEY_PATH")
  elif [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    log "missing updater signing key: set TAURI_SIGNING_PRIVATE_KEY_PATH=/path/to/updater.key or TAURI_SIGNING_PRIVATE_KEY"
    exit 2
  fi

  mkdir -p "$DIST_DIR"
  rm -f "$artifact_path" "$sig_path"

  log "creating updater artifact from signed stapled app"
  COPYFILE_DISABLE=1 tar -czf "$artifact_path" -C "$STAGE_ROOT" "$APP_NAME.app"

  log "signing updater artifact"
  # tauri signer sign writes the .sig file itself; discard verbose stdout.
  pnpm --silent "${signer_args[@]}" "$artifact_path" >/dev/null
  if [[ ! -s "$sig_path" ]]; then
    log "Tauri signer did not produce a signature file at $sig_path"
    exit 1
  fi

  UPDATER_ARTIFACT_PATH="$artifact_path"
  UPDATER_SIGNATURE_PATH="$sig_path"

  ls -lh "$artifact_path" "$sig_path"
}

main() {
  require_command node
  require_command pnpm
  require_command file
  require_command tar
  require_command hdiutil
  require_command codesign
  require_command spctl
  require_command xcrun
  require_command ditto

  if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
    log "macOS release build requires macOS arm64"
    exit 2
  fi

  if [[ ! -f "$ENTITLEMENTS" ]]; then
    log "missing entitlements file: $ENTITLEMENTS"
    exit 2
  fi

  cd "$APP_DIR"
  local app_version
  app_version="$(version)"

  local stale_dmg_dir="$TAURI_DIR/target/release/bundle/dmg"
  if [[ -d "$stale_dmg_dir" ]]; then
    log "removing stale unsigned DMGs under $stale_dmg_dir to prevent accidental distribution"
    rm -rf "$stale_dmg_dir"
  fi

  log "building unsigned macOS app bundle"
  pnpm tauri build --bundles app --no-sign

  if [[ ! -d "$BUILT_APP" ]]; then
    log "expected app bundle not found: $BUILT_APP"
    exit 1
  fi

  log "copying app bundle without resource forks or Finder metadata"
  ditto --norsrc "$BUILT_APP" "$SIGNED_APP"

  sign_macho_files

  log "signing app bundle"
  codesign --remove-signature "$SIGNED_APP" >/dev/null 2>&1 || true
  codesign \
    --force \
    --deep \
    --timestamp \
    --options runtime \
    --entitlements "$ENTITLEMENTS" \
    --sign "$SIGNING_IDENTITY" \
    "$SIGNED_APP"

  log "verifying app signature"
  codesign --verify --deep --strict --verbose=4 "$SIGNED_APP"

  notarize_and_staple_app

  log "verifying stapled app with Gatekeeper"
  spctl --assess --type execute --verbose=4 "$SIGNED_APP"

  create_sign_updater_artifact "$app_version"
  create_sign_notarize_dmg "$app_version"

  log "release complete"
  log "DMG: $DIST_DIR/$APP_NAME-$app_version-macos-arm64.dmg"
  log "Updater artifact: $UPDATER_ARTIFACT_PATH"
  log "Updater signature: $UPDATER_SIGNATURE_PATH"
}

main "$@"
