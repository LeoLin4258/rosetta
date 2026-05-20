#!/usr/bin/env bash
# Build a local PDFMathTranslate pack directly into Rosetta's app-data layout.
#
# This is a dogfood/staging helper, not the final downloadable release-pack
# builder. It creates the path that `managed_pdf2zh` already probes:
#
#   ~/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar/pack/macos-arm64/bin/pdf2zh
#
# Typical usage from `rosetta-app/`:
#
#   bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
#   pnpm tauri dev
#
# Override knobs:
#
#   PDF2ZH_VERSION=1.7.9 PYTHON=python3 bash src-tauri/scripts/stage-pdf2zh-pack-local.sh

set -euo pipefail

PDF2ZH_VERSION="${PDF2ZH_VERSION:-1.7.9}"
PYTHON_BIN="${PYTHON:-python3}"
APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "::error::local pdf2zh pack staging currently supports macOS arm64 only" >&2
  exit 2
fi

PACK_ROOT="${ROSETTA_PDF2ZH_PACK_DIR:-$HOME/Library/Application Support/$APP_ID/pdf2zh-sidecar/pack/macos-arm64}"
VENV_DIR="$PACK_ROOT/python"
BIN_DIR="$PACK_ROOT/bin"

echo "[pdf2zh-pack] staging pdf2zh==$PDF2ZH_VERSION into:" >&2
echo "  $PACK_ROOT" >&2

rm -rf "$PACK_ROOT"
mkdir -p "$BIN_DIR"

"$PYTHON_BIN" -m venv "$VENV_DIR"
"$VENV_DIR/bin/python" -m pip install --upgrade pip
"$VENV_DIR/bin/python" -m pip install "pdf2zh==$PDF2ZH_VERSION"

# pdf2zh 1.7.9 is not NumPy 2 compatible under Python 3.13:
# np.fromstring(binary) was removed. Patch the local staged copy so dogfood
# matches what the app currently expects. The real release pack should either
# pin a compatible environment or apply this patch during construction.
"$VENV_DIR/bin/python" - <<'PY'
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
    raise SystemExit(f"::error::could not find expected NumPy call in {target}")
PY

cat > "$BIN_DIR/pdf2zh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
exec "$PACK_ROOT/python/bin/python" -m pdf2zh.pdf2zh "$@"
SH
chmod 0755 "$BIN_DIR/pdf2zh"

echo "[pdf2zh-pack] staged binary:" >&2
ls -lh "$BIN_DIR/pdf2zh" >&2
"$BIN_DIR/pdf2zh" --version >&2
