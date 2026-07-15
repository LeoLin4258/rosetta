#!/usr/bin/env bash
# Build the release-ready Rosetta PDF layout component for Linux x64.

set -euo pipefail

PDF2ZH_VERSION="${PDF2ZH_VERSION:-1.9.11}"
PDF2ZH_COMMIT="${PDF2ZH_COMMIT:-990bed055d372772f5cec8ef4a982a8f767d64a4}"
PDF2ZH_REPOSITORY_URL="${PDF2ZH_REPOSITORY_URL:-https://github.com/LeoLin4258/PDFMathTranslate.git}"
PDF2ZH_SOURCE_PATH="${PDF2ZH_SOURCE_PATH:-}"
PBS_RELEASE="${PBS_RELEASE:-20260602}"
PBS_PYTHON_VERSION="${PBS_PYTHON_VERSION:-3.12.13}"
PBS_DEFAULT_URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PBS_PYTHON_VERSION}+${PBS_RELEASE}-x86_64-unknown-linux-gnu-install_only.tar.gz"
PBS_TARBALL_URL="${PBS_TARBALL_URL:-$PBS_DEFAULT_URL}"
PBS_TARBALL_FILE="${PBS_TARBALL_FILE:-}"
PIP_INDEX_URL="${PDF2ZH_PIP_INDEX_URL:-https://pypi.org/simple}"
DOCLAYOUT_MODEL_FILENAME="doclayout_yolo_docstructbench_imgsz1024.onnx"
DOCLAYOUT_MODEL_URL="${DOCLAYOUT_MODEL_URL:-https://huggingface.co/wybxc/DocLayout-YOLO-DocStructBench-onnx/resolve/main/$DOCLAYOUT_MODEL_FILENAME?download=true}"
DOCLAYOUT_MODEL_FILE="${DOCLAYOUT_MODEL_FILE:-}"
BABELDOC_FONT_SOURCE_DIR="${BABELDOC_FONT_SOURCE_DIR:-}"

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::pdf2zh Linux release pack build requires Linux x86_64" >&2
  exit 2
fi

for command in curl git ldd sha256sum tar; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "::error::missing required command: $command" >&2
    exit 2
  fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"
DIST_DIR="${ROSETTA_PDF2ZH_DIST_DIR:-$APP_DIR/dist/pdf-layout}"
DOWNLOAD_DIR="${ROSETTA_PDF2ZH_DOWNLOAD_DIR:-$HOME/.cache/rosetta/pdf2zh-linux-release}"
REQUIREMENTS="$SCRIPT_DIR/requirements-pdf2zh-linux-x64.txt"
ARCHIVE_NAME="rosetta-pdf2zh-linux-x64.tar.gz"
ARCHIVE_PATH="$DIST_DIR/$ARCHIVE_NAME"
BUILD_ROOT="$(mktemp -d)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

if [[ ! -f "$REQUIREMENTS" ]]; then
  echo "::error::missing Linux PDF runtime requirements: $REQUIREMENTS" >&2
  exit 2
fi

if [[ -z "$PDF2ZH_SOURCE_PATH" ]]; then
  PDF2ZH_SOURCE_PATH="$BUILD_ROOT/PDFMathTranslate"
  echo "[pdf2zh-linux-release] cloning pinned PDFMathTranslate fork" >&2
  git clone --filter=blob:none --no-checkout "$PDF2ZH_REPOSITORY_URL" "$PDF2ZH_SOURCE_PATH" >&2
  git -C "$PDF2ZH_SOURCE_PATH" fetch --depth 1 origin "$PDF2ZH_COMMIT" >&2
  git -C "$PDF2ZH_SOURCE_PATH" checkout --detach "$PDF2ZH_COMMIT" >&2
fi

if [[ ! -f "$PDF2ZH_SOURCE_PATH/pyproject.toml" ]]; then
  echo "::error::PDFMathTranslate source checkout not found: $PDF2ZH_SOURCE_PATH" >&2
  exit 2
fi
if ! git -C "$PDF2ZH_SOURCE_PATH" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "::error::release builds require a Git PDFMathTranslate checkout" >&2
  exit 2
fi
ACTUAL_PDF2ZH_COMMIT="$(git -C "$PDF2ZH_SOURCE_PATH" rev-parse HEAD)"
if [[ "$ACTUAL_PDF2ZH_COMMIT" != "$PDF2ZH_COMMIT" ]]; then
  echo "::error::PDFMathTranslate commit mismatch: expected $PDF2ZH_COMMIT, got $ACTUAL_PDF2ZH_COMMIT" >&2
  exit 2
