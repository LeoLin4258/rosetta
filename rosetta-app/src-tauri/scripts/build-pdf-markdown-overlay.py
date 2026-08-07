#!/usr/bin/env python3
"""Build deterministic, isolated PyMuPDF4LLM overlays for release profiles."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any, Iterable


ARCHIVE_ROOT = "pdf-markdown-overlay"
MANIFEST_SCHEMA = "rosetta-pdf-markdown-overlay-build/1"
EXPECTED_VERSIONS = {
    "pymupdf4llm": "1.28.0",
    "pymupdf-layout": "1.28.0",
    "PyMuPDF": "1.28.0",
    "tabulate": "0.10.0",
}
REQUIRED_LAYOUT_RESOURCES = {
    "feature_imf1.onnx",
    "layout_rf2.4.1+imf1.onnx",
    "layout_rf2.4.1+imf1.yaml",
    "table_grid_model_v4_ep.onnx",
}
TARGETS = {
    "windows-x64": {
        "platform": "win_amd64",
        "archive": "rosetta-pdf-markdown-windows-x64.zip",
    },
    "macos-arm64": {
        "platform": "macosx_11_0_arm64",
        "archive": "rosetta-pdf-markdown-macos-arm64.tar.gz",
    },
    "linux-x64": {
        "platform": "manylinux_2_28_x86_64",
        "archive": "rosetta-pdf-markdown-linux-x64.tar.gz",
    },
}


class BuildError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_write_json(path: Path, value: Any) -> None:
    payload = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    temp_path = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with temp_path.open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, path)
    finally:
        temp_path.unlink(missing_ok=True)


def safe_wheel_member(name: str) -> PurePosixPath:
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise BuildError(f"Unsafe wheel member: {name}")
    return path


def installed_relative_path(path: PurePosixPath) -> PurePosixPath | None:
    for index, part in enumerate(path.parts):
        if part.endswith(".data") and index + 1 < len(path.parts):
            category = path.parts[index + 1]
            remainder = path.parts[index + 2 :]
            if category in {"purelib", "platlib"}:
                return PurePosixPath(*remainder)
            return None
    return path


def extract_wheels(wheel_paths: Iterable[Path], overlay: Path) -> None:
    overlay.mkdir(parents=True, exist_ok=True)
    for wheel_path in sorted(wheel_paths, key=lambda path: path.name.casefold()):
        with zipfile.ZipFile(wheel_path) as archive:
            for member in sorted(archive.infolist(), key=lambda item: item.filename):
                if member.is_dir():
                    continue
                relative = installed_relative_path(safe_wheel_member(member.filename))
                if relative is None or not relative.parts:
                    continue
                destination = overlay.joinpath(*relative.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(member) as source, destination.open("wb") as target:
                    shutil.copyfileobj(source, target)


def prune_overlay(overlay: Path) -> dict[str, int]:
    before = sum(path.stat().st_size for path in overlay.rglob("*") if path.is_file())
    for directory in sorted(
        (path for path in overlay.rglob("__pycache__") if path.is_dir()),
        key=lambda path: len(path.parts),
        reverse=True,
    ):
        shutil.rmtree(directory)
    for path in overlay.rglob("*"):
        if path.is_file() and path.suffix.lower() in {".pyc", ".pyo"}:
            path.unlink()
    for relative in (
        Path("pymupdf") / "mupdf-devel",
        Path("pymupdf4llm") / "llama",
    ):
        target = overlay / relative
        if target.exists():
            shutil.rmtree(target)
    model_root = overlay / "pymupdf" / "layout" / "resources" / "onnx"
    if not model_root.is_dir():
        raise BuildError("pymupdf-layout model directory is missing")
    for model in model_root.iterdir():
        if model.is_file() and model.name not in REQUIRED_LAYOUT_RESOURCES:
            model.unlink()
    present = {path.name for path in model_root.iterdir() if path.is_file()}
    if present != REQUIRED_LAYOUT_RESOURCES:
        raise BuildError("Pinned default layout model closure is incomplete")
    after = sum(path.stat().st_size for path in overlay.rglob("*") if path.is_file())
    return {"beforeBytes": before, "afterBytes": after, "removedBytes": before - after}


def normalized_tar_info(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    info.mode = 0o755 if info.isdir() else 0o644
    return info


def write_deterministic_zip(overlay: Path, archive_path: Path) -> None:
    with zipfile.ZipFile(
        archive_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for path in sorted(overlay.rglob("*"), key=lambda item: item.as_posix()):
            if not path.is_file():
                continue
            relative = path.relative_to(overlay).as_posix()
            info = zipfile.ZipInfo(f"{ARCHIVE_ROOT}/{relative}", (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, path.read_bytes(), compresslevel=9)


def write_deterministic_tar_gz(overlay: Path, archive_path: Path) -> None:
    with tempfile.NamedTemporaryFile(suffix=".tar", delete=False) as handle:
        tar_path = Path(handle.name)
    try:
        with tarfile.open(tar_path, "w", format=tarfile.PAX_FORMAT) as archive:
            root_info = tarfile.TarInfo(ARCHIVE_ROOT)
            root_info.type = tarfile.DIRTYPE
            archive.addfile(normalized_tar_info(root_info))
            for path in sorted(overlay.rglob("*"), key=lambda item: item.as_posix()):
                relative = path.relative_to(overlay).as_posix()
                archive.add(
                    path,
                    arcname=f"{ARCHIVE_ROOT}/{relative}",
                    recursive=False,
                    filter=normalized_tar_info,
                )
        with tar_path.open("rb") as source, archive_path.open("wb") as raw_target:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw_target, mtime=0) as target:
                shutil.copyfileobj(source, target)
    finally:
        tar_path.unlink(missing_ok=True)


def download_wheels(
    target: str, requirements: Path, wheel_dir: Path, python: Path
) -> list[Path]:
    target_config = TARGETS[target]
    command = [
        str(python),
        "-m",
        "pip",
        "download",
        "--disable-pip-version-check",
        "--no-deps",
        "--only-binary=:all:",
        "--implementation",
        "cp",
        "--python-version",
        "312",
        "--abi",
        "cp312",
        "--platform",
        target_config["platform"],
        "--dest",
        str(wheel_dir),
        "--requirement",
        str(requirements),
    ]
    completed = subprocess.run(command, check=False)
    if completed.returncode != 0:
        raise BuildError(f"Wheel download failed for {target}")
    wheels = sorted(wheel_dir.glob("*.whl"))
    if len(wheels) != len(EXPECTED_VERSIONS):
        raise BuildError(f"Expected four wheels for {target}, found {len(wheels)}")
    return wheels


def sanitized_python_environment(overlay: Path | None = None) -> dict[str, str]:
    environment = os.environ.copy()
    for name in (
        "PYTHONHOME",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "PYTHONUSERBASE",
        "VIRTUAL_ENV",
    ):
        environment.pop(name, None)
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    if overlay is not None:
        environment["PYTHONPATH"] = str(overlay)
    return environment


def read_pymupdf_version(python: Path, overlay: Path | None = None) -> str:
    completed = subprocess.run(
        [str(python), "-c", "import pymupdf; print(pymupdf.__version__)"],
        check=False,
        capture_output=True,
        text=True,
        env=sanitized_python_environment(overlay),
    )
    if completed.returncode != 0:
        raise BuildError("Unable to resolve PyMuPDF version")
    return completed.stdout.strip()


def runtime_preflight(
    base_python: Path, overlay: Path, smoke_pdf: Path, scratch: Path
) -> dict[str, Any]:
    base_version_before = read_pymupdf_version(base_python)
    script = r"""
