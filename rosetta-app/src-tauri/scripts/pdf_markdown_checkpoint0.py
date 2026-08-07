#!/usr/bin/env python3
"""Reproducible Checkpoint 0 corpus benchmark for PDF -> structured JSON."""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import math
import os
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from collections import Counter
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterable


CORPUS_SCHEMA = "rosetta-pdf-markdown-corpus/1"
REPORT_SCHEMA = "rosetta-pdf-markdown-checkpoint0-report/1"
EXPECTED_DOCUMENT_COUNT = 24
EXPECTED_VERSIONS = {
    "pymupdf4llm": "1.28.0",
    "pymupdf-layout": "1.28.0",
    "PyMuPDF": "1.28.0",
}
WINDOW_SIZE = 8
MAX_WINDOW_SIZE = 10
BODY_BOX_CLASSES = {
    "title",
    "section-header",
    "text",
    "list-item",
    "caption",
    "footnote",
    "table",
    "formula",
}
KNOWN_BOX_CLASSES = BODY_BOX_CLASSES | {"picture", "page-header", "page-footer"}


class CheckpointError(RuntimeError):
    pass


def atomic_write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
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


def atomic_write_text(path: Path, payload: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with temp_path.open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, path)
    finally:
        temp_path.unlink(missing_ok=True)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def parse_roots(values: Iterable[str]) -> dict[str, Path]:
    roots: dict[str, Path] = {}
    for value in values:
        name, separator, raw_path = value.partition("=")
        if not separator or not re.fullmatch(r"[a-z][a-z0-9-]*", name):
            raise CheckpointError(f"Invalid root mapping: {value!r}")
        root = Path(raw_path).resolve(strict=True)
        if not root.is_dir():
            raise CheckpointError(f"Corpus root is not a directory: {name}")
        roots[name] = root
    return roots


def safe_resolve(root: Path, relative_path: str) -> Path:
    candidate_relative = Path(relative_path)
    if candidate_relative.is_absolute() or ".." in candidate_relative.parts:
        raise CheckpointError("Corpus path must be a safe relative path")
    candidate = (root / candidate_relative).resolve(strict=True)
    try:
        candidate.relative_to(root)
    except ValueError as error:
        raise CheckpointError("Corpus path escapes its configured root") from error
    if not candidate.is_file():
        raise CheckpointError("Corpus entry is not a file")
    return candidate


def load_manifest(path: Path) -> dict[str, Any]:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema") != CORPUS_SCHEMA:
        raise CheckpointError("Unsupported corpus manifest schema")
    documents = manifest.get("documents")
    if not isinstance(documents, list) or len(documents) != EXPECTED_DOCUMENT_COUNT:
        raise CheckpointError("Checkpoint 0 corpus must contain exactly 24 documents")
    ids = [document.get("id") for document in documents]
    if any(not isinstance(document_id, str) or not document_id for document_id in ids):
        raise CheckpointError("Every corpus document needs a non-empty id")
    if len(ids) != len(set(ids)):
        raise CheckpointError("Corpus document ids must be unique")
    return manifest


def resolve_documents(
    manifest: dict[str, Any], roots: dict[str, Path], verify_hashes: bool = True
) -> list[tuple[dict[str, Any], Path]]:
    resolved: list[tuple[dict[str, Any], Path]] = []
    for document in manifest["documents"]:
        root_name = document.get("root")
        if root_name not in roots:
            raise CheckpointError(f"Missing corpus root mapping: {root_name}")
        source = safe_resolve(roots[root_name], document.get("path", ""))
        if source.suffix.lower() != ".pdf":
            raise CheckpointError(f"Corpus entry is not a PDF: {document['id']}")
        if verify_hashes and sha256_file(source) != document.get("sha256"):
            raise CheckpointError(f"Corpus fingerprint mismatch: {document['id']}")
        resolved.append((document, source))
    return resolved


