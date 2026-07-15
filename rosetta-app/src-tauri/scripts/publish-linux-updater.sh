#!/usr/bin/env bash
# Upload a signed Linux x64 AppImage release to Supabase and create an
# unpublished release row. Publishing remains a separate deliberate action.

set -euo pipefail

APP_NAME="${APP_NAME:-rosetta}"
SUPABASE_PROJECT_URL="${SUPABASE_PROJECT_URL:-https://bdujdewqopcgwijhfbcz.supabase.co}"
SUPABASE_BUCKET="${SUPABASE_BUCKET:-rosetta-releases}"
PUBLISHER_USER_AGENT="${PUBLISHER_USER_AGENT:-Rosetta-Release-Publisher/1.0}"
TARGET="${TARGET:-linux}"
ARCH="${ARCH:-x86_64}"
NOTES_FILE="${NOTES_FILE:-}"
PUBLISH="${PUBLISH:-false}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$(cd "$TAURI_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist/release"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "::error::missing required command: $1" >&2
    exit 2
  fi
}

require_env() {
  if [[ -z "${!1:-}" ]]; then
    echo "::error::missing required environment variable: $1" >&2
    exit 2
  fi
}

json_escape() {
  node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1"
}

check_metadata() {
  local artifact="$1"
  local metadata
  local expected_sha expected_size actual_sha actual_size

  for metadata in "$artifact.sha256" "$artifact.size"; do
    if [[ ! -f "$metadata" ]]; then
      echo "::error::missing release metadata: $metadata" >&2
      exit 2
    fi
  done

  expected_sha="$(awk 'NR == 1 { print $1 }' "$artifact.sha256")"
  expected_size="$(tr -d '[:space:]' < "$artifact.size")"
  actual_sha="$(sha256sum "$artifact" | awk '{ print $1 }')"
  actual_size="$(stat -c '%s' "$artifact")"

  if [[ "$expected_sha" != "$actual_sha" ]]; then
    echo "::error::SHA-256 metadata does not match $artifact" >&2
    exit 2
  fi
  if [[ "$expected_size" != "$actual_size" ]]; then
    echo "::error::size metadata does not match $artifact" >&2
    exit 2
  fi
}

upload_artifact() {
  local artifact="$1"
  local storage_path="$2"

  curl -s --fail-with-body \
    --request POST \
    --user-agent "$PUBLISHER_USER_AGENT" \
    --header "Content-Type: application/octet-stream" \
    --header "x-upsert: true" \
    --data-binary "@$artifact" \
    --config - \
    "$SUPABASE_PROJECT_URL/storage/v1/object/$SUPABASE_BUCKET/$storage_path" >/dev/null <<CURL_CONFIG
header = "Authorization: Bearer ${SUPABASE_SERVICE_ROLE_KEY}"
header = "apikey: ${SUPABASE_SERVICE_ROLE_KEY}"
CURL_CONFIG
}

