#!/usr/bin/env python3
"""Prove production and Markdown PyMuPDF versions stay isolated concurrently."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path


def read_base_version(python: Path) -> str:
    completed = subprocess.run(
        [str(python), "-I", "-c", "import pymupdf; print(pymupdf.VersionBind)"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    return completed.stdout.strip().splitlines()[-1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-python", type=Path, required=True)
    parser.add_argument("--overlay", type=Path, required=True)
    parser.add_argument("--worker", type=Path, default=Path(__file__).resolve().parents[1] / "src" / "managed_pdf_markdown" / "rosetta_pdf_markdown_worker.py")
    parser.add_argument("--jobs-root", type=Path, required=True)
    args = parser.parse_args()
    base_python = args.base_python.resolve(strict=True)
    overlay = args.overlay.resolve(strict=True)
    worker_path = args.worker.resolve(strict=True)
    jobs_root = args.jobs_root.resolve(strict=True)
    before = read_base_version(base_python)
    env = os.environ.copy()
    for key in ["PYTHONHOME", "PYTHONPATH", "PYTHONSTARTUP", "PYTHONUSERBASE", "VIRTUAL_ENV", "CONDA_PREFIX", "LD_LIBRARY_PATH"]:
        env.pop(key, None)
    env.update({"PYTHONPATH": str(overlay), "PYTHONNOUSERSITE": "1", "CUDA_VISIBLE_DEVICES": "", "ROSETTA_PDF_MARKDOWN_JOBS_ROOT": str(jobs_root)})
    worker = subprocess.Popen([str(base_python), str(worker_path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True, encoding="utf-8")
    try:
        assert worker.stdin and worker.stdout
        worker.stdin.write('{"type":"hello"}\n')
        worker.stdin.flush()
        ready = json.loads(worker.stdout.readline())
        concurrent = read_base_version(base_python)
        worker.stdin.write('{"type":"shutdown"}\n')
        worker.stdin.flush()
        worker.stdout.readline()
    finally:
        worker.wait(timeout=10)
    after = read_base_version(base_python)
    if before != "1.25.2" or concurrent != "1.25.2" or after != "1.25.2":
        raise SystemExit("production PyMuPDF identity changed")
    if ready.get("type") != "ready" or ready.get("versions", {}).get("PyMuPDF") != "1.28.0":
        raise SystemExit("Markdown worker did not resolve PyMuPDF 1.28.0")
    print(json.dumps({"productionBefore": before, "productionConcurrent": concurrent, "productionAfter": after, "markdown": ready["versions"]["PyMuPDF"], "providers": ready["providers"]}, sort_keys=True))


if __name__ == "__main__":
    main()
