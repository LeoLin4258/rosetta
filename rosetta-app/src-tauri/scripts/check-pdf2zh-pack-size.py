#!/usr/bin/env python3

import argparse
import json
from pathlib import Path


BASELINE = {
    "size_bytes": 510_388_352,
    "unpacked_size_bytes": 1_353_005_365,
    "file_count": 21_573,
    "max_single_file_bytes": 218_461_128,
    "symlink_count": 1_048,
}

LIMITS = {
    "archive_hard_bytes": 650 * 1024 * 1024,
    "relative_warning_percent": 5,
    "relative_hard_percent": 15,
    "unpacked_warning_bytes": 1_420_655_634,
    "unpacked_hard_bytes": 1_555_956_170,
    "file_count_warning": 22_652,
    "file_count_hard": 24_809,
    "max_single_file_hard_bytes": 256 * 1024 * 1024,
}

REQUIRED_METRICS = tuple(BASELINE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check a Linux pdf2zh pack manifest against release size budgets."
    )
    parser.add_argument("manifest")
    parser.add_argument("--output")
    return parser.parse_args()


def load_metrics(path: Path) -> dict[str, int]:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    metrics = {}
    for name in REQUIRED_METRICS:
        value = manifest.get(name)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValueError(f"manifest field {name} must be a non-negative integer")
        metrics[name] = value
    return metrics


def evaluate(metrics: dict[str, int]) -> dict[str, object]:
    failures = []
    warnings = []
    archive_growth_percent = (
        (metrics["size_bytes"] - BASELINE["size_bytes"])
        * 100
        / BASELINE["size_bytes"]
    )

    if metrics["size_bytes"] > LIMITS["archive_hard_bytes"]:
        failures.append("archive exceeds the 650 MiB absolute hard cap")
    if archive_growth_percent > LIMITS["relative_hard_percent"]:
        failures.append("archive growth exceeds the 15 percent relative hard limit")
    elif archive_growth_percent > LIMITS["relative_warning_percent"]:
        warnings.append("archive growth exceeds the 5 percent warning threshold")

    if metrics["unpacked_size_bytes"] > LIMITS["unpacked_hard_bytes"]:
        failures.append("unpacked bytes exceed the 15 percent hard limit")
    elif metrics["unpacked_size_bytes"] > LIMITS["unpacked_warning_bytes"]:
        warnings.append("unpacked bytes exceed the 5 percent warning threshold")

    if metrics["file_count"] > LIMITS["file_count_hard"]:
        failures.append("regular file count exceeds the 15 percent hard limit")
    elif metrics["file_count"] > LIMITS["file_count_warning"]:
        warnings.append("regular file count exceeds the 5 percent warning threshold")

    if metrics["max_single_file_bytes"] > LIMITS["max_single_file_hard_bytes"]:
        failures.append("largest regular file exceeds the 256 MiB hard cap")
    elif metrics["max_single_file_bytes"] > BASELINE["max_single_file_bytes"]:
        warnings.append("largest regular file exceeds the published baseline")

    if metrics["symlink_count"] > BASELINE["symlink_count"]:
        warnings.append("symlink count exceeds the published baseline")

    status = "failed" if failures else "warning" if warnings else "passed"
    return {
        "schemaVersion": 1,
        "platform": "linux-x64",
        "status": status,
        "baseline": BASELINE,
        "limits": LIMITS,
        "metrics": metrics,
        "archiveGrowthPercent": round(archive_growth_percent, 4),
        "warnings": warnings,
        "failures": failures,
    }


def main() -> None:
    args = parse_args()
    manifest_path = Path(args.manifest)
    try:
        result = evaluate(load_metrics(manifest_path))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        result = {
            "schemaVersion": 1,
            "platform": "linux-x64",
            "status": "failed",
            "warnings": [],
            "failures": [str(error)],
        }

    rendered = json.dumps(result, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    print(rendered, end="")
    if result["status"] == "failed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
