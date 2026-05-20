#!/usr/bin/env bash
# Fetch the pdfium native library into `src-tauri/resources/pdf-sidecar/`
# so the Tauri bundle step can ship it.
#
# Typical usage right before `pnpm tauri build`:
#
#   bash src-tauri/scripts/fetch-pdfium.sh
#
# Or with a specific platform / pdfium release:
#
#   bash src-tauri/scripts/fetch-pdfium.sh --platform mac-arm64 --pdfium-tag chromium/7834
#
# Bash + curl + shasum + tar only. If the upstream host is unreachable, set
# HTTPS_PROXY in the environment before invoking — curl will pick it up
# automatically.

set -euo pipefail

# --- argument parsing --------------------------------------------------------

usage() {
  cat >&2 <<EOF
Usage: $0 [--platform <id>] [--pdfium-tag <tag>] [--skip-existing]

Options:
  --platform <id>     One of: mac-arm64 (default on Apple Silicon), mac-x64,
                      win-x64, linux-x64. Defaults to autodetect.
  --pdfium-tag <tag>  pdfium-binaries release tag (default: chromium/7834).
                      See https://github.com/bblanchon/pdfium-binaries/releases
  --skip-existing     Skip download if the target file already exists with the
                      expected SHA256.
  -h, --help          Show this message.
EOF
}

PDFIUM_TAG="chromium/7834"
PLATFORM=""
SKIP_EXISTING=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)     PLATFORM="$2"; shift 2 ;;
    --pdfium-tag)   PDFIUM_TAG="$2"; shift 2 ;;
    --skip-existing) SKIP_EXISTING=1; shift ;;
    -h|--help)      usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

# --- platform detection ------------------------------------------------------

if [[ -z "$PLATFORM" ]]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) PLATFORM="mac-arm64" ;;
    Darwin-x86_64) PLATFORM="mac-x64" ;;
    Linux-x86_64) PLATFORM="linux-x64" ;;
    *) echo "::error::cannot autodetect platform from $(uname -s)-$(uname -m); pass --platform" >&2; exit 2 ;;
  esac
fi

# --- known SHA256 hashes -----------------------------------------------------
# Pinned for chromium/7834. Update when bumping --pdfium-tag.
# To regenerate: download the file and run `shasum -a 256 <file>`.

PDFIUM_SHA_mac_arm64="2b733774416de02482281c0abc7589b08dc908896ecef2bfc31a85c5b5ffd572"
# TODO: fill these in when we add Mac x64 / Windows builds.
PDFIUM_SHA_mac_x64=""
PDFIUM_SHA_win_x64=""
PDFIUM_SHA_linux_x64=""

# --- target layout -----------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RES_DIR="$SRC_TAURI_DIR/resources/pdf-sidecar"

PDFIUM_DIR="$RES_DIR/pdfium/$PLATFORM"

mkdir -p "$PDFIUM_DIR"

# --- helpers -----------------------------------------------------------------

verify_sha() {
  local file="$1"
  local expected="$2"
  local actual
  actual=$(shasum -a 256 "$file" | awk '{print $1}')
  if [[ "$actual" != "$expected" ]]; then
    echo "::error::SHA256 mismatch for $file" >&2
    echo "  expected: $expected" >&2
    echo "  got:      $actual" >&2
    return 1
  fi
}

download_with_sha() {
  local url="$1"
  local dst="$2"
  local expected_sha="$3"

  if [[ "$SKIP_EXISTING" == "1" && -f "$dst" ]] && verify_sha "$dst" "$expected_sha" 2>/dev/null; then
    echo "[fetch-pdfium] reusing existing $dst" >&2
    return 0
  fi

  echo "[fetch-pdfium] downloading $url" >&2
  curl --fail --location --silent --show-error -o "$dst" "$url"
  verify_sha "$dst" "$expected_sha"
}

resolve_pdfium_sha() {
  # Look up the SHA constant by platform id. Bash 3 (macOS) doesn't have
  # associative arrays, so use indirect variable expansion.
  local key="PDFIUM_SHA_${PLATFORM//-/_}"
  echo "${!key:-}"
}

resolve_pdfium_lib_name() {
  case "$PLATFORM" in
    mac-arm64|mac-x64) echo "libpdfium.dylib" ;;
    linux-x64) echo "libpdfium.so" ;;
    win-x64) echo "pdfium.dll" ;;
    *) echo "::error::unsupported platform $PLATFORM" >&2; exit 2 ;;
  esac
}

# --- pdfium download ---------------------------------------------------------

PDFIUM_SHA="$(resolve_pdfium_sha)"
if [[ -z "$PDFIUM_SHA" ]]; then
  echo "::error::no pinned SHA256 for platform $PLATFORM at tag $PDFIUM_TAG; add it to this script" >&2
  exit 2
fi

PDFIUM_ARCHIVE_NAME="pdfium-${PLATFORM}.tgz"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG}/${PDFIUM_ARCHIVE_NAME}"

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

pdfium_archive="$tmp_root/$PDFIUM_ARCHIVE_NAME"
download_with_sha "$PDFIUM_URL" "$pdfium_archive" "$PDFIUM_SHA"

# Extract just the dynamic library.
extract_root="$tmp_root/extract"
mkdir -p "$extract_root"
tar -xzf "$pdfium_archive" -C "$extract_root"

lib_name="$(resolve_pdfium_lib_name)"
extracted_lib="$(find "$extract_root" -type f -name "$lib_name" | head -n1)"
if [[ -z "$extracted_lib" ]]; then
  echo "::error::could not locate $lib_name inside $pdfium_archive" >&2
  exit 1
fi

install -m 0644 "$extracted_lib" "$PDFIUM_DIR/$lib_name"
echo "[fetch-pdfium] installed $PDFIUM_DIR/$lib_name" >&2

# Also stage the upstream LICENSE so we can satisfy redistribution requirements.
license_src="$(find "$extract_root" -maxdepth 2 -type f -iname 'LICENSE*' | head -n1)"
if [[ -n "$license_src" ]]; then
  install -m 0644 "$license_src" "$PDFIUM_DIR/LICENSE.pdfium"
fi

# Record the pdfium release tag so the runtime can log it if needed.
echo "$PDFIUM_TAG" > "$PDFIUM_DIR/VERSION"

echo "[fetch-pdfium] done. Staged:" >&2
ls -lh "$PDFIUM_DIR/$lib_name" >&2