def import_engine() -> tuple[Any, Any, Any]:
    import importlib.metadata

    import pymupdf
    import pymupdf4llm

    actual_versions = {
        name: importlib.metadata.version(name) for name in EXPECTED_VERSIONS
    }
    if actual_versions != EXPECTED_VERSIONS:
        raise CheckpointError(
            "PDF Markdown engine version mismatch: "
            + json.dumps(actual_versions, sort_keys=True)
        )
    if hasattr(pymupdf4llm, "use_layout"):
        pymupdf4llm.use_layout(True)
    if not callable(getattr(pymupdf4llm, "to_json", None)):
        raise CheckpointError("pymupdf4llm.to_json() is unavailable")
    return pymupdf, pymupdf4llm, actual_versions


def document_page_count(source: Path, pymupdf: Any) -> int:
    with pymupdf.open(source) as document:
        return int(document.page_count)


def refresh_manifest(
    manifest_path: Path, roots: dict[str, Path], output_path: Path
) -> None:
    manifest = load_manifest(manifest_path)
    pymupdf, _, _ = import_engine()
    for document, source in resolve_documents(manifest, roots, verify_hashes=False):
        document["sha256"] = sha256_file(source)
        document["pageCount"] = document_page_count(source, pymupdf)
        document["bytes"] = source.stat().st_size
    atomic_write_json(output_path, manifest)


def box_text(box: dict[str, Any]) -> str:
    lines: list[str] = []
    for line in box.get("textlines") or []:
        text = "".join(
            span.get("text", "")
            for span in line.get("spans") or []
            if isinstance(span, dict)
        )
        if text:
            lines.append(text)
    return "\n".join(lines).strip()


def normalized_text(value: str) -> str:
    return " ".join(value.split()).casefold()


def intersection_ratio(inner: dict[str, Any], outer: dict[str, Any]) -> float:
    ix0 = max(float(inner.get("x0", 0)), float(outer.get("x0", 0)))
    iy0 = max(float(inner.get("y0", 0)), float(outer.get("y0", 0)))
    ix1 = min(float(inner.get("x1", 0)), float(outer.get("x1", 0)))
    iy1 = min(float(inner.get("y1", 0)), float(outer.get("y1", 0)))
    intersection = max(0.0, ix1 - ix0) * max(0.0, iy1 - iy0)
    area = max(0.0, float(inner.get("x1", 0)) - float(inner.get("x0", 0))) * max(
        0.0, float(inner.get("y1", 0)) - float(inner.get("y0", 0))
    )
    return intersection / area if area else 0.0


def analyze_page(page: dict[str, Any], expected_page_number: int) -> dict[str, Any]:
    boxes = page.get("boxes") or []
    unknown = Counter(
        str(box.get("boxclass"))
        for box in boxes
        if box.get("boxclass") not in KNOWN_BOX_CLASSES
    )
    body_boxes = [box for box in boxes if box.get("boxclass") in BODY_BOX_CLASSES]
    picture_boxes = [box for box in boxes if box.get("boxclass") == "picture"]
    texts = [normalized_text(box_text(box)) for box in body_boxes]
    adjacent_duplicates = sum(
        bool(left) and left == right for left, right in zip(texts, texts[1:])
    )
    figure_text_overlap = sum(
        intersection_ratio(body_box, picture_box) >= 0.5
        for picture_box in picture_boxes
        for body_box in body_boxes
    )
    classes = Counter(str(box.get("boxclass")) for box in boxes)
    return {
        "pageNumber": page.get("page_number"),
        "pageIdentityValid": page.get("page_number") == expected_page_number,
        "boxCount": len(boxes),
        "boxClasses": dict(sorted(classes.items())),
        "unknownBoxClasses": dict(sorted(unknown.items())),
        "bodyBoxCount": len(body_boxes),
        "bodyCharacterCount": sum(len(box_text(box)) for box in body_boxes),
        "emptyBody": not any(texts),
        "adjacentDuplicateBodyBoxes": adjacent_duplicates,
        "figureInternalTextOverlapCount": figure_text_overlap,
    }


