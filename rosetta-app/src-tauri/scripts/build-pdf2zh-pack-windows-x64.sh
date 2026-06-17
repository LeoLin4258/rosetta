#!/usr/bin/env bash
# Build a release-ready PDF layout component pack for Windows x64.
#
# Mirrors build-pdf2zh-pack-macos-arm64.sh but targets Windows:
# - python-build-standalone for x86_64-pc-windows-msvc
# - No bash shim — the worker invokes python.exe directly via -m pdf2zh.pdf2zh
#
# Must run on a Windows machine (via Git Bash / MSYS2 / WSL with access to
# Windows python-build-standalone).
#
# Usage from rosetta-app/:
#
#   bash src-tauri/scripts/build-pdf2zh-pack-windows-x64.sh
#
# Output:
#
#   dist/pdf-layout/rosetta-pdf2zh-windows-x64.tar.gz
#   dist/pdf-layout/rosetta-pdf2zh-windows-x64.tar.gz.sha256
#   dist/pdf-layout/manifest.json
#
# After the build, upload the .tar.gz and .sha256 to a GitHub Release under
# LeoLin4258/rosetta-assets with tag pdf-layout-pack-windows-x64-vYYYY.MM.DD.N,
# then pin pack_download_urls / pack_sha256 / pack_size_bytes in profile.rs.
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
PBS_DEFAULT_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PBS_PYTHON_VERSION}+${PBS_RELEASE}-x86_64-pc-windows-msvc-install_only.tar.gz"
PBS_TARBALL_URL="${PBS_TARBALL_URL:-$PBS_DEFAULT_URL}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$REPO_ROOT/dist/pdf-layout"
ARCHIVE_NAME="rosetta-pdf2zh-windows-x64.tar.gz"
ARCHIVE_PATH="$DIST_DIR/$ARCHIVE_NAME"

BUILD_ROOT="$(mktemp -d)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

PACK_DIR="$BUILD_ROOT/windows-x64"
PYTHON_DIR="$PACK_DIR/python"
PBS_TARBALL="$BUILD_ROOT/pbs.tar.gz"

echo "[pdf2zh-release] building pdf2zh==$PDF2ZH_VERSION for windows-x64" >&2
echo "[pdf2zh-release] PBS python:  $PBS_PYTHON_VERSION (release $PBS_RELEASE)" >&2
echo "[pdf2zh-release] build root:  $BUILD_ROOT" >&2

mkdir -p "$PACK_DIR"

echo "[pdf2zh-release] downloading python-build-standalone" >&2
echo "  $PBS_TARBALL_URL" >&2
curl -fsSL --retry 3 -o "$PBS_TARBALL" "$PBS_TARBALL_URL"

echo "[pdf2zh-release] extracting CPython into pack" >&2
tar -xzf "$PBS_TARBALL" -C "$PACK_DIR"

if [[ ! -f "$PYTHON_DIR/python.exe" ]]; then
  echo "::error::PBS tarball did not produce expected python/python.exe under $PACK_DIR" >&2
  exit 1
fi

PBS_REPORTED_VERSION="$("$PYTHON_DIR/python.exe" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
echo "[pdf2zh-release] PBS python ready: $PBS_REPORTED_VERSION" >&2

echo "[pdf2zh-release] installing pdf2zh==$PDF2ZH_VERSION into pack python" >&2
"$PYTHON_DIR/python.exe" -m pip install --upgrade pip --quiet
"$PYTHON_DIR/python.exe" -m pip install "pdf2zh==$PDF2ZH_VERSION" --quiet

echo "[pdf2zh-release] applying NumPy 2 compatibility patch" >&2
"$PYTHON_DIR/python.exe" - <<'PY'
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
"$PYTHON_DIR/python.exe" "$SCRIPT_DIR/patch-pdf2zh-color-preservation.py"

echo "[pdf2zh-release] removing Python bytecode caches" >&2
find "$PACK_DIR" \( -name '__pycache__' -type d -prune -exec rm -rf {} + \) -o \( -name '*.pyc' -type f -delete \)

echo "[pdf2zh-release] smoke test:" >&2
"$PYTHON_DIR/python.exe" -m pdf2zh.pdf2zh --version >&2

echo "[pdf2zh-release] verifying no stale bytecode:" >&2
STALE="$(find "$PACK_DIR" \( -name '__pycache__' -o -name '*.pyc' \) 2>/dev/null | head -5)"
if [[ -n "$STALE" ]]; then
  echo "::error::stale Python bytecode found after scrub:" >&2
  echo "$STALE" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
rm -f "$ARCHIVE_PATH" "$ARCHIVE_PATH.sha256"

echo "[pdf2zh-release] archiving to: $ARCHIVE_PATH" >&2
tar -czf "$ARCHIVE_PATH" -C "$BUILD_ROOT" "windows-x64"

# Cross-platform size + sha256
if command -v sha256sum &>/dev/null; then
  SHA256="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')"
elif command -v shasum &>/dev/null; then
  SHA256="$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')"
else
  SHA256="$(openssl dgst -sha256 "$ARCHIVE_PATH" | awk '{print $NF}')"
fi

if stat --version &>/dev/null 2>&1; then
  SIZE_BYTES="$(stat -c '%s' "$ARCHIVE_PATH")"
else
  SIZE_BYTES="$(stat -f '%z' "$ARCHIVE_PATH")"
fi

echo "$SHA256  $ARCHIVE_NAME" > "$ARCHIVE_PATH.sha256"

cat > "$DIST_DIR/manifest.json" <<EOF
{
  "profile_id": "windows-x64-pdf2zh",
  "pack_filename": "$ARCHIVE_NAME",
  "pdf2zh_version": "$PDF2ZH_VERSION",
  "python_runtime": "python-build-standalone $PBS_PYTHON_VERSION (release $PBS_RELEASE)",
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
echo "     tag: pdf-layout-pack-windows-x64-v$(date +%Y.%m.%d).1" >&2
echo "  2. Upload: $ARCHIVE_PATH" >&2
echo "     Upload: $ARCHIVE_PATH.sha256" >&2
echo "  3. Pin in src-tauri/src/managed_pdf2zh/profile.rs:" >&2
echo "     pack_size_bytes: Some($SIZE_BYTES)," >&2
echo "     pack_sha256: Some(\"$SHA256\")," >&2
echo "     pack_download_urls: &[\"https://github.com/LeoLin4258/rosetta-assets/releases/download/<TAG>/$ARCHIVE_NAME\"]," >&2