main() {
  for command in awk cargo curl file grep node sha256sum stat tar; do
    require_command "$command"
  done
  require_env SUPABASE_SERVICE_ROLE_KEY

  if [[ "$APP_NAME" != "rosetta" || "$TARGET" != "linux" || "$ARCH" != "x86_64" || "$SUPABASE_BUCKET" != "rosetta-releases" ]]; then
    echo "::error::Linux publishing requires APP_NAME=rosetta TARGET=linux ARCH=x86_64 SUPABASE_BUCKET=rosetta-releases" >&2
    exit 2
  fi
  if [[ "$PUBLISH" != "true" && "$PUBLISH" != "false" ]]; then
    echo "::error::PUBLISH must be true or false" >&2
    exit 2
  fi

  local package_version tauri_version cargo_version
  package_version="$(cd "$APP_DIR" && node -p "require('./package.json').version")"
  tauri_version="$(cd "$APP_DIR" && node -p "require('./src-tauri/tauri.conf.json').version")"
  cargo_version="$({ cd "$TAURI_DIR" && cargo metadata --no-deps --format-version 1; } | node -e 'const fs = require("fs"); const data = JSON.parse(fs.readFileSync(0, "utf8")); console.log(data.packages.find((pkg) => pkg.name === "rosetta-app").version)')"

  if [[ "$package_version" != "$tauri_version" || "$package_version" != "$cargo_version" ]]; then
    echo "::error::version mismatch: package.json=$package_version tauri.conf.json=$tauri_version Cargo.toml=$cargo_version" >&2
    exit 2
  fi

  local appimage updater signature appimage_name updater_name
  appimage="$DIST_DIR/Rosetta-$package_version-linux-x64.AppImage"
  updater="$appimage.tar.gz"
  signature="$updater.sig"
  appimage_name="$(basename "$appimage")"
  updater_name="$(basename "$updater")"

  for artifact in "$appimage" "$updater" "$signature"; do
    if [[ ! -s "$artifact" ]]; then
      echo "::error::missing signed Linux release artifact: $artifact" >&2
      echo "Run rosetta-app/src-tauri/scripts/release-linux.sh first." >&2
      exit 2
    fi
  done
  if ! file "$appimage" | grep -q 'ELF 64-bit.*x86-64'; then
    echo "::error::AppImage is not Linux x86_64: $appimage" >&2
    exit 2
  fi

  check_metadata "$appimage"
  check_metadata "$updater"

  local archive_entries
  archive_entries="$(tar -tzf "$updater")"
  if [[ "$archive_entries" != "$appimage_name" ]]; then
    echo "::error::updater archive must contain exactly $appimage_name" >&2
    exit 2
  fi

  local appimage_sha appimage_size updater_size signature_text notes
  local appimage_storage_path updater_storage_path
  appimage_sha="$(sha256sum "$appimage" | awk '{ print $1 }')"
  appimage_size="$(stat -c '%s' "$appimage")"
  updater_size="$(stat -c '%s' "$updater")"
  signature_text="$(tr -d '\r\n' < "$signature")"
  if [[ -z "$signature_text" ]]; then
    echo "::error::updater signature is empty: $signature" >&2
    exit 2
  fi

  appimage_storage_path="linux/x86_64/$package_version/$appimage_name"
  updater_storage_path="linux/x86_64/$package_version/$updater_name"
  if [[ -n "$NOTES_FILE" ]]; then
    notes="$(<"$NOTES_FILE")"
  else
    notes="Rosetta $package_version for Ubuntu 24.04 or newer, x86_64."
  fi

  echo "[linux-publish] uploading updater artifact ($updater_size bytes)" >&2
  upload_artifact "$updater" "$updater_storage_path"
  echo "[linux-publish] uploading AppImage ($appimage_size bytes)" >&2
  upload_artifact "$appimage" "$appimage_storage_path"

  local payload
  payload="$(
    printf '{"app":%s,"version":%s,"target":%s,"arch":%s,"storage_bucket":%s,"storage_path":%s,"installer_storage_path":%s,"installer_sha256":%s,"installer_size_bytes":%s,"signature":%s,"notes":%s,"is_published":%s}' \
      "$(json_escape "$APP_NAME")" \
      "$(json_escape "$package_version")" \
      "$(json_escape "$TARGET")" \
      "$(json_escape "$ARCH")" \
      "$(json_escape "$SUPABASE_BUCKET")" \
      "$(json_escape "$updater_storage_path")" \
      "$(json_escape "$appimage_storage_path")" \
      "$(json_escape "$appimage_sha")" \
      "$appimage_size" \
      "$(json_escape "$signature_text")" \
      "$(json_escape "$notes")" \
      "$PUBLISH"
  )"

  echo "[linux-publish] writing release metadata (is_published=$PUBLISH)" >&2
  curl -s --fail-with-body \
    --request POST \
    --user-agent "$PUBLISHER_USER_AGENT" \
    --header "Content-Type: application/json" \
    --header "Prefer: resolution=merge-duplicates" \
    --data "$payload" \
    --config - \
    "$SUPABASE_PROJECT_URL/rest/v1/app_releases?on_conflict=app,version,target,arch" >/dev/null <<CURL_CONFIG
header = "Authorization: Bearer ${SUPABASE_SERVICE_ROLE_KEY}"
header = "apikey: ${SUPABASE_SERVICE_ROLE_KEY}"
CURL_CONFIG

  echo "[linux-publish] upload complete: $TARGET-$ARCH $package_version, published=$PUBLISH" >&2
  if [[ "$PUBLISH" != "true" ]]; then
    cat <<EOF

Release row is unpublished. After updater and download smoke testing, publish it with:

curl --fail-with-body \\
  --request PATCH \\
  --header "Authorization: Bearer \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "apikey: \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "Content-Type: application/json" \\
  --data '{"is_published":true}' \\
  "$SUPABASE_PROJECT_URL/rest/v1/app_releases?app=eq.$APP_NAME&version=eq.$package_version&target=eq.$TARGET&arch=eq.$ARCH"
EOF
  fi
}

main "$@"
