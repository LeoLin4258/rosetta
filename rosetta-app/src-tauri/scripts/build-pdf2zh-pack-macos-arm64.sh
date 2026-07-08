#!/usr/bin/env bash
# Build a release-ready PDF layout component pack for macOS arm64.
#
# Unlike stage-pdf2zh-pack-local.sh (which installs directly into app-data for
# dogfood testing), this script builds into a clean temporary directory and
# produces a distributable archive with SHA256 checksum.
#
# The pack ships a relocatable CPython (python-build-standalone "install_only"
# variant), NOT a `python -m venv` of the developer's system Python — a venv
# leaves absolute symlinks to /Library/Frameworks/.../python3.13 that break on
# end-user machines without that exact install.
#
# Usage from rosetta-app/:
#
#   bash src-tauri/scripts/build-pdf2zh-pack-macos-arm64.sh
#
# Output:
#
#   dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz
#   dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz.sha256
#   dist/pdf-layout/manifest.json
#
# After the build, upload the .tar.gz and .sha256 to a GitHub Release under
# LeoLin4258/rosetta-assets with tag pdf-layout-pack-macos-arm64-vYYYY.MM.DD.N,
# then pin pack_download_urls / pack_sha256 / pack_size_bytes in profile.rs.
#
# Override knobs:
#
#   PDF2ZH_SOURCE_PATH=... local PDFMathTranslate fork checkout to install
#   PDF2ZH_VERSION=1.9.11  version label for the manifest
#   PBS_RELEASE=20260602   python-build-standalone release tag
#   PBS_PYTHON_VERSION=3.12.13   CPython version inside that PBS release
#   PBS_TARBALL_URL=...    full override of the PBS download URL
#   DOCLAYOUT_MODEL_URL=...  DocLayout ONNX model download URL
#   DOCLAYOUT_MODEL_FILE=... copy an already-downloaded DocLayout ONNX model
#   BABELDOC_FONT_SOURCE_DIR=... copy required BabelDOC fonts from this directory

set -euo pipefail

PDF2ZH_VERSION="${PDF2ZH_VERSION:-1.9.11}"
PDF2ZH_SOURCE_PATH="${PDF2ZH_SOURCE_PATH:-}"
PBS_RELEASE="${PBS_RELEASE:-20260602}"
PBS_PYTHON_VERSION="${PBS_PYTHON_VERSION:-3.12.13}"
PBS_DEFAULT_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PBS_PYTHON_VERSION}+${PBS_RELEASE}-aarch64-apple-darwin-install_only.tar.gz"
PBS_TARBALL_URL="${PBS_TARBALL_URL:-$PBS_DEFAULT_URL}"
DOCLAYOUT_MODEL_FILENAME="doclayout_yolo_docstructbench_imgsz1024.onnx"
DOCLAYOUT_MODEL_URL="${DOCLAYOUT_MODEL_URL:-https://huggingface.co/wybxc/DocLayout-YOLO-DocStructBench-onnx/resolve/main/$DOCLAYOUT_MODEL_FILENAME?download=true}"
DOCLAYOUT_MODEL_FILE="${DOCLAYOUT_MODEL_FILE:-}"
BABELDOC_FONT_SOURCE_DIR="${BABELDOC_FONT_SOURCE_DIR:-}"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "::error::pdf2zh release pack build requires macOS arm64" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$REPO_ROOT/dist/pdf-layout"
ARCHIVE_NAME="rosetta-pdf2zh-macos-arm64.tar.gz"
ARCHIVE_PATH="$DIST_DIR/$ARCHIVE_NAME"

if [[ -z "$PDF2ZH_SOURCE_PATH" ]]; then
  PDF2ZH_SOURCE_PATH="$(cd "$REPO_ROOT/../.." && pwd)/PDFMathTranslate"
fi
if [[ ! -f "$PDF2ZH_SOURCE_PATH/pyproject.toml" ]]; then
  echo "::error::PDFMathTranslate source checkout not found: $PDF2ZH_SOURCE_PATH" >&2
  echo "Set PDF2ZH_SOURCE_PATH to the fork checkout that contains pdf2zh/rosetta_engine.py." >&2
  exit 1
fi

BUILD_ROOT="$(mktemp -d)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

PACK_DIR="$BUILD_ROOT/macos-arm64"
PYTHON_DIR="$PACK_DIR/python"
BIN_DIR="$PACK_DIR/bin"
MODELS_DIR="$PACK_DIR/models"
BABELDOC_CACHE_DIR="$PACK_DIR/assets/babeldoc"
PBS_TARBALL="$BUILD_ROOT/pbs.tar.gz"

echo "[pdf2zh-release] building PDFMathTranslate fork: $PDF2ZH_SOURCE_PATH" >&2
echo "[pdf2zh-release] pdf2zh version label: $PDF2ZH_VERSION" >&2
echo "[pdf2zh-release] PBS python:  $PBS_PYTHON_VERSION (release $PBS_RELEASE)" >&2
echo "[pdf2zh-release] build root:  $BUILD_ROOT" >&2

