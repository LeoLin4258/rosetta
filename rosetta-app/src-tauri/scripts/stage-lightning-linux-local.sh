#!/usr/bin/env bash
# Stage the pinned Linux RWKV Lightning runtime and PTH model directly into
# Rosetta's app-data layout for development testing.

set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::local RWKV Lightning staging supports Linux x86_64 only" >&2
  exit 2
fi

APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"
APP_DATA_ROOT="${ROSETTA_APP_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID}"
MANAGED_ROOT="$APP_DATA_ROOT/managed-rwkv"
RUNTIME_DIR="$MANAGED_ROOT/runtimes/rwkv-lightning-cuda-sm75-gcc15-v1.0.3-linux-x64"
MODEL_DIR="$MANAGED_ROOT/models/rwkv7-0.4b-translate-pth"
DOWNLOAD_DIR="${ROSETTA_LIGHTNING_DOWNLOAD_DIR:-/tmp/rosetta-lightning-linux-x64}"

RUNTIME_ARCHIVE_NAME="RWKV_lightning_CUDA_sm75+_Linux_GCC15_V1.0.3.zip"
RUNTIME_ARCHIVE_URL="${LIGHTNING_ARCHIVE_URL:-https://github.com/Alic-Li/rwkv_lightning_cuda/releases/download/V1.0.3/$RUNTIME_ARCHIVE_NAME}"
RUNTIME_ARCHIVE_SIZE=430509983
RUNTIME_ARCHIVE_SHA256="403c34ddaa52661f3cd9d20bb4d4995036978bc0b8b0bf9119360a1655d21005"

MODEL_FILENAME="RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth"
MODEL_URL="${RWKV_PTH_URL:-https://hf-mirror.com/Alic-Li/RWKV_v7_G1_Translate/resolve/main/$MODEL_FILENAME}"
MODEL_SIZE=901775740
MODEL_SHA256="b9a1b013c3a938515f8b9bc23c28d815fa6f839eef77a943e92e7e70d35a0527"

verify_file() {
  local path="$1"
  local expected_size="$2"
  local expected_sha="$3"
  local actual_size actual_sha

  [[ -f "$path" ]] || return 1
  actual_size="$(stat -c '%s' "$path")"
  [[ "$actual_size" == "$expected_size" ]] || return 1
  actual_sha="$(sha256sum "$path" | awk '{print $1}')"
  [[ "$actual_sha" == "$expected_sha" ]]
}

download_verified() {
  local url="$1"
  local destination="$2"
  local expected_size="$3"
  local expected_sha="$4"

  if verify_file "$destination" "$expected_size" "$expected_sha"; then
    echo "[lightning-stage] reusing verified $destination" >&2
    return 0
  fi

  rm -f "$destination.partial"
  echo "[lightning-stage] downloading $url" >&2
  curl --fail --location --retry 5 -o "$destination.partial" "$url"
  if ! verify_file "$destination.partial" "$expected_size" "$expected_sha"; then
    echo "::error::download verification failed: $destination.partial" >&2
    exit 1
  fi
  mv "$destination.partial" "$destination"
}

mkdir -p "$DOWNLOAD_DIR" "$RUNTIME_DIR" "$MODEL_DIR"

if [[ -n "${LIGHTNING_ARCHIVE_FILE:-}" ]]; then
  RUNTIME_ARCHIVE="$LIGHTNING_ARCHIVE_FILE"
  verify_file "$RUNTIME_ARCHIVE" "$RUNTIME_ARCHIVE_SIZE" "$RUNTIME_ARCHIVE_SHA256" || {
    echo "::error::runtime archive verification failed: $RUNTIME_ARCHIVE" >&2
    exit 1
  }
else
  RUNTIME_ARCHIVE="$DOWNLOAD_DIR/$RUNTIME_ARCHIVE_NAME"
  download_verified "$RUNTIME_ARCHIVE_URL" "$RUNTIME_ARCHIVE" "$RUNTIME_ARCHIVE_SIZE" "$RUNTIME_ARCHIVE_SHA256"
fi

extract_root="$(mktemp -d)"
trap 'rm -rf "$extract_root"' EXIT
unzip -q "$RUNTIME_ARCHIVE" -d "$extract_root"
if [[ ! -f "$extract_root/rwkv_lighting_cuda/rwkv_lighting_cuda" ]]; then
  echo "::error::runtime archive is missing rwkv_lighting_cuda" >&2
  exit 1
fi
cp -a "$extract_root/rwkv_lighting_cuda/." "$RUNTIME_DIR/"
chmod 0755 "$RUNTIME_DIR/rwkv_lighting_cuda"

if [[ -n "${RWKV_PTH_FILE:-}" ]]; then
  MODEL_SOURCE="$RWKV_PTH_FILE"
  verify_file "$MODEL_SOURCE" "$MODEL_SIZE" "$MODEL_SHA256" || {
    echo "::error::PTH verification failed: $MODEL_SOURCE" >&2
    exit 1
  }
else
  MODEL_SOURCE="$DOWNLOAD_DIR/$MODEL_FILENAME"
  download_verified "$MODEL_URL" "$MODEL_SOURCE" "$MODEL_SIZE" "$MODEL_SHA256"
fi
install -m 0644 "$MODEL_SOURCE" "$MODEL_DIR/$MODEL_FILENAME"

echo "[lightning-stage] runtime:" >&2
LD_LIBRARY_PATH="$RUNTIME_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$RUNTIME_DIR/rwkv_lighting_cuda" --help >/dev/null
echo "[lightning-stage] model:" >&2
sha256sum "$MODEL_DIR/$MODEL_FILENAME" >&2
echo "[lightning-stage] staged into $MANAGED_ROOT" >&2
