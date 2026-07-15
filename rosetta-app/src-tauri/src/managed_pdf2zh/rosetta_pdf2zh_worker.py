# Rosetta PDF engine worker.
#
# This process is intentionally a thin JSON protocol host. The PDF algorithm
# lives in the PDFMathTranslate fork at pdf2zh.rosetta_engine; Rosetta owns
# translation orchestration and page commit policy.

import json
import os
from collections import OrderedDict
from pathlib import Path
import sys
import threading
import time
import traceback


_emit_lock = threading.Lock()
_prepare_cache = OrderedDict()
_persistent_cache_owner_keys = set()
_DEFAULT_PREPARE_CACHE_ENTRIES = 6
_MAX_PREPARE_CACHE_ENTRIES = 32


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


def timed_prepare_run(engine, *args, **kwargs):
    buckets = {
        "fontAssets": 0.0,
        "prepareDocument": 0.0,
        "layout": 0.0,
        "unitCollection": 0.0,
    }
    targets = {
        "download_remote_fonts": "fontAssets",
        "prepare_pdf_document": "prepareDocument",
        "build_layout_mask": "layout",
        "collect_page_units": "unitCollection",
    }
    originals = {}

    for name, bucket in targets.items():
        original = getattr(engine, name, None)
        if original is None:
            continue
        originals[name] = original

        def timed(*call_args, _original=original, _bucket=bucket, **call_kwargs):
            started = time.perf_counter()
            try:
                return _original(*call_args, **call_kwargs)
            finally:
                buckets[_bucket] += (time.perf_counter() - started) * 1000

        setattr(engine, name, timed)

    started = time.perf_counter()
    try:
        prepared = engine.prepareRun(*args, **kwargs)
    finally:
        total_ms = (time.perf_counter() - started) * 1000
        for name, original in originals.items():
            setattr(engine, name, original)

    measured_ms = sum(buckets.values())
    timings = {key: int(value) for key, value in buckets.items()}
    timings["total"] = int(total_ms)
    timings["other"] = max(0, int(total_ms - measured_ms))
    timings["cacheReset"] = 0
    return prepared, timings


def prepare_cache_max_entries():
    raw_value = os.environ.get("ROSETTA_PDF_PREPARE_CACHE_ENTRIES", "")
    try:
        configured = int(raw_value)
    except (TypeError, ValueError):
        configured = _DEFAULT_PREPARE_CACHE_ENTRIES
    return max(1, min(configured, _MAX_PREPARE_CACHE_ENTRIES))


def dispose_cached_run(engine, prepared_run_id, suppress_errors=False):
    try:
        engine.disposeRun(prepared_run_id)
    except BaseException:
        if not suppress_errors:
            raise


def dispose_prepare_cache(engine):
    cached = list(_prepare_cache.values())
    _prepare_cache.clear()
    first_error = None
    for entry in cached:
        try:
            engine.disposeRun(entry["preparedRunId"])
        except BaseException as error:
            if first_error is None:
                first_error = error
    if first_error is not None:
        raise first_error


def remove_cached_run(prepared_run_id):
    for cache_key, entry in list(_prepare_cache.items()):
        if entry["preparedRunId"] == prepared_run_id:
            del _prepare_cache[cache_key]
            return entry
    return None


def discover_persistent_cache_owner_keys(engine, model_path):
    jobs_root_text = os.environ.get("ROSETTA_PDF_PREPARE_CACHE_JOBS_ROOT", "").strip()
    if not jobs_root_text:
        return []
    model_signature = getattr(engine, "persistent_layout_model_signature", None)
    cache_schema = getattr(engine, "_PERSISTENT_LAYOUT_CACHE_SCHEMA", None)
    engine_version = getattr(engine, "ENGINE_VERSION", None)
    if not callable(model_signature) or cache_schema is None or not engine_version:
        return []
    try:
        expected_model = model_signature(model_path)
        job_dirs = [path for path in Path(jobs_root_text).iterdir() if path.is_dir()]
    except (OSError, ValueError, TypeError):
        return []

    owners = []
    for job_dir in job_dirs:
        cache_root = job_dir / "pdf-prepare-cache" / "v1"
        source_metadata_path = job_dir / "pdf_source.json"
        try:
            source_metadata = json.loads(source_metadata_path.read_text(encoding="utf-8"))
            source_fingerprint = str(source_metadata.get("sourceFingerprint") or "")
            entries = [path for path in cache_root.iterdir() if path.is_dir()]
        except (OSError, ValueError, TypeError, json.JSONDecodeError):
            continue
        for entry in entries:
            try:
                manifest = json.loads(
                    (entry / "manifest.json").read_text(encoding="utf-8")
                )
            except (OSError, ValueError, TypeError, json.JSONDecodeError):
                continue
            if (
                manifest.get("schemaVersion") == cache_schema
                and manifest.get("engineVersion") == engine_version
                and manifest.get("sourceFingerprint") == source_fingerprint
                and manifest.get("model") == expected_model
                and manifest.get("layoutFile") == "layout.npz"
                and (entry / "layout.npz").is_file()
            ):
                owners.append(job_dir.name)
                break
    return sorted(set(owners))


def remember_persistent_cache_owner(owner_key, options):
    if not owner_key:
        return
    cache_dir_text = str(options.get("persistentLayoutCacheDir") or "").strip()
    if not cache_dir_text:
        return
    cache_dir = Path(cache_dir_text)
    if (cache_dir / "manifest.json").is_file() and (cache_dir / "layout.npz").is_file():
        _persistent_cache_owner_keys.add(owner_key)