def sanitize_vendor_json(
    value: dict[str, Any], document_id: str, image_root: Path
) -> dict[str, Any]:
    value["filename"] = f"{document_id}.pdf"
    value["image_path"] = "images"
    for page in value.get("pages") or []:
        for box in page.get("boxes") or []:
            image = box.get("image")
            if not isinstance(image, str) or not image:
                continue
            image_path = Path(image).resolve()
            try:
                relative = image_path.relative_to(image_root.resolve())
            except ValueError as error:
                raise CheckpointError("Vendor image path escaped the output root") from error
            box["image"] = relative.as_posix()
    return value


def render_diagnostic_markdown(value: dict[str, Any]) -> str:
    lines = ["<!-- Diagnostic rendering from to_json(); not export authority. -->", ""]
    for page in value.get("pages") or []:
        lines.extend([f"<!-- page {page.get('page_number')} -->", ""])
        for box in page.get("boxes") or []:
            box_class = box.get("boxclass")
            text = box_text(box)
            if box_class in {"page-header", "page-footer"}:
                continue
            if box_class == "title" and text:
                lines.extend([f"# {text}", ""])
            elif box_class == "section-header" and text:
                level = min(6, max(2, int(box.get("header_level") or 2)))
                lines.extend([f"{'#' * level} {text}", ""])
            elif box_class == "list-item" and text:
                lines.extend([f"- {text}", ""])
            elif box_class == "picture":
                image = box.get("image") or "missing-image"
                lines.extend([f"![]({image})", ""])
            elif box_class == "formula" and text:
                lines.extend(["```text", text, "```", ""])
            elif box_class == "table":
                lines.extend([text or "[preserved table structure]", ""])
            elif text:
                lines.extend([text, ""])
    return "\n".join(lines).rstrip() + "\n"


def extract_window(
    pymupdf4llm: Any,
    source: Path,
    document_id: str,
    zero_based_pages: list[int],
    output_root: Path,
) -> tuple[dict[str, Any], float]:
    if not zero_based_pages or len(zero_based_pages) > MAX_WINDOW_SIZE:
        raise CheckpointError("Extraction window must contain 1-10 pages")
    image_root = output_root / "images"
    image_root.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    combined: dict[str, Any] | None = None
    combined_pages: list[dict[str, Any]] = []
    for zero_based_page in zero_based_pages:
        raw = pymupdf4llm.to_json(
            str(source),
            pages=[zero_based_page],
            use_ocr=False,
            force_text=False,
            write_images=True,
            image_path=str(image_root),
            show_progress=False,
        )
        value = json.loads(raw)
        if value.get("use_ocr") not in (0, False):
            raise CheckpointError("Vendor JSON reports OCR enabled")
        if value.get("force_text") is not False or value.get("write_images") is not True:
            raise CheckpointError("Vendor JSON reports an unexpected extraction policy")
        pages = value.get("pages")
        if not isinstance(pages, list) or [page.get("page_number") for page in pages] != [
            zero_based_page + 1
        ]:
            raise CheckpointError("Vendor JSON page identity mismatch")
        if combined is None:
            combined = value
        combined_pages.extend(pages)
        release_window_memory_from_module()
    elapsed = time.perf_counter() - started
    if combined is None:
        raise CheckpointError("Vendor JSON returned no window result")
    combined["pages"] = combined_pages
    expected_pages = [page + 1 for page in zero_based_pages]
    if [page.get("page_number") for page in combined_pages] != expected_pages:
        raise CheckpointError("Vendor JSON page identity mismatch")
    return sanitize_vendor_json(combined, document_id, image_root), elapsed


def release_window_memory_from_module() -> None:
    gc.collect()


