#!/usr/bin/env python3
"""Run the PDF -> Markdown release gates without launching the desktop UI."""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPORT_SCHEMA = "rosetta-pdf-markdown-release-acceptance/1"
EXPECTED_STRUCTURE_DEFECTS = {
    "adjacentDuplicateBodyBoxes": 1,
    "emptyBodyPages": 6,
    "figureInternalTextOverlap": 1,
    "invalidPageIdentity": 0,
    "unknownBoxClasses": 0,
}


class AcceptanceError(RuntimeError):
    pass


def windows_defaults() -> tuple[Path | None, Path | None, Path | None]:
    local = os.environ.get("LOCALAPPDATA")
    roaming = os.environ.get("APPDATA")
    if os.name != "nt" or not local or not roaming:
        return None, None, None
    local_root = Path(local) / "com.rosetta.desktop"
    return (
        local_root
        / "pdf2zh-sidecar"
        / "pack"
        / "windows-amd64"
        / "python"
        / "python.exe",
        local_root
        / "pdf-markdown-component"
        / "overlay"
        / "windows-x64",
        Path(roaming) / "com.rosetta.desktop" / "jobs",
    )


def resolve_command(command: list[str]) -> list[str]:
    executable = shutil.which(command[0])
    if executable is None:
        raise AcceptanceError(f"Required command is unavailable: {command[0]}")
    resolved = [executable, *command[1:]]
    if os.name == "nt" and Path(executable).suffix.lower() in {".cmd", ".bat"}:
        return [os.environ.get("COMSPEC", "cmd.exe"), "/d", "/s", "/c", *resolved]
    return resolved


def run_check(
    checks: list[dict[str, Any]],
    name: str,
    command: list[str],
    cwd: Path,
    *,
    env: dict[str, str] | None = None,
    timeout: int = 3600,
) -> None:
    print(f"[acceptance] {name} ...", flush=True)
    started = time.perf_counter()
    completed = subprocess.run(
        resolve_command(command),
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )
    seconds = round(time.perf_counter() - started, 3)
    check = {
        "name": name,
        "status": "passed" if completed.returncode == 0 else "failed",
        "seconds": seconds,
    }
    checks.append(check)
    if completed.returncode != 0:
        tail = "\n".join(
            (completed.stdout + "\n" + completed.stderr).splitlines()[-80:]
        )
        raise AcceptanceError(f"{name} failed (exit {completed.returncode})\n{tail}")
    print(f"[acceptance] {name}: passed ({seconds:.3f}s)", flush=True)


