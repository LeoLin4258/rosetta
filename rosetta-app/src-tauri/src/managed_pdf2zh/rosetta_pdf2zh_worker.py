# Rosetta pdf2zh persistent worker.
#
# Written to disk by the Rosetta app at spawn time (embedded via include_str!
# in managed_pdf2zh/worker.rs) and executed with the pdf2zh pack's bundled
# CPython. Do NOT edit the on-disk copy; it is overwritten on every spawn.
#
# Why this exists: importing and preparing the PDF layout / conversion stack is
# too expensive to pay for every PDF job. This worker pays that cost once per
# process and then serves translate jobs over a line-based JSON protocol:
#
#   stdin  (Rosetta -> worker): {"id": "...", "cmd": "translate", ...}
#   stdout (worker -> Rosetta): {"id": "...", "event": "ready|page|done|error", ...}
#
# pdf2zh's own logging / tqdm progress keeps flowing on stderr, where the
# Rust side parses it exactly like the CLI path. The real stdout is reserved
# for protocol lines: fd 1 is duplicated for the protocol channel and then
# redirected to stderr so stray library prints can't corrupt the protocol.
#
# Per-page streaming: instead of calling cli.extract_text() (which paints all
# pages + does font/xref/dual-PDF preprocessing each invocation), we inline
# the pymupdf preprocess + pdfminer/layout loop and:
#   - cache the layout model across translate calls
#   - apply patches incrementally per page so each finished page can be saved
#     as a single-page PDF and announced via a "page" protocol event
#   - skip the dual PDF (Rosetta never consumes it)
#   - skip the final mono save (Rosetta consumes the per-page outputs)

import json
import logging
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import threading
import time
import traceback


_layout_model = None
_layout_model_path = None
_layout_backend = None
_emit_lock = threading.Lock()


def make_protocol_channel():
    # Windows pipe text encoding follows the active system locale by default
    # (for example GBK on a Chinese installation). The Rust side reads the
    # line protocol as UTF-8, so locale-encoded warmup labels would terminate
    # its stdout reader even though the worker process was still healthy.
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


def emit_stage(proto, job_id, stage, status, page_number=None, duration_ms=None, details=None):
    payload = {
        "id": job_id,
        "event": "stage",
        "stage": stage,
        "status": status,
    }
    if page_number is not None:
        payload["pageNumber"] = int(page_number)
    if duration_ms is not None:
        payload["durationMs"] = int(duration_ms)
    if details:
        payload["details"] = details
    emit(proto, payload)


def stage_start(proto, job_id, stage, page_number=None, details=None):
    emit_stage(proto, job_id, stage, "started", page_number=page_number, details=details)
    return time.time()


def stage_done(proto, job_id, stage, started_at, page_number=None, details=None):
    emit_stage(
        proto,
        job_id,
        stage,
        "completed",
        page_number=page_number,
        duration_ms=int((time.time() - started_at) * 1000),
        details=details,
    )


def stage_failed(proto, job_id, stage, started_at, page_number=None, details=None):
    emit_stage(
        proto,
        job_id,
        stage,
        "failed",
        page_number=page_number,
        duration_ms=int((time.time() - started_at) * 1000),
        details=details,
    )


def safe_len(value):
    try:
        return len(value)
    except Exception:
        return None


def summarize_layout_tree(root):
    stack = [(root, 0)]
    counts = {}
    max_depth = 0
    total_items = 0
    while stack:
        item, depth = stack.pop()
        total_items += 1
        max_depth = max(max_depth, depth)
        name = type(item).__name__
        counts[name] = counts.get(name, 0) + 1
        try:
            children = list(item)
        except Exception:
            children = []
        for child in reversed(children):
            stack.append((child, depth + 1))
    return {
        "itemCount": total_items,
        "maxDepth": max_depth,
        "ltCharCount": counts.get("LTChar", 0),
        "ltLineCount": counts.get("LTLine", 0),
        "ltFigureCount": counts.get("LTFigure", 0),
        "ltTextLineCount": counts.get("LTTextLine", 0),
        "ltTextBoxCount": counts.get("LTTextBox", 0)
        + counts.get("LTTextBoxHorizontal", 0)
        + counts.get("LTTextBoxVertical", 0),
    }