fi
if [[ -n "$(git -C "$PDF2ZH_SOURCE_PATH" status --porcelain --untracked-files=all)" ]]; then
  echo "::error::PDFMathTranslate checkout must be clean for a release pack" >&2
  exit 2
fi

PACK_DIR="$BUILD_ROOT/linux-x64"
PYTHON_DIR="$PACK_DIR/python"
BIN_DIR="$PACK_DIR/bin"
MODELS_DIR="$PACK_DIR/models"
BABELDOC_CACHE_DIR="$PACK_DIR/assets/babeldoc"
LICENSES_DIR="$PACK_DIR/licenses"
DOCLAYOUT_MODEL_PATH="$MODELS_DIR/$DOCLAYOUT_MODEL_FILENAME"

echo "[pdf2zh-linux-release] PDFMathTranslate: $ACTUAL_PDF2ZH_COMMIT" >&2
echo "[pdf2zh-linux-release] Python: $PBS_PYTHON_VERSION (PBS $PBS_RELEASE)" >&2
echo "[pdf2zh-linux-release] build root: $BUILD_ROOT" >&2

mkdir -p "$PACK_DIR" "$BIN_DIR" "$MODELS_DIR" "$LICENSES_DIR"

if [[ -n "$PBS_TARBALL_FILE" ]]; then
  if [[ ! -s "$PBS_TARBALL_FILE" ]]; then
    echo "::error::PBS_TARBALL_FILE is missing or empty: $PBS_TARBALL_FILE" >&2
    exit 2
  fi
  PBS_TARBALL="$PBS_TARBALL_FILE"
else
  mkdir -p "$DOWNLOAD_DIR"
  PBS_TARBALL="$DOWNLOAD_DIR/$(basename "$PBS_TARBALL_URL")"
  if [[ ! -s "$PBS_TARBALL" ]]; then
    echo "[pdf2zh-linux-release] downloading python-build-standalone" >&2
    rm -f "$PBS_TARBALL.partial"
    curl -fsSL --retry 5 -o "$PBS_TARBALL.partial" "$PBS_TARBALL_URL"
    mv "$PBS_TARBALL.partial" "$PBS_TARBALL"
  else
    echo "[pdf2zh-linux-release] reusing cached python-build-standalone" >&2
  fi
fi
tar -xzf "$PBS_TARBALL" -C "$PACK_DIR"
if [[ ! -x "$PYTHON_DIR/bin/python" ]]; then
  echo "::error::PBS archive did not produce python/bin/python" >&2
  exit 1
fi

REPORTED_PYTHON_VERSION="$("$PYTHON_DIR/bin/python" -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
if [[ "$REPORTED_PYTHON_VERSION" != "$PBS_PYTHON_VERSION" ]]; then
  echo "::error::PBS Python version mismatch: expected $PBS_PYTHON_VERSION, got $REPORTED_PYTHON_VERSION" >&2
  exit 1
fi

echo "[pdf2zh-linux-release] installing pinned runtime dependencies" >&2
"$PYTHON_DIR/bin/python" -m pip install --upgrade "pip==26.1.2" --index-url "$PIP_INDEX_URL" --quiet
"$PYTHON_DIR/bin/python" -m pip install --requirement "$REQUIREMENTS" --index-url "$PIP_INDEX_URL" --quiet
"$PYTHON_DIR/bin/python" -m pip install "$PDF2ZH_SOURCE_PATH" --no-deps --index-url "$PIP_INDEX_URL" --quiet

echo "[pdf2zh-linux-release] applying Rosetta PDF patches" >&2
"$PYTHON_DIR/bin/python" - <<'PY'
from pathlib import Path
import pdf2zh

target = Path(pdf2zh.__file__).resolve().parent / "high_level.py"
text = target.read_text()
old = "np.fromstring(pix.samples, np.uint8)"
new = "np.frombuffer(pix.samples, np.uint8)"
if old in text:
    target.write_text(text.replace(old, new))
elif new not in text:
    raise SystemExit(f"::error::could not find expected NumPy call in {target}")
PY
"$PYTHON_DIR/bin/python" "$SCRIPT_DIR/patch-pdf2zh-color-preservation.py"

echo "[pdf2zh-linux-release] staging BabelDOC fonts" >&2
FONT_ARGS=(--cache-dir "$BABELDOC_CACHE_DIR")
if [[ -n "$BABELDOC_FONT_SOURCE_DIR" ]]; then
  FONT_ARGS+=(--font-source-dir "$BABELDOC_FONT_SOURCE_DIR")
