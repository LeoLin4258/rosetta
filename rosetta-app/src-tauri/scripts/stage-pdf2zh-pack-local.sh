#!/usr/bin/env bash
# Build a local PDFMathTranslate pack directly into Rosetta's app-data layout.
#
# This is a dogfood/staging helper, not the final downloadable release-pack
# builder. It creates the path that `managed_pdf2zh` already probes:
#
#   ~/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar/pack/macos-arm64/bin/pdf2zh
#
# The pack ships a relocatable CPython (python-build-standalone "install_only"
# variant), NOT a `python -m venv` of the developer's system Python — a venv
# leaves absolute symlinks to /Library/Frameworks/.../python3.13 that break on
# end-user machines without that exact install.
#
# Typical usage from `rosetta-app/`:
#
#   bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
#   pnpm tauri dev
#
# Override knobs:
#
#   PDF2ZH_VERSION=1.7.9   pdf2zh package version to install
#   PBS_RELEASE=20260510   python-build-standalone release tag
#   PBS_PYTHON_VERSION=3.13.13   CPython version inside that PBS release
#   PBS_TARBALL_URL=...    full override of the PBS download URL

set -euo pipefail

PDF2ZH_VERSION="${PDF2ZH_VERSION:-1.7.9}"
PBS_RELEASE="${PBS_RELEASE:-20260510}"
PBS_PYTHON_VERSION="${PBS_PYTHON_VERSION:-3.13.13}"
PBS_DEFAULT_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PBS_PYTHON_VERSION}+${PBS_RELEASE}-aarch64-apple-darwin-install_only.tar.gz"
PBS_TARBALL_URL="${PBS_TARBALL_URL:-$PBS_DEFAULT_URL}"
APP_ID="${ROSETTA_APP_ID:-com.rosetta.desktop}"

if [[ "$(uname -s)-$(uname -m)" != "Darwin-arm64" ]]; then
  echo "::error::local pdf2zh pack staging currently supports macOS arm64 only" >&2
  exit 2
fi

PACK_ROOT="${ROSETTA_PDF2ZH_PACK_DIR:-$HOME/Library/Application Support/$APP_ID/pdf2zh-sidecar/pack/macos-arm64}"
PYTHON_DIR="$PACK_ROOT/python"
BIN_DIR="$PACK_ROOT/bin"

echo "[pdf2zh-pack] staging pdf2zh==$PDF2ZH_VERSION into:" >&2
echo "  $PACK_ROOT" >&2
echo "[pdf2zh-pack] PBS python: $PBS_PYTHON_VERSION (release $PBS_RELEASE)" >&2

rm -rf "$PACK_ROOT"
mkdir -p "$PACK_ROOT" "$BIN_DIR"

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

echo "[pdf2zh-pack] installing pdf2zh==$PDF2ZH_VERSION" >&2
"$PYTHON_DIR/bin/python" -m pip install --upgrade pip
"$PYTHON_DIR/bin/python" -m pip install "pdf2zh==$PDF2ZH_VERSION"

# pdf2zh 1.7.9 is not NumPy 2 compatible under Python 3.13:
# np.fromstring(binary) was removed. Patch the local staged copy so dogfood
# matches what the app currently expects. The real release pack should either
# pin a compatible environment or apply this patch during construction.
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
    raise SystemExit(f"::error::could not find expected NumPy call in {target}")

for cache_dir in root.rglob("__pycache__"):
    for child in cache_dir.iterdir():
        child.unlink()
    cache_dir.rmdir()
PY

cat > "$BIN_DIR/pdf2zh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACK_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
exec "$PACK_ROOT/python/bin/python" -m pdf2zh.pdf2zh "$@"
SH
chmod 0755 "$BIN_DIR/pdf2zh"

echo "[pdf2zh-pack] staged binary:" >&2
ls -lh "$BIN_DIR/pdf2zh" >&2
"$BIN_DIR/pdf2zh" --version >&2