def install_page_process_probe(device, proto, job_id, page_number):
    context = {
        "pageNumber": page_number,
        "translateRequestCount": 0,
        "translateBatchCount": 0,
        "translateBatchItems": 0,
        "translateFailedCount": 0,
        "translateBatchFailedCount": 0,
        "translateRequestMs": 0,
        "translateBatchMs": 0,
        "translateInputChars": 0,
        "translateOutputChars": 0,
        "lock": threading.Lock(),
    }
    device._rosetta_probe_context = context

    if not hasattr(device, "_rosetta_original_translate"):
        original_translate = device.translator.translate
        device._rosetta_original_translate = original_translate

        def rosetta_translate_probe(text):
            probe = getattr(device, "_rosetta_probe_context", None)
            if not probe:
                return original_translate(text)
            source_chars = len(text) if isinstance(text, str) else 0
            with probe["lock"]:
                probe["translateRequestCount"] += 1
                request_index = probe["translateRequestCount"]
                probe["translateInputChars"] += source_chars
            started_at = stage_start(
                proto,
                job_id,
                "page.processPage.translateRequest",
                probe["pageNumber"],
                details={
                    "requestIndex": request_index,
                    "sourceChars": source_chars,
                },
            )
            try:
                result = original_translate(text)
            except BaseException as error:
                duration_ms = int((time.time() - started_at) * 1000)
                with probe["lock"]:
                    probe["translateFailedCount"] += 1
                    probe["translateRequestMs"] += duration_ms
                stage_failed(
                    proto,
                    job_id,
                    "page.processPage.translateRequest",
                    started_at,
                    probe["pageNumber"],
                    details={
                        "requestIndex": request_index,
                        "sourceChars": source_chars,
                        "errorType": type(error).__name__,
                    },
                )
                raise
            duration_ms = int((time.time() - started_at) * 1000)
            output_chars = len(result) if isinstance(result, str) else 0
            with probe["lock"]:
                probe["translateRequestMs"] += duration_ms
                probe["translateOutputChars"] += output_chars
            stage_done(
                proto,
                job_id,
                "page.processPage.translateRequest",
                started_at,
                probe["pageNumber"],
                details={
                    "requestIndex": request_index,
                    "sourceChars": source_chars,
                    "outputChars": output_chars,
                },
            )
            return result

        device.translator.translate = rosetta_translate_probe

    if hasattr(device.translator, "translate_many") and not hasattr(
        device, "_rosetta_original_translate_many"
    ):
        original_translate_many = device.translator.translate_many
        device._rosetta_original_translate_many = original_translate_many

        def rosetta_translate_many_probe(texts, *args, **kwargs):
            probe = getattr(device, "_rosetta_probe_context", None)
            if not probe:
                return original_translate_many(texts, *args, **kwargs)
            item_count = len(texts) if isinstance(texts, (list, tuple)) else 0
            source_chars = (
                sum(len(text) for text in texts if isinstance(text, str))
                if isinstance(texts, (list, tuple))
                else 0
            )
            with probe["lock"]:
                probe["translateBatchCount"] += 1
                batch_index = probe["translateBatchCount"]
                probe["translateBatchItems"] += item_count
                probe["translateInputChars"] += source_chars
            started_at = stage_start(
                proto,
                job_id,
                "page.processPage.translateBatch",
                probe["pageNumber"],
                details={
                    "requestIndex": batch_index,
                    "itemCount": item_count,
                    "inputChars": source_chars,
                },
            )
            try:
                result = original_translate_many(texts, *args, **kwargs)
            except BaseException as error:
                duration_ms = int((time.time() - started_at) * 1000)
                with probe["lock"]:
                    probe["translateBatchFailedCount"] += 1
                    probe["translateBatchMs"] += duration_ms
                stage_failed(
                    proto,
                    job_id,
                    "page.processPage.translateBatch",
                    started_at,
                    probe["pageNumber"],
                    details={
                        "requestIndex": batch_index,
                        "itemCount": item_count,
                        "inputChars": source_chars,
                        "errorType": type(error).__name__,
                    },
                )
                raise
            duration_ms = int((time.time() - started_at) * 1000)
            output_chars = (
                sum(len(text) for text in result if isinstance(text, str))
                if isinstance(result, (list, tuple))
                else 0
            )
            with probe["lock"]:
                probe["translateBatchMs"] += duration_ms
                probe["translateOutputChars"] += output_chars
            stage_done(
                proto,
                job_id,
                "page.processPage.translateBatch",
                started_at,
                probe["pageNumber"],
                details={
                    "requestIndex": batch_index,
                    "itemCount": item_count,
                    "inputChars": source_chars,
                    "outputChars": output_chars,
                },
            )
            return result

        device.translator.translate_many = rosetta_translate_many_probe

    if not hasattr(device, "_rosetta_original_receive_layout"):
        original_receive_layout = device.receive_layout
        device._rosetta_original_receive_layout = original_receive_layout

        def rosetta_receive_layout_probe(ltpage):
            probe = getattr(device, "_rosetta_probe_context", None)
            if not probe:
                return original_receive_layout(ltpage)
            summary = summarize_layout_tree(ltpage)
            started_at = stage_start(
                proto,
                job_id,
                "page.processPage.receiveLayout",
                probe["pageNumber"],
                details=summary,
            )
            try:
                result = original_receive_layout(ltpage)
            except BaseException as error:
                stage_failed(
                    proto,
                    job_id,
                    "page.processPage.receiveLayout",
                    started_at,
                    probe["pageNumber"],
                    details={**summary, "errorType": type(error).__name__},
                )
                raise
            stage_done(
                proto,
                job_id,
                "page.processPage.receiveLayout",
                started_at,
                probe["pageNumber"],
                details={**summary, **translate_probe_summary(probe)},
            )
            return result

        device.receive_layout = rosetta_receive_layout_probe

    return context


def translate_probe_summary(context):
    with context["lock"]:
        return {
            "translateRequestCount": context["translateRequestCount"],
            "translateBatchCount": context["translateBatchCount"],
            "translateBatchItems": context["translateBatchItems"],
            "translateFailedCount": context["translateFailedCount"],
            "translateBatchFailedCount": context["translateBatchFailedCount"],
            "translateRequestMs": context["translateRequestMs"],
            "translateBatchMs": context["translateBatchMs"],
            "translateInputChars": context["translateInputChars"],
            "translateOutputChars": context["translateOutputChars"],
        }


def page_ctm(page):
    x0, y0, x1, y1 = page.cropbox
    if page.rotate == 90:
        return (0, -1, 1, 0, -y0, x1)
    if page.rotate == 180:
        return (-1, 0, 0, -1, x1, y1)
    if page.rotate == 270:
        return (0, 1, -1, 0, y1, -x0)
    return (1, 0, 0, 1, -x0, -y0)


