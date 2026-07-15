#!/usr/bin/env bash
# Build the Rosetta Linux x64 AppImage and, for a real release, its signed
# Tauri updater archive.

set -euo pipefail

UNSIGNED_PREVIEW=0
ALLOW_DIRTY_PREVIEW=0

usage() {
  cat <<'EOF'
Usage: release-linux.sh [--unsigned-preview] [--allow-dirty-preview]

  --unsigned-preview     Build only the downloadable AppImage. Do not create
                         an updater archive or signature.
  --allow-dirty-preview  Allow a dirty worktree. Requires --unsigned-preview.

Real release builds require a clean worktree and TAURI_SIGNING_PRIVATE_KEY_PATH
or TAURI_SIGNING_PRIVATE_KEY. Preview artifacts must not be published.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --unsigned-preview)
      UNSIGNED_PREVIEW=1
      shift
      ;;
    --allow-dirty-preview)
      ALLOW_DIRTY_PREVIEW=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "::error::unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$ALLOW_DIRTY_PREVIEW" == "1" && "$UNSIGNED_PREVIEW" != "1" ]]; then
  echo "::error::--allow-dirty-preview requires --unsigned-preview" >&2
  exit 2
fi

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::Linux release builds require a Linux x86_64 host" >&2
  exit 2
fi