import importlib.metadata
import json
import pathlib
import pymupdf
import pymupdf4llm

expected = {
    "pymupdf4llm": "1.28.0",
    "pymupdf-layout": "1.28.0",
    "PyMuPDF": "1.28.0",
}
actual = {name: importlib.metadata.version(name) for name in expected}
if actual != expected or pymupdf.__version__ != "1.28.0":
    raise SystemExit("version isolation failed")
layout_wrapper = pymupdf._get_layout.__closure__[0].cell_contents
providers = list(layout_wrapper._model._providers)
if providers != ["CPUExecutionProvider"]:
    raise SystemExit("layout engine is not CPU-only")
output = pathlib.Path(__import__("sys").argv[2])
output.mkdir(parents=True, exist_ok=True)
raw = pymupdf4llm.to_json(
    __import__("sys").argv[1],
    pages=[0],
    use_ocr=False,
    force_text=False,
    write_images=True,
    image_path=str(output),
    show_progress=False,
)
value = json.loads(raw)
if value.get("use_ocr") not in (0, False):
    raise SystemExit("OCR was enabled")
if value.get("force_text") is not False or value.get("write_images") is not True:
    raise SystemExit("extraction policy mismatch")
if [page.get("page_number") for page in value.get("pages", [])] != [1]:
    raise SystemExit("page identity mismatch")
