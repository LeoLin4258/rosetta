#!/usr/bin/env bash
# Archive the staged local PDFMathTranslate pack into a .tar.gz dogfood bundle.
#
# This is the companion to `stage-pdf2zh-pack-local.sh`. It does not build or
# install pdf2zh; it only packages an already-staged pack so the managed
# installer path can be tested through `ROSETTA_PDF2ZH_PACK_URL=file://...`.
#
# Typical usage from `rosetta-app/`:
#
#   bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
#   bash src-tauri/scripts/archive-pdf2zh-pack-local.sh
#   ROSETTA_PDF2ZH_PACK_URL="file:///tmp/rosetta-pdf2zh-macos-arm64.tar.gz" pnpm tauri dev
#
# Override knobs:
#
#   ROSETTA_APP_ID=com.rosetta.desktop
#   ROSETTA_PDF2ZH_PACK_DIR="/path/to/macos-arm64"
#   ROSETTA_PDF2ZH_ARCHIVE="/tmp/rosetta-pdf2zh-macos-arm64.tar.gz"

set -euo pipefail

APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"
PACK_DIR="${ROSETTA_PDF2ZH_PACK_DIR:-$HOME/Library/Application Support/$APP_ID/pdf2zh-sidecar/pack/macos-arm64}"
ARCHIVE_PATH="${ROSETTA_PDF2ZH_ARCHIVE:-/tmp/rosetta-pdf2zh-macos-arm64.tar.gz}"
PACK_NAME="$(basename "$PACK_DIR")"
PACK_PARENT="$(cd "$(dirname "$PACK_DIR")" && pwd)"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "::error::local pdf2zh pack archiving currently supports macOS arm64 only" >&2
  exit 2
fi

if [[ ! -d "$PACK_DIR" ]]; then
  echo "::error::staged pdf2zh pack not found: $PACK_DIR" >&2
  echo "Run: bash src-tauri/scripts/stage-pdf2zh-pack-local.sh" >&2
  exit 2
fi

if [[ ! -x "$PACK_DIR/bin/pdf2zh" ]]; then
  echo "::error::staged pdf2zh binary is missing or not executable: $PACK_DIR/bin/pdf2zh" >&2
  exit 2
fi
DOCLAYOUT_MODEL_PATH="$PACK_DIR/models/doclayout_yolo_docstructbench_imgsz1024.pt"
if [[ ! -s "$DOCLAYOUT_MODEL_PATH" ]]; then
  echo "::error::staged DocLayout-YOLO model is missing: $DOCLAYOUT_MODEL_PATH" >&2
  exit 2
fi

echo "[pdf2zh-pack] removing Python bytecode caches from staged pack" >&2
find "$PACK_DIR" \( -name '__pycache__' -type d -prune -exec rm -rf {} + \) -o \( -name '*.pyc' -type f -delete \)

mkdir -p "$(dirname "$ARCHIVE_PATH")"
rm -f "$ARCHIVE_PATH"

echo "[pdf2zh-pack] archiving:" >&2
echo "  source: $PACK_DIR" >&2
echo "  target: $ARCHIVE_PATH" >&2

tar -czf "$ARCHIVE_PATH" -C "$PACK_PARENT" "$PACK_NAME"

SIZE_BYTES="$(stat -f '%z' "$ARCHIVE_PATH")"
SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"

echo "[pdf2zh-pack] archive ready:" >&2
ls -lh "$ARCHIVE_PATH" >&2
echo "[pdf2zh-pack] size bytes: $SIZE_BYTES" >&2
echo "[pdf2zh-pack] sha256: $SHA256" >&2
echo >&2
echo "[pdf2zh-pack] dogfood command:" >&2
echo "  ROSETTA_PDF2ZH_PACK_URL=\"file://$ARCHIVE_PATH\" pnpm tauri dev" >&2