def process_page_with_stages(interpreter, device, page, proto, job_id, page_number):
    """Equivalent to pdf2zh.pdfinterp.PDFPageInterpreter.process_page with probes."""
    x0, y0, _x1, _y1 = page.cropbox
    ctm = page_ctm(page)
    probe = install_page_process_probe(device, proto, job_id, page_number)
    try:
        t0 = stage_start(
            proto,
            job_id,
            "page.processPage.beginPage",
            page_number,
            details={
                "rotate": page.rotate,
                "cropbox": [float(value) for value in page.cropbox],
            },
        )
        device.begin_page(page, ctm)
        stage_done(proto, job_id, "page.processPage.beginPage", t0, page_number)

        t0 = stage_start(
            proto,
            job_id,
            "page.processPage.renderStreams",
            page_number,
            details={
                "resourceCount": safe_len(page.resources),
                "streamObjectCount": safe_len(page.contents),
            },
        )
        ops_base = interpreter.render_contents(page.resources, page.contents, ctm=ctm)
        stage_done(
            proto,
            job_id,
            "page.processPage.renderStreams",
            t0,
            page_number,
            details={
                "baseOpsChars": len(ops_base) if isinstance(ops_base, str) else None,
                "fontIdCount": safe_len(interpreter.fontid),
                "fontMapCount": safe_len(interpreter.fontmap),
            },
        )

        t0 = stage_start(proto, job_id, "page.processPage.endPage", page_number)
        device.fontid = interpreter.fontid
        device.fontmap = interpreter.fontmap
        ops_new = device.end_page(page)
        stage_done(
            proto,
            job_id,
            "page.processPage.endPage",
            t0,
            page_number,
            details={
                "translatedOpsChars": len(ops_new) if isinstance(ops_new, str) else None,
                **translate_probe_summary(probe),
            },
        )

        t0 = stage_start(
            proto,
            job_id,
            "page.processPage.patchStreams",
            page_number,
            details={"streamObjectCount": safe_len(page.contents)},
        )
        interpreter.obj_patch[
            page.page_xref
        ] = f"q {ops_base}Q 1 0 0 1 {x0} {y0} cm {ops_new}"
        for obj in page.contents:
            interpreter.obj_patch[obj.objid] = ""
        stage_done(
            proto,
            job_id,
            "page.processPage.patchStreams",
            t0,
            page_number,
            details={
                "streamObjectCount": safe_len(page.contents),
                "pagePatchChars": safe_len(interpreter.obj_patch.get(page.page_xref)),
            },
        )
    finally:
        device._rosetta_probe_context = None


def get_layout_model(model_path):
    """Re-use the layout model object across translate calls."""
    global _layout_model, _layout_model_path, _layout_backend
    if _layout_model is not None and _layout_model_path == model_path:
        return _layout_model

    from pdf2zh.doclayout import OnnxModel

    _layout_model = OnnxModel(model_path)
    _layout_backend = "onnx"
    _layout_model_path = model_path
    return _layout_model


def layout_device_name(model):
    onnx_session = getattr(model, "model", None)
    if onnx_session is not None and hasattr(onnx_session, "get_providers"):
        return ",".join(onnx_session.get_providers())
    return "cpu"


def warm_layout_predict(model_path):
    started_at = time.time()
    width = 596
    height = 842
    imgsz = int(height / 32) * 32
    device_name = "unknown"
    try:
        import numpy as np

        model = get_layout_model(model_path)
        device_name = layout_device_name(model)
        dummy = np.zeros((height, width, 3), dtype=np.uint8)
        result = model.predict(
            dummy,
            imgsz=imgsz,
            device=device_name,
            verbose=False,
        )[0]
        return {
            "status": "completed",
            "durationMs": int((time.time() - started_at) * 1000),
            "device": device_name,
            "width": width,
            "height": height,
            "imgsz": imgsz,
            "boxCount": len(result.boxes),
            "backend": _layout_backend,
        }
    except Exception as error:
        return {
            "status": "failed",
            "durationMs": int((time.time() - started_at) * 1000),
            "device": device_name,
            "width": width,
            "height": height,
            "imgsz": imgsz,
            "backend": _layout_backend,
            "reason": f"{type(error).__name__}: {str(error)[:300]}",
        }


def truthy_env(name):
    return os.environ.get(name, "").strip().lower() in ("1", "true", "yes", "on")


def falsy_env(name):
    return os.environ.get(name, "").strip().lower() in ("0", "false", "no", "off")


def cross_page_batch_enabled(service):
    if service != "rosetta-batch":
        return False
    if falsy_env("ROSETTA_PDF_CROSS_PAGE_BATCH"):
        return False
    if truthy_env("ROSETTA_PDF_DISABLE_CROSS_PAGE_BATCH"):
        return False
    return True


def layout_imgsz_for_pix(pix, service):
    native = max(32, int(pix.height / 32) * 32)
    raw = os.environ.get("ROSETTA_PDF_LAYOUT_IMGSZ", "").strip().lower()
    if raw in ("native", "auto", "original"):
        return native, native, raw
    if raw:
        try:
            requested = int(raw)
            if requested > 0:
                return max(32, int(requested / 32) * 32), native, "env"
        except ValueError:
            pass
    if service == "rosetta-batch":
        return min(native, 640), native, "rosetta-batch-default"
    return native, native, "native"


class DeferredTranslationCollector:
    supports_batch = True

    def __init__(self, delegate):
        self.delegate = delegate
        self.lang_out = getattr(delegate, "lang_out", "")
        self.requests = []

    def translate_many(self, texts, *args, **kwargs):
        items = list(texts)
        self.requests.append(items)
        return items

    def translate(self, text, *args, **kwargs):
        self.requests.append([text])
        return text

    def flattened_texts(self):
        return [text for request in self.requests for text in request]


class PretranslatedTranslator:
    supports_batch = True

    def __init__(self, delegate, source_texts, translations):
        if len(source_texts) != len(translations):
            raise RuntimeError(
                "pretranslated result count mismatch "
                f"(expected {len(source_texts)}, got {len(translations)})"
            )
        self.delegate = delegate
        self.lang_out = getattr(delegate, "lang_out", "")
        self.translations_by_source = {}
        for source, translation in zip(source_texts, translations):
            self.translations_by_source.setdefault(source, translation)

    def translate_many(self, texts, *args, **kwargs):
        missing = [text for text in texts if text not in self.translations_by_source]
        if missing:
            raise RuntimeError(
                "pretranslated PDF replay missed "
                f"{len(missing)} translation item(s)"
            )
        return [self.translations_by_source[text] for text in texts]

    def translate(self, text, *args, **kwargs):
        return self.translate_many([text])[0]


