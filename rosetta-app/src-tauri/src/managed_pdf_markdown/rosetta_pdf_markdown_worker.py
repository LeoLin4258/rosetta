"""Isolated PyMuPDF4LLM JSON worker. Protocol output is stdout-only JSONL."""

from __future__ import annotations

import importlib.metadata
import contextlib
import io
import json
import os
import sys
from pathlib import Path

PROTOCOL_OUTPUT = os.fdopen(os.dup(sys.stdout.fileno()), "wb", buffering=0)
NULL_OUTPUT = os.open(os.devnull, os.O_WRONLY)
os.dup2(NULL_OUTPUT, sys.stdout.fileno())
os.close(NULL_OUTPUT)

PROTOCOL = 1
MAX_REQUEST_BYTES = 64 * 1024
MAX_RESPONSE_BYTES = 64 * 1024 * 1024
MAX_WINDOW_PAGES = 10
EXPECTED = {
    "pymupdf4llm": "1.28.0",
    "pymupdf-layout": "1.28.0",
    "PyMuPDF": "1.28.0",
}


def emit(payload: dict) -> None:
    encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(encoded) > MAX_RESPONSE_BYTES:
        encoded = json.dumps(
            {"type": "error", "code": "response-too-large", "message": "worker response exceeded its size limit"},
            separators=(",", ":"),
        ).encode("utf-8")
    PROTOCOL_OUTPUT.write(encoded + b"\n")


def package_versions() -> dict[str, str]:
    return {name: importlib.metadata.version(name) for name in EXPECTED}


def load_engine():
    versions = package_versions()
    if versions != EXPECTED:
        raise RuntimeError("version-mismatch")
    with contextlib.redirect_stdout(io.StringIO()):
        import pymupdf
        import pymupdf4llm

    layout_wrapper = pymupdf._get_layout.__closure__[0].cell_contents
    providers = list(layout_wrapper._model._providers)
    if providers != ["CPUExecutionProvider"]:
        raise RuntimeError("non-cpu-provider")
    if pymupdf.__version__ != EXPECTED["PyMuPDF"]:
        raise RuntimeError("version-mismatch")
    return pymupdf4llm, versions, providers


def inside(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return path != root
    except ValueError:
        return False


def checked_paths(request: dict) -> tuple[Path, Path]:
    jobs_root = Path(os.environ["ROSETTA_PDF_MARKDOWN_JOBS_ROOT"]).resolve(strict=True)
    source = Path(request.get("sourcePath", "")).resolve(strict=True)
    output = Path(request.get("tempDir", "")).resolve(strict=True)
    if not source.is_file() or source.name != "source.pdf" or not inside(source, jobs_root):
        raise ValueError("invalid-source-path")
    if not output.is_dir() or not inside(output, jobs_root):
        raise ValueError("invalid-temp-path")
    return source, output


def extract_window(engine, request: dict) -> None:
    request_id = request.get("id")
    pages = request.get("pages")
    if (
        not isinstance(request_id, str)
        or not request_id
        or not isinstance(pages, list)
        or not 1 <= len(pages) <= MAX_WINDOW_PAGES
        or any(not isinstance(page, int) or isinstance(page, bool) or page < 0 for page in pages)
        or pages != sorted(set(pages))
    ):
        raise ValueError("invalid-request")
    source, output = checked_paths(request)
    results = []
    for completed, page in enumerate(pages, 1):
        image_dir = output / f"page-{page + 1:04d}-images"
        image_dir.mkdir(parents=False, exist_ok=False)
        with contextlib.redirect_stdout(io.StringIO()):
            raw = engine.to_json(
                str(source),
                pages=[page],
                use_ocr=False,
                force_text=False,
                write_images=True,
                image_path=str(image_dir),
            )
        if isinstance(raw, str):
            raw = json.loads(raw)
        results.append({"pageIndex": page, "json": raw})
        emit({"type": "windowProgress", "id": request_id, "completed": completed, "total": len(pages)})
    emit({"type": "windowResult", "id": request_id, "pages": results})


def main() -> None:
    engine = None
    versions = None
    providers = None
    while True:
        line = sys.stdin.buffer.readline(MAX_REQUEST_BYTES + 2)
        if not line:
            return
        if len(line) > MAX_REQUEST_BYTES + 1 or not line.endswith(b"\n"):
            emit({"type": "error", "code": "request-too-large", "message": "worker request exceeded its size limit"})
            return
        try:
            request = json.loads(line)
            if not isinstance(request, dict):
                raise ValueError("invalid-request")
            request_type = request.get("type")
            if request_type == "hello":
                if set(request) != {"type"}:
                    raise ValueError("unknown-request-field")
                if engine is None:
                    engine, versions, providers = load_engine()
                emit({"type": "ready", "protocol": PROTOCOL, "versions": versions, "providers": providers, "integrationBoundary": "to_json", "cpuOnly": True})
            elif request_type == "extractWindow":
                if set(request) != {"type", "id", "sourcePath", "pages", "tempDir"}:
                    raise ValueError("unknown-request-field")
                if engine is None:
                    raise RuntimeError("hello-required")
                extract_window(engine, request)
            elif request_type == "shutdown":
                if set(request) != {"type"}:
                    raise ValueError("unknown-request-field")
                emit({"type": "shutdown"})
                return
            else:
                raise ValueError("unknown-request")
        except (ValueError, KeyError, json.JSONDecodeError) as error:
            emit({"type": "error", "code": str(error), "message": "worker request was rejected"})
        except Exception as error:
            code = str(error) if str(error) in {"version-mismatch", "non-cpu-provider", "hello-required"} else "extraction-failed"
            emit({"type": "error", "code": code, "message": "PDF Markdown worker failed"})


if __name__ == "__main__":
    main()
