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
DMG_BACKGROUND="$TAURI_DIR/icons/dmg-background.png"
BUILT_APP="$TAURI_DIR/target/release/bundle/macos/$APP_NAME.app"
DIST_DIR="$REPO_ROOT/dist/release"
STAGE_ROOT="$(mktemp -d)"
SIGNED_APP="$STAGE_ROOT/$APP_NAME.app"
APP_ZIP="$STAGE_ROOT/$APP_NAME.zip"
UPDATER_ARTIFACT_PATH=""
UPDATER_SIGNATURE_PATH=""
DMG_ATTACHED_VOLUME_PATH=""

cleanup() {
  if [[ -n "$DMG_ATTACHED_VOLUME_PATH" && -d "$DMG_ATTACHED_VOLUME_PATH" ]]; then
    hdiutil detach "$DMG_ATTACHED_VOLUME_PATH" -quiet >/dev/null 2>&1 || true
  fi
  rm -rf "$STAGE_ROOT"
}
trap cleanup EXIT

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

log() {
  printf "  %s\n" "$*" >&2
}

step() {
  printf "\n${BOLD}${CYAN}▶ %s${RESET}\n" "$*" >&2
}

ok() {
  printf "  ${GREEN}✓ %s${RESET}\n" "$*" >&2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

is_codesign_timestamp_failure() {
  local output="$1"
  [[ "$output" == *"timestamp service"* || "$output" == *"Timestamp service"* ]]
}

codesign_with_retries() {
  local max_attempts="${CODESIGN_MAX_ATTEMPTS:-5}"
  local retry_delay="${CODESIGN_RETRY_DELAY_SECONDS:-8}"
  local attempt=1
  local output
  local status

  while true; do
    set +e
    output="$(codesign "$@" 2>&1)"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
      if [[ -n "$output" ]]; then
        printf "%s\n" "$output" >&2
      fi
      return 0
    fi

    if is_codesign_timestamp_failure "$output" && [[ "$attempt" -lt "$max_attempts" ]]; then
      log "codesign timestamp service unavailable; retrying in ${retry_delay}s ($attempt/$max_attempts)"
      sleep "$retry_delay"
      attempt=$((attempt + 1))
      continue
    fi

    if [[ -n "$output" ]]; then
      printf "%s\n" "$output" >&2
    fi
    return "$status"
  done
}

detach_existing_volume_if_mounted() {
  local volume_path="$1"

  if [[ ! -e "$volume_path" ]]; then
    return 0
  fi

  if mount | grep -F " on $volume_path " >/dev/null; then
    log "$volume_path is already mounted; detaching existing volume"
    hdiutil detach "$volume_path" -quiet
    return 0
  fi

  printf "  \033[0;31m✗ %s already exists but is not a mounted volume. Remove or rename it before building a release.\033[0m\n" "$volume_path" >&2
  exit 1
}

version() {
  node -p "require('./package.json').version"
}

sign_file() {
  local path="$1"
  local identifier="$2"

  codesign --remove-signature "$path" >/dev/null 2>&1 || true
  codesign_with_retries \
    --force \
    --timestamp \
    --options runtime \
    --sign "$SIGNING_IDENTITY" \
    --identifier "$identifier" \
    "$path"
}

sign_macho_files() {
  local main_executable="$SIGNED_APP/Contents/MacOS/rosetta-app"

  step "Signing Mach-O binaries"
  while IFS= read -r -d '' path; do
    if ! file "$path" | grep -Eq 'Mach-O|dynamically linked shared library'; then
      continue
    fi

    if [[ "$path" == "$main_executable" ]]; then
      codesign --remove-signature "$path" >/dev/null 2>&1 || true
      codesign_with_retries \
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
  step "Notarizing app (this takes a few minutes)"
  ditto -c -k --keepParent "$SIGNED_APP" "$APP_ZIP"
  xcrun notarytool submit "$APP_ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
  ok "Notarization accepted"

  step "Stapling notarization ticket"
  xcrun stapler staple "$SIGNED_APP"
  xcrun stapler validate "$SIGNED_APP"
  ok "Ticket stapled"
}

create_sign_notarize_dmg() {
  local app_version="$1"
  local dmg_path="$DIST_DIR/$APP_NAME-$app_version-macos-arm64.dmg"
  local rw_dmg="$STAGE_ROOT/$APP_NAME-rw.dmg"
  local tmp_dmg="$STAGE_ROOT/$APP_NAME-final.dmg"
  local dmg_source="$STAGE_ROOT/dmg-source"
  local volume_path="/Volumes/$APP_NAME"
  local window_width=645
  local window_height=391
  local window_x=400
  local window_y=100
  local window_right=$((window_x + window_width))
  local window_bottom=$((window_y + window_height))

  mkdir -p "$DIST_DIR"
  rm -f "$dmg_path" "$rw_dmg" "$tmp_dmg"
  rm -rf "$dmg_source"
  mkdir -p "$dmg_source/.background"

  detach_existing_volume_if_mounted "$volume_path"

  step "Creating DMG"
  ditto --norsrc "$SIGNED_APP" "$dmg_source/$APP_NAME.app"
  ln -s /Applications "$dmg_source/Applications"
  ditto --norsrc "$DMG_BACKGROUND" "$dmg_source/.background/dmg-background.png"
  hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$dmg_source" \
    -ov \
    -format UDRW \
    "$rw_dmg" >/dev/null

  hdiutil attach "$rw_dmg" -nobrowse -quiet
  DMG_ATTACHED_VOLUME_PATH="$volume_path"
  osascript <<OSA
set backgroundImage to POSIX file "$volume_path/.background/dmg-background.png" as alias
tell application "Finder"
  tell disk "$APP_NAME"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {$window_x, $window_y, $window_right, $window_bottom}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 112
    set background picture of viewOptions to backgroundImage
    set position of item "$APP_NAME.app" of container window to {176, 205}
    set position of item "Applications" of container window to {492, 205}
    update without registering applications
    close
    open
    delay 1
    set the bounds of container window to {$window_x, $window_y, $((window_right - 10)), $((window_bottom - 10))}
  end tell
  delay 1
  tell disk "$APP_NAME"
    set the bounds of container window to {$window_x, $window_y, $window_right, $window_bottom}
  end tell
  delay 3
end tell
OSA
  SetFile -a V "$volume_path/.background"
  if [[ ! -s "$volume_path/.DS_Store" ]]; then
    printf "  \033[0;31m✗ Finder did not persist DMG layout metadata at %s/.DS_Store\033[0m\n" "$volume_path" >&2
    exit 1
  fi
  hdiutil detach "$volume_path" -quiet
  DMG_ATTACHED_VOLUME_PATH=""

  hdiutil convert "$rw_dmg" -format UDZO -imagekey zlib-level=9 -o "$tmp_dmg" >/dev/null
  ok "DMG created"

  step "Signing & notarizing DMG (this takes a few minutes)"
  codesign_with_retries --force --timestamp --sign "$SIGNING_IDENTITY" "$tmp_dmg"
  xcrun notarytool submit "$tmp_dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  ok "Notarization accepted"

  step "Stapling DMG ticket"
  xcrun stapler staple "$tmp_dmg"
  xcrun stapler validate "$tmp_dmg"
  ditto --norsrc "$tmp_dmg" "$dmg_path"
  ok "Ticket stapled"

  step "Verifying DMG with Gatekeeper"
  spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_path"
  ok "Gatekeeper accepted"
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

  step "Creating & signing updater artifact"
  COPYFILE_DISABLE=1 tar -czf "$artifact_path" -C "$STAGE_ROOT" "$APP_NAME.app"
  # tauri signer sign writes the .sig file itself; discard verbose stdout.
  pnpm --silent "${signer_args[@]}" "$artifact_path" >/dev/null
  if [[ ! -s "$sig_path" ]]; then
    printf "  \033[0;31m✗ Tauri signer did not produce a signature file at %s\033[0m\n" "$sig_path" >&2
    exit 1
  fi
  ok "Updater artifact signed"

  UPDATER_ARTIFACT_PATH="$artifact_path"
  UPDATER_SIGNATURE_PATH="$sig_path"
}

main() {
  require_command node
  require_command pnpm
  require_command file
  require_command tar
  require_command hdiutil
  require_command osascript
  require_command SetFile
  require_command codesign
  require_command spctl
  require_command xcrun
  require_command ditto

  if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
    printf "  \033[0;31m✗ macOS release build requires macOS arm64\033[0m\n" >&2
    exit 2
  fi

  if [[ ! -f "$ENTITLEMENTS" ]]; then
    printf "  \033[0;31m✗ missing entitlements file: %s\033[0m\n" "$ENTITLEMENTS" >&2
    exit 2
  fi

  if [[ ! -f "$DMG_BACKGROUND" ]]; then
    printf "  \033[0;31m✗ missing DMG background image: %s\033[0m\n" "$DMG_BACKGROUND" >&2
    exit 2
  fi

  cd "$APP_DIR"
  local app_version
  app_version="$(version)"

  printf "\n${BOLD}═══════════════════════════════════════${RESET}\n" >&2
  printf "${BOLD}  Rosetta macOS Release  —  v%s${RESET}\n" "$app_version" >&2
  printf "${BOLD}═══════════════════════════════════════${RESET}\n" >&2

  local stale_dmg_dir="$TAURI_DIR/target/release/bundle/dmg"
  if [[ -d "$stale_dmg_dir" ]]; then
    rm -rf "$stale_dmg_dir"
  fi

  step "Building app bundle (pnpm tauri build)"
  pnpm tauri build --bundles app --no-sign

  if [[ ! -d "$BUILT_APP" ]]; then
    printf "  \033[0;31m✗ expected app bundle not found: %s\033[0m\n" "$BUILT_APP" >&2
    exit 1
  fi
  ok "App bundle built"

  step "Preparing clean copy (removing resource forks)"
  ditto --norsrc "$BUILT_APP" "$SIGNED_APP"

  sign_macho_files
  ok "Mach-O binaries signed"

  step "Signing app bundle"
  codesign --remove-signature "$SIGNED_APP" >/dev/null 2>&1 || true
  codesign_with_retries \
    --force \
    --deep \
    --timestamp \
    --options runtime \
    --entitlements "$ENTITLEMENTS" \
    --sign "$SIGNING_IDENTITY" \
    "$SIGNED_APP"
  codesign --verify --deep --strict "$SIGNED_APP"
  ok "App bundle signed and verified"

  notarize_and_staple_app

  step "Verifying stapled app with Gatekeeper"
  spctl --assess --type execute --verbose=4 "$SIGNED_APP"
  ok "Gatekeeper accepted"

  create_sign_updater_artifact "$app_version"
  create_sign_notarize_dmg "$app_version"

  local dmg_size updater_size
  dmg_size="$(du -sh "$DIST_DIR/$APP_NAME-$app_version-macos-arm64.dmg" | cut -f1)"
  updater_size="$(du -sh "$UPDATER_ARTIFACT_PATH" | cut -f1)"

  printf "\n${BOLD}${GREEN}✓ Release complete${RESET}\n" >&2
  printf "${BOLD}───────────────────────────────────────${RESET}\n" >&2
  printf "  ${YELLOW}Version${RESET}   v%s\n" "$app_version" >&2
  printf "  ${YELLOW}DMG${RESET}       %s  (%s)\n" "$(basename "$DIST_DIR/$APP_NAME-$app_version-macos-arm64.dmg")" "$dmg_size" >&2
  printf "  ${YELLOW}Updater${RESET}   %s  (%s)\n" "$(basename "$UPDATER_ARTIFACT_PATH")" "$updater_size" >&2
  printf "${BOLD}───────────────────────────────────────${RESET}\n" >&2
  printf "\nNext: run ${BOLD}publish-macos-updater.sh${RESET} to upload to Supabase.\n" >&2
}

main "$@"
