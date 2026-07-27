#!/usr/bin/env python3

import argparse
import hashlib
import json
import re
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare two pdf2zh pack inventories and their pip freezes."
    )
    parser.add_argument("old_inventory")
    parser.add_argument("new_inventory")
    parser.add_argument("--old-freeze", required=True)
    parser.add_argument("--new-freeze", required=True)
    parser.add_argument("--top", type=int, default=100)
    parser.add_argument("--pretty", action="store_true")
    return parser.parse_args()


def canonical_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, object]:
    with path.open(encoding="utf-8") as file:
        return json.load(file)


def parse_freeze(path: Path) -> dict[str, dict[str, str]]:
    packages = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if " @ " in line:
            name, value = line.split(" @ ", 1)
            version = value
        elif "==" in line:
            name, version = line.split("==", 1)
        else:
            name = line
            version = line
        packages[canonical_name(name)] = {
            "name": name,
            "value": version,
            "line": line,
        }
    return packages


def path_size_map(entries: list[dict[str, object]]) -> dict[str, int]:
    return {str(entry["path"]): int(entry["bytes"]) for entry in entries}


def size_deltas(
    old_entries: list[dict[str, object]],
    new_entries: list[dict[str, object]],
    limit: int,
) -> list[dict[str, object]]:
    old_sizes = path_size_map(old_entries)
    new_sizes = path_size_map(new_entries)
    deltas = [
        {
            "path": path,
            "oldBytes": old_sizes.get(path, 0),
            "newBytes": new_sizes.get(path, 0),
            "deltaBytes": new_sizes.get(path, 0) - old_sizes.get(path, 0),
        }
        for path in old_sizes.keys() | new_sizes.keys()
        if new_sizes.get(path, 0) != old_sizes.get(path, 0)
    ]
    deltas.sort(key=lambda item: (-abs(int(item["deltaBytes"])), str(item["path"])))
    return deltas[:limit]


def distribution_map(inventory: dict[str, object]) -> dict[str, dict[str, object]]:
    site_packages = dict(inventory["sitePackages"])
    distributions = list(site_packages["distributions"])
    return {
        canonical_name(str(distribution["name"])): dict(distribution)
        for distribution in distributions
    }


def distribution_deltas(
    old_inventory: dict[str, object], new_inventory: dict[str, object], limit: int
) -> list[dict[str, object]]:
    old_distributions = distribution_map(old_inventory)
    new_distributions = distribution_map(new_inventory)
    deltas = []
    for name in old_distributions.keys() | new_distributions.keys():
        old_distribution = old_distributions.get(name)
        new_distribution = new_distributions.get(name)
        old_bytes = int(old_distribution["bytes"]) if old_distribution else 0
        new_bytes = int(new_distribution["bytes"]) if new_distribution else 0
        old_version = str(old_distribution["version"]) if old_distribution else None
        new_version = str(new_distribution["version"]) if new_distribution else None
        if old_distribution is None:
            change = "added"
        elif new_distribution is None:
            change = "removed"
        elif old_version != new_version:
            change = "upgraded"
        elif old_bytes != new_bytes:
            change = "size-changed"
        else:
            continue
        deltas.append(
            {
                "name": (
                    str(new_distribution["name"])
                    if new_distribution
                    else str(old_distribution["name"])
                ),
                "change": change,
                "oldVersion": old_version,
                "newVersion": new_version,
                "oldBytes": old_bytes,
                "newBytes": new_bytes,
                "deltaBytes": new_bytes - old_bytes,
            }
        )
    deltas.sort(key=lambda item: (-abs(int(item["deltaBytes"])), str(item["name"]).lower()))
    return deltas[:limit]


def freeze_diff(old_path: Path, new_path: Path) -> dict[str, list[dict[str, object]]]:
    old_packages = parse_freeze(old_path)
    new_packages = parse_freeze(new_path)
    added = []
    removed = []
    changed = []
    for name in sorted(old_packages.keys() | new_packages.keys()):
        old_package = old_packages.get(name)
        new_package = new_packages.get(name)
        if old_package is None:
            added.append(new_package)
        elif new_package is None:
            removed.append(old_package)
        elif old_package["line"] != new_package["line"]:
            changed.append(
                {
                    "name": new_package["name"],
                    "old": old_package["line"],
                    "new": new_package["line"],
                }
            )
    return {"added": added, "removed": removed, "changed": changed}