def cached_owner_keys():
    jobs_root_text = os.environ.get("ROSETTA_PDF_PREPARE_CACHE_JOBS_ROOT", "").strip()
    if jobs_root_text:
        jobs_root = Path(jobs_root_text)
        for owner_key in list(_persistent_cache_owner_keys):
            if not (jobs_root / owner_key / "pdf-prepare-cache" / "v1").is_dir():
                _persistent_cache_owner_keys.discard(owner_key)
    owners = sorted(_persistent_cache_owner_keys)
    for entry in _prepare_cache.values():
        owner_key = entry.get("ownerKey")
        if owner_key and owner_key not in owners:
            owners.append(owner_key)
    return owners


def cache_prepared_run(engine, cache_key, entry, max_entries):
    internal_key = cache_key or f'prepared:{entry["preparedRunId"]}'
    replaced = _prepare_cache.pop(internal_key, None)
    if replaced is not None:
        dispose_cached_run(engine, replaced["preparedRunId"], suppress_errors=True)
    _prepare_cache[internal_key] = entry
    while len(_prepare_cache) > max_entries:
        _, evicted = _prepare_cache.popitem(last=False)
        dispose_cached_run(engine, evicted["preparedRunId"], suppress_errors=True)


def run_prepare(job, proto, engine):
    job_id = str(job.get("id", ""))
    started_at = time.perf_counter()
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

    cache_key = str(job.get("cacheKey") or "")
    cache_owner_key = str(job.get("cacheOwnerKey") or "")
    cache_max_entries = prepare_cache_max_entries()
    cache_hit = False
    cache_tier = "miss"
    emit_stage(proto, job_id, "preparePdfWindow", "started")
    reset_run = getattr(engine, "resetRun", None)
    active = _prepare_cache.get(cache_key) if cache_key else None
    if active is not None and callable(reset_run):
        try:
            reset_started = time.perf_counter()
            reset_run(active["preparedRunId"])
            reset_ms = int((time.perf_counter() - reset_started) * 1000)
        except BaseException:
            del _prepare_cache[cache_key]
            dispose_cached_run(
                engine, active["preparedRunId"], suppress_errors=True
            )
            active = None
        else:
            _prepare_cache.move_to_end(cache_key)
            prepared = active["prepared"]
            units = active["units"]
            timings = {
                "total": reset_ms,
                "fontAssets": 0,
                "prepareDocument": 0,
                "layout": 0,
                "unitCollection": 0,
                "other": 0,
                "cacheReset": reset_ms,
            }
            cache_hit = True
            cache_tier = "memory"

    if not cache_hit:
        if not callable(reset_run):
            dispose_prepare_cache(engine)
        prepared, timings = timed_prepare_run(
            engine,
            input_pdf,
            job.get("pages"),
            job.get("langIn", "en"),
            job.get("langOut", "zh"),
            options,
        )
        persistent_cache_hit = bool(prepared.get("persistentLayoutCacheHit"))
        if persistent_cache_hit:
            cache_hit = True
            cache_tier = "disk"
        try:
            units = engine.collectUnits(prepared["preparedRunId"])
        except BaseException:
            dispose_cached_run(
                engine, prepared["preparedRunId"], suppress_errors=True
            )
            raise
        entry = {
            "cacheKey": cache_key,
            "ownerKey": cache_owner_key,
            "preparedRunId": prepared["preparedRunId"],
            "prepared": prepared,
            "units": units,
        }
        cache_prepared_run(engine, cache_key, entry, cache_max_entries)
        remember_persistent_cache_owner(cache_owner_key, options)

    duration_ms = int((time.perf_counter() - started_at) * 1000)
    emit_stage(
        proto,
        job_id,
        "preparePdfWindow",
        "completed",
        duration_ms=duration_ms,
        details={
            "preparedRunId": prepared["preparedRunId"],
            "pageCount": len(prepared.get("pages") or []),
            "unitCount": len(units),
            "sourceChars": sum(unit.get("sourceChars", 0) for unit in units),
            "cacheHit": cache_hit,
            "cacheTier": cache_tier,
            "cacheEntryCount": len(_prepare_cache),
            "cacheMaxEntries": cache_max_entries,
            "cachedOwnerKeys": cached_owner_keys(),
            "timingsMs": timings,
        },
    )
    emit(
        proto,
        {
            "id": job_id,
            "event": "prepared_pdf_window",
            "preparedRun": prepared,
            "units": units,
            "cacheHit": cache_hit,
            "cacheTier": cache_tier,
            "cachedOwnerKeys": cached_owner_keys(),
            "timingsMs": timings,
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
        remove_cached_run(prepared_run_id)
        engine.disposeRun(prepared_run_id)
    emit(
        proto,
        {
            "id": job_id,
            "event": "disposed_pdf_window",
            "cachedOwnerKeys": cached_owner_keys(),
        },
    )


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
        _persistent_cache_owner_keys.update(
            discover_persistent_cache_owner_keys(engine, model_path)
        )
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
            "cachedOwnerKeys": cached_owner_keys(),
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
                    "cachedOwnerKeys": cached_owner_keys(),
                },
            )

    return 0


if __name__ == "__main__":
    sys.exit(main())