print(json.dumps({"versions": actual, "pages": 1, "providers": providers}, sort_keys=True))
"""
    completed = subprocess.run(
        [str(base_python), "-c", script, str(smoke_pdf), str(scratch)],
        check=False,
        capture_output=True,
        text=True,
        env=sanitized_python_environment(overlay),
    )
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip().splitlines()
        suffix = f": {detail[-1]}" if detail else ""
        raise BuildError(f"Trimmed overlay runtime preflight failed{suffix}")
    probe = json.loads(completed.stdout.strip().splitlines()[-1])
    base_version_after = read_pymupdf_version(base_python)
    if base_version_before != base_version_after:
        raise BuildError("Overlay changed the production worker's PyMuPDF version")
    return {
        "status": "passed",
        "basePyMuPDFBefore": base_version_before,
        "basePyMuPDFAfter": base_version_after,
        "overlayPyMuPDF": "1.28.0",
        "executionProviders": probe["providers"],
    }


def build_target(
    target: str,
    requirements: Path,
    output_dir: Path,
    python: Path,
    base_python: Path | None,
    smoke_pdf: Path | None,
    base_pack_bytes: int | None,
) -> dict[str, Any]:
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f"rosetta-pdf-markdown-{target}-") as temp:
        temp_root = Path(temp)
        wheel_dir = temp_root / "wheels"
        overlay = temp_root / "overlay"
        wheel_dir.mkdir()
        wheels = download_wheels(target, requirements, wheel_dir, python)
        extract_wheels(wheels, overlay)
        pruning = prune_overlay(overlay)
        current_target = (
            (target == "windows-x64" and sys.platform == "win32")
            or (target == "macos-arm64" and sys.platform == "darwin")
            or (target == "linux-x64" and sys.platform.startswith("linux"))
        )
        if current_target and base_python and smoke_pdf:
            preflight = runtime_preflight(
                base_python.resolve(strict=True),
                overlay,
                smoke_pdf.resolve(strict=True),
                temp_root / "smoke",
            )
        else:
            preflight = {"status": "not-run-on-native-host"}
        archive_path = output_dir / TARGETS[target]["archive"]
        archive_temp = archive_path.with_name(f".{archive_path.name}.{os.getpid()}.tmp")
        archive_temp.unlink(missing_ok=True)
        if archive_path.suffix == ".zip":
            write_deterministic_zip(overlay, archive_temp)
        else:
            write_deterministic_tar_gz(overlay, archive_temp)
        os.replace(archive_temp, archive_path)
        files = [path for path in overlay.rglob("*") if path.is_file()]
        archive_bytes = archive_path.stat().st_size
        manifest = {
            "schema": MANIFEST_SCHEMA,
            "target": target,
            "archiveFilename": archive_path.name,
            "archiveBytes": archive_bytes,
            "archiveSha256": sha256_file(archive_path),
            "unpackedBytes": sum(path.stat().st_size for path in files),
            "fileCount": len(files),
            "versions": EXPECTED_VERSIONS,
            "cpuOnly": True,
            "useOcr": False,
            "forceText": False,
            "writeImages": True,
            "integrationBoundary": "to_json",
            "requiredLayoutResources": sorted(REQUIRED_LAYOUT_RESOURCES),
            "pruning": pruning,
            "runtimePreflight": preflight,
            "basePackBytes": base_pack_bytes,
            "cumulativeManagedPdfBytes": (
                base_pack_bytes + archive_bytes if base_pack_bytes is not None else None
            ),
        }
        manifest_path = output_dir / f"{target}-manifest.json"
        atomic_write_json(manifest_path, manifest)
        return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--requirements", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--base-python", type=Path)
    parser.add_argument("--smoke-pdf", type=Path)
    parser.add_argument("--base-pack-bytes", type=int)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    if bool(args.base_python) != bool(args.smoke_pdf):
        raise BuildError("--base-python and --smoke-pdf must be provided together")
    manifest = build_target(
        args.target,
        args.requirements.resolve(strict=True),
        args.output_dir,
        args.python.resolve(strict=True),
        args.base_python,
        args.smoke_pdf,
        args.base_pack_bytes,
    )
    print(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
