#!/usr/bin/env python3
"""Run two fresh PDF workers and require a durable layout-cache hit."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


WORKER_PATH = (
    Path(__file__).resolve().parents[1]
    / "src"
    / "managed_pdf2zh"
    / "rosetta_pdf2zh_worker.py"
)


def run_worker(input_pdf: Path, root: Path, run: int, pages: list[int]) -> tuple[dict, dict]:
    scratch = root / f"scratch-{run}"
    scratch.mkdir(parents=True, exist_ok=True)
    job = {
        "id": f"run-{run}",
        "cmd": "prepare_pdf_window",
        "file": str(input_pdf),
        "outputDir": str(scratch),
        "tmpDir": str(scratch),
        "pages": pages,
        "langIn": "en",
        "langOut": "zh",
        "thread": 1,
        "cacheKey": "persistent-worker-probe-v1",
        "cacheOwnerKey": "job-probe",
        "options": {
            "cleanupScratchDir": False,
            "persistentLayoutCacheDir": str(
                root / "jobs" / "job-probe" / "pdf-prepare-cache" / "v1" / "entry"
            ),
            "persistentLayoutCacheKey": "persistent-worker-probe-v1",
            "persistentSourceFingerprint": "persistent-worker-probe-source",
        },
    }
    stdin = "\n".join(
        (
            json.dumps(job, ensure_ascii=False),
            json.dumps({"id": f"exit-{run}", "cmd": "exit"}),
            "",
        )
    )
    completed = subprocess.run(
        [sys.executable, str(WORKER_PATH)],
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={
            **os.environ,
            "ROSETTA_PDF_PREPARE_CACHE_JOBS_ROOT": str(root / "jobs"),
        },
        check=False,
        timeout=300,
    )
    events = []
    for line in completed.stdout.splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    prepared = next(
        (event for event in events if event.get("event") == "prepared_pdf_window"),
        None,
    )
    ready = next((event for event in events if event.get("event") == "ready"), None)
    if completed.returncode != 0 or ready is None or prepared is None:
        raise RuntimeError(
            f"worker run {run} failed with exit {completed.returncode}:\n"
            f"{completed.stderr[-4000:]}\nprotocol={events[-4:]}"
        )
    return ready, prepared


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=Path)
    parser.add_argument("--page-count", type=int, default=10)
    args = parser.parse_args()
    input_pdf = args.pdf.resolve()
    if not input_pdf.is_file():
        raise SystemExit(f"PDF does not exist: {input_pdf}")
    pages = list(range(1, max(1, args.page_count) + 1))

    with tempfile.TemporaryDirectory(prefix="rosetta-worker-cache-probe-") as root_text:
        root = Path(root_text)
        job_dir = root / "jobs" / "job-probe"
        job_dir.mkdir(parents=True)
        (job_dir / "pdf_source.json").write_text(
            json.dumps({"sourceFingerprint": "persistent-worker-probe-source"}),
            encoding="utf-8",
        )
        first_ready, first = run_worker(input_pdf, root, 1, pages)
        second_ready, second = run_worker(input_pdf, root, 2, pages)
        if first_ready.get("cachedOwnerKeys"):
            raise SystemExit("first worker unexpectedly discovered a durable cache")
        if "job-probe" not in (second_ready.get("cachedOwnerKeys") or []):
            raise SystemExit(
                "second worker did not restore the durable cache owner during startup"
            )
        if first.get("cacheTier") != "miss":
            raise SystemExit(f"first worker unexpectedly hit cache: {first.get('cacheTier')}")
        if second.get("cacheTier") != "disk" or not second.get("cacheHit"):
            raise SystemExit(f"second worker did not hit disk cache: {second.get('cacheTier')}")
        if len(first.get("units") or []) != len(second.get("units") or []):
            raise SystemExit("unit count changed after restoring the disk cache")
        second_timings = second.get("timingsMs") or {}
        if second_timings.get("layout") != 0:
            raise SystemExit(f"disk restore still ran layout inference: {second_timings}")

        print(
            "persistent-worker-cache-probe-ok "
            f"pages={len(pages)} units={len(second.get('units') or [])} "
            f"firstMs={(first.get('timingsMs') or {}).get('total')} "
            f"restoredMs={second_timings.get('total')} "
            f"restoredUnitCollectionMs={second_timings.get('unitCollection')}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