for command in cargo file git node pnpm sha256sum tar; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "::error::missing required command: $command" >&2
    exit 2
  fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$(cd "$TAURI_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist/release"
PDFIUM_LIBRARY="$TAURI_DIR/resources/pdf-sidecar/pdfium/linux-x64/libpdfium.so"

package_version="$(cd "$APP_DIR" && node -p "require('./package.json').version")"
tauri_version="$(cd "$APP_DIR" && node -p "require('./src-tauri/tauri.conf.json').version")"
cargo_version="$({ cd "$TAURI_DIR" && cargo metadata --no-deps --format-version 1; } | node -e 'const fs = require("fs"); const data = JSON.parse(fs.readFileSync(0, "utf8")); console.log(data.packages.find((pkg) => pkg.name === "rosetta-app").version)')"

if [[ "$package_version" != "$tauri_version" || "$package_version" != "$cargo_version" ]]; then
  echo "::error::version mismatch: package.json=$package_version tauri.conf.json=$tauri_version Cargo.toml=$cargo_version" >&2
  exit 2
fi

worktree_status="$(git -C "$REPO_ROOT" status --porcelain --untracked-files=all)"
if [[ -n "$worktree_status" && "$ALLOW_DIRTY_PREVIEW" != "1" ]]; then
  echo "::error::release builds require a clean worktree" >&2
  printf '%s\n' "$worktree_status" >&2
  exit 2
fi
if [[ -n "$worktree_status" ]]; then
  echo "::warning::building a non-publishable preview from a dirty worktree" >&2
fi

if [[ ! -f "$PDFIUM_LIBRARY" ]]; then
  echo "::error::missing staged Linux PDFium library: $PDFIUM_LIBRARY" >&2
  echo "Run: bash src-tauri/scripts/fetch-pdfium.sh --platform linux-x64" >&2
  exit 2
fi
if ! file "$PDFIUM_LIBRARY" | grep -q 'ELF 64-bit.*x86-64'; then
  echo "::error::staged PDFium library is not Linux x86_64: $PDFIUM_LIBRARY" >&2
  file "$PDFIUM_LIBRARY" >&2
  exit 2
fi

if [[ "$UNSIGNED_PREVIEW" != "1" ]]; then
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    echo "::error::set TAURI_SIGNING_PRIVATE_KEY_PATH or TAURI_SIGNING_PRIVATE_KEY" >&2
    exit 2
  fi
  if [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" && ! -f "$TAURI_SIGNING_PRIVATE_KEY_PATH" ]]; then
    echo "::error::TAURI_SIGNING_PRIVATE_KEY_PATH does not exist: $TAURI_SIGNING_PRIVATE_KEY_PATH" >&2
    exit 2
  fi

  signing_public_key_path="${TAURI_SIGNING_PUBLIC_KEY_PATH:-}"
  if [[ -z "$signing_public_key_path" && -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    signing_public_key_path="$TAURI_SIGNING_PRIVATE_KEY_PATH.pub"
  fi
  if [[ -z "$signing_public_key_path" || ! -f "$signing_public_key_path" ]]; then
    echo "::error::set TAURI_SIGNING_PUBLIC_KEY_PATH or place the public key next to the private key as <private-key>.pub" >&2
    exit 2
  fi

  configured_public_key="$(cd "$APP_DIR" && node -p "require('./src-tauri/tauri.conf.json').plugins.updater.pubkey.trim()")"
  release_public_key="$(tr -d '\r\n' < "$signing_public_key_path")"
  if [[ "$configured_public_key" != "$release_public_key" ]]; then
    echo "::error::release updater public key does not match tauri.conf.json" >&2
    exit 2
  fi
fi

echo "[linux-release] building Rosetta $package_version AppImage" >&2
(
  cd "$APP_DIR"
  pnpm tauri build \
    --config src-tauri/tauri.linux.conf.json \
    --bundles appimage \
    --no-sign
)

BUNDLE_DIR="$TAURI_DIR/target/release/bundle/appimage"
built_appimage="$BUNDLE_DIR/Rosetta_${package_version}_amd64.AppImage"
if [[ ! -f "$built_appimage" ]]; then
  echo "::error::expected AppImage was not produced: $built_appimage" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
appimage="$DIST_DIR/Rosetta-$package_version-linux-x64.AppImage"
updater="$appimage.tar.gz"
rm -f "$appimage" "$appimage.sha256" "$appimage.size" \
  "$updater" "$updater.sig" "$updater.sha256" "$updater.size"
install -m 0755 "$built_appimage" "$appimage"

if ! file "$appimage" | grep -q 'ELF 64-bit.*x86-64'; then
  echo "::error::built AppImage is not Linux x86_64: $appimage" >&2
  file "$appimage" >&2
  exit 1
fi

appimage_sha256="$(sha256sum "$appimage" | awk '{print $1}')"
appimage_size="$(stat -c '%s' "$appimage")"
printf '%s  %s\n' "$appimage_sha256" "$(basename "$appimage")" > "$appimage.sha256"
printf '%s\n' "$appimage_size" > "$appimage.size"

echo "[linux-release] AppImage ready: $appimage" >&2
echo "[linux-release] AppImage size: $appimage_size" >&2
echo "[linux-release] AppImage sha256: $appimage_sha256" >&2

if [[ "$UNSIGNED_PREVIEW" == "1" ]]; then
  echo "[linux-release] unsigned preview complete; do not publish this artifact" >&2
  exit 0
fi

echo "[linux-release] creating Tauri updater archive" >&2
tar -czf "$updater" -C "$DIST_DIR" "$(basename "$appimage")"

signer_args=(--silent tauri signer sign)
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
  signer_args+=(-f "$TAURI_SIGNING_PRIVATE_KEY_PATH")
fi
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
  signer_args+=(--password=)
fi
signer_args+=("$updater")
(
  cd "$APP_DIR"
  pnpm "${signer_args[@]}" >/dev/null
)

if [[ ! -s "$updater.sig" ]]; then
  echo "::error::Tauri updater signature was not created: $updater.sig" >&2
  exit 1
fi

updater_sha256="$(sha256sum "$updater" | awk '{print $1}')"
updater_size="$(stat -c '%s' "$updater")"
printf '%s  %s\n' "$updater_sha256" "$(basename "$updater")" > "$updater.sha256"
printf '%s\n' "$updater_size" > "$updater.size"

echo "[linux-release] signed updater ready: $updater" >&2
echo "[linux-release] updater size: $updater_size" >&2
echo "[linux-release] updater sha256: $updater_sha256" >&2
