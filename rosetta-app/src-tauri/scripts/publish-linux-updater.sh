#!/usr/bin/env bash
# Upload a signed Linux x64 AppImage release to GitHub Releases and create an
# unpublished Supabase release row. Publishing remains a separate action.

set -euo pipefail

APP_NAME="${APP_NAME:-rosetta}"
SUPABASE_PROJECT_URL="${SUPABASE_PROJECT_URL:-https://bdujdewqopcgwijhfbcz.supabase.co}"
SUPABASE_BUCKET="${SUPABASE_BUCKET:-rosetta-releases}"
PUBLISHER_USER_AGENT="${PUBLISHER_USER_AGENT:-Rosetta-Release-Publisher/1.0}"
GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-LeoLin4258/rosetta}"
GITHUB_RELEASE_TAG="${GITHUB_RELEASE_TAG:-}"
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

authorization_config_line() {
  if [[ "$SUPABASE_SERVICE_ROLE_KEY" != sb_secret_* ]]; then
    printf 'header = "Authorization: Bearer %s"\n' "$SUPABASE_SERVICE_ROLE_KEY"
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

main() {
  for command in awk cargo curl file gh git grep head mktemp node sha256sum stat tar; do
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
  if [[ "$GITHUB_REPOSITORY" != "LeoLin4258/rosetta" ]]; then
    echo "::error::Linux application releases must use GITHUB_REPOSITORY=LeoLin4258/rosetta" >&2
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
  local release_tag release_commit updater_url installer_url release_json existing_commit
  appimage_sha="$(sha256sum "$appimage" | awk '{ print $1 }')"
  appimage_size="$(stat -c '%s' "$appimage")"
  updater_size="$(stat -c '%s' "$updater")"
  signature_text="$(tr -d '\r\n' < "$signature")"
  if [[ -z "$signature_text" ]]; then
    echo "::error::updater signature is empty: $signature" >&2
    exit 2
  fi

  release_tag="${GITHUB_RELEASE_TAG:-v$package_version}"
  if [[ ! "$release_tag" =~ ^[A-Za-z0-9._-]+$ ]]; then
    echo "::error::invalid GitHub release tag: $release_tag" >&2
    exit 2
  fi
  if [[ -n "$NOTES_FILE" ]]; then
    notes="$(<"$NOTES_FILE")"
  else
    notes="Rosetta $package_version for Ubuntu 24.04 or newer, x86_64."
  fi

  if [[ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=no)" ]]; then
    echo "::error::GitHub release uploads require a clean tracked worktree" >&2
    git -C "$REPO_ROOT" status --short --untracked-files=no >&2
    exit 2
  fi
  release_commit="$(git -C "$REPO_ROOT" rev-parse HEAD)"
  if ! gh api "repos/$GITHUB_REPOSITORY/commits/$release_commit" >/dev/null; then
    echo "::error::release commit is not available on GitHub: $release_commit" >&2
    exit 2
  fi

  if release_json="$(gh release view "$release_tag" --repo "$GITHUB_REPOSITORY" --json isDraft,isPrerelease 2>/dev/null)"; then
    if [[ "$(node -e 'const value=JSON.parse(process.argv[1]); process.stdout.write(String(!value.isDraft && value.isPrerelease))' "$release_json")" != "true" ]]; then
      echo "::error::existing GitHub release must be a published prerelease: $release_tag" >&2
      exit 2
    fi
    existing_commit="$(gh api "repos/$GITHUB_REPOSITORY/commits/$release_tag" --jq .sha)"
    if [[ "$existing_commit" != "$release_commit" ]]; then
      echo "::error::GitHub release $release_tag points to $existing_commit, expected $release_commit" >&2
      exit 2
    fi
  else
    echo "[linux-publish] creating GitHub prerelease $release_tag" >&2
    gh release create "$release_tag" \
      --repo "$GITHUB_REPOSITORY" \
      --target "$release_commit" \
      --title "Rosetta $package_version" \
      --notes "$notes" \
      --prerelease
  fi

  echo "[linux-publish] uploading GitHub release assets" >&2
  gh release upload "$release_tag" \
    "$appimage" "$appimage.sha256" "$appimage.size" \
    "$updater" "$updater.sha256" "$signature" "$updater.size" \
    --repo "$GITHUB_REPOSITORY"

  updater_url="https://github.com/$GITHUB_REPOSITORY/releases/download/$release_tag/$updater_name"
  installer_url="https://github.com/$GITHUB_REPOSITORY/releases/download/$release_tag/$appimage_name"

  local payload
  payload="$(
    printf '{"app":%s,"version":%s,"target":%s,"arch":%s,"storage_bucket":%s,"storage_path":null,"updater_url":%s,"installer_storage_path":null,"installer_url":%s,"installer_sha256":%s,"installer_size_bytes":%s,"signature":%s,"notes":%s,"is_published":%s}' \
      "$(json_escape "$APP_NAME")" \
      "$(json_escape "$package_version")" \
      "$(json_escape "$TARGET")" \
      "$(json_escape "$ARCH")" \
      "$(json_escape "$SUPABASE_BUCKET")" \
      "$(json_escape "$updater_url")" \
      "$(json_escape "$installer_url")" \
      "$(json_escape "$appimage_sha")" \
      "$appimage_size" \
      "$(json_escape "$signature_text")" \
      "$(json_escape "$notes")" \
      "$PUBLISH"
  )"

  local response_file
  response_file="$(mktemp)"
  echo "[linux-publish] writing release metadata (is_published=$PUBLISH)" >&2
  if ! curl -sS --fail-with-body \
    --request POST \
    --user-agent "$PUBLISHER_USER_AGENT" \
    --header "Content-Type: application/json" \
    --header "Prefer: resolution=merge-duplicates" \
    --data "$payload" \
    --output "$response_file" \
    --config - \
    "$SUPABASE_PROJECT_URL/rest/v1/app_releases?on_conflict=app,version,target,arch" <<CURL_CONFIG
$(authorization_config_line)
header = "apikey: ${SUPABASE_SERVICE_ROLE_KEY}"
CURL_CONFIG
  then
    echo "::error::failed to write Linux release metadata" >&2
    head -c 4096 "$response_file" >&2
    echo >&2
    rm -f "$response_file"
    return 1
  fi
  rm -f "$response_file"

  echo "[linux-publish] upload complete: $TARGET-$ARCH $package_version, published=$PUBLISH" >&2
  echo "[linux-publish] GitHub release: https://github.com/$GITHUB_REPOSITORY/releases/tag/$release_tag" >&2
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
