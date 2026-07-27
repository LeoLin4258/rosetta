#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::pdf2zh Linux dependency lock requires Linux x86_64" >&2
  exit 2
fi

for command in python3 uv; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "::error::missing required command: $command" >&2
    exit 2
  fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INPUTS_MANIFEST="$SCRIPT_DIR/pdf2zh-linux-x64-inputs.json"
REQUIREMENTS="$SCRIPT_DIR/requirements-pdf2zh-linux-x64.txt"
LOCKFILE="$SCRIPT_DIR/requirements-pdf2zh-linux-x64.lock"

EXPECTED_UV_VERSION="$(python3 - "$INPUTS_MANIFEST" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as file:
    print(json.load(file)["lockGenerator"]["version"])
PY
)"
ACTUAL_UV_VERSION="$(uv --version | awk '{print $2}')"
if [[ "$ACTUAL_UV_VERSION" != "$EXPECTED_UV_VERSION" ]]; then
  echo "::error::uv version mismatch: expected $EXPECTED_UV_VERSION, got $ACTUAL_UV_VERSION" >&2
  exit 2
fi

uv pip compile "$REQUIREMENTS" \
  --generate-hashes \
  --no-annotate \
  --only-binary :all: \
  --python-version 3.12.13 \
  --python-platform x86_64-unknown-linux-gnu \
  --custom-compile-command \
  "uv pip compile requirements-pdf2zh-linux-x64.txt --generate-hashes --no-annotate --only-binary :all: --python-version 3.12.13 --python-platform x86_64-unknown-linux-gnu --output-file requirements-pdf2zh-linux-x64.lock" \
  --output-file "$LOCKFILE"

sha256sum "$LOCKFILE"