def translate_streaming(job, proto, model_path):
    """Custom replacement for pdf2zh.pdf2zh.extract_text().

    Differences vs upstream:
      - layout model cached via get_layout_model (no per-call construction)
      - obj_patch entries applied to doc_en incrementally as each page's
        process_page returns, so the freshly-translated page can be extracted
        into a single-page PDF and announced before the loop continues
      - skips the dual PDF (we don't use it) and the final mono save
        (per-page outputs are the cache contents)
      - emits timing data so we can see what still dominates the run

    Layout / preprocessing semantics match upstream pdf2zh exactly; we still
    open with pymupdf, inject the china-ss / tiro fonts on every page, scan
    every xref, save the -en.pdf scratch file, and feed that to pdfminer.
    """
    import numpy as np
    import pymupdf
    import tqdm
    try:
        from pdf2zh.converter import TextConverter
        from pdf2zh.pdfinterp import PDFPageInterpreter, PDFResourceManager
        from pdf2zh.pdfpage import PDFPage
        using_new_pdf2zh_api = False
    except ImportError:
        from pdfminer.pdfinterp import PDFResourceManager
        from pdfminer.pdfpage import PDFPage
        from pdf2zh.converter import TranslateConverter as TextConverter
        from pdf2zh.pdfinterp import PDFPageInterpreterEx as PDFPageInterpreter
        using_new_pdf2zh_api = True

    job_id = str(job.get("id", ""))
    file_path = job["file"]
    output_dir = job["outputDir"]
    pages = job.get("pages")
    pages_zero_based = [int(p) - 1 for p in pages] if pages else None
    thread = int(job.get("thread", 4))
    lang_in = job.get("langIn", "auto")
    lang_out = job.get("langOut", "auto")
    service = job.get("service", "google")
    ignore_cache = bool(job.get("ignoreCache"))
    envs = job.get("env") or {}
    from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

    font_path = download_remote_fonts(lang_out.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)

    timings = {}
    emit_stage(
        proto,
        job_id,
        "job",
        "started",
        details={
            "requestedPageCount": len(pages) if pages else None,
            "thread": thread,
            "langIn": lang_in,
            "langOut": lang_out,
            "service": service,
            "ignoreCache": ignore_cache,
        },
    )

    t = stage_start(
        proto,
        job_id,
        "preprocess.openPrepareAndSavePdf",
        details={"requestedPageCount": len(pages) if pages else None},
    )
    doc_en = pymupdf.open(file_path)
    page_count = doc_en.page_count
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc_en:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    xreflen = doc_en.xref_length()
    for xref in range(1, xreflen):
        for label in ["Resources/", ""]:
            try:
                font_res = doc_en.xref_get_key(xref, f"{label}Font")
                target_key_prefix = f"{label}Font/"
                if font_res[0] == "xref":
                    resource_xref_id = re.search(r"(\d+) 0 R", font_res[1]).group(1)
                    xref = int(resource_xref_id)
                    font_res = ("dict", doc_en.xref_object(xref))
                    target_key_prefix = ""
                if font_res[0] == "dict":
                    for font_name, _font_file in font_list:
                        target_key = f"{target_key_prefix}{font_name}"
                        font_exist = doc_en.xref_get_key(xref, target_key)
                        if font_exist[0] == "null":
                            doc_en.xref_set_key(
                                xref, target_key, f"{font_id[font_name]} 0 R"
                            )
            except Exception:
                pass
    filename = os.path.splitext(os.path.basename(file_path))[0]
    en_path = os.path.join(output_dir, f"{filename}-en.pdf")
    doc_en.save(en_path)
    timings["preprocessMs"] = int((time.time() - t) * 1000)
    stage_done(
        proto,
        job_id,
        "preprocess.openPrepareAndSavePdf",
        t,
        details={
            "sourcePageCount": page_count,
            "xrefLength": xreflen,
            "fontNames": font_list,
            "preparedPdfBytes": os.path.getsize(en_path) if os.path.exists(en_path) else None,
        },
    )

    t = stage_start(proto, job_id, "model.getLayoutModel")
    model = get_layout_model(model_path)
    timings["modelReadyMs"] = int((time.time() - t) * 1000)
    stage_done(
        proto,
        job_id,
        "model.getLayoutModel",
        t,
        details={"modelPathPresent": bool(model_path), "backend": _layout_backend},
    )

    layout: dict = {}

    def make_interpreter(stage_name, obj_patch):
        t_init = stage_start(
            proto,
            job_id,
            stage_name,
            details={"thread": thread, "ignoreCache": ignore_cache},
        )
        rsrcmgr = PDFResourceManager(caching=not ignore_cache)
        if using_new_pdf2zh_api:
            device = TextConverter(
                rsrcmgr,
                vfont="",
                vchar="",
                thread=thread,
                layout=layout,
                lang_in=lang_in,
                lang_out=lang_out,
                service=service,
                noto_name=noto_name,
                noto=noto,
                envs=envs,
                prompt=None,
                ignore_cache=ignore_cache,
            )
        else:
            device = TextConverter(
                rsrcmgr,
                sys.stdout,
                codec="utf-8",
                laparams=None,
                vfont="",
                vchar="",
                thread=thread,
                layout=layout,
                lang_in=lang_in,
                lang_out=lang_out,
                service=service,
            )
        interpreter = PDFPageInterpreter(rsrcmgr, device, obj_patch)
        stage_done(proto, job_id, stage_name, t_init)
        return rsrcmgr, device, interpreter

    def collect_page_translation_units(interpreter, device, collector, page, page_number):
        before_count = len(collector.flattened_texts())
        t_collect = stage_start(
            proto,
            job_id,
            "page.collectTranslationUnits",
            page_number,
            details={"requestCountBefore": len(collector.requests)},
        )
        try:
            device.begin_page(page, page_ctm(page))
            ops_base = interpreter.render_contents(
                page.resources, page.contents, ctm=page_ctm(page)
            )
            ltpage = device.cur_item
            device.fontid = interpreter.fontid
            device.fontmap = interpreter.fontmap
            device.end_page(page)
        except BaseException as error:
            stage_failed(
                proto,
                job_id,
                "page.collectTranslationUnits",
                t_collect,
                page_number,
                details={"errorType": type(error).__name__},
            )
            raise
        after_texts = collector.flattened_texts()
        new_texts = after_texts[before_count:]
        stage_done(
            proto,
            job_id,
            "page.collectTranslationUnits",
            t_collect,
            page_number,
            details={
                "itemCount": len(new_texts),
                "inputChars": sum(len(text) for text in new_texts),
                "requestCount": len(collector.requests),
            },
        )
        return {
            "ltpage": ltpage,
            "opsBase": ops_base,
            "fontid": dict(interpreter.fontid),
            "fontmap": dict(interpreter.fontmap),
            "contentObjIds": [
                obj.objid for obj in page.contents if hasattr(obj, "objid")
            ],
            "cropbox": [float(value) for value in page.cropbox],
        }

    obj_patch: dict = {}
    _rsrcmgr, device, interpreter = make_interpreter(
        "pdfminer.initializeInterpreter",
        obj_patch,
    )

    t_translate = time.time()
    yolo_total = 0.0
    process_total = 0.0
    save_total = 0.0
    pages_translated = 0
    replay_page_cache = {}
    single_page_save_deflate = 1 if truthy_env("ROSETTA_PDF_SINGLE_PAGE_DEFLATE") else 0
    single_page_save_deflate_images = (
        0 if falsy_env("ROSETTA_PDF_SINGLE_PAGE_DEFLATE_IMAGES") else 1
    )

    t = stage_start(
        proto,
        job_id,
        "pdfminer.loadPages",
        details={"requestedPageCount": len(pages) if pages else None},
    )
    with open(en_path, "rb") as fp:
        page_iter = PDFPage.get_pages(
            fp,
            pages_zero_based,
            maxpages=0,
            password="",
            caching=not ignore_cache,
        )
        page_list = list(page_iter)
        total = len(page_list)
        requested_indices = (
            set(pages_zero_based) if pages_zero_based is not None else None
        )
        page_indices = [
            index
            for index in range(page_count)
            if requested_indices is None or index in requested_indices
        ][:total]
        stage_done(
            proto,
            job_id,
            "pdfminer.loadPages",
            t,
            details={"loadedPageCount": total},
        )

        use_cross_page_batch = (
            using_new_pdf2zh_api
            and cross_page_batch_enabled(service)
            and hasattr(device.translator, "translate_many")
        )
        if use_cross_page_batch:
            real_translator = device.translator
            collector = DeferredTranslationCollector(real_translator)
            device.translator = collector
            t_collect_all = stage_start(
                proto,
                job_id,
                "crossPageBatch.collect",
                details={"pageCount": total},
            )
            for page, page_index in tqdm.tqdm(
                zip(page_list, page_indices), total=total, position=0
            ):
                page.pageno = page_index
                page_number = page_index + 1

                t0 = stage_start(proto, job_id, "page.pixmapAndImage", page_number)
                pix = doc_en[page_index].get_pixmap()
                image = np.frombuffer(pix.samples, np.uint8).reshape(
                    pix.height, pix.width, 3
                )[:, :, ::-1]
                stage_done(
                    proto,
                    job_id,
                    "page.pixmapAndImage",
                    t0,
                    page_number,
                    details={
                        "width": pix.width,
                        "height": pix.height,
                        "sampleBytes": len(pix.samples),
                        "pass": "collect",
                    },
                )
                device_name = layout_device_name(model)
                imgsz, native_imgsz, imgsz_source = layout_imgsz_for_pix(pix, service)
                t0 = stage_start(
                    proto,
                    job_id,
                    "page.yoloPredict",
                    page_number,
                    details={
                        "device": device_name,
                        "imgsz": imgsz,
                        "nativeImgsz": native_imgsz,
                        "imgszSource": imgsz_source,
                        "pass": "collect",
                    },
                )
                predict_started = t0
                page_layout = model.predict(
                    image,
                    imgsz=imgsz,
                    device=device_name,
                    verbose=False,
                )[0]
                stage_done(
                    proto,
                    job_id,
                    "page.yoloPredict",
                    t0,
                    page_number,
                    details={
                        "device": device_name,
                        "imgsz": imgsz,
                        "nativeImgsz": native_imgsz,
                        "imgszSource": imgsz_source,
                        "boxCount": len(page_layout.boxes),
                        "pass": "collect",
                    },
                )
                yolo_total += time.time() - predict_started

                t0 = stage_start(
                    proto,
                    job_id,
                    "page.layoutMask",
                    page_number,
                    details={"boxCount": len(page_layout.boxes), "pass": "collect"},
                )
                box = np.ones((pix.height, pix.width))
                h, w = box.shape
                vcls = ["abandon", "figure", "table", "isolate_formula", "formula_caption"]
                for i, d in enumerate(page_layout.boxes):
                    if page_layout.names[int(d.cls)] not in vcls:
                        x0, y0, x1, y1 = d.xyxy.squeeze()
                        x0, y0, x1, y1 = (
                            np.clip(int(x0 - 1), 0, w - 1),
                            np.clip(int(h - y1 - 1), 0, h - 1),
                            np.clip(int(x1 + 1), 0, w - 1),
                            np.clip(int(h - y0 + 1), 0, h - 1),
                        )
                        box[y0:y1, x0:x1] = i + 2
                for i, d in enumerate(page_layout.boxes):
                    if page_layout.names[int(d.cls)] in vcls:
                        x0, y0, x1, y1 = d.xyxy.squeeze()
                        x0, y0, x1, y1 = (
                            np.clip(int(x0 - 1), 0, w - 1),
                            np.clip(int(h - y1 - 1), 0, h - 1),
                            np.clip(int(x1 + 1), 0, w - 1),
                            np.clip(int(h - y0 + 1), 0, h - 1),
                        )
                        box[y0:y1, x0:x1] = 0
                layout[page_index] = box
                stage_done(
                    proto,
                    job_id,
                    "page.layoutMask",
                    t0,
                    page_number,
                    details={"layoutHeight": h, "layoutWidth": w, "pass": "collect"},
                )

                replay_page_cache[page_index] = collect_page_translation_units(
                    interpreter,
                    device,
                    collector,
                    page,
                    page_number,
                )

            collected_texts = collector.flattened_texts()
            stage_done(
                proto,
                job_id,
                "crossPageBatch.collect",
                t_collect_all,
                details={
                    "pageCount": total,
                    "requestCount": len(collector.requests),
                    "itemCount": len(collected_texts),
                    "inputChars": sum(len(text) for text in collected_texts),
                },
            )

            t_batch = stage_start(
                proto,
                job_id,
                "crossPageBatch.translate",
                details={
                    "itemCount": len(collected_texts),
                    "inputChars": sum(len(text) for text in collected_texts),
                },
            )
            try:
                collected_translations = real_translator.translate_many(collected_texts)
            except BaseException as error:
                stage_failed(
                    proto,
                    job_id,
                    "crossPageBatch.translate",
                    t_batch,
                    details={
                        "itemCount": len(collected_texts),
                        "inputChars": sum(len(text) for text in collected_texts),
                        "errorType": type(error).__name__,
                    },
                )
                raise
            stage_done(
                proto,
                job_id,
                "crossPageBatch.translate",
                t_batch,
                details={
                    "itemCount": len(collected_texts),
                    "inputChars": sum(len(text) for text in collected_texts),
                    "outputChars": sum(len(text) for text in collected_translations),
                },
            )

            obj_patch = {}
            _rsrcmgr, device, interpreter = make_interpreter(
                "pdfminer.initializeReplayInterpreter",
                obj_patch,
            )
            device.translator = PretranslatedTranslator(
                real_translator,
                collected_texts,
                collected_translations,
            )

        for page, page_index in tqdm.tqdm(
            zip(page_list, page_indices), total=total, position=0
        ):
            # Newer pdfminer.six PDFPage objects no longer expose pageno, but
            # pdf2zh's converter still uses it to build LTPage.pageid and look
            # up layout masks. Reattach the zero-based document page index here.
            page.pageno = page_index
            page_number = page_index + 1
            emit_stage(
                proto,
                job_id,
                "page",
                "started",
                page_number=page_number,
                details={
                    "pageIndex": page_index,
                    "pageOrdinalInRun": pages_translated + 1,
                    "totalPagesInRun": total,
                },
            )

            page_started = time.time()
            if use_cross_page_batch and page_index in layout:
                t0 = stage_start(
                    proto,
                    job_id,
                    "page.layoutMask.reuse",
                    page_number,
                    details={"sourcePass": "collect"},
                )
                reused_layout = layout[page_index]
                h, w = reused_layout.shape
                stage_done(
                    proto,
                    job_id,
                    "page.layoutMask.reuse",
                    t0,
                    page_number,
                    details={"layoutHeight": h, "layoutWidth": w},
                )
            else:
                t0 = stage_start(proto, job_id, "page.pixmapAndImage", page_number)
                pix = doc_en[page_index].get_pixmap()
                image = np.frombuffer(pix.samples, np.uint8).reshape(
                    pix.height, pix.width, 3
                )[:, :, ::-1]
                stage_done(
                    proto,
                    job_id,
                    "page.pixmapAndImage",
                    t0,
                    page_number,
                    details={
                        "width": pix.width,
                        "height": pix.height,
                        "sampleBytes": len(pix.samples),
                    },
                )
                device_name = layout_device_name(model)
                imgsz, native_imgsz, imgsz_source = layout_imgsz_for_pix(pix, service)
                t0 = stage_start(
                    proto,
                    job_id,
                    "page.yoloPredict",
                    page_number,
                    details={
                        "device": device_name,
                        "imgsz": imgsz,
                        "nativeImgsz": native_imgsz,
                        "imgszSource": imgsz_source,
                    },
                )
                predict_started = t0
                page_layout = model.predict(
                    image,
                    imgsz=imgsz,
                    device=device_name,
                    verbose=False,
                )[0]
                stage_done(
                    proto,
                    job_id,
                    "page.yoloPredict",
                    t0,
                    page_number,
                    details={
                        "device": device_name,
                        "imgsz": imgsz,
                        "nativeImgsz": native_imgsz,
                        "imgszSource": imgsz_source,
                        "boxCount": len(page_layout.boxes),
                    },
                )
                yolo_total += time.time() - predict_started

                t0 = stage_start(
                    proto,
                    job_id,
                    "page.layoutMask",
                    page_number,
                    details={"boxCount": len(page_layout.boxes)},
                )
                box = np.ones((pix.height, pix.width))
                h, w = box.shape
                vcls = ["abandon", "figure", "table", "isolate_formula", "formula_caption"]
                for i, d in enumerate(page_layout.boxes):
                    if page_layout.names[int(d.cls)] not in vcls:
                        x0, y0, x1, y1 = d.xyxy.squeeze()
                        x0, y0, x1, y1 = (
                            np.clip(int(x0 - 1), 0, w - 1),
                            np.clip(int(h - y1 - 1), 0, h - 1),
                            np.clip(int(x1 + 1), 0, w - 1),
                            np.clip(int(h - y0 + 1), 0, h - 1),
                        )
                        box[y0:y1, x0:x1] = i + 2
                for i, d in enumerate(page_layout.boxes):
                    if page_layout.names[int(d.cls)] in vcls:
                        x0, y0, x1, y1 = d.xyxy.squeeze()
                        x0, y0, x1, y1 = (
                            np.clip(int(x0 - 1), 0, w - 1),
                            np.clip(int(h - y1 - 1), 0, h - 1),
                            np.clip(int(x1 + 1), 0, w - 1),
                            np.clip(int(h - y0 + 1), 0, h - 1),
                        )
                        box[y0:y1, x0:x1] = 0
                layout[page_index] = box
                stage_done(
                    proto,
                    job_id,
                    "page.layoutMask",
                    t0,
                    page_number,
                    details={"layoutHeight": h, "layoutWidth": w},
                )

            t0 = stage_start(proto, job_id, "page.prepareContentStream", page_number)
            page.rotate = page.rotate % 360
            page.page_xref = doc_en.get_new_xref()
            doc_en.update_object(page.page_xref, "<<>>")
            doc_en.update_stream(page.page_xref, b"")
            doc_en[page_index].set_contents(page.page_xref)
            stage_done(
                proto,
                job_id,
                "page.prepareContentStream",
                t0,
                page_number,
                details={"pageXref": page.page_xref},
            )

            t0 = stage_start(
                proto,
                job_id,
                "page.processPage",
                page_number,
                details={"existingPatchCount": len(obj_patch)},
            )
            before_keys = set(obj_patch.keys())
            cached_replay = replay_page_cache.get(page_index) if use_cross_page_batch else None
            if cached_replay:
                x0, y0, _x1, _y1 = page.cropbox
                probe = install_page_process_probe(device, proto, job_id, page_number)
                try:
                    t_replay = stage_start(
                        proto,
                        job_id,
                        "page.processPage.replayLayout",
                        page_number,
                        details={
                            "cachedContentObjectCount": len(
                                cached_replay["contentObjIds"]
                            ),
                        },
                    )
                    device.fontid = cached_replay["fontid"]
                    device.fontmap = cached_replay["fontmap"]
                    ops_new = device.receive_layout(cached_replay["ltpage"])
                    stage_done(
                        proto,
                        job_id,
                        "page.processPage.replayLayout",
                        t_replay,
                        page_number,
                        details={
                            "translatedOpsChars": len(ops_new)
                            if isinstance(ops_new, str)
                            else None,
                            **translate_probe_summary(probe),
                        },
                    )

                    t_patch = stage_start(
                        proto,
                        job_id,
                        "page.processPage.patchStreams",
                        page_number,
                        details={
                            "streamObjectCount": len(cached_replay["contentObjIds"]),
                            "source": "cachedReplay",
                        },
                    )
                    interpreter.obj_patch[
                        page.page_xref
                    ] = f"q {cached_replay['opsBase']}Q 1 0 0 1 {x0} {y0} cm {ops_new}"
                    for obj_id in cached_replay["contentObjIds"]:
                        interpreter.obj_patch[obj_id] = ""
                    stage_done(
                        proto,
                        job_id,
                        "page.processPage.patchStreams",
                        t_patch,
                        page_number,
                        details={
                            "streamObjectCount": len(cached_replay["contentObjIds"]),
                            "pagePatchChars": safe_len(
                                interpreter.obj_patch.get(page.page_xref)
                            ),
                            "source": "cachedReplay",
                        },
                    )
                finally:
                    device._rosetta_probe_context = None
            else:
                process_page_with_stages(
                    interpreter,
                    device,
                    page,
                    proto,
                    job_id,
                    page_number,
                )
            new_keys = list(set(obj_patch.keys()) - before_keys)
            process_total += time.time() - t0
            stage_done(
                proto,
                job_id,
                "page.processPage",
                t0,
                page_number,
                details={
                    "newPatchCount": len(new_keys),
                    "totalPatchCount": len(obj_patch),
                },
            )

            t0 = stage_start(
                proto,
                job_id,
                "page.applyPatches",
                page_number,
                details={"newPatchCount": len(new_keys)},
            )
            for obj_id in new_keys:
                doc_en.update_stream(obj_id, obj_patch[obj_id].encode())
            stage_done(
                proto,
                job_id,
                "page.applyPatches",
                t0,
                page_number,
                details={"newPatchCount": len(new_keys)},
            )

            t0 = stage_start(proto, job_id, "page.saveSinglePdf", page_number)
            single = pymupdf.open()
            t_insert = stage_start(
                proto,
                job_id,
                "page.saveSinglePdf.insertPage",
                page_number,
            )
            single.insert_pdf(doc_en, from_page=page_index, to_page=page_index)
            stage_done(
                proto,
                job_id,
                "page.saveSinglePdf.insertPage",
                t_insert,
                page_number,
            )
            page_out_path = os.path.join(
                output_dir, f"page-{page_number:04}.pdf"
            )
            # Page artifacts are local cache entries. Keep compression off by
            # default because translated page streams already contain large
            # rewritten objects and deflate can dominate warm-worker runtime.
            t_write = stage_start(
                proto,
                job_id,
                "page.saveSinglePdf.writeFile",
                page_number,
                details={
                    "deflate": single_page_save_deflate,
                    "deflateImages": single_page_save_deflate_images,
                },
            )
            single.save(
                page_out_path,
                deflate=single_page_save_deflate,
                deflate_images=single_page_save_deflate_images,
            )
            stage_done(
                proto,
                job_id,
                "page.saveSinglePdf.writeFile",
                t_write,
                page_number,
                details={
                    "deflate": single_page_save_deflate,
                    "deflateImages": single_page_save_deflate_images,
                    "outputBytes": os.path.getsize(page_out_path)
                    if os.path.exists(page_out_path)
                    else None,
                },
            )
            single.close()
            save_total += time.time() - t0
            stage_done(
                proto,
                job_id,
                "page.saveSinglePdf",
                t0,
                page_number,
                details={
                    "outputBytes": os.path.getsize(page_out_path)
                    if os.path.exists(page_out_path)
                    else None,
                },
            )
            pages_translated += 1

            t0 = stage_start(proto, job_id, "page.emitPageEvent", page_number)
            emit(
                proto,
                {
                    "id": job_id,
                    "event": "page",
                    "pageNumber": page_number,
                    "file": page_out_path,
                },
            )
            stage_done(proto, job_id, "page.emitPageEvent", t0, page_number)
            emit_stage(
                proto,
                job_id,
                "page",
                "completed",
                page_number=page_number,
                duration_ms=int((time.time() - page_started) * 1000),
                details={
                    "pageOrdinalInRun": pages_translated,
                    "totalPagesInRun": total,
                },
            )

    t = stage_start(proto, job_id, "cleanup.closePdfminer")
    device.close()
    stage_done(proto, job_id, "cleanup.closePdfminer", t)
    timings["translateMs"] = int((time.time() - t_translate) * 1000)
    timings["yoloMs"] = int(yolo_total * 1000)
    timings["processPageMs"] = int(process_total * 1000)
    timings["perPageSaveMs"] = int(save_total * 1000)
    timings["pagesTranslated"] = pages_translated
    timings["sourcePageCount"] = page_count

    t = stage_start(proto, job_id, "cleanup.closePdf")
    doc_en.close()
    stage_done(proto, job_id, "cleanup.closePdf", t)
    t = stage_start(proto, job_id, "cleanup.removePreparedPdf")
    try:
        os.remove(en_path)
        removed_prepared = True
    except OSError:
        removed_prepared = False
    stage_done(
        proto,
        job_id,
        "cleanup.removePreparedPdf",
        t,
        details={"removed": removed_prepared},
    )
    emit_stage(
        proto,
        job_id,
        "job",
        "completed",
        duration_ms=timings["translateMs"],
        details=timings,
    )

    return timings