fi
ROSETTA_BABELDOC_CACHE_DIR="$BABELDOC_CACHE_DIR" \
  "$PYTHON_DIR/bin/python" "$SCRIPT_DIR/stage-pdf2zh-font-assets.py" "${FONT_ARGS[@]}"

if [[ -n "$DOCLAYOUT_MODEL_FILE" ]]; then
  cp "$DOCLAYOUT_MODEL_FILE" "$DOCLAYOUT_MODEL_PATH"
else
  echo "[pdf2zh-linux-release] downloading DocLayout ONNX model" >&2
  curl -fL --retry 5 -o "$DOCLAYOUT_MODEL_PATH" "$DOCLAYOUT_MODEL_URL"
fi
if [[ ! -s "$DOCLAYOUT_MODEL_PATH" ]]; then
  echo "::error::DocLayout ONNX model is missing" >&2
  exit 1
fi

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

cp "$PDF2ZH_SOURCE_PATH/LICENSE" "$LICENSES_DIR/PDFMathTranslate-AGPL-3.0.txt"
"$PYTHON_DIR/bin/python" -m pip freeze --all | LC_ALL=C sort > "$PACK_DIR/requirements.freeze.txt"

run_pack_smoke() {
  local root="$1"
  local model="$root/models/$DOCLAYOUT_MODEL_FILENAME"
  local fonts="$root/assets/babeldoc"
  "$root/bin/pdf2zh" --version >&2
  ROSETTA_DOCLAYOUT_MODEL="$model" ROSETTA_BABELDOC_CACHE_DIR="$fonts" \
    "$root/python/bin/python" - <<'PY'
import os
import tempfile
from pathlib import Path

import pymupdf
from babeldoc.assets.assets import get_font_and_metadata
from pdf2zh import rosetta_engine
from pdf2zh.doclayout import OnnxModel

if rosetta_engine.ENGINE_CONTRACT_VERSION != 2:
    raise SystemExit("::error::unexpected Rosetta PDF engine contract")

cache = Path(os.environ["ROSETTA_BABELDOC_CACHE_DIR"]).resolve()
for font_name in (
    "SourceHanSansCN-Regular.ttf",
    "SourceHanSansCN-Bold.ttf",
    "GoNotoKurrent-Regular.ttf",
):
    font_path, _ = get_font_and_metadata(font_name)
    Path(font_path).resolve().relative_to(cache)

model = OnnxModel(os.environ["ROSETTA_DOCLAYOUT_MODEL"])
if not model.model.get_providers():
    raise SystemExit("::error::DocLayout ONNX model has no execution provider")

with tempfile.TemporaryDirectory(prefix="rosetta-pdf-pack-smoke-") as temp:
    root = Path(temp)
    source = root / "source.pdf"
    document = pymupdf.open()
    page = document.new_page(width=400, height=300)
    page.insert_text((72, 72), "Rosetta translates documents locally.", fontsize=12)
    document.save(source)
    document.close()

    prepared = rosetta_engine.prepareRun(
        str(source),
        [1],
        "en",
        "zh",
        {
            "scratchDir": str(root / "scratch"),
            "modelPath": os.environ["ROSETTA_DOCLAYOUT_MODEL"],
            "cleanupScratchDir": False,
        },
    )
    try:
        units = rosetta_engine.collectUnits(prepared["preparedRunId"])
        if not units:
            raise SystemExit("::error::real PDF smoke test collected no translation units")
        translations = {unit["unitId"]: "Rosetta local translation." for unit in units}
        results = rosetta_engine.renderPages(
            prepared["preparedRunId"], translations, str(root / "out"), pages=[1]
        )
        if len(results) != 1 or results[0]["status"] != "translated":
            raise SystemExit(f"::error::real PDF render failed: {results}")
        artifact = Path(results[0]["artifactPath"])
        rendered = pymupdf.open(artifact)
        try:
            if rendered.page_count != 1 or not rendered[0].get_text().strip():
                raise SystemExit("::error::rendered PDF is unreadable or has no text")
        finally:
            rendered.close()
    finally:
        rosetta_engine.disposeRun(prepared["preparedRunId"])

print("pdf-pack-real-render-ok providers=" + ",".join(model.model.get_providers()))
PY
}

echo "[pdf2zh-linux-release] in-place real PDF smoke test" >&2
run_pack_smoke "$PACK_DIR"

echo "[pdf2zh-linux-release] relocation real PDF smoke test" >&2
RELOCATED_DIR="$BUILD_ROOT/linux-x64-relocated"
mv "$PACK_DIR" "$RELOCATED_DIR"
run_pack_smoke "$RELOCATED_DIR"
mv "$RELOCATED_DIR" "$PACK_DIR"

