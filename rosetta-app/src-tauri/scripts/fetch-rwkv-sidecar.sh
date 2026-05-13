#!/usr/bin/env bash
# Fetch the RWKV sidecar tarball produced by
# `.github/workflows/build-rwkv-sidecar-macos.yml` and stage it into the
# `src-tauri/binaries/` and `src-tauri/resources/` directories that Tauri's
# bundle step expects.
#
# Typical usage right before `pnpm tauri build` on macOS arm64:
#
#   bash src-tauri/scripts/fetch-rwkv-sidecar.sh --tag sidecar-v0.1.0
#
# Local-dev shortcut (skip the network round-trip and use the working
# rwkv-mobile build sitting next to the Rosetta checkout):
#
#   bash src-tauri/scripts/fetch-rwkv-sidecar.sh --local ../../rwkv-mobile
#
# The script is intentionally Bash + curl + shasum only so it runs on every
# macOS dev machine without extra installs.

set -euo pipefail

# --- argument parsing --------------------------------------------------------

usage() {
  cat >&2 <<EOF
Usage: $0 [--tag <sidecar-tag>] [--commit <short-sha>] [--local <rwkv-mobile-dir>] [--repo owner/name]

One of --tag / --commit / --local must be provided.

Options:
  --tag <name>      GitHub Release tag to download from (e.g. sidecar-v0.1.0).
  --commit <short>  7-character upstream commit short SHA matching the tarball
                    name suffix (e.g. 498ae7e). Requires --tag too.
  --local <dir>     Skip download; copy a freshly built rwkv_server from
                    <dir>/build/examples/rwkv_server and tokenizer from
                    <dir>/assets/b_rwkv_vocab_v20230424.txt directly.
  --repo owner/name Override the GitHub repo (default: LeoLin4258/rosetta).
  --sha256 <hex>    Optional pre-known SHA256 of the tarball; if set the
                    script verifies before extracting.
  -h, --help        Show this message.
EOF
}

REPO="LeoLin4258/rosetta"
TAG=""
COMMIT_SHORT=""
LOCAL_DIR=""
EXPECTED_SHA=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)     TAG="$2"; shift 2 ;;
    --commit)  COMMIT_SHORT="$2"; shift 2 ;;
    --local)   LOCAL_DIR="$2"; shift 2 ;;
    --repo)    REPO="$2"; shift 2 ;;
    --sha256)  EXPECTED_SHA="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

# --- target layout -----------------------------------------------------------

# All paths below are resolved relative to src-tauri/ (one level up from the
# scripts/ directory the user invoked us from).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_DIR="$SRC_TAURI_DIR/binaries"
RES_DIR="$SRC_TAURI_DIR/resources/rwkv-sidecar"
SIDECAR_NAME="rwkv-server-aarch64-apple-darwin"
TOKENIZER_NAME="b_rwkv_vocab_v20230424.txt"

mkdir -p "$BIN_DIR" "$RES_DIR"

# --- copy helpers ------------------------------------------------------------

install_files() {
  local source_dir="$1"
  local sidecar_src="$source_dir/$SIDECAR_NAME"
  local tokenizer_src="$source_dir/$TOKENIZER_NAME"
  local manifest_src="$source_dir/MANIFEST.json"

  for f in "$sidecar_src" "$tokenizer_src"; do
    if [[ ! -f "$f" ]]; then
      echo "::error::missing $f in staged sidecar bundle" >&2
      exit 1
    fi
  done

  # If MANIFEST.json is present (release tarball path), spot-check the per-file
  # SHA256 entries before installing. Local-dev path won't have it.
  if [[ -f "$manifest_src" ]]; then
    verify_manifest "$manifest_src" "$source_dir"
  fi

  install -m 0755 "$sidecar_src" "$BIN_DIR/$SIDECAR_NAME"
  install -m 0644 "$tokenizer_src" "$RES_DIR/$TOKENIZER_NAME"
  if [[ -f "$manifest_src" ]]; then
    install -m 0644 "$manifest_src" "$RES_DIR/MANIFEST.json"
  fi

  echo "Staged sidecar:" >&2
  ls -lh "$BIN_DIR/$SIDECAR_NAME" "$RES_DIR/$TOKENIZER_NAME" >&2
}

