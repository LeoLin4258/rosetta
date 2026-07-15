#!/usr/bin/env python3
"""Verify that durable PDF layout artifacts survive engine disposal.

This is an engine integration probe, not part of the shipped worker. It proves
that compressed layout masks can rebuild pdfminer replay state and still
render a valid page after all process-local state is released.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import tempfile
import time

from pdf2zh import rosetta_engine as engine


def prepare(input_pdf: Path, scratch: Path, cache_dir: Path) -> tuple[dict, float]:
    started = time.perf_counter()
    prepared = engine.prepareRun(
        str(input_pdf),
        [1],
        "en",
        "zh",
        {
            "scratchDir": str(scratch),
            "cleanupScratchDir": False,
            "thread": 1,
            "modelPath": os.environ["ROSETTA_DOCLAYOUT_MODEL"],
            "persistentLayoutCacheDir": str(cache_dir),
            "persistentLayoutCacheKey": "probe-cache-key-v1",
            "persistentSourceFingerprint": "probe-source-fingerprint",
        },
    )
    return prepared, (time.perf_counter() - started) * 1000


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("pdf", type=Path)
    args = parser.parse_args()
    input_pdf = args.pdf.resolve()
    if not input_pdf.is_file():
        raise SystemExit(f"PDF does not exist: {input_pdf}")

    with tempfile.TemporaryDirectory(prefix="rosetta-pdf-cache-probe-") as root_text:
        root = Path(root_text)
        first_scratch = root / "first"
        second_scratch = root / "second"
        cache_dir = root / "cache"
        render_dir = root / "render"
        for path in (first_scratch, second_scratch, cache_dir, render_dir):
            path.mkdir(parents=True, exist_ok=True)

        prepared, full_ms = prepare(input_pdf, first_scratch, cache_dir)
        if prepared.get("persistentLayoutCacheHit"):
            raise SystemExit("first prepare unexpectedly reported a disk cache hit")
        cached_layout = cache_dir / "layout.npz"
        cached_manifest = cache_dir / "manifest.json"
        if not cached_layout.is_file() or not cached_manifest.is_file():
            raise SystemExit("first prepare did not persist the layout cache")
        engine.disposeRun(prepared["preparedRunId"])

        restored, restored_ms = prepare(input_pdf, second_scratch, cache_dir)
        if not restored.get("persistentLayoutCacheHit"):
            raise SystemExit("second prepare did not restore the disk layout cache")

        units = engine.collectUnits(restored["preparedRunId"])
        translations = {
            unit["unitId"]: unit["sourceText"]
            for unit in units
            if unit["requiresTranslation"]
        }
        results = engine.renderPages(
            restored["preparedRunId"], translations, str(render_dir), pages=[1]
        )
        engine.disposeRun(restored["preparedRunId"])

        if len(results) != 1 or results[0]["status"] not in {"translated", "no_text"}:
            raise SystemExit(f"restored render failed: {results}")

        print(
            "persistent-prepare-cache-probe-ok "
            f"fullMs={int(full_ms)} restoredMs={int(restored_ms)} "
            f"layoutBytes={cached_layout.stat().st_size} "
            f"units={len(units)} status={results[0]['status']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
