#!/usr/bin/env python3
"""Measure MuPDF identity text replay without retaining document content.

This is an isolated PDF v3 engine-selection probe. It is not imported by the
app or shipped as a production renderer. All mutated PDFs and rasterized pages
live in a temporary directory; stdout contains metrics and hashes only.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from typing import Any

import pymupdf
from PIL import Image, ImageChops, ImageStat


@dataclass(frozen=True)
class TextSpan:
    text: str
    bbox: tuple[float, float, float, float]
    origin: tuple[float, float]
    font_resource: str | None
    font_size: float
    color: tuple[float, float, float]
    opacity: float
    render_mode: int
    supported: bool


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare MuPDF save, overlay, and same-text replacement fidelity."
    )
    parser.add_argument("pdf", type=Path)
    parser.add_argument("--page", type=int, default=1, help="1-based page number")
    parser.add_argument("--render-width", type=int, default=1200)
    parser.add_argument(
        "--pdftoppm",
        type=Path,
        default=None,
        help="Optional pdftoppm executable or Windows .cmd wrapper",
    )
    return parser.parse_args()


def normalize_font_name(value: str) -> str:
    name = value.lstrip("/")
    if "+" in name and len(name.split("+", 1)[0]) == 6:
        name = name.split("+", 1)[1]
    return name


def page_font_resources(page: pymupdf.Page) -> dict[str, str]:
    resources: dict[str, str] = {}
    for font in page.get_fonts(full=True):
        base_font = normalize_font_name(str(font[3]))
        resource_name = str(font[4])
        resources.setdefault(base_font, resource_name)
    return resources


def extract_spans(page: pymupdf.Page) -> list[TextSpan]:
    resources = page_font_resources(page)
    spans: list[TextSpan] = []
    for trace in page.get_texttrace():
        chars = trace.get("chars", ())
        if not chars:
            continue
        text = "".join(chr(character[0]) for character in chars)
        direction = trace.get("dir", (1.0, 0.0))
        writing_mode = int(trace.get("wmode", 0))
        render_mode = int(trace.get("type", 0))
        font_resource = resources.get(normalize_font_name(str(trace.get("font", ""))))
        supported = (
            bool(text)
            and font_resource is not None
            and writing_mode == 0
            and abs(float(direction[0]) - 1.0) < 0.0001
            and abs(float(direction[1])) < 0.0001
            and render_mode in {0, 1, 2}
        )
        color = trace.get("color", (0.0, 0.0, 0.0))
        if len(color) != 3:
            supported = False
            color = (0.0, 0.0, 0.0)
        spans.append(
            TextSpan(
                text=text,
                bbox=tuple(float(value) for value in trace["bbox"]),
                origin=tuple(float(value) for value in chars[0][2]),
                font_resource=font_resource,
                font_size=float(trace["size"]),
                color=tuple(float(value) for value in color),
                opacity=float(trace.get("opacity", 1.0)),
                render_mode=render_mode,
                supported=supported,
            )
        )
    return spans


def preflight_font_resources(
    source_bytes: bytes, page_index: int, spans: list[TextSpan]
) -> set[str]:
    document = pymupdf.open(stream=source_bytes, filetype="pdf")
    page = document[page_index]
    replayable: set[str] = set()
    resources = {
        span.font_resource
        for span in spans
        if span.supported and span.font_resource is not None
    }
    for resource in resources:
        try:
            page.insert_font(fontname=resource)
            replayable.add(resource)
        except Exception:
            continue
    document.close()
    return replayable


def add_text(page: pymupdf.Page, spans: list[TextSpan]) -> tuple[int, int]:
    inserted = 0
    failed = 0
    for span in spans:
        if not span.supported:
            continue
        try:
            result = page.insert_text(
                span.origin,
                span.text,
                fontname=span.font_resource,
                fontsize=span.font_size,
                color=span.color,
                render_mode=span.render_mode,
                fill_opacity=span.opacity,
                stroke_opacity=span.opacity,
                overlay=True,
            )
        except Exception:
            failed += 1
            continue
        if result > 0:
            inserted += 1
        else:
            failed += 1
    return inserted, failed


def mutate_pdf(
    source_bytes: bytes, page_index: int, spans: list[TextSpan], mode: str
) -> tuple[bytes, dict[str, int]]:
    document = pymupdf.open(stream=source_bytes, filetype="pdf")
    page = document[page_index]
    inserted = 0
    failed = 0
    if mode == "overlay":
        inserted, failed = add_text(page, spans)
    elif mode == "replacement":
        for span in spans:
            if span.supported:
                page.add_redact_annot(span.bbox, fill=False, cross_out=False)
        page.apply_redactions(images=0, graphics=0, text=0)
        inserted, failed = add_text(page, spans)
    elif mode != "save_only":
        raise ValueError(f"unsupported probe mode: {mode}")

    output = document.tobytes(
        garbage=False,
        clean=False,
        deflate=False,
        no_new_id=True,
        use_objstms=0,
    )
    document.close()
    return output, {"inserted_spans": inserted, "failed_insertions": failed}


def text_fingerprint(pdf_bytes: bytes, page_index: int) -> dict[str, Any]:
    document = pymupdf.open(stream=pdf_bytes, filetype="pdf")
    value = document[page_index].get_text("text", sort=False)
    document.close()
    return {
        "chars": len(value),
        "sha256": hashlib.sha256(value.encode("utf-8")).hexdigest(),
        "value": value,
    }


def first_difference(source: str, output: str) -> int | None:
    for index, (left, right) in enumerate(zip(source, output)):
        if left != right:
            return index
    if len(source) != len(output):
        return min(len(source), len(output))
    return None


def resolve_pdftoppm(explicit: Path | None) -> Path:
    if explicit is not None:
        candidate = explicit.resolve()
    else:
        found = shutil.which("pdftoppm")
        if found is None:
            raise SystemExit("pdftoppm was not found; pass --pdftoppm explicitly")
        candidate = Path(found).resolve()
    if not candidate.is_file():
        raise SystemExit(f"pdftoppm does not exist: {candidate}")
    return candidate


def run_program(executable: Path, arguments: list[str]) -> None:
    if os.name == "nt" and executable.suffix.lower() in {".cmd", ".bat"}:
        process = subprocess.run(
            [
                os.environ.get("COMSPEC", "cmd.exe"),
                "/d",
                "/c",
                "call",
                str(executable),
                *arguments,
            ],
            check=False,
            capture_output=True,
            text=True,
        )
    else:
        process = subprocess.run(
            [str(executable), *arguments],
            check=False,
            capture_output=True,
            text=True,
        )
    if process.returncode != 0:
        message = process.stderr.strip() or process.stdout.strip()
        raise RuntimeError(f"pdftoppm failed with code {process.returncode}: {message}")


def render_page(
    pdftoppm: Path,
    pdf_path: Path,
    page_number: int,
    render_width: int,
    output_prefix: Path,
) -> Path:
    run_program(
        pdftoppm,
        [
            "-f",
            str(page_number),
            "-l",
            str(page_number),
            "-singlefile",
            "-scale-to-x",
            str(render_width),
            "-scale-to-y",
            "-1",
            "-png",
            str(pdf_path),
            str(output_prefix),
        ],
    )
    output = output_prefix.with_suffix(".png")
    if not output.is_file():
        raise RuntimeError(f"pdftoppm did not produce {output}")
    return output


def compare_images(source_path: Path, output_path: Path) -> dict[str, Any]:
    with Image.open(source_path) as source_file, Image.open(output_path) as output_file:
        source = source_file.convert("RGBA")
        output = output_file.convert("RGBA")
        if source.size != output.size:
            return {
                "dimensions_match": False,
                "source_dimensions": list(source.size),
                "output_dimensions": list(output.size),
            }
        difference = ImageChops.difference(source, output)
        changed_pixels = sum(1 for pixel in difference.getdata() if pixel != (0, 0, 0, 0))
        pixel_count = source.width * source.height
        statistics = ImageStat.Stat(difference)
        return {
            "dimensions_match": True,
            "width": source.width,
            "height": source.height,
            "changed_pixels": changed_pixels,
            "changed_pixel_ratio": changed_pixels / pixel_count if pixel_count else 0.0,
            "mean_absolute_channel_difference": sum(statistics.mean) / 4.0,
            "max_channel_difference": max(extreme[1] for extreme in statistics.extrema),
        }


def probe(args: argparse.Namespace) -> dict[str, Any]:
    source_path = args.pdf.resolve()
    if not source_path.is_file():
        raise SystemExit(f"PDF does not exist: {source_path}")
    if args.page < 1:
        raise SystemExit("--page must be at least 1")
    if args.render_width < 1:
        raise SystemExit("--render-width must be at least 1")

    pdftoppm = resolve_pdftoppm(args.pdftoppm)
    source_bytes = source_path.read_bytes()
    source_document = pymupdf.open(stream=source_bytes, filetype="pdf")
    if args.page > source_document.page_count:
        raise SystemExit(
            f"page {args.page} is outside the document page count {source_document.page_count}"
        )
    page_index = args.page - 1
    source_page = source_document[page_index]
    original_fonts = {font[0] for font in source_page.get_fonts(full=True)}
    spans = extract_spans(source_page)
    source_document.close()
    font_candidates = {
        span.font_resource
        for span in spans
        if span.supported and span.font_resource is not None
    }
    replayable_fonts = preflight_font_resources(source_bytes, page_index, spans)
    spans = [
        TextSpan(
            text=span.text,
            bbox=span.bbox,
            origin=span.origin,
            font_resource=span.font_resource,
            font_size=span.font_size,
            color=span.color,
            opacity=span.opacity,
            render_mode=span.render_mode,
            supported=span.supported and span.font_resource in replayable_fonts,
        )
        for span in spans
    ]
    source_text = text_fingerprint(source_bytes, page_index)

    result: dict[str, Any] = {
        "schema": "rosetta-pdf-v3-mupdf-identity-probe/1",
        "engine": f"PyMuPDF {pymupdf.__version__}",
        "mupdf": pymupdf.mupdf_version,
        "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
        "source_pdf_bytes": len(source_bytes),
        "page": args.page,
        "render_width": args.render_width,
        "source_text_chars": source_text["chars"],
        "source_text_sha256": source_text["sha256"],
        "trace_spans": len(spans),
        "supported_spans": sum(span.supported for span in spans),
        "unsupported_spans": sum(not span.supported for span in spans),
        "font_resource_candidates": len(font_candidates),
        "replayable_font_resources": len(replayable_fonts),
        "unreplayable_font_resources": len(font_candidates - replayable_fonts),
        "modes": {},
    }

    with tempfile.TemporaryDirectory(prefix="rosetta-pdf-v3-mupdf-") as root_text:
        root = Path(root_text)
        source_png = render_page(
            pdftoppm, source_path, args.page, args.render_width, root / "source"
        )
        for mode in ("save_only", "overlay", "replacement"):
            started = time.perf_counter()
            output_bytes, mutation = mutate_pdf(source_bytes, page_index, spans, mode)
            output_path = root / f"{mode}.pdf"
            output_path.write_bytes(output_bytes)
            output_text = text_fingerprint(output_bytes, page_index)
            output_document = pymupdf.open(stream=output_bytes, filetype="pdf")
            output_fonts = {font[0] for font in output_document[page_index].get_fonts(full=True)}
            output_document.close()
            output_png = render_page(
                pdftoppm,
                output_path,
                args.page,
                args.render_width,
                root / mode,
            )
            mode_result = {
                **mutation,
                "output_pdf_bytes": len(output_bytes),
                "size_ratio": len(output_bytes) / len(source_bytes),
                "output_text_chars": output_text["chars"],
                "output_text_sha256": output_text["sha256"],
                "text_exact_match": output_text["value"] == source_text["value"],
                "first_text_difference_index": first_difference(
                    source_text["value"], output_text["value"]
                ),
                "new_font_xrefs": len(output_fonts - original_fonts),
                "pixel_difference": compare_images(source_png, output_png),
                "elapsed_ms": round((time.perf_counter() - started) * 1000),
            }
            result["modes"][mode] = mode_result

    return result


def main() -> int:
    result = probe(parse_args())
    print(json.dumps(result, ensure_ascii=True, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
