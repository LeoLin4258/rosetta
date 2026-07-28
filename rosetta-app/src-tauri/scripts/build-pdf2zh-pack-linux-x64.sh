#!/usr/bin/env bash
# Build the release-ready Rosetta PDF layout component for Linux x64.

set -euo pipefail

PDF2ZH_SOURCE_PATH="${PDF2ZH_SOURCE_PATH:-}"
PBS_TARBALL_FILE="${PBS_TARBALL_FILE:-}"
DOCLAYOUT_MODEL_FILE="${DOCLAYOUT_MODEL_FILE:-}"
BABELDOC_FONT_SOURCE_DIR="${BABELDOC_FONT_SOURCE_DIR:-}"

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::pdf2zh Linux release pack build requires Linux x86_64" >&2
  exit 2
fi

for command in curl git ldd python3 sha256sum tar tee; do
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
REQUIREMENTS_LOCK="$SCRIPT_DIR/requirements-pdf2zh-linux-x64.lock"
INPUTS_MANIFEST="$SCRIPT_DIR/pdf2zh-linux-x64-inputs.json"
LOCK_COMPILER_SCRIPT="$SCRIPT_DIR/compile-pdf2zh-pack-linux-x64-lock.sh"
COLOR_PATCH_SCRIPT="$SCRIPT_DIR/patch-pdf2zh-color-preservation.py"
FONT_STAGER_SCRIPT="$SCRIPT_DIR/stage-pdf2zh-font-assets.py"
SBOM_SCRIPT="$SCRIPT_DIR/generate-pdf2zh-pack-sbom.py"
ENGINE_CAPABILITIES_MANIFEST="$SCRIPT_DIR/pdf2zh-engine-capabilities.json"
ARCHIVE_NAME="rosetta-pdf2zh-linux-x64.tar.gz"
ARCHIVE_PATH="$DIST_DIR/$ARCHIVE_NAME"
BUILD_ROOT="$(mktemp -d)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

for required_file in "$REQUIREMENTS" "$REQUIREMENTS_LOCK" "$INPUTS_MANIFEST" \
  "$LOCK_COMPILER_SCRIPT" "$COLOR_PATCH_SCRIPT" "$FONT_STAGER_SCRIPT" \
  "$SBOM_SCRIPT" "$ENGINE_CAPABILITIES_MANIFEST"; do
  if [[ ! -f "$required_file" ]]; then
    echo "::error::missing Linux PDF build input: $required_file" >&2
    exit 2
  fi
done

eval "$(python3 - "$INPUTS_MANIFEST" <<'PY'
import json
import shlex
import sys

with open(sys.argv[1], encoding="utf-8") as file:
    inputs = json.load(file)

values = {
    "LOCK_GENERATOR_NAME": inputs["lockGenerator"]["name"],
    "LOCK_GENERATOR_VERSION": inputs["lockGenerator"]["version"],
    "PIP_INDEX_URL": inputs["dependencyIndexUrl"],
    "PBS_RELEASE": inputs["pythonBuildStandalone"]["release"],
    "PBS_PYTHON_VERSION": inputs["pythonBuildStandalone"]["pythonVersion"],
    "PBS_TARBALL_FILENAME": inputs["pythonBuildStandalone"]["filename"],
    "PBS_TARBALL_URL": inputs["pythonBuildStandalone"]["url"],
    "PBS_TARBALL_SHA256": inputs["pythonBuildStandalone"]["sha256"],
    "PDF2ZH_VERSION": inputs["pdfMathTranslate"]["version"],
    "PDF2ZH_REPOSITORY_URL": inputs["pdfMathTranslate"]["repositoryUrl"],
    "PDF2ZH_COMMIT": inputs["pdfMathTranslate"]["commit"],
    "DOCLAYOUT_MODEL_FILENAME": inputs["docLayoutModel"]["filename"],
    "DOCLAYOUT_MODEL_URL": inputs["docLayoutModel"]["url"],
    "DOCLAYOUT_MODEL_SHA256": inputs["docLayoutModel"]["sha256"],
    "BABELDOC_VERSION": inputs["babeldoc"]["version"],
}
for name, value in values.items():
    print(f"{name}={shlex.quote(str(value))}")
