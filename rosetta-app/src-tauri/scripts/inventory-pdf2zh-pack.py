#!/usr/bin/env python3

import argparse
import csv
import hashlib
import io
import json
import os
import posixpath
import tarfile
from collections import defaultdict
from email.parser import BytesParser
from pathlib import PurePosixPath


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a deterministic size inventory for a pdf2zh tar archive."
    )
    parser.add_argument("archive")
    parser.add_argument("--duplicate-threshold", type=int, default=1024 * 1024)
    parser.add_argument("--top", type=int, default=100)
    parser.add_argument("--pretty", action="store_true")
    return parser.parse_args()


def normalized_member_path(name: str) -> str:
    normalized = posixpath.normpath(name.lstrip("./"))
    return "" if normalized == "." else normalized


def relative_pack_path(path: str, pack_root: str) -> str:
    if path == pack_root:
        return ""
    prefix = f"{pack_root}/"
    return path[len(prefix) :] if path.startswith(prefix) else path


def primary_area(relative_path: str) -> str:
    if not relative_path:
        return "root"
    parts = PurePosixPath(relative_path).parts
    if parts[0] == "models":
        return "model"
    if parts[:2] == ("assets", "babeldoc"):
        return "font_assets"
    if "site-packages" in parts:
        return "site_packages"
    if parts[0] == "python":
        return "python_runtime"
    if parts[0] == "licenses":
        return "licenses"
    if parts[0] == "bin":
        return "launcher"
    return "other"


def is_residue(relative_path: str) -> bool:
    path = PurePosixPath(relative_path)
    lowered_parts = {part.lower() for part in path.parts}
    return bool(
        lowered_parts.intersection({"__pycache__", ".cache", "cache", "build", "tmp"})
        or path.suffix.lower() in {".pyc", ".pyo"}
        or path.name.endswith(".optimized")
    )


def is_test_header_or_static(relative_path: str) -> bool:
    path = PurePosixPath(relative_path)
    lowered_parts = {part.lower() for part in path.parts}
    return bool(
        lowered_parts.intersection({"test", "tests", "testing", "include", "headers"})
        or path.suffix.lower() in {".a", ".c", ".cc", ".cpp", ".h", ".hpp"}
    )


def sorted_size_entries(values: dict[str, int], limit: int) -> list[dict[str, object]]:
    return [
        {"path": path, "bytes": size}
        for path, size in sorted(values.items(), key=lambda item: (-item[1], item[0]))[:limit]
    ]


def parse_distributions(
    captured_files: dict[str, bytes], member_sizes: dict[str, int]
) -> tuple[list[dict[str, object]], int, int]:
    distributions = []
    owned_paths: set[str] = set()
    site_packages_total = sum(
        size for path, size in member_sizes.items() if "/site-packages/" in f"/{path}/"
    )

    for metadata_path, metadata_bytes in sorted(captured_files.items()):
        if not metadata_path.endswith(".dist-info/METADATA"):
            continue
        dist_info_dir = posixpath.dirname(metadata_path)
        record_path = f"{dist_info_dir}/RECORD"
        record_bytes = captured_files.get(record_path)
        if record_bytes is None:
            continue
        metadata = BytesParser().parsebytes(metadata_bytes)
        site_packages_dir = metadata_path.split("/site-packages/", 1)[0] + "/site-packages"
        distribution_paths: set[str] = set()
        for row in csv.reader(io.StringIO(record_bytes.decode("utf-8", errors="replace"))):
            if not row:
                continue
            distribution_paths.add(
                posixpath.normpath(posixpath.join(site_packages_dir, row[0]))
            )
        existing_paths = distribution_paths.intersection(member_sizes)
        owned_paths.update(existing_paths)
        distributions.append(
            {
                "name": metadata.get("Name", PurePosixPath(dist_info_dir).name),
                "version": metadata.get("Version", "unknown"),
                "bytes": sum(member_sizes[path] for path in existing_paths),
                "fileCount": len(existing_paths),
                "recordedFileCount": len(distribution_paths),
            }
        )

    distributions.sort(key=lambda item: (-int(item["bytes"]), str(item["name"]).lower()))
    owned_site_package_paths = {
        path for path in owned_paths if "/site-packages/" in f"/{path}/"
    }
    attributed_bytes = sum(member_sizes[path] for path in owned_site_package_paths)
    return distributions, site_packages_total, attributed_bytes