echo "[pdf2zh-linux-release] checking Python and runtime module dependencies" >&2
if ldd "$PYTHON_DIR/bin/python" | grep -q 'not found'; then
  echo "::error::standalone Python has a missing system dependency" >&2
  ldd "$PYTHON_DIR/bin/python" >&2
  exit 1
fi
ROSETTA_DOCLAYOUT_MODEL="$DOCLAYOUT_MODEL_PATH" \
ROSETTA_BABELDOC_CACHE_DIR="$BABELDOC_CACHE_DIR" \
  "$PYTHON_DIR/bin/python" - <<'PY'
import importlib

modules = [
    "azure.ai.translation.text",
    "babeldoc",
    "cryptography",
    "cv2",
    "deepl",
    "fontTools",
    "huggingface_hub",
    "numpy",
    "ollama",
    "onnx",
    "onnxruntime",
    "openai",
    "pdf2zh",
    "pdfminer",
    "peewee",
    "pikepdf",
    "PIL",
    "psutil",
    "pydantic",
    "pymupdf",
    "requests",
    "rich",
    "scipy",
    "shapely",
    "tenacity",
    "tencentcloud",
    "tqdm",
    "xinference_client",
]
for module in modules:
    importlib.import_module(module)
print(f"pdf-pack-runtime-imports-ok count={len(modules)}")
PY

find "$PACK_DIR" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "$PACK_DIR" -type f -name '*.pyc' -delete
rm -f "$DOCLAYOUT_MODEL_PATH.optimized"

STALE="$(find "$PACK_DIR" \( -name '__pycache__' -o -name '*.pyc' \) 2>/dev/null | head -5)"
if [[ -n "$STALE" ]]; then
  echo "::error::stale Python bytecode remains in pack" >&2
  echo "$STALE" >&2
  exit 1
fi
LEAKED="$(find "$PACK_DIR" -type l -lname '/*' 2>/dev/null | head -5)"
if [[ -n "$LEAKED" ]]; then
  echo "::error::absolute symlinks remain in pack" >&2
  echo "$LEAKED" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
rm -f "$ARCHIVE_PATH" "$ARCHIVE_PATH.sha256" \
  "$DIST_DIR/linux-x64-manifest.json" "$DIST_DIR/linux-x64-requirements.freeze.txt"

echo "[pdf2zh-linux-release] archiving $ARCHIVE_PATH" >&2
tar -czf "$ARCHIVE_PATH" -C "$BUILD_ROOT" linux-x64
SIZE_BYTES="$(stat -c '%s' "$ARCHIVE_PATH")"
SHA256="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')"
REQUIREMENTS_SHA256="$(sha256sum "$REQUIREMENTS" | awk '{print $1}')"
MODEL_SHA256="$(sha256sum "$DOCLAYOUT_MODEL_PATH" | awk '{print $1}')"
FREEZE_SHA256="$(sha256sum "$PACK_DIR/requirements.freeze.txt" | awk '{print $1}')"

printf '%s  %s\n' "$SHA256" "$ARCHIVE_NAME" > "$ARCHIVE_PATH.sha256"
cp "$PACK_DIR/requirements.freeze.txt" "$DIST_DIR/linux-x64-requirements.freeze.txt"
cat > "$DIST_DIR/linux-x64-manifest.json" <<EOF
{
  "profile_id": "linux-x64-pdf2zh",
  "pack_filename": "$ARCHIVE_NAME",
  "pdf2zh_version": "$PDF2ZH_VERSION",
  "pdf2zh_commit": "$ACTUAL_PDF2ZH_COMMIT",
  "python_runtime": "python-build-standalone $PBS_PYTHON_VERSION (release $PBS_RELEASE)",
  "requirements_sha256": "$REQUIREMENTS_SHA256",
  "resolved_requirements_sha256": "$FREEZE_SHA256",
  "layout_model": "$DOCLAYOUT_MODEL_FILENAME",
  "layout_model_sha256": "$MODEL_SHA256",
  "sha256": "$SHA256",
  "size_bytes": $SIZE_BYTES,
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

echo "[pdf2zh-linux-release] release pack ready" >&2
echo "  archive: $ARCHIVE_PATH" >&2
echo "  size:    $SIZE_BYTES" >&2
echo "  sha256:  $SHA256" >&2
echo "  tag:     pdf-layout-pack-linux-x64-v$(date -u +%Y.%m.%d).1" >&2