mkdir -p "$PACK_DIR" "$BIN_DIR" "$MODELS_DIR"

echo "[pdf2zh-release] downloading python-build-standalone" >&2
echo "  $PBS_TARBALL_URL" >&2
curl -fsSL --retry 3 -o "$PBS_TARBALL" "$PBS_TARBALL_URL"

echo "[pdf2zh-release] extracting CPython into pack" >&2
tar -xzf "$PBS_TARBALL" -C "$PACK_DIR"

if [[ ! -x "$PYTHON_DIR/bin/python" ]]; then
  echo "::error::PBS tarball did not produce expected python/bin/python under $PACK_DIR" >&2
  exit 1
fi

PBS_REPORTED_VERSION="$("$PYTHON_DIR/bin/python" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
echo "[pdf2zh-release] PBS python ready: $PBS_REPORTED_VERSION" >&2

echo "[pdf2zh-release] installing PDFMathTranslate fork into pack python" >&2
"$PYTHON_DIR/bin/python" -m pip install --upgrade pip --quiet
"$PYTHON_DIR/bin/python" -m pip install "$PDF2ZH_SOURCE_PATH" --quiet

echo "[pdf2zh-release] applying NumPy 2 compatibility patch" >&2
"$PYTHON_DIR/bin/python" - <<'PY'
from pathlib import Path
import pdf2zh

root = Path(pdf2zh.__file__).resolve().parent
target = root / "high_level.py"
text = target.read_text()
old = "np.fromstring(pix.samples, np.uint8)"
new = "np.frombuffer(pix.samples, np.uint8)"
if old in text:
    target.write_text(text.replace(old, new))
    print(f"[pdf2zh-release] patched {target}")
elif new in text:
    print(f"[pdf2zh-release] patch already present in {target}")
else:
    raise SystemExit(f"::error::could not find expected NumPy call in {target}")
PY

echo "[pdf2zh-release] applying PDF color preservation patch" >&2
"$PYTHON_DIR/bin/python" "$SCRIPT_DIR/patch-pdf2zh-color-preservation.py"

echo "[pdf2zh-release] staging BabelDOC font assets" >&2
FONT_ARGS=(--cache-dir "$BABELDOC_CACHE_DIR")
if [[ -n "$BABELDOC_FONT_SOURCE_DIR" ]]; then
  FONT_ARGS+=(--font-source-dir "$BABELDOC_FONT_SOURCE_DIR")
fi
ROSETTA_BABELDOC_CACHE_DIR="$BABELDOC_CACHE_DIR" "$PYTHON_DIR/bin/python" "$SCRIPT_DIR/stage-pdf2zh-font-assets.py" "${FONT_ARGS[@]}"

DOCLAYOUT_MODEL_PATH="$MODELS_DIR/$DOCLAYOUT_MODEL_FILENAME"
if [[ -n "$DOCLAYOUT_MODEL_FILE" ]]; then
  echo "[pdf2zh-release] copying ONNX layout model:" >&2
  echo "  $DOCLAYOUT_MODEL_FILE" >&2
  cp "$DOCLAYOUT_MODEL_FILE" "$DOCLAYOUT_MODEL_PATH"
else
  echo "[pdf2zh-release] downloading ONNX layout model:" >&2
  echo "  $DOCLAYOUT_MODEL_URL" >&2
  curl -fL --retry 3 -o "$DOCLAYOUT_MODEL_PATH" "$DOCLAYOUT_MODEL_URL"
fi
if [[ ! -s "$DOCLAYOUT_MODEL_PATH" ]]; then
  echo "::error::ONNX layout model was not staged at $DOCLAYOUT_MODEL_PATH" >&2
  exit 1
fi

echo "[pdf2zh-release] Rosetta engine smoke test:" >&2
ROSETTA_DOCLAYOUT_MODEL="$DOCLAYOUT_MODEL_PATH" ROSETTA_BABELDOC_CACHE_DIR="$BABELDOC_CACHE_DIR" "$PYTHON_DIR/bin/python" - <<'PY'
import os

import pdf2zh
from babeldoc.assets.assets import get_font_and_metadata
from pdf2zh import rosetta_engine
from pdf2zh.doclayout import OnnxModel

if rosetta_engine.ENGINE_CONTRACT_VERSION != 2:
    raise SystemExit("::error::pdf2zh.rosetta_engine contract version is not 2")

for font_name in [
    "SourceHanSansCN-Regular.ttf",
    "SourceHanSansCN-Bold.ttf",
    "GoNotoKurrent-Regular.ttf",
]:
    font_path, _ = get_font_and_metadata(font_name)
    if not str(font_path).startswith(os.environ["ROSETTA_BABELDOC_CACHE_DIR"]):
        raise SystemExit(f"::error::BabelDOC font is not served from pack assets: {font_path}")