PY
)"

eval "$(python3 - "$ENGINE_CAPABILITIES_MANIFEST" <<'PY'
import json
import shlex
import sys

with open(sys.argv[1], encoding="utf-8") as file:
    manifest = json.load(file)

for name, value in {
    "ENGINE_CONTRACT_VERSION": manifest["engineContractVersion"],
    "ENGINE_REVISION": manifest["engineRevision"],
}.items():
    print(f"{name}={shlex.quote(str(value))}")
PY
)"

if [[ "$LOCK_GENERATOR_NAME" != "uv" ]]; then
  echo "::error::unsupported dependency lock generator: $LOCK_GENERATOR_NAME" >&2
  exit 2
fi

verify_sha256() {
  local path="$1"
  local expected="$2"
  local label="$3"
  local actual
  actual="$(sha256sum "$path" | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    echo "::error::$label SHA-256 mismatch: expected $expected, got $actual" >&2
    exit 1
  fi
}

mkdir -p "$DIST_DIR"
BUILD_LOG_PATH="${ROSETTA_PDF2ZH_BUILD_LOG:-$DIST_DIR/linux-x64-build.log}"
exec > >(tee "$BUILD_LOG_PATH") 2>&1

if ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "::error::release builds require a Git Rosetta checkout" >&2
  exit 2
fi
ROSETTA_COMMIT="$(git -C "$REPO_ROOT" rev-parse HEAD)"
BUILD_SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"
REQUIREMENTS_SHA256="$(sha256sum "$REQUIREMENTS" | awk '{print $1}')"
DEPENDENCY_LOCK_SHA256="$(sha256sum "$REQUIREMENTS_LOCK" | awk '{print $1}')"
INPUTS_MANIFEST_SHA256="$(sha256sum "$INPUTS_MANIFEST" | awk '{print $1}')"
LOCK_COMPILER_SHA256="$(sha256sum "$LOCK_COMPILER_SCRIPT" | awk '{print $1}')"
COLOR_PATCH_SHA256="$(sha256sum "$COLOR_PATCH_SCRIPT" | awk '{print $1}')"
FONT_STAGER_SHA256="$(sha256sum "$FONT_STAGER_SCRIPT" | awk '{print $1}')"
SBOM_SCRIPT_SHA256="$(sha256sum "$SBOM_SCRIPT" | awk '{print $1}')"
ENGINE_CAPABILITIES_MANIFEST_SHA256="$(sha256sum "$ENGINE_CAPABILITIES_MANIFEST" | awk '{print $1}')"
BUILD_RECIPE_ID="$(printf '%s\n' \
  "rosetta_commit=$ROSETTA_COMMIT" \
  "build_script_sha256=$BUILD_SCRIPT_SHA256" \
  "requirements_sha256=$REQUIREMENTS_SHA256" \
  "dependency_lock_sha256=$DEPENDENCY_LOCK_SHA256" \
  "inputs_manifest_sha256=$INPUTS_MANIFEST_SHA256" \
  "lock_compiler_sha256=$LOCK_COMPILER_SHA256" \
  "color_patch_sha256=$COLOR_PATCH_SHA256" \
  "font_stager_sha256=$FONT_STAGER_SHA256" \
  "sbom_script_sha256=$SBOM_SCRIPT_SHA256" \
  "engine_capabilities_manifest_sha256=$ENGINE_CAPABILITIES_MANIFEST_SHA256" \
  | sha256sum | awk '{print $1}')"

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
SOURCE_DATE_EPOCH="$(git -C "$PDF2ZH_SOURCE_PATH" show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH

PACK_DIR="$BUILD_ROOT/linux-x64"
PYTHON_DIR="$PACK_DIR/python"
PYTHON_STDLIB_DIR="$PYTHON_DIR/lib/python${PBS_PYTHON_VERSION%.*}"
BIN_DIR="$PACK_DIR/bin"
MODELS_DIR="$PACK_DIR/models"
BABELDOC_CACHE_DIR="$PACK_DIR/assets/babeldoc"
LICENSES_DIR="$PACK_DIR/licenses"
DOCLAYOUT_MODEL_PATH="$MODELS_DIR/$DOCLAYOUT_MODEL_FILENAME"

