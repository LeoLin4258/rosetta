"""Run Docling against the project's PDF fixtures and dump the structured
output to JSON for comparison with the Rust spike's chars+baseline approach.

Usage:
    .venv/bin/python scripts/run.py [fixture-name-or-path]

Without an argument runs against all fixtures under
`rosetta-app/src-tauri/fixtures/pdf/`.
"""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

from docling.document_converter import DocumentConverter

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURE_DIR = REPO_ROOT / "src-tauri" / "fixtures" / "pdf"
OUTPUT_DIR = Path(__file__).resolve().parent.parent / "output"


def run_one(converter: DocumentConverter, source: Path) -> dict:
    name = source.stem
    print(f"\n== {source.name} ==", flush=True)
    t0 = time.perf_counter()
    result = converter.convert(source)
    elapsed = time.perf_counter() - t0
    print(f"   conversion: {elapsed:.1f}s", flush=True)

    doc = result.document
    summary = {
        "fixture": source.name,
        "elapsed_seconds": round(elapsed, 2),
        "num_pages": len(doc.pages),
        "num_texts": len(doc.texts),
        "num_tables": len(doc.tables),
        "num_pictures": len(doc.pictures),
    }
    print(
        f"   pages={summary['num_pages']} texts={summary['num_texts']} "
        f"tables={summary['num_tables']} pictures={summary['num_pictures']}",
        flush=True,
    )

    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    md_path = OUTPUT_DIR / f"{name}.md"
    md_path.write_text(doc.export_to_markdown(), encoding="utf-8")
    print(f"   wrote {md_path.relative_to(REPO_ROOT)}", flush=True)

    # Also dump the structured tree so we can see what we'd actually get to
    # work with on the Rust side.
    raw_path = OUTPUT_DIR / f"{name}.docling.json"
    raw_path.write_text(
        json.dumps(doc.export_to_dict(), ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(f"   wrote {raw_path.relative_to(REPO_ROOT)}", flush=True)

    return summary


def main() -> int:
    sources: list[Path]
    if len(sys.argv) > 1:
        arg = sys.argv[1]
        candidate = Path(arg)
        if not candidate.exists():
            candidate = FIXTURE_DIR / arg
        if not candidate.is_file():
            candidate = FIXTURE_DIR / f"{arg}.pdf"
        if not candidate.is_file():
            print(f"No such fixture: {arg}", file=sys.stderr)
            return 2
        sources = [candidate]
    else:
        sources = sorted(FIXTURE_DIR.glob("*.pdf"))

    if not sources:
        print(f"No PDFs in {FIXTURE_DIR}", file=sys.stderr)
        return 2

    print("Initializing DocumentConverter (loads models on first run)…", flush=True)
    converter = DocumentConverter()

    summaries = [run_one(converter, src) for src in sources]
    print("\n== summary ==")
    for s in summaries:
        print(f"  {s['fixture']:40s} {s['elapsed_seconds']:6.1f}s  "
              f"pages={s['num_pages']:3d}  texts={s['num_texts']:4d}  "
              f"tables={s['num_tables']:3d}  pics={s['num_pictures']:3d}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