verify_manifest() {
  local manifest="$1"
  local dir="$2"
  python3 - "$manifest" "$dir" <<'PY'
import json, sys, hashlib, pathlib

manifest_path, base = sys.argv[1], pathlib.Path(sys.argv[2])
data = json.loads(open(manifest_path).read())
ok = True
for entry in data.get("files", []):
    name = entry["name"]
    expected = entry["sha256"]
    actual = hashlib.sha256((base / name).read_bytes()).hexdigest()
    if actual != expected:
        print(f"::error::SHA256 mismatch for {name}: expected {expected}, got {actual}", file=sys.stderr)
        ok = False
    else:
        print(f"verified {name}: {expected[:16]}...", file=sys.stderr)
if not ok:
    sys.exit(1)
PY
}

# --- local-dev path ----------------------------------------------------------

if [[ -n "$LOCAL_DIR" ]]; then
  echo "Staging from local rwkv-mobile checkout: $LOCAL_DIR" >&2
  build_dir="$(cd "$LOCAL_DIR" && pwd)"
  server_src="$build_dir/build/examples/rwkv_server"
  tokenizer_src="$build_dir/assets/$TOKENIZER_NAME"
  for f in "$server_src" "$tokenizer_src"; do
    if [[ ! -f "$f" ]]; then
      echo "::error::missing $f — did you run cmake --build?" >&2
      exit 1
    fi
  done

  staged="$(mktemp -d)"
  cp "$server_src" "$staged/$SIDECAR_NAME"
  cp "$tokenizer_src" "$staged/$TOKENIZER_NAME"
  install_files "$staged"
  rm -rf "$staged"
  exit 0
fi

# --- release-tag path --------------------------------------------------------

if [[ -z "$TAG" ]]; then
  echo "::error::--tag is required when not using --local" >&2
  usage
  exit 2
fi

short="${COMMIT_SHORT:-}"
if [[ -z "$short" ]]; then
  # If the tag follows the convention `sidecar-vX.Y.Z-<short>`, derive the
  # short SHA from the tag suffix; otherwise the caller has to pass --commit.
  short="${TAG##*-}"
fi
if [[ -z "$short" || "$short" == "$TAG" ]]; then
  echo "::error::cannot derive commit short SHA from tag '$TAG'; pass --commit" >&2
  exit 2
fi

tar_name="rwkv-sidecar-macos-arm64-${short}.tar.gz"
download_url="https://github.com/${REPO}/releases/download/${TAG}/${tar_name}"

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT
tar_path="$tmp_root/$tar_name"

echo "Downloading $download_url" >&2
curl --fail --location --silent --show-error -o "$tar_path" "$download_url"

if [[ -n "$EXPECTED_SHA" ]]; then
  actual_sha=$(shasum -a 256 "$tar_path" | awk '{print $1}')
  if [[ "$actual_sha" != "$EXPECTED_SHA" ]]; then
    echo "::error::tarball SHA256 mismatch: expected $EXPECTED_SHA, got $actual_sha" >&2
    exit 1
  fi
  echo "Verified tarball SHA256: $actual_sha" >&2
fi

extract_root="$tmp_root/extract"
mkdir -p "$extract_root"
tar -xzf "$tar_path" -C "$extract_root"

# The tarball always contains a single top-level directory; find it.
inner_dir="$(find "$extract_root" -mindepth 1 -maxdepth 1 -type d | head -n1)"
if [[ -z "$inner_dir" ]]; then
  echo "::error::tarball layout unexpected; no top-level directory" >&2
  exit 1
fi

install_files "$inner_dir"