echo "[pdf2zh-linux-release] PDFMathTranslate: $ACTUAL_PDF2ZH_COMMIT" >&2
echo "[pdf2zh-linux-release] Rosetta: $ROSETTA_COMMIT" >&2
echo "[pdf2zh-linux-release] Python: $PBS_PYTHON_VERSION (PBS $PBS_RELEASE)" >&2
echo "[pdf2zh-linux-release] dependency lock: $DEPENDENCY_LOCK_SHA256 (uv $LOCK_GENERATOR_VERSION)" >&2
echo "[pdf2zh-linux-release] build recipe: $BUILD_RECIPE_ID" >&2
echo "[pdf2zh-linux-release] build root: $BUILD_ROOT" >&2

mkdir -p "$PACK_DIR" "$BIN_DIR" "$MODELS_DIR" "$LICENSES_DIR"
cp "$ENGINE_CAPABILITIES_MANIFEST" "$PACK_DIR/engine-capabilities.json"

while IFS=$'\t' read -r LICENSE_FILENAME LICENSE_URL LICENSE_SHA256; do
  LICENSE_CACHE_DIR="$DOWNLOAD_DIR/licenses"
  LICENSE_CACHE_PATH="$LICENSE_CACHE_DIR/$LICENSE_FILENAME"
  mkdir -p "$LICENSE_CACHE_DIR"
  if [[ ! -s "$LICENSE_CACHE_PATH" ]]; then
    echo "[pdf2zh-linux-release] downloading license asset: $LICENSE_FILENAME" >&2
    rm -f "$LICENSE_CACHE_PATH.partial"
    curl -fsSL --retry 5 -o "$LICENSE_CACHE_PATH.partial" "$LICENSE_URL"
    verify_sha256 "$LICENSE_CACHE_PATH.partial" "$LICENSE_SHA256" "$LICENSE_FILENAME"
    mv "$LICENSE_CACHE_PATH.partial" "$LICENSE_CACHE_PATH"
  fi
  verify_sha256 "$LICENSE_CACHE_PATH" "$LICENSE_SHA256" "$LICENSE_FILENAME"
  cp "$LICENSE_CACHE_PATH" "$LICENSES_DIR/$LICENSE_FILENAME"
done < <(python3 - "$INPUTS_MANIFEST" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as file:
    inputs = json.load(file)
for asset in inputs["licenseAssets"]:
    print(asset["filename"], asset["url"], asset["sha256"], sep="\t")
PY
)

if [[ -n "$PBS_TARBALL_FILE" ]]; then
  if [[ ! -s "$PBS_TARBALL_FILE" ]]; then
    echo "::error::PBS_TARBALL_FILE is missing or empty: $PBS_TARBALL_FILE" >&2
    exit 2
  fi
  PBS_TARBALL="$PBS_TARBALL_FILE"
else
  mkdir -p "$DOWNLOAD_DIR"
  PBS_TARBALL="$DOWNLOAD_DIR/$PBS_TARBALL_FILENAME"
  if [[ ! -s "$PBS_TARBALL" ]]; then
    echo "[pdf2zh-linux-release] downloading python-build-standalone" >&2
    rm -f "$PBS_TARBALL.partial"
    curl -fsSL --retry 5 -o "$PBS_TARBALL.partial" "$PBS_TARBALL_URL"
    verify_sha256 "$PBS_TARBALL.partial" "$PBS_TARBALL_SHA256" "python-build-standalone archive"
    mv "$PBS_TARBALL.partial" "$PBS_TARBALL"
  else
    echo "[pdf2zh-linux-release] reusing cached python-build-standalone" >&2
  fi
fi
verify_sha256 "$PBS_TARBALL" "$PBS_TARBALL_SHA256" "python-build-standalone archive"
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
"$PYTHON_DIR/bin/python" -m pip install \
  --requirement "$REQUIREMENTS_LOCK" \
  --require-hashes \
  --only-binary=:all: \
  --no-deps \
  --index-url "$PIP_INDEX_URL" \
  --quiet