class MemorySampler:
    def __init__(self) -> None:
        try:
            import psutil
        except ImportError as error:
            raise CheckpointError("Checkpoint benchmark requires psutil") from error
        self.psutil = psutil
        self.process = psutil.Process(os.getpid())
        self.peak_rss = 0
        self.stop_event = threading.Event()
        self.thread = threading.Thread(target=self._sample, daemon=True)

    def _sample(self) -> None:
        while not self.stop_event.wait(0.01):
            processes = [self.process, *self.process.children(recursive=True)]
            rss = 0
            for process in processes:
                try:
                    rss += process.memory_info().rss
                except self.psutil.Error:
                    pass
            self.peak_rss = max(self.peak_rss, rss)

    def __enter__(self) -> "MemorySampler":
        self.thread.start()
        return self

    def __exit__(self, *_: Any) -> None:
        self.stop_event.set()
        self.thread.join()
        self._sample_once()

    def _sample_once(self) -> None:
        try:
            self.peak_rss = max(self.peak_rss, self.process.memory_info().rss)
        except self.psutil.Error:
            pass


def run_probe(arguments: list[str], environment: dict[str, str] | None = None) -> float:
    started = time.perf_counter()
    completed = subprocess.run(
        [sys.executable, str(Path(__file__).resolve()), *arguments],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    elapsed = time.perf_counter() - started
    if completed.returncode != 0:
        raise CheckpointError("Cold-start probe failed")
    return elapsed


def percentile(values: list[float], quantile: float) -> float:
    if not values:
        raise CheckpointError("Cannot calculate a percentile from no values")
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * quantile) - 1)]


def release_window_memory(pymupdf: Any) -> None:
    gc.collect()
    tools = getattr(pymupdf, "TOOLS", None)
    if tools is not None and callable(getattr(tools, "store_shrink", None)):
        tools.store_shrink(100)
    gc.collect()


def benchmark(
    manifest_path: Path,
    roots: dict[str, Path],
    output_root: Path,
) -> dict[str, Any]:
    manifest = load_manifest(manifest_path)
    documents = resolve_documents(manifest, roots)
    first_document, first_source = documents[0]
    cold_ready_seconds = run_probe(["_probe-ready"])
    with tempfile.TemporaryDirectory(prefix="rosetta-pdf-markdown-cold-") as temp:
        cold_first_page_seconds = run_probe(
            [
                "_probe-extract",
                "--source",
                str(first_source),
                "--document-id",
                first_document["id"],
                "--output",
                temp,
            ]
        )

    import_started = time.perf_counter()
    pymupdf, pymupdf4llm, versions = import_engine()
    import_seconds = time.perf_counter() - import_started
    output_root.mkdir(parents=True, exist_ok=True)
    document_reports: list[dict[str, Any]] = []
    seconds_per_page: list[float] = []
    aggregate_defects = Counter()
    measured_window_index = 0

    with MemorySampler() as memory:
        for document, source in documents:
            actual_page_count = document_page_count(source, pymupdf)
            if actual_page_count != document.get("pageCount"):
                raise CheckpointError(f"Corpus page count mismatch: {document['id']}")
            document_root = output_root / "documents" / document["id"]
            page_reports: list[dict[str, Any]] = []
            window_reports: list[dict[str, Any]] = []
            for start in range(0, actual_page_count, WINDOW_SIZE):
                pages = list(range(start, min(start + WINDOW_SIZE, actual_page_count)))
                value, seconds = extract_window(
                    pymupdf4llm, source, document["id"], pages, document_root
                )
                window_name = f"page-{pages[0] + 1:04d}-{pages[-1] + 1:04d}"
                atomic_write_json(document_root / "windows" / f"{window_name}.json", value)
                markdown_path = document_root / "windows" / f"{window_name}.diagnostic.md"
                atomic_write_text(markdown_path, render_diagnostic_markdown(value))
                per_page = seconds / len(pages)
                if measured_window_index > 0:
                    seconds_per_page.extend([per_page] * len(pages))
                measured_window_index += 1
                window_reports.append(
                    {
                        "firstPage": pages[0] + 1,
                        "lastPage": pages[-1] + 1,
                        "pages": len(pages),
                        "seconds": round(seconds, 6),
                        "secondsPerPage": round(per_page, 6),
                    }
                )
                for offset, page in enumerate(value["pages"]):
                    report = analyze_page(page, pages[offset] + 1)
                    page_reports.append(report)
                    aggregate_defects["invalidPageIdentity"] += int(
                        not report["pageIdentityValid"]
                    )
                    aggregate_defects["emptyBodyPages"] += int(report["emptyBody"])
                    aggregate_defects["adjacentDuplicateBodyBoxes"] += report[
                        "adjacentDuplicateBodyBoxes"
                    ]
                    aggregate_defects["figureInternalTextOverlap"] += report[
                        "figureInternalTextOverlapCount"
                    ]
                    aggregate_defects["unknownBoxClasses"] += sum(
                        report["unknownBoxClasses"].values()
                    )
                release_window_memory(pymupdf)
                process_rss_after = memory.process.memory_info().rss
                window_reports[-1]["cumulativePeakRssBytes"] = memory.peak_rss
                window_reports[-1]["processRssAfterCleanupBytes"] = process_rss_after
            document_reports.append(
                {
                    "id": document["id"],
                    "categories": document.get("categories", []),
                    "pageCount": actual_page_count,
                    "windows": window_reports,
                    "pages": page_reports,
                }
            )

    report = {
        "schema": REPORT_SCHEMA,
        "engine": {
            "versions": versions,
            "cpuOnly": True,
            "useOcr": False,
            "forceText": False,
            "writeImages": True,
            "integrationBoundary": "to_json",
        },
        "corpus": {
            "documentCount": len(documents),
            "pageCount": sum(document["pageCount"] for document, _ in documents),
        },
        "performance": {
            "coldWorkerReadySeconds": round(cold_ready_seconds, 6),
            "coldFirstPageSeconds": round(cold_first_page_seconds, 6),
            "inProcessImportSeconds": round(import_seconds, 6),
            "warmMedianSecondsPerPage": round(statistics.median(seconds_per_page), 6),
            "warmP95SecondsPerPage": round(percentile(seconds_per_page, 0.95), 6),
            "peakRssBytes": memory.peak_rss,
        },
        "structureDefects": dict(sorted(aggregate_defects.items())),
        "documents": document_reports,
    }
    atomic_write_json(output_root / "report.json", report)
    return report


