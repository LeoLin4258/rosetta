#!/usr/bin/env bash
# Upload the macOS Apple Silicon Tauri updater artifact to Supabase and create
# an unpublished release row. Run this only after producing a signed/notarized
# macOS release artifact and verifying the updater artifact.

set -euo pipefail

APP_NAME="${APP_NAME:-rosetta}"
SUPABASE_PROJECT_URL="${SUPABASE_PROJECT_URL:-https://bdujdewqopcgwijhfbcz.supabase.co}"
SUPABASE_BUCKET="${SUPABASE_BUCKET:-rosetta-releases}"
TARGET="${TARGET:-darwin}"
ARCH="${ARCH:-aarch64}"
NOTES_FILE="${NOTES_FILE:-}"
PUBLISH="${PUBLISH:-false}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$(cd "$TAURI_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"

log() {
  printf '[publish-macos-updater] %s\n' "$*" >&2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

require_env() {
  if [[ -z "${!1:-}" ]]; then
    log "missing required environment variable: $1"
    exit 2
  fi
}

json_escape() {
  node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1"
}

version() {
  cd "$APP_DIR"
  node -p "require('./package.json').version"
}

config_version() {
  cd "$APP_DIR"
  node -p "require('./src-tauri/tauri.conf.json').version"
}

cargo_version() {
  cd "$TAURI_DIR"
  cargo metadata --no-deps --format-version 1 | node -e 'const fs = require("fs"); const data = JSON.parse(fs.readFileSync(0, "utf8")); console.log(data.packages.find((pkg) => pkg.name === "rosetta-app").version)'
}

find_artifact() {
  local app_version="$1"
  local bundle_dir="$TAURI_DIR/target/release/bundle"
  local versioned_artifact

  if [[ ! -d "$bundle_dir" ]]; then
    return 0
  fi

  versioned_artifact="$(
    find "$bundle_dir" -type f \
      \( -name "*.app.tar.gz" -o -name "*.tar.gz" \) \
      ! -name "*.sig" \
      | awk -v version="$app_version" 'index($0, version) > 0' \
      | sort \
      | head -n 1
  )"

  if [[ -n "$versioned_artifact" ]]; then
    printf '%s\n' "$versioned_artifact"
    return 0
  fi

  find "$bundle_dir" -path "*/macos/*.app.tar.gz" -type f \
    ! -name "*.sig" \
    | sort \
    | head -n 1
}

main() {
  require_command node
  require_command cargo
  require_command curl
  require_env SUPABASE_SERVICE_ROLE_KEY

  local app_version tauri_version rust_version
  app_version="$(version)"
  tauri_version="$(config_version)"
  rust_version="$(cargo_version)"

  if [[ "$app_version" != "$tauri_version" || "$app_version" != "$rust_version" ]]; then
    log "version mismatch: package.json=$app_version tauri.conf.json=$tauri_version Cargo.toml=$rust_version"
    exit 2
  fi

  local artifact
  artifact="${UPDATER_ARTIFACT:-$(find_artifact "$app_version")}"

  if [[ -z "$artifact" || ! -f "$artifact" ]]; then
    log "could not find updater artifact for version $app_version under $TAURI_DIR/target/release/bundle"
    log "set UPDATER_ARTIFACT=/absolute/path/to/artifact to publish a specific file"
    exit 2
  fi

  local sig_file="$artifact.sig"
  if [[ ! -f "$sig_file" ]]; then
    log "missing signature file: $sig_file"
    exit 2
  fi

  local signature notes storage_path artifact_name artifact_size
  signature="$(tr -d '\n' < "$sig_file")"
  artifact_name="$(basename "$artifact")"
  artifact_size="$(wc -c < "$artifact" | tr -d ' ')"
  storage_path="macos/aarch64/$app_version/$artifact_name"

  if [[ -n "$NOTES_FILE" ]]; then
    notes="$(cat "$NOTES_FILE")"
  else
    notes="Rosetta $app_version"
  fi

  log "uploading $artifact_name ($artifact_size bytes) to $SUPABASE_BUCKET/$storage_path"
  curl --fail-with-body \
    --request POST \
    --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
    --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
    --header "Content-Type: application/octet-stream" \
    --header "x-upsert: true" \
    --data-binary "@$artifact" \
    "$SUPABASE_PROJECT_URL/storage/v1/object/$SUPABASE_BUCKET/$storage_path" >/dev/null

  local payload
  payload="$(
    printf '{"app":%s,"version":%s,"target":%s,"arch":%s,"storage_bucket":%s,"storage_path":%s,"signature":%s,"notes":%s,"is_published":%s}' \
      "$(json_escape "$APP_NAME")" \
      "$(json_escape "$app_version")" \
      "$(json_escape "$TARGET")" \
      "$(json_escape "$ARCH")" \
      "$(json_escape "$SUPABASE_BUCKET")" \
      "$(json_escape "$storage_path")" \
      "$(json_escape "$signature")" \
      "$(json_escape "$notes")" \
      "$(if [[ "$PUBLISH" == "true" ]]; then printf true; else printf false; fi)"
  )"

  log "upserting release metadata with is_published=$PUBLISH"
  curl --fail-with-body \
    --request POST \
    --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
    --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
    --header "Content-Type: application/json" \
    --header "Prefer: resolution=merge-duplicates" \
    --data "$payload" \
    "$SUPABASE_PROJECT_URL/rest/v1/app_releases?on_conflict=app,version,target,arch" >/dev/null

  log "uploaded updater artifact:"
  printf '  version: %s\n' "$app_version"
  printf '  platform: %s-%s\n' "$TARGET" "$ARCH"
  printf '  storage: %s/%s\n' "$SUPABASE_BUCKET" "$storage_path"
  printf '  published: %s\n' "$PUBLISH"

  if [[ "$PUBLISH" != "true" ]]; then
    cat <<EOF

Release row is unpublished. After smoke testing, publish it with:

curl --fail-with-body \\
  --request PATCH \\
  --header "Authorization: Bearer \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "apikey: \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "Content-Type: application/json" \\
  --data '{"is_published":true}' \\
  "$SUPABASE_PROJECT_URL/rest/v1/app_releases?app=eq.$APP_NAME&version=eq.$app_version&target=eq.$TARGET&arch=eq.$ARCH"
EOF
  fi
}

main "$@"
