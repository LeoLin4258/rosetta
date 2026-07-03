#!/usr/bin/env bash
# Build a local PDFMathTranslate pack directly into Rosetta's app-data layout.
#
# This is a dogfood/staging helper, not the final downloadable release-pack
# builder. It creates the path that `managed_pdf2zh` already probes:
#
#   ~/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar/pack/macos-arm64/bin/pdf2zh
#
# The pack ships a relocatable CPython (python-build-standalone "install_only"
# variant), NOT a `python -m venv` of the developer's system Python. A venv
# leaves absolute symlinks to /Library/Frameworks/... that break on end-user
# machines without that exact install.
#
# Typical usage from `rosetta-app/`:
#
#   bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
#
# Override knobs:
#
#   PDF2ZH_SOURCE_PATH=... local PDFMathTranslate fork checkout to install
#   PDF2ZH_VERSION=1.9.11  version label for logging
#   PBS_RELEASE=20260602   python-build-standalone release tag
#   PBS_PYTHON_VERSION=3.12.13   CPython version inside that PBS release
#   PBS_TARBALL_URL=...    full override of the PBS download URL
#   DOCLAYOUT_MODEL_URL=...  DocLayout ONNX model download URL
#   DOCLAYOUT_MODEL_FILE=... copy an already-downloaded DocLayout ONNX model

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
APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROSETTA_APP_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "::error::local pdf2zh pack staging currently supports macOS arm64 only" >&2
  exit 2
fi

if [[ -z "$PDF2ZH_SOURCE_PATH" ]]; then
  PDF2ZH_SOURCE_PATH="$(cd "$ROSETTA_APP_DIR/../.." && pwd)/PDFMathTranslate"
fi
if [[ ! -f "$PDF2ZH_SOURCE_PATH/pyproject.toml" ]]; then
  echo "::error::PDFMathTranslate source checkout not found: $PDF2ZH_SOURCE_PATH" >&2
  echo "Set PDF2ZH_SOURCE_PATH to the fork checkout that contains pdf2zh/rosetta_engine.py." >&2
  exit 1
fi

PACK_ROOT="${ROSETTA_PDF2ZH_PACK_DIR:-$HOME/Library/Application Support/$APP_ID/pdf2zh-sidecar/pack/macos-arm64}"
PYTHON_DIR="$PACK_ROOT/python"
BIN_DIR="$PACK_ROOT/bin"
MODELS_DIR="$PACK_ROOT/models"

echo "[pdf2zh-pack] staging PDFMathTranslate fork into:" >&2
echo "  $PACK_ROOT" >&2
echo "[pdf2zh-pack] source: $PDF2ZH_SOURCE_PATH" >&2
echo "[pdf2zh-pack] version label: $PDF2ZH_VERSION" >&2
echo "[pdf2zh-pack] PBS python: $PBS_PYTHON_VERSION (release $PBS_RELEASE)" >&2

rm -rf "$PACK_ROOT"
mkdir -p "$PACK_ROOT" "$BIN_DIR" "$MODELS_DIR"

DOWNLOAD_TMP="$(mktemp -d)"
trap 'rm -rf "$DOWNLOAD_TMP"' EXIT

PBS_TARBALL="$DOWNLOAD_TMP/pbs.tar.gz"

echo "[pdf2zh-pack] downloading python-build-standalone" >&2
echo "  $PBS_TARBALL_URL" >&2
curl -fsSL --retry 3 -o "$PBS_TARBALL" "$PBS_TARBALL_URL"

echo "[pdf2zh-pack] extracting CPython into pack" >&2
tar -xzf "$PBS_TARBALL" -C "$PACK_ROOT"

if [[ ! -x "$PYTHON_DIR/bin/python" ]]; then
  echo "::error::PBS tarball did not produce expected python/bin/python under $PACK_ROOT" >&2
  exit 1
fi

PBS_REPORTED_VERSION="$("$PYTHON_DIR/bin/python" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
echo "[pdf2zh-pack] PBS python ready: $PBS_REPORTED_VERSION" >&2

echo "[pdf2zh-pack] installing PDFMathTranslate fork" >&2
"$PYTHON_DIR/bin/python" -m pip install --upgrade pip
"$PYTHON_DIR/bin/python" -m pip install "$PDF2ZH_SOURCE_PATH"

echo "[pdf2zh-pack] applying NumPy compatibility patch if needed" >&2
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
    print(f"[pdf2zh-pack] patched {target}")
elif new in text:
    print(f"[pdf2zh-pack] patch already present in {target}")
else:
    print(f"[pdf2zh-pack] NumPy compatibility patch not needed for {target}")
PY

echo "[pdf2zh-pack] applying PDF color preservation patch" >&2
"$PYTHON_DIR/bin/python" "$SCRIPT_DIR/patch-pdf2zh-color-preservation.py"

DOCLAYOUT_MODEL_PATH="$MODELS_DIR/$DOCLAYOUT_MODEL_FILENAME"
if [[ -n "$DOCLAYOUT_MODEL_FILE" ]]; then
  echo "[pdf2zh-pack] copying ONNX layout model" >&2
  echo "  $DOCLAYOUT_MODEL_FILE" >&2
  cp "$DOCLAYOUT_MODEL_FILE" "$DOCLAYOUT_MODEL_PATH"
else
  echo "[pdf2zh-pack] downloading ONNX layout model" >&2
  echo "  $DOCLAYOUT_MODEL_URL" >&2
  curl -fL --retry 3 -o "$DOCLAYOUT_MODEL_PATH" "$DOCLAYOUT_MODEL_URL"
fi
if [[ ! -s "$DOCLAYOUT_MODEL_PATH" ]]; then
  echo "::error::ONNX layout model was not staged at $DOCLAYOUT_MODEL_PATH" >&2
  exit 1
fi

echo "[pdf2zh-pack] Rosetta engine smoke test:" >&2
ROSETTA_DOCLAYOUT_MODEL="$DOCLAYOUT_MODEL_PATH" "$PYTHON_DIR/bin/python" - <<'PY'
import os

import pdf2zh
from pdf2zh import rosetta_engine
from pdf2zh.doclayout import OnnxModel

if rosetta_engine.ENGINE_CONTRACT_VERSION != 2:
    raise SystemExit("::error::pdf2zh.rosetta_engine contract version is not 2")

model = OnnxModel(os.environ["ROSETTA_DOCLAYOUT_MODEL"])
providers = ",".join(model.model.get_providers())
print(f"pdf-pack-imports-ok pdf2zh={pdf2zh.__version__} contract={rosetta_engine.ENGINE_CONTRACT_VERSION} providers={providers}")
PY
rm -f "$DOCLAYOUT_MODEL_PATH.optimized"

cat > "$BIN_DIR/pdf2zh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export ROSETTA_DOCLAYOUT_MODEL="${ROSETTA_DOCLAYOUT_MODEL:-$PACK_ROOT/models/doclayout_yolo_docstructbench_imgsz1024.onnx}"
exec "$PACK_ROOT/python/bin/python" -m pdf2zh.pdf2zh "$@"
SH
chmod 0755 "$BIN_DIR/pdf2zh"

echo "[pdf2zh-pack] staged binary:" >&2
ls -lh "$BIN_DIR/pdf2zh" >&2
ls -lh "$DOCLAYOUT_MODEL_PATH" >&2
"$BIN_DIR/pdf2zh" --version >&2