"$PYTHON_DIR/bin/python" -m pip install "$PDF2ZH_SOURCE_PATH" \
  --no-build-isolation \
  --no-deps \
  --index-url "$PIP_INDEX_URL" \
  --quiet

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
"$PYTHON_DIR/bin/python" "$COLOR_PATCH_SCRIPT"

echo "[pdf2zh-linux-release] staging BabelDOC fonts" >&2
FONT_ARGS=(--cache-dir "$BABELDOC_CACHE_DIR")
if [[ -n "$BABELDOC_FONT_SOURCE_DIR" ]]; then
  FONT_ARGS+=(--font-source-dir "$BABELDOC_FONT_SOURCE_DIR")
fi
ROSETTA_BABELDOC_CACHE_DIR="$BABELDOC_CACHE_DIR" \
  "$PYTHON_DIR/bin/python" "$FONT_STAGER_SCRIPT" "${FONT_ARGS[@]}"

"$PYTHON_DIR/bin/python" - "$INPUTS_MANIFEST" "$BABELDOC_CACHE_DIR" "$BABELDOC_VERSION" <<'PY'
import hashlib
import importlib.metadata
import json
from pathlib import Path
import sys

inputs_path = Path(sys.argv[1])
cache_dir = Path(sys.argv[2])
expected_babeldoc_version = sys.argv[3]
with inputs_path.open(encoding="utf-8") as file:
    inputs = json.load(file)

actual_babeldoc_version = importlib.metadata.version("BabelDOC")
if actual_babeldoc_version != expected_babeldoc_version:
    raise SystemExit(
        f"::error::BabelDOC version mismatch: expected {expected_babeldoc_version}, "
        f"got {actual_babeldoc_version}"
    )

for font in inputs["babeldoc"]["fonts"]:
    path = cache_dir / "fonts" / font["filename"]
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != font["sha256"]:
        raise SystemExit(
            f"::error::BabelDOC font SHA-256 mismatch for {path}: "
            f"expected {font['sha256']}, got {actual}"
        )
PY

if [[ -n "$DOCLAYOUT_MODEL_FILE" ]]; then
  cp "$DOCLAYOUT_MODEL_FILE" "$DOCLAYOUT_MODEL_PATH"
else
  echo "[pdf2zh-linux-release] downloading DocLayout ONNX model" >&2
  curl -fL --retry 5 -o "$DOCLAYOUT_MODEL_PATH.partial" "$DOCLAYOUT_MODEL_URL"
  verify_sha256 "$DOCLAYOUT_MODEL_PATH.partial" "$DOCLAYOUT_MODEL_SHA256" "DocLayout model"
  mv "$DOCLAYOUT_MODEL_PATH.partial" "$DOCLAYOUT_MODEL_PATH"
fi
if [[ ! -s "$DOCLAYOUT_MODEL_PATH" ]]; then
  echo "::error::DocLayout ONNX model is missing" >&2
  exit 1
fi
verify_sha256 "$DOCLAYOUT_MODEL_PATH" "$DOCLAYOUT_MODEL_SHA256" "DocLayout model"

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
"$PYTHON_DIR/bin/python" -m pip freeze --all \
  | sed -E "s#^pdf2zh @ file://.*#pdf2zh==$PDF2ZH_VERSION#" \
  | LC_ALL=C sort > "$PACK_DIR/requirements.freeze.txt"
cp "$REQUIREMENTS_LOCK" "$PACK_DIR/requirements.lock"
cp "$INPUTS_MANIFEST" "$PACK_DIR/build-inputs.json"
cat > "$PACK_DIR/build-recipe.json" <<EOF
{
  "schema_version": 1,
  "build_recipe_id": "$BUILD_RECIPE_ID",
  "rosetta_commit": "$ROSETTA_COMMIT",
  "pdf2zh_commit": "$ACTUAL_PDF2ZH_COMMIT",
  "source_date_epoch": $SOURCE_DATE_EPOCH,
  "lock_generator": "$LOCK_GENERATOR_NAME $LOCK_GENERATOR_VERSION",
  "dependency_index_url": "$PIP_INDEX_URL",
  "build_script_sha256": "$BUILD_SCRIPT_SHA256",
  "requirements_sha256": "$REQUIREMENTS_SHA256",
  "dependency_lock_sha256": "$DEPENDENCY_LOCK_SHA256",
  "inputs_manifest_sha256": "$INPUTS_MANIFEST_SHA256",
  "lock_compiler_sha256": "$LOCK_COMPILER_SHA256",
  "color_patch_sha256": "$COLOR_PATCH_SHA256",
  "font_stager_sha256": "$FONT_STAGER_SHA256",
  "sbom_script_sha256": "$SBOM_SCRIPT_SHA256",
  "engine_capabilities_manifest_sha256": "$ENGINE_CAPABILITIES_MANIFEST_SHA256",
  "engine_revision": $ENGINE_REVISION
}
EOF