def run_translate(job, proto, model_path):
    output_dir = job["outputDir"]
    os.makedirs(output_dir, exist_ok=True)
    ignore_cache = bool(job.get("ignoreCache"))

    for key, value in (job.get("env") or {}).items():
        os.environ[key] = str(value)

    tmp_dir = job.get("tmpDir")
    if tmp_dir:
        os.makedirs(tmp_dir, exist_ok=True)
        os.environ["TMPDIR"] = tmp_dir
        os.environ["TEMP"] = tmp_dir
        os.environ["TMP"] = tmp_dir
        tempfile.tempdir = tmp_dir

    # pdf2zh.cache is imported during worker prewarm, before per-job TMPDIR is
    # known, so its module-level cache_dir otherwise keeps pointing at the
    # process/system temp cache across all later jobs. Rebind it for each
    # translation task so "force retranslate" cannot reuse old paragraph text.
    import pdf2zh.cache as pdf2zh_cache

    cache_root = os.path.join(tempfile.gettempdir(), "cache")
    pdf2zh_cache.cache_dir = cache_root
    if ignore_cache and os.path.isdir(cache_root):
        shutil.rmtree(cache_root, ignore_errors=True)
    os.makedirs(cache_root, exist_ok=True)

    return translate_streaming(job, proto, model_path)


