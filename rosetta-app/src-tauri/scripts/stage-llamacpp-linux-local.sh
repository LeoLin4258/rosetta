#!/usr/bin/env bash
# Stage the pinned Linux llama.cpp Vulkan runtime and RWKV GGUF directly into
# Rosetta's app-data layout. This is for development testing, not release
# packaging, and does not install any system packages.

set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::local llama.cpp staging supports Linux x86_64 only" >&2
  exit 2
fi

APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"
APP_DATA_ROOT="${ROSETTA_APP_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID}"
MANAGED_ROOT="$APP_DATA_ROOT/managed-rwkv"
RUNTIME_DIR="$MANAGED_ROOT/runtimes/llama-cpp-vulkan-b9775-linux-x64"
MODEL_DIR="$MANAGED_ROOT/models/rwkv7-g1d-0.4b-translate-gguf-q8"
DOWNLOAD_DIR="${ROSETTA_LLAMACPP_DOWNLOAD_DIR:-/tmp/rosetta-llamacpp-linux-x64}"

RUNTIME_ARCHIVE_NAME="llama-b9775-bin-ubuntu-vulkan-x64.tar.gz"
RUNTIME_ARCHIVE_URL="${LLAMACPP_ARCHIVE_URL:-https://github.com/ggml-org/llama.cpp/releases/download/b9775/$RUNTIME_ARCHIVE_NAME}"
RUNTIME_ARCHIVE_SIZE=30904747
RUNTIME_ARCHIVE_SHA256="4cb7b0ea54f36613a0568b1929d29e76246d612a1ff5504c4d8043008131ba17"

MODEL_FILENAME="RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf"
MODEL_URL="${RWKV_GGUF_URL:-https://modelscope.cn/models/RWKV/rwkv-mobile-models/resolve/master/gguf/$MODEL_FILENAME}"
MODEL_SIZE=501498208
MODEL_SHA256="f0f1c64455d075236df309457e4730fe763489e5fc8c038ce3f29d9963dec96b"

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
    echo "[llamacpp-stage] reusing verified $destination" >&2
    return 0
  fi

  rm -f "$destination.partial"
  echo "[llamacpp-stage] downloading $url" >&2
  curl --fail --location --retry 5 -o "$destination.partial" "$url"
  if ! verify_file "$destination.partial" "$expected_size" "$expected_sha"; then
    echo "::error::download verification failed: $destination.partial" >&2
    exit 1
  fi
  mv "$destination.partial" "$destination"
}

mkdir -p "$DOWNLOAD_DIR" "$RUNTIME_DIR" "$MODEL_DIR"

if [[ -n "${LLAMACPP_ARCHIVE_FILE:-}" ]]; then
  RUNTIME_ARCHIVE="$LLAMACPP_ARCHIVE_FILE"
  if ! verify_file "$RUNTIME_ARCHIVE" "$RUNTIME_ARCHIVE_SIZE" "$RUNTIME_ARCHIVE_SHA256"; then
    echo "::error::runtime archive verification failed: $RUNTIME_ARCHIVE" >&2
    exit 1
  fi
else
  RUNTIME_ARCHIVE="$DOWNLOAD_DIR/$RUNTIME_ARCHIVE_NAME"
  download_verified "$RUNTIME_ARCHIVE_URL" "$RUNTIME_ARCHIVE" "$RUNTIME_ARCHIVE_SIZE" "$RUNTIME_ARCHIVE_SHA256"
fi

extract_root="$(mktemp -d)"
trap 'rm -rf "$extract_root"' EXIT
tar -xzf "$RUNTIME_ARCHIVE" -C "$extract_root"
if [[ ! -x "$extract_root/llama-b9775/llama-server" ]]; then
  echo "::error::runtime archive is missing llama-b9775/llama-server" >&2
  exit 1
fi
cp -a "$extract_root/llama-b9775/." "$RUNTIME_DIR/"
chmod 0755 "$RUNTIME_DIR/llama-server"

if [[ -n "${RWKV_GGUF_FILE:-}" ]]; then
  MODEL_SOURCE="$RWKV_GGUF_FILE"
  if ! verify_file "$MODEL_SOURCE" "$MODEL_SIZE" "$MODEL_SHA256"; then
    echo "::error::GGUF verification failed: $MODEL_SOURCE" >&2
    exit 1
  fi
else
  MODEL_SOURCE="$DOWNLOAD_DIR/$MODEL_FILENAME"
  download_verified "$MODEL_URL" "$MODEL_SOURCE" "$MODEL_SIZE" "$MODEL_SHA256"
fi
install -m 0644 "$MODEL_SOURCE" "$MODEL_DIR/$MODEL_FILENAME"

echo "[llamacpp-stage] runtime:" >&2
"$RUNTIME_DIR/llama-server" --version >&2
echo "[llamacpp-stage] Vulkan backend:" >&2
[[ -f "$RUNTIME_DIR/libggml-vulkan.so" ]]
ls -lh "$RUNTIME_DIR/libggml-vulkan.so" >&2
echo "[llamacpp-stage] model:" >&2
sha256sum "$MODEL_DIR/$MODEL_FILENAME" >&2
echo "[llamacpp-stage] staged into $MANAGED_ROOT" >&2
