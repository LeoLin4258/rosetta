"""Run MinerU (pipeline backend) on the same fixtures as docling-probe.

Usage:
    .venv/bin/python scripts/run.py [fixture-name]

Without args runs against all fixtures.
"""
from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURE_DIR = REPO_ROOT / "src-tauri" / "fixtures" / "pdf"
OUTPUT_DIR = Path(__file__).resolve().parent.parent / "output"
MINERU_BIN = Path(__file__).resolve().parent.parent / ".venv" / "bin" / "mineru"


def run_one(source: Path) -> dict:
    name = source.stem
    out_dir = OUTPUT_DIR / name
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"\n== {source.name} ==", flush=True)
    t0 = time.perf_counter()
    proc = subprocess.run(
        [
            str(MINERU_BIN),
            "-p", str(source),
            "-o", str(out_dir),
            "-b", "pipeline",
            "-m", "auto",
        ],
        capture_output=True,
        text=True,
    )
    elapsed = time.perf_counter() - t0
    if proc.returncode != 0:
        print(f"   FAILED in {elapsed:.1f}s", flush=True)
        print(f"   stderr tail:\n{proc.stderr[-1500:]}", flush=True)
        return {"fixture": source.name, "error": True, "elapsed_seconds": round(elapsed, 2)}
    print(f"   conversion: {elapsed:.1f}s", flush=True)

    # MinerU writes a hierarchical output. Find the parsed middle.json + md.
    md_files = list(out_dir.rglob("*.md"))
    middle_files = list(out_dir.rglob("*_middle.json"))
    summary = {
        "fixture": source.name,
        "elapsed_seconds": round(elapsed, 2),
    }
    if middle_files:
        middle = json.loads(middle_files[0].read_text(encoding="utf-8"))
        pdf_info = middle.get("pdf_info") or []
        summary["num_pages"] = len(pdf_info)
        n_blocks = 0
        roles: dict[str, int] = {}
        for page in pdf_info:
            for block in page.get("para_blocks", []) or []:
                n_blocks += 1
                t = block.get("type", "?")
                roles[t] = roles.get(t, 0) + 1
        summary["num_blocks"] = n_blocks
        summary["role_breakdown"] = roles
        print(
            f"   pages={summary['num_pages']} blocks={summary['num_blocks']} "
            f"roles={roles}",
            flush=True,
        )
    if md_files:
        print(f"   md: {md_files[0].relative_to(REPO_ROOT)}", flush=True)
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

    summaries = [run_one(src) for src in sources]
    print("\n== summary ==")
    for s in summaries:
        if s.get("error"):
            print(f"  {s['fixture']:42s} ERROR after {s['elapsed_seconds']}s")
        else:
            print(
                f"  {s['fixture']:42s} {s['elapsed_seconds']:6.1f}s  "
                f"pages={s.get('num_pages', '?'):>3}  blocks={s.get('num_blocks', '?'):>4}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