def emit_warming(proto, step, total, label):
    """Announce the next phase of the warmup before paying its cost.

    The Rust side mirrors {step,totalSteps,label} into the worker status so
    the header badge / topbar pill can show "[N/M label]"; without this, a
    fresh reinstall sits on a single "PDF engine warming" label for 30 s+ and
    looks frozen.
    """
    emit(
        proto,
        {
            "event": "warming",
            "step": step,
            "totalSteps": total,
            "label": label,
        },
    )


def setup_pdf2zh_logging(cli):
    """Set up pdf2zh logging across old and new PDFMathTranslate APIs."""
    setup_log = getattr(cli, "setup_log", None)
    if callable(setup_log):
        setup_log()
        return

    try:
        from rich.logging import RichHandler

        handlers = [RichHandler(console=None, show_path=False)]
    except Exception:
        handlers = None

    logging.basicConfig(level=logging.INFO, handlers=handlers)
    for logger_name in ("httpx", "openai", "httpcore", "http11"):
        logger = logging.getLogger(logger_name)
        logger.setLevel(logging.CRITICAL)
        logger.propagate = False


def main():
    proto = make_protocol_channel()

    import_started = time.time()
    try:
        emit_warming(proto, 1, 3, "加载 pdf2zh")
        from pdf2zh import pdf2zh as cli

        emit_warming(proto, 2, 3, "检查 PDF 组件")
        setup_pdf2zh_logging(cli)
        model_path = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
        if not model_path or not Path(model_path).is_file():
            raise RuntimeError(
                "ROSETTA_DOCLAYOUT_MODEL is missing or does not point to a file; "
                "update the Rosetta PDF component so the ONNX layout model is bundled."
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

    mps_enabled = False
    mps_reason = "not used by ONNX layout runtime"
    emit_warming(proto, 3, 3, "预热版面模型")
    layout_warmup = warm_layout_predict(model_path)
    if layout_warmup["status"] == "completed":
        print(
            "[pdf2zh-worker] layout predict warmup completed "
            f"({layout_warmup['durationMs']} ms, "
            f"device={layout_warmup['device']}, imgsz={layout_warmup['imgsz']}, "
            f"backend={layout_warmup.get('backend', '-')})",
            file=sys.stderr,
        )
    else:
        print(
            "[pdf2zh-worker] layout predict warmup failed "
            f"({layout_warmup['durationMs']} ms, "
            f"device={layout_warmup['device']}, reason={layout_warmup.get('reason', '-')})",
            file=sys.stderr,
        )
    emit(
        proto,
        {
            "event": "ready",
            "importMs": int((time.time() - import_started) * 1000),
            "mps": mps_enabled,
            "mpsReason": mps_reason,
            "yoloWarmupMs": layout_warmup["durationMs"],
            "yoloWarmupStatus": layout_warmup["status"],
            "yoloWarmupDevice": layout_warmup["device"],
            "yoloWarmupReason": layout_warmup.get("reason"),
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
        if cmd == "ping":
            emit(proto, {"id": job_id, "event": "pong"})
            continue
        if cmd == "exit":
            break
        if cmd != "translate":
            emit(proto, {"id": job_id, "event": "error", "message": f"unknown cmd: {cmd}"})
            continue

        try:
            timings = run_translate(job, proto, model_path)
            emit(proto, {"id": job_id, "event": "done", "timings": timings})
        except BaseException:
            emit(
                proto,
                {
                    "id": job_id,
                    "event": "error",
                    "message": traceback.format_exc(limit=8),
                },
            )

    return 0


if __name__ == "__main__":
    sys.exit(main())