def inventory_summary(inventory: dict[str, object]) -> dict[str, object]:
    return {
        key: inventory[key]
        for key in (
            "archive",
            "archiveBytes",
            "archiveSha256",
            "unpackedBytes",
            "archiveToUnpackedRatio",
            "unpackedToArchiveRatio",
            "regularFileCount",
            "directoryCount",
            "symlinkCount",
            "hardlinkCount",
        )
    }


def main() -> None:
    args = parse_args()
    old_inventory_path = Path(args.old_inventory)
    new_inventory_path = Path(args.new_inventory)
    old_freeze_path = Path(args.old_freeze)
    new_freeze_path = Path(args.new_freeze)
    old_inventory = load_json(old_inventory_path)
    new_inventory = load_json(new_inventory_path)
    old_unpacked = int(old_inventory["unpackedBytes"])
    new_unpacked = int(new_inventory["unpackedBytes"])
    old_site_packages = dict(old_inventory["sitePackages"])
    new_site_packages = dict(new_inventory["sitePackages"])
    old_residue = dict(old_inventory["residue"])
    new_residue = dict(new_inventory["residue"])
    old_tests = dict(old_inventory["testsHeadersStaticLibraries"])
    new_tests = dict(new_inventory["testsHeadersStaticLibraries"])
    old_duplicates = dict(old_inventory["duplicateFilesAtOrAboveThreshold"])
    new_duplicates = dict(new_inventory["duplicateFilesAtOrAboveThreshold"])

    report = {
        "schemaVersion": 1,
        "inputs": {
            "oldInventorySha256": file_sha256(old_inventory_path),
            "newInventorySha256": file_sha256(new_inventory_path),
            "oldFreezeSha256": file_sha256(old_freeze_path),
            "newFreezeSha256": file_sha256(new_freeze_path),
        },
        "old": inventory_summary(old_inventory),
        "new": inventory_summary(new_inventory),
        "delta": {
            "archiveBytes": int(new_inventory["archiveBytes"])
            - int(old_inventory["archiveBytes"]),
            "unpackedBytes": new_unpacked - old_unpacked,
            "regularFileCount": int(new_inventory["regularFileCount"])
            - int(old_inventory["regularFileCount"]),
            "directoryCount": int(new_inventory["directoryCount"])
            - int(old_inventory["directoryCount"]),
            "symlinkCount": int(new_inventory["symlinkCount"])
            - int(old_inventory["symlinkCount"]),
            "hardlinkCount": int(new_inventory["hardlinkCount"])
            - int(old_inventory["hardlinkCount"]),
            "sitePackagesBytes": int(new_site_packages["bytes"])
            - int(old_site_packages["bytes"]),
            "sitePackagesUnattributedBytes": int(new_site_packages["unattributedBytes"])
            - int(old_site_packages["unattributedBytes"]),
            "residueBytes": int(new_residue["bytes"]) - int(old_residue["bytes"]),
            "testsHeadersStaticLibrariesBytes": int(new_tests["bytes"])
            - int(old_tests["bytes"]),
            "duplicateBytesAtOrAboveThreshold": int(new_duplicates["duplicateBytes"])
            - int(old_duplicates["duplicateBytes"]),
        },
        "growthCoverage": {
            "directoryExplainedBytes": new_unpacked - old_unpacked,
            "directoryExplainedRatio": 1.0 if new_unpacked != old_unpacked else None,
            "oldSitePackagesAttributedRatio": old_site_packages["attributedRatio"],
            "newSitePackagesAttributedRatio": new_site_packages["attributedRatio"],
        },
        "primaryCategoryDeltas": size_deltas(
            list(old_inventory["primaryCategories"]),
            list(new_inventory["primaryCategories"]),
            args.top,
        ),
        "topLevelAreaDeltas": size_deltas(
            list(old_inventory["topLevelAreas"]),
            list(new_inventory["topLevelAreas"]),
            args.top,
        ),
        "topDirectoryDeltas": size_deltas(
            list(old_inventory["topDirectories"]),
            list(new_inventory["topDirectories"]),
            args.top,
        ),
        "topFileDeltas": size_deltas(
            list(old_inventory["topFiles"]),
            list(new_inventory["topFiles"]),
            args.top,
        ),
        "distributionDeltas": distribution_deltas(old_inventory, new_inventory, args.top),
        "freezeDiff": freeze_diff(old_freeze_path, new_freeze_path),
    }
    print(
        json.dumps(
            report,
            ensure_ascii=False,
            indent=2 if args.pretty else None,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