model = OnnxModel(os.environ["ROSETTA_DOCLAYOUT_MODEL"])
providers = ",".join(model.model.get_providers())
print(f"pdf-pack-imports-ok pdf2zh={pdf2zh.__version__} contract={rosetta_engine.ENGINE_CONTRACT_VERSION} providers={providers}")
PY

echo "[pdf2zh-release] removing Python bytecode caches" >&2
find "$PACK_DIR" \( -name '__pycache__' -type d -prune -exec rm -rf {} + \) -o \( -name '*.pyc' -type f -delete \)
rm -f "$DOCLAYOUT_MODEL_PATH.optimized"

cat > "$BIN_DIR/pdf2zh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export ROSETTA_BABELDOC_CACHE_DIR="${ROSETTA_BABELDOC_CACHE_DIR:-$PACK_ROOT/assets/babeldoc}"
export ROSETTA_DOCLAYOUT_MODEL="${ROSETTA_DOCLAYOUT_MODEL:-$PACK_ROOT/models/doclayout_yolo_docstructbench_imgsz1024.onnx}"
exec "$PACK_ROOT/python/bin/python" -m pdf2zh.pdf2zh "$@"
SH
chmod 0755 "$BIN_DIR/pdf2zh"

echo "[pdf2zh-release] in-place smoke test:" >&2
"$BIN_DIR/pdf2zh" --version >&2

echo "[pdf2zh-release] relocation smoke test (rename pack root, re-run shim):" >&2
RELOCATED_DIR="$BUILD_ROOT/macos-arm64-relocated"
mv "$PACK_DIR" "$RELOCATED_DIR"
"$RELOCATED_DIR/bin/pdf2zh" --version >&2
mv "$RELOCATED_DIR" "$PACK_DIR"

echo "[pdf2zh-release] verifying no stale bytecode:" >&2
STALE="$(find "$PACK_DIR" \( -name '__pycache__' -o -name '*.pyc' \) 2>/dev/null | head -5)"
if [[ -n "$STALE" ]]; then
  echo "::error::stale Python bytecode found after scrub:" >&2
  echo "$STALE" >&2
  exit 1
fi

echo "[pdf2zh-release] verifying no absolute symlinks leak developer paths:" >&2
LEAKED="$(find "$PACK_DIR" -type l -lname '/*' 2>/dev/null | head -5)"
if [[ -n "$LEAKED" ]]; then
  echo "::error::absolute symlinks present in pack (would break on user machines):" >&2
  echo "$LEAKED" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
rm -f "$ARCHIVE_PATH" "$ARCHIVE_PATH.sha256"

echo "[pdf2zh-release] archiving to: $ARCHIVE_PATH" >&2
tar -czf "$ARCHIVE_PATH" -C "$BUILD_ROOT" "macos-arm64"

SIZE_BYTES="$(stat -f '%z' "$ARCHIVE_PATH")"
SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"

echo "$SHA256  $ARCHIVE_NAME" > "$ARCHIVE_PATH.sha256"

cat > "$DIST_DIR/manifest.json" <<EOF
{
  "profile_id": "macos-arm64-pdf2zh",
  "pack_filename": "$ARCHIVE_NAME",
  "pdf2zh_version": "$PDF2ZH_VERSION",
  "pdf2zh_source_path": "$PDF2ZH_SOURCE_PATH",
  "python_runtime": "python-build-standalone $PBS_PYTHON_VERSION (release $PBS_RELEASE)",
  "layout_model": "$DOCLAYOUT_MODEL_FILENAME",
  "sha256": "$SHA256",
  "size_bytes": $SIZE_BYTES,
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

echo "[pdf2zh-release] done." >&2
ls -lh "$ARCHIVE_PATH" >&2
echo "[pdf2zh-release] size bytes:  $SIZE_BYTES" >&2
echo "[pdf2zh-release] sha256:      $SHA256" >&2
echo >&2
echo "[pdf2zh-release] next steps:" >&2
echo "  1. Create a GitHub Release under LeoLin4258/rosetta-assets" >&2
echo "     tag: pdf-layout-pack-macos-arm64-v$(date +%Y.%m.%d).1" >&2
echo "  2. Upload: $ARCHIVE_PATH" >&2
echo "     Upload: $ARCHIVE_PATH.sha256" >&2
echo "  3. Pin in src-tauri/src/managed_pdf2zh/profile.rs:" >&2
echo "     pack_size_bytes: Some($SIZE_BYTES)," >&2
echo "     pack_sha256: Some(\"$SHA256\")," >&2
echo "     pack_download_urls: &[\"https://github.com/LeoLin4258/rosetta-assets/releases/download/<TAG>/$ARCHIVE_NAME\"]," >&2
