# Rosetta PDF engine worker.
#
# This process is intentionally a thin JSON protocol host. The PDF algorithm
# lives in the PDFMathTranslate fork at pdf2zh.rosetta_engine; Rosetta owns
# translation orchestration and page commit policy.

import json
import os
from pathlib import Path
import sys
import threading
import time
import traceback


_emit_lock = threading.Lock()


def make_protocol_channel():
    proto = os.fdopen(
        os.dup(1),
        "w",
        buffering=1,
        encoding="utf-8",
        errors="backslashreplace",
    )
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    return proto


def emit(proto, payload):
    with _emit_lock:
        proto.write(json.dumps(payload, ensure_ascii=False) + "\n")
        proto.flush()


def safe_error(error):
    text = f"{type(error).__name__}: {error}"
    return text[:800]


def emit_stage(proto, job_id, stage, status, duration_ms=None, details=None):
    payload = {
        "id": job_id,
        "event": "stage",
        "stage": stage,
        "status": status,
    }
    if duration_ms is not None:
        payload["durationMs"] = int(duration_ms)
    if details:
        payload["details"] = details
    emit(proto, payload)


def run_prepare(job, proto, engine):
    job_id = str(job.get("id", ""))
    started_at = time.time()
    input_pdf = job["file"]
    output_dir = job.get("outputDir")
    scratch_dir = job.get("tmpDir") or (
        str(Path(output_dir) / "tmp") if output_dir else None
    )
    options = dict(job.get("options") or {})
    if scratch_dir:
        Path(scratch_dir).mkdir(parents=True, exist_ok=True)
        options["scratchDir"] = scratch_dir
        options.setdefault("cleanupScratchDir", False)
    model_path = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
    if model_path:
        options.setdefault("modelPath", model_path)
    if "thread" in job:
        options.setdefault("thread", int(job.get("thread") or 1))
    if "layoutImgsz" in job:
        options.setdefault("layoutImgsz", job.get("layoutImgsz"))
    options.setdefault(
        "singlePageDeflate",
        os.environ.get("ROSETTA_PDF_SINGLE_PAGE_DEFLATE", "").lower()
        in ("1", "true", "yes", "on"),
    )
    options.setdefault(
        "singlePageDeflateImages",
        os.environ.get("ROSETTA_PDF_SINGLE_PAGE_DEFLATE_IMAGES", "").lower()
        not in ("0", "false", "no", "off"),
    )

    emit_stage(proto, job_id, "preparePdfWindow", "started")
    prepared = engine.prepareRun(
        input_pdf,
        job.get("pages"),
        job.get("langIn", "en"),
        job.get("langOut", "zh"),
        options,
    )
    units = engine.collectUnits(prepared["preparedRunId"])
    emit_stage(
        proto,
        job_id,
        "preparePdfWindow",
        "completed",
        duration_ms=int((time.time() - started_at) * 1000),
        details={
            "preparedRunId": prepared["preparedRunId"],
            "pageCount": len(prepared.get("pages") or []),
            "unitCount": len(units),
            "sourceChars": sum(unit.get("sourceChars", 0) for unit in units),
        },
    )
    emit(
        proto,
        {
            "id": job_id,
            "event": "prepared_pdf_window",
            "preparedRun": prepared,
            "units": units,
        },
    )


def run_render(job, proto, engine):
    job_id = str(job.get("id", ""))
    started_at = time.time()
    prepared_run_id = job["preparedRunId"]
    output_dir = job["outputDir"]
    translations = job.get("translationsByUnitId") or {}

    emit_stage(proto, job_id, "renderPdfWindow", "started")

    page_count = 0
    failed_count = 0

    def on_page_result(result):
        nonlocal page_count, failed_count
        page_count += 1
        if result.get("status") == "failed":
            failed_count += 1
        emit(
            proto,
            {
                "id": job_id,
                "event": "page_result",
                "pageResult": result,
            },
        )

    results = engine.renderPages(
        prepared_run_id,
        translations,
        output_dir,
        pages=job.get("pages"),
        onPageResult=on_page_result,
    )
    emit_stage(
        proto,
        job_id,
        "renderPdfWindow",
        "completed",
        duration_ms=int((time.time() - started_at) * 1000),
        details={
            "pageCount": page_count,
            "failedPageCount": failed_count,
        },
    )
    emit(
        proto,
        {
            "id": job_id,
            "event": "done",
            "resultCount": len(results),
        },
    )


def run_dispose(job, proto, engine):
    job_id = str(job.get("id", ""))
    prepared_run_id = job.get("preparedRunId")
    if prepared_run_id:
        engine.disposeRun(prepared_run_id)
    emit(proto, {"id": job_id, "event": "disposed_pdf_window"})


def main():
    proto = make_protocol_channel()
    import_started = time.time()
    try:
        emit(
            proto,
            {
                "event": "warming",
                "step": 1,
                "totalSteps": 2,
                "label": "加载 PDF engine",
            },
        )
        from pdf2zh import rosetta_engine as engine

        model_path = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
        if not model_path or not Path(model_path).is_file():
            raise RuntimeError(
                "ROSETTA_DOCLAYOUT_MODEL is missing or does not point to a file; "
                "update the Rosetta PDF component pack."
            )
        emit(
            proto,
            {
                "event": "warming",
                "step": 2,
                "totalSteps": 2,
                "label": "预热版面模型",
            },
        )
        capabilities = engine.prewarm({"modelPath": model_path})
    except Exception:
        emit(
            proto,
            {
                "event": "fatal",
                "message": traceback.format_exc(limit=8),
            },
        )
        return 3

    timings = capabilities.get("timingsMs") or {}
    emit(
        proto,
        {
            "event": "ready",
            "importMs": int((time.time() - import_started) * 1000),
            "yoloWarmupMs": timings.get("syntheticLayoutPrediction"),
            "yoloWarmupStatus": "completed",
            "yoloWarmupDevice": "onnx",
            "capabilities": capabilities,
        },
    )

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            job = json.loads(line)
        except json.JSONDecodeError:
            emit(proto, {"event": "error", "message": "invalid job json"})
            continue

        job_id = str(job.get("id", ""))
        cmd = job.get("cmd")
        try:
            if cmd == "ping":
                emit(proto, {"id": job_id, "event": "pong"})
            elif cmd == "exit":
                break
            elif cmd == "prepare_pdf_window":
                run_prepare(job, proto, engine)
            elif cmd == "render_pdf_window":
                run_render(job, proto, engine)
            elif cmd == "dispose_pdf_window":
                run_dispose(job, proto, engine)
            else:
                emit(
                    proto,
                    {
                        "id": job_id,
                        "event": "error",
                        "message": f"unknown cmd: {cmd}",
                    },
                )
        except BaseException as error:
            emit(
                proto,
                {
                    "id": job_id,
                    "event": "error",
                    "message": safe_error(error),
                },
            )

    return 0


if __name__ == "__main__":
    sys.exit(main())