def probe_ready() -> None:
    _, _, versions = import_engine()
    print(json.dumps({"ready": True, "versions": versions}, sort_keys=True))


def probe_extract(source: Path, document_id: str, output: Path) -> None:
    _, pymupdf4llm, _ = import_engine()
    value, seconds = extract_window(pymupdf4llm, source, document_id, [0], output)
    print(
        json.dumps(
            {"pages": len(value.get("pages", [])), "seconds": round(seconds, 6)},
            sort_keys=True,
        )
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    refresh = subparsers.add_parser("refresh-manifest")
    refresh.add_argument("--manifest", type=Path, required=True)
    refresh.add_argument("--output", type=Path, required=True)
    refresh.add_argument("--root", action="append", default=[])

    run = subparsers.add_parser("run")
    run.add_argument("--manifest", type=Path, required=True)
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--root", action="append", default=[])

    subparsers.add_parser("_probe-ready")
    probe = subparsers.add_parser("_probe-extract")
    probe.add_argument("--source", type=Path, required=True)
    probe.add_argument("--document-id", required=True)
    probe.add_argument("--output", type=Path, required=True)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    if args.command == "refresh-manifest":
        refresh_manifest(args.manifest, parse_roots(args.root), args.output)
    elif args.command == "run":
        report = benchmark(args.manifest, parse_roots(args.root), args.output)
        print(
            json.dumps(
                {
                    "documents": report["corpus"]["documentCount"],
                    "pages": report["corpus"]["pageCount"],
                    "performance": report["performance"],
                    "structureDefects": report["structureDefects"],
                },
                ensure_ascii=False,
                indent=2,
            )
        )
    elif args.command == "_probe-ready":
        probe_ready()
    else:
        probe_extract(args.source, args.document_id, args.output)


if __name__ == "__main__":
    try:
        main()
    except CheckpointError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