def inventory_archive(
    archive_path: str, duplicate_threshold: int, top_limit: int
) -> dict[str, object]:
    archive_hash = hashlib.sha256()
    with open(archive_path, "rb") as archive_file:
        for chunk in iter(lambda: archive_file.read(1024 * 1024), b""):
            archive_hash.update(chunk)

    member_sizes: dict[str, int] = {}
    directory_sizes: dict[str, int] = defaultdict(int)
    top_level_sizes: dict[str, int] = defaultdict(int)
    category_sizes: dict[str, int] = defaultdict(int)
    residue_sizes: dict[str, int] = {}
    test_header_static_sizes: dict[str, int] = {}
    captured_files: dict[str, bytes] = {}
    duplicate_groups: dict[tuple[int, str], list[str]] = defaultdict(list)
    pack_root = ""
    regular_file_count = 0
    directory_count = 0
    symlink_count = 0
    hardlink_count = 0

    with tarfile.open(archive_path, mode="r|*") as archive:
        for member in archive:
            member_path = normalized_member_path(member.name)
            if not member_path:
                continue
            if not pack_root:
                pack_root = PurePosixPath(member_path).parts[0]
            relative_path = relative_pack_path(member_path, pack_root)
            if member.isdir():
                directory_count += 1
                continue
            if member.issym():
                symlink_count += 1
                continue
            if member.islnk():
                hardlink_count += 1
                continue
            if not member.isfile():
                continue

            regular_file_count += 1
            member_sizes[member_path] = member.size
            relative_parts = PurePosixPath(relative_path).parts
            top_level = relative_parts[0] if relative_parts else "root"
            top_level_sizes[top_level] += member.size
            category_sizes[primary_area(relative_path)] += member.size
            for depth in range(1, len(relative_parts)):
                directory = posixpath.join(pack_root, *relative_parts[:depth])
                directory_sizes[directory] += member.size
            if is_residue(relative_path):
                residue_sizes[member_path] = member.size
            if is_test_header_or_static(relative_path):
                test_header_static_sizes[member_path] = member.size

            should_capture = member_path.endswith(".dist-info/METADATA") or member_path.endswith(
                ".dist-info/RECORD"
            )
            should_hash = member.size >= duplicate_threshold
            if not should_capture and not should_hash:
                continue
            extracted = archive.extractfile(member)
            if extracted is None:
                continue
            captured_content = bytearray()
            content_hash = hashlib.sha256() if should_hash else None
            for chunk in iter(lambda: extracted.read(1024 * 1024), b""):
                if should_capture:
                    captured_content.extend(chunk)
                if content_hash is not None:
                    content_hash.update(chunk)
            if should_capture:
                captured_files[member_path] = bytes(captured_content)
            if content_hash is not None:
                duplicate_groups[(member.size, content_hash.hexdigest())].append(member_path)

    distributions, site_packages_total, attributed_site_packages = parse_distributions(
        captured_files, member_sizes
    )
    duplicates = [
        {
            "bytesEach": size,
            "copies": len(paths),
            "duplicateBytes": size * (len(paths) - 1),
            "sha256": digest,
            "paths": sorted(paths),
        }
        for (size, digest), paths in duplicate_groups.items()
        if len(paths) > 1
    ]
    duplicates.sort(
        key=lambda item: (-int(item["duplicateBytes"]), str(item["sha256"]))
    )
    unpacked_bytes = sum(member_sizes.values())
    archive_bytes = PurePosixPath(archive_path)
    archive_size = os.stat(archive_path).st_size

    return {
        "schemaVersion": 1,
        "archive": str(archive_bytes),
        "archiveBytes": archive_size,
        "archiveSha256": archive_hash.hexdigest(),
        "packRoot": pack_root,
        "unpackedBytes": unpacked_bytes,
        "archiveToUnpackedRatio": archive_size / unpacked_bytes if unpacked_bytes else None,
        "unpackedToArchiveRatio": unpacked_bytes / archive_size if archive_size else None,
        "regularFileCount": regular_file_count,
        "directoryCount": directory_count,
        "symlinkCount": symlink_count,
        "hardlinkCount": hardlink_count,
        "topLevelAreas": sorted_size_entries(top_level_sizes, top_limit),
        "primaryCategories": sorted_size_entries(category_sizes, top_limit),
        "topDirectories": sorted_size_entries(directory_sizes, top_limit),
        "topFiles": sorted_size_entries(member_sizes, top_limit),
        "sitePackages": {
            "bytes": site_packages_total,
            "attributedBytes": attributed_site_packages,
            "unattributedBytes": site_packages_total - attributed_site_packages,
            "attributedRatio": (
                attributed_site_packages / site_packages_total if site_packages_total else None
            ),
            "distributions": distributions,
        },
        "residue": {
            "bytes": sum(residue_sizes.values()),
            "fileCount": len(residue_sizes),
            "topFiles": sorted_size_entries(residue_sizes, top_limit),
        },
        "testsHeadersStaticLibraries": {
            "bytes": sum(test_header_static_sizes.values()),
            "fileCount": len(test_header_static_sizes),
            "topFiles": sorted_size_entries(test_header_static_sizes, top_limit),
        },
        "duplicateFilesAtOrAboveThreshold": {
            "thresholdBytes": duplicate_threshold,
            "duplicateBytes": sum(int(item["duplicateBytes"]) for item in duplicates),
            "groups": duplicates[:top_limit],
        },
    }


def main() -> None:
    args = parse_args()
    inventory = inventory_archive(args.archive, args.duplicate_threshold, args.top)
    print(
        json.dumps(
            inventory,
            ensure_ascii=False,
            indent=2 if args.pretty else None,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