def write_report(path: Path, report: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def validate_corpus_report(report: dict[str, Any]) -> dict[str, Any]:
    corpus = report.get("corpus", {})
    performance = report.get("performance", {})
    defects = report.get("structureDefects", {})
    if corpus.get("documentCount") != 24 or corpus.get("pageCount") != 240:
        raise AcceptanceError("Release corpus identity is not 24 documents / 240 pages")
    if performance.get("warmMedianSecondsPerPage", float("inf")) > 0.6:
        raise AcceptanceError("Release corpus warm median exceeded 0.6 seconds/page")
    if performance.get("warmP95SecondsPerPage", float("inf")) > 1.5:
        raise AcceptanceError("Release corpus warm p95 exceeded 1.5 seconds/page")
    if defects != EXPECTED_STRUCTURE_DEFECTS:
        raise AcceptanceError(
            "Release corpus structure flags changed: "
            + json.dumps(defects, sort_keys=True)
        )
    return {
        "documents": corpus["documentCount"],
        "pages": corpus["pageCount"],
        "performance": performance,
        "structureDefects": defects,
    }


def build_parser() -> argparse.ArgumentParser:
    default_python, default_overlay, default_jobs = windows_defaults()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-python", type=Path, default=default_python)
    parser.add_argument("--overlay", type=Path, default=default_overlay)
    parser.add_argument("--jobs-root", type=Path, default=default_jobs)
    parser.add_argument("--docling-root", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument(
        "--skip-corpus",
        action="store_true",
        help="Run fast code/runtime gates without the 24-document corpus.",
    )
    return parser


def main() -> None:
    script = Path(__file__).resolve()
    tauri_root = script.parents[1]
    app_root = script.parents[2]
    repo_root = script.parents[3]
    args = build_parser().parse_args()
    report_path = args.report or (
        tauri_root
        / "target"
        / "pdf-markdown-release-acceptance"
        / ("report-fast.json" if args.skip_corpus else "report.json")
    )
    checks: list[dict[str, Any]] = []
    report: dict[str, Any] = {
        "schema": REPORT_SCHEMA,
        "startedAt": datetime.now(timezone.utc).isoformat(),
        "platform": {"os": platform.system(), "arch": platform.machine()},
        "checks": checks,
        "status": "running",
    }

    try:
        for name, value in {
            "base Python": args.base_python,
            "Markdown overlay": args.overlay,
            "jobs root": args.jobs_root,
        }.items():
            if value is None or not value.exists():
                raise AcceptanceError(f"{name} is missing; pass its explicit path")
        base_python = args.base_python.resolve(strict=True)
        overlay = args.overlay.resolve(strict=True)
        jobs_root = args.jobs_root.resolve(strict=True)

        run_check(
            checks,
            "workspace translation state",
            ["node", "scripts/test-workspace-translation-state.mjs"],
            app_root,
        )
        run_check(checks, "frontend typecheck", ["pnpm", "typecheck"], app_root)
        run_check(
            checks,
            "PDF production boundary",
            ["pnpm", "check:pdf-production-boundary"],
            app_root,
        )
        run_check(
            checks,
            "PDF Markdown file lifecycle",
            ["cargo", "test", "pdf_markdown_release_acceptance_end_to_end"],
            tauri_root,
        )
        run_check(checks, "Rosetta jobs regression", ["cargo", "test", "rosetta_jobs"], tauri_root)
        run_check(
            checks,
            "managed Markdown runtime regression",
            ["cargo", "test", "managed_pdf_markdown"],
            tauri_root,
        )
        run_check(
            checks,
            "worker protocol",
            [sys.executable, str(script.with_name("test-rosetta-pdf-markdown-worker.py"))],
            repo_root,
        )
        run_check(
            checks,
            "Checkpoint 0 harness",
            [sys.executable, str(script.with_name("test-pdf-markdown-checkpoint0.py"))],
            repo_root,
        )
        run_check(
            checks,
            "concurrent PyMuPDF isolation",
            [
                sys.executable,
                str(script.with_name("test-pdf-markdown-runtime-isolation.py")),
                "--base-python",
                str(base_python),
                "--overlay",
                str(overlay),
                "--jobs-root",
                str(jobs_root),
            ],
            repo_root,
        )

        if not args.skip_corpus:
            docling_root = args.docling_root or repo_root.parent / "docling"
            if not docling_root.is_dir():
                raise AcceptanceError("Docling corpus root is missing; pass --docling-root")
            corpus_env = os.environ.copy()
            for key in [
                "PYTHONHOME",
                "PYTHONSTARTUP",
                "PYTHONUSERBASE",
                "VIRTUAL_ENV",
                "CONDA_PREFIX",
                "LD_LIBRARY_PATH",
            ]:
                corpus_env.pop(key, None)
            corpus_env.update(
                {
                    "PYTHONPATH": str(overlay),
                    "PYTHONNOUSERSITE": "1",
                    "CUDA_VISIBLE_DEVICES": "",
                }
            )
            with tempfile.TemporaryDirectory(prefix="rosetta-pdf-markdown-release-") as temp:
                output = Path(temp)
                run_check(
                    checks,
                    "24-document release corpus",
                    [
                        str(base_python),
                        str(script.with_name("pdf_markdown_checkpoint0.py")),
                        "run",
                        "--manifest",
                        str(script.with_name("pdf-markdown-corpus-manifest.json")),
                        "--output",
                        str(output),
                        "--root",
                        f"rosetta={repo_root}",
                        "--root",
                        f"docling={docling_root.resolve(strict=True)}",
                    ],
                    repo_root,
                    env=corpus_env,
                )
                report["corpus"] = validate_corpus_report(
                    json.loads((output / "report.json").read_text(encoding="utf-8"))
                )

        report["status"] = "passed"
    except (AcceptanceError, subprocess.TimeoutExpired) as error:
        report["status"] = "failed"
        report["error"] = str(error)
        raise
    finally:
        report["finishedAt"] = datetime.now(timezone.utc).isoformat()
        write_report(report_path.resolve(), report)
        print(f"[acceptance] report: {report_path.resolve()}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (AcceptanceError, subprocess.TimeoutExpired) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