run_pack_smoke() {
  local root="$1"
  local model="$root/models/$DOCLAYOUT_MODEL_FILENAME"
  local fonts="$root/assets/babeldoc"
  "$root/bin/pdf2zh" --version >&2
  ROSETTA_DOCLAYOUT_MODEL="$model" ROSETTA_BABELDOC_CACHE_DIR="$fonts" \
  ROSETTA_PDF_ENGINE_CAPABILITIES="$root/engine-capabilities.json" \
    "$root/python/bin/python" - <<'PY'
import json
import os
import tempfile
from pathlib import Path

import pymupdf
from babeldoc.assets.assets import get_font_and_metadata
from pdf2zh import rosetta_engine
from pdf2zh.doclayout import OnnxModel

expected = json.loads(
    Path(os.environ["ROSETTA_PDF_ENGINE_CAPABILITIES"]).read_text(encoding="utf-8")
)
actual = rosetta_engine.prewarm(
    {"modelPath": os.environ["ROSETTA_DOCLAYOUT_MODEL"]}
)
if actual.get("contractVersion") != expected["engineContractVersion"]:
    raise SystemExit("::error::unexpected Rosetta PDF engine contract")
if actual.get("engineRevision", 0) < expected["engineRevision"]:
    raise SystemExit("::error::Rosetta PDF engine revision is too old")
missing_capabilities = sorted(
    set(expected["capabilities"]) - set(actual.get("capabilities", []))
)
if missing_capabilities:
    raise SystemExit(
        "::error::Rosetta PDF engine is missing capabilities: "
        + ", ".join(missing_capabilities)
    )
if not callable(getattr(rosetta_engine, "resetRun", None)):
    raise SystemExit("::error::Rosetta PDF engine does not support reusable prepared runs")
if not callable(getattr(rosetta_engine, "load_persistent_layout_cache", None)):
    raise SystemExit("::error::Rosetta PDF engine does not support durable layout cache")

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
            "persistentLayoutCacheDir": str(root / "layout-cache"),
            "persistentLayoutCacheKey": "pack-smoke-v1",
            "persistentSourceFingerprint": "pack-smoke-source",
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
        rosetta_engine.resetRun(prepared["preparedRunId"])
        reset_results = rosetta_engine.renderPages(
            prepared["preparedRunId"], translations, str(root / "out-reset"), pages=[1]
        )
        if len(reset_results) != 1 or reset_results[0]["status"] != "translated":
            raise SystemExit(f"::error::real PDF reset render failed: {reset_results}")
    finally:
        rosetta_engine.disposeRun(prepared["preparedRunId"])

    restored = rosetta_engine.prepareRun(
        str(source),
        [1],
        "en",
        "zh",
        {
            "scratchDir": str(root / "scratch-restored"),
            "modelPath": os.environ["ROSETTA_DOCLAYOUT_MODEL"],
            "cleanupScratchDir": False,
            "persistentLayoutCacheDir": str(root / "layout-cache"),
            "persistentLayoutCacheKey": "pack-smoke-v1",
            "persistentSourceFingerprint": "pack-smoke-source",
        },
    )
    try:
        if not restored.get("persistentLayoutCacheHit"):
            raise SystemExit("::error::real PDF durable layout cache did not restore")
        restored_units = rosetta_engine.collectUnits(restored["preparedRunId"])
        if len(restored_units) != len(units):
            raise SystemExit("::error::durable layout restore changed translation units")
    finally:
        rosetta_engine.disposeRun(restored["preparedRunId"])

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

pack_size_bytes() {
  du -sb "$PACK_DIR" | awk '{print $1}'
}

log_prune_delta() {
  local label="$1"
  local before="$2"
  local after="$3"
  echo "[pdf2zh-linux-release] prune $label: $((before - after)) bytes ($before -> $after)" >&2
}

echo "[pdf2zh-linux-release] pruning proven non-runtime content" >&2
PRUNE_BEFORE="$(pack_size_bytes)"
rm -rf "$PYTHON_DIR/include"
find "$PYTHON_DIR" -type f -name '*.a' -delete
PRUNE_AFTER="$(pack_size_bytes)"
log_prune_delta "headers-static-libraries" "$PRUNE_BEFORE" "$PRUNE_AFTER"

PRUNE_BEFORE="$PRUNE_AFTER"
rm -rf \
  "$PYTHON_DIR/lib/tcl9" \
  "$PYTHON_DIR/lib/tcl9.0" \
  "$PYTHON_DIR/lib/tk9.0" \
  "$PYTHON_DIR/lib/itcl4.3.5" \
  "$PYTHON_DIR/lib/thread3.0.4" \
  "$PYTHON_STDLIB_DIR/tkinter" \
  "$PYTHON_STDLIB_DIR/idlelib"
rm -f \
  "$PYTHON_DIR/lib/libtcl9.0.so" \
  "$PYTHON_DIR/lib/libtcl9tk9.0.so" \
  "$PYTHON_STDLIB_DIR"/lib-dynload/_tkinter*.so \
  "$PYTHON_DIR/bin"/idle3 \
  "$PYTHON_DIR/bin"/idle3.12 \
  "$PYTHON_DIR/bin"/tclsh* \
  "$PYTHON_DIR/bin"/wish*
PRUNE_AFTER="$(pack_size_bytes)"
log_prune_delta "tcl-tk-idle" "$PRUNE_BEFORE" "$PRUNE_AFTER"

PRUNE_BEFORE="$PRUNE_AFTER"
find "$PYTHON_STDLIB_DIR" -type d \( -name test -o -name tests \) \
  -prune -exec rm -rf {} +
PRUNE_AFTER="$(pack_size_bytes)"
log_prune_delta "test-directories" "$PRUNE_BEFORE" "$PRUNE_AFTER"

PRUNE_BEFORE="$PRUNE_AFTER"
find "$PYTHON_DIR/bin" -mindepth 1 -maxdepth 1 \
  \( -type f -o -type l \) \
  ! -name python \
  ! -name python3 \
  ! -name "python${PBS_PYTHON_VERSION%.*}" \
  -delete
PRUNE_AFTER="$(pack_size_bytes)"
log_prune_delta "non-runtime-console-scripts" "$PRUNE_BEFORE" "$PRUNE_AFTER"

if [[ -d "$PYTHON_DIR/include" ]] \
  || find "$PYTHON_DIR" -type f -name '*.a' -print -quit | grep -q . \
  || find "$PYTHON_STDLIB_DIR" -type d \( -name test -o -name tests \) \
    -print -quit | grep -q .; then
  echo "::error::proven non-runtime content remains after pruning" >&2
  exit 1
fi
if ldd "$PYTHON_DIR/bin/python" | grep -q 'not found'; then
  echo "::error::pruned standalone Python has a missing system dependency" >&2
  ldd "$PYTHON_DIR/bin/python" >&2
  exit 1
fi

echo "[pdf2zh-linux-release] pruned real PDF smoke test" >&2
run_pack_smoke "$PACK_DIR"

find "$PACK_DIR" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "$PACK_DIR" -type f -name '*.pyc' -delete
rm -f "$DOCLAYOUT_MODEL_PATH.optimized"

echo "[pdf2zh-linux-release] generating SBOM and license inventory" >&2
PYTHONDONTWRITEBYTECODE=1 \
  "$PYTHON_DIR/bin/python" "$SBOM_SCRIPT" --pack-dir "$PACK_DIR"

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
  "$DIST_DIR/linux-x64-manifest.json" \
  "$DIST_DIR/linux-x64-build-recipe.json" \
  "$DIST_DIR/linux-x64-build-inputs.json" \
  "$DIST_DIR/linux-x64-requirements.freeze.txt" \
  "$DIST_DIR/linux-x64-requirements.lock" \
  "$DIST_DIR/linux-x64-sbom.cdx.json" \
  "$DIST_DIR/linux-x64-licenses.inventory.json"

echo "[pdf2zh-linux-release] archiving $ARCHIVE_PATH" >&2
tar -czf "$ARCHIVE_PATH" -C "$BUILD_ROOT" linux-x64
SIZE_BYTES="$(stat -c '%s' "$ARCHIVE_PATH")"
SHA256="$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')"
REQUIREMENTS_SHA256="$(sha256sum "$REQUIREMENTS" | awk '{print $1}')"
MODEL_SHA256="$(sha256sum "$DOCLAYOUT_MODEL_PATH" | awk '{print $1}')"
FREEZE_SHA256="$(sha256sum "$PACK_DIR/requirements.freeze.txt" | awk '{print $1}')"
BUILD_RECIPE_SHA256="$(sha256sum "$PACK_DIR/build-recipe.json" | awk '{print $1}')"
SBOM_SHA256="$(sha256sum "$PACK_DIR/sbom.cdx.json" | awk '{print $1}')"
LICENSE_INVENTORY_SHA256="$(sha256sum "$PACK_DIR/licenses.inventory.json" | awk '{print $1}')"

printf '%s  %s\n' "$SHA256" "$ARCHIVE_NAME" > "$ARCHIVE_PATH.sha256"
cp "$PACK_DIR/requirements.freeze.txt" "$DIST_DIR/linux-x64-requirements.freeze.txt"
cp "$PACK_DIR/requirements.lock" "$DIST_DIR/linux-x64-requirements.lock"
cp "$PACK_DIR/build-inputs.json" "$DIST_DIR/linux-x64-build-inputs.json"
cp "$PACK_DIR/build-recipe.json" "$DIST_DIR/linux-x64-build-recipe.json"
cp "$PACK_DIR/sbom.cdx.json" "$DIST_DIR/linux-x64-sbom.cdx.json"
cp "$PACK_DIR/licenses.inventory.json" "$DIST_DIR/linux-x64-licenses.inventory.json"
cat > "$DIST_DIR/linux-x64-manifest.json" <<EOF
{
  "profile_id": "linux-x64-pdf2zh",
  "pack_filename": "$ARCHIVE_NAME",
  "build_recipe_id": "$BUILD_RECIPE_ID",
  "build_recipe_sha256": "$BUILD_RECIPE_SHA256",
  "rosetta_commit": "$ROSETTA_COMMIT",
  "pdf2zh_version": "$PDF2ZH_VERSION",
  "pdf2zh_commit": "$ACTUAL_PDF2ZH_COMMIT",
  "python_runtime": "python-build-standalone $PBS_PYTHON_VERSION (release $PBS_RELEASE)",
  "python_runtime_sha256": "$PBS_TARBALL_SHA256",
  "requirements_sha256": "$REQUIREMENTS_SHA256",
  "dependency_lock_sha256": "$DEPENDENCY_LOCK_SHA256",
  "resolved_requirements_sha256": "$FREEZE_SHA256",
  "inputs_manifest_sha256": "$INPUTS_MANIFEST_SHA256",
  "lock_compiler_sha256": "$LOCK_COMPILER_SHA256",
  "build_script_sha256": "$BUILD_SCRIPT_SHA256",
  "color_patch_sha256": "$COLOR_PATCH_SHA256",
  "font_stager_sha256": "$FONT_STAGER_SHA256",
  "sbom_script_sha256": "$SBOM_SCRIPT_SHA256",
  "engine_contract_version": $ENGINE_CONTRACT_VERSION,
  "engine_revision": $ENGINE_REVISION,
  "engine_capabilities_manifest_sha256": "$ENGINE_CAPABILITIES_MANIFEST_SHA256",
  "sbom_sha256": "$SBOM_SHA256",
  "license_inventory_sha256": "$LICENSE_INVENTORY_SHA256",
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
echo "  recipe:  $BUILD_RECIPE_ID" >&2
echo "  log:     $BUILD_LOG_PATH" >&2
echo "  tag:     pdf-layout-pack-linux-x64-v$(date -u +%Y.%m.%d).1" >&2
