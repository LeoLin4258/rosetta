# Rosetta pdf2zh persistent worker.
#
# Written to disk by the Rosetta app at spawn time (embedded via include_str!
# in managed_pdf2zh/worker.rs) and executed with the pdf2zh pack's bundled
# CPython. Do NOT edit the on-disk copy — it is overwritten on every spawn.
#
# Why this exists: importing doclayout_yolo (torch + torchvision + opencv)
# costs ~13 s, while actually loading the YOLO layout model costs ~0.07 s.
# The pdf2zh CLI pays that import on every invocation; this worker pays it
# once per process and then serves translate jobs over a line-based JSON
# protocol:
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
# the pymupdf preprocess + pdfminer/YOLO loop and:
#   - cache the YOLO model across translate calls
#   - apply patches incrementally per page so each finished page can be saved
#     as a single-page PDF and announced via a "page" protocol event
#   - skip the dual PDF (Rosetta never consumes it)
#   - skip the final mono save (Rosetta consumes the per-page outputs)

import json
import os
from pathlib import Path
import sys
import tempfile
import time
import traceback


_yolo_model = None
_yolo_model_path = None


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
    proto.write(json.dumps(payload, ensure_ascii=False) + "\n")
    proto.flush()


def probe_and_enable_mps(torch, doclayout_yolo, model_path):
    """Optionally upgrade pdf2zh's hardcoded device="cpu" predict calls to
    MPS via a monkeypatch, gated on a successful page-sized probe inference.

    OFF BY DEFAULT: measured on an M4 mini (2026-06-11, 18-page doc), MPS was
    ~0.8 s/page SLOWER than CPU — the DocLayout YOLO model is small enough
    that per-call transfer + graph dispatch + CPU-fallback op bouncing
    (PYTORCH_ENABLE_MPS_FALLBACK) outweighs the compute win. Kept behind an
    env switch for future experiments (bigger models, torch upgrades).

    Returns (enabled, reason) so the ready event can explain the choice.
    """
    if os.environ.get("ROSETTA_PDF2ZH_ENABLE_MPS", "") not in ("1", "true", "yes"):
        return False, "cpu default (MPS measured slower; opt in with ROSETTA_PDF2ZH_ENABLE_MPS=1)"
    try:
        if not torch.backends.mps.is_built():
            return False, "torch built without MPS support"
        if not torch.backends.mps.is_available():
            return False, "torch.backends.mps.is_available() = False"
        import numpy as np

        model = doclayout_yolo.YOLOv10(model_path)
        # Page-sized probe: YOLOv10's post-processing does top-k(300) over
        # anchor candidates, and a tiny image has fewer than 300 anchors,
        # which fails with "selected index k out of range". Real pages are
        # ~1000 px so probe at the size pdf2zh actually uses.
        dummy = np.zeros((1056, 816, 3), dtype=np.uint8)
        model.predict(dummy, imgsz=1024, device="mps", verbose=False)
    except Exception as error:
        return False, f"probe failed: {type(error).__name__}: {str(error)[:300]}"

    original_predict = doclayout_yolo.YOLOv10.predict

    def rosetta_predict(self, *args, **kwargs):
        if kwargs.get("device") == "cpu":
            kwargs["device"] = "mps"
        return original_predict(self, *args, **kwargs)

    doclayout_yolo.YOLOv10.predict = rosetta_predict
    return True, "enabled"


def get_yolo_model(model_path):
    """Re-use the YOLO model object across translate calls.

    `pdf2zh.pdf2zh.extract_text` re-instantiates this on every invocation,
    which adds avoidable Python-side construction time. The model itself is
    small and stateless — caching it across calls is safe.
    """
    import doclayout_yolo

    global _yolo_model, _yolo_model_path
    if _yolo_model is None or _yolo_model_path != model_path:
        _yolo_model = doclayout_yolo.YOLOv10(model_path)
        _yolo_model_path = model_path
    return _yolo_model


def translate_streaming(job, proto, model_path):
    """Custom replacement for pdf2zh.pdf2zh.extract_text().

    Differences vs upstream:
      - YOLO model cached via get_yolo_model (no per-call construction)
      - obj_patch entries applied to doc_en incrementally as each page's
        process_page returns, so the freshly-translated page can be extracted
        into a single-page PDF and announced before the loop continues
      - skips the dual PDF (we don't use it) and the final mono save
        (per-page outputs are the cache contents)
      - emits timing data so we can see what still dominates the run

    Layout / preprocessing semantics match upstream pdf2zh exactly — we still
    open with pymupdf, inject the china-ss / tiro fonts on every page, scan
    every xref, save the -en.pdf scratch file, and feed that to pdfminer.
    """
    import numpy as np
    import pymupdf
    import torch
    import tqdm
    from pdf2zh.converter import TextConverter
    from pdf2zh.pdfinterp import PDFPageInterpreter, PDFResourceManager
    from pdf2zh.pdfpage import PDFPage

    job_id = str(job.get("id", ""))
    file_path = job["file"]
    output_dir = job["outputDir"]
    pages = job.get("pages")
    pages_zero_based = [int(p) - 1 for p in pages] if pages else None
    thread = int(job.get("thread", 4))
    lang_in = job.get("langIn", "auto")
    lang_out = job.get("langOut", "auto")
    service = job.get("service", "google")

    timings = {}

    t = time.time()
    doc_en = pymupdf.open(file_path)
    page_count = doc_en.page_count
    font_list = ["china-ss", "tiro"]
    font_id = {}
    for page in doc_en:
        for font in font_list:
            font_id[font] = page.insert_font(font)
    xreflen = doc_en.xref_length()
    for xref in range(1, xreflen):
        for label in ["Resources/", ""]:
            try:
                font_res = doc_en.xref_get_key(xref, f"{label}Font")
                if font_res[0] == "dict":
                    for font in font_list:
                        font_exist = doc_en.xref_get_key(
                            xref, f"{label}Font/{font}"
                        )
                        if font_exist[0] == "null":
                            doc_en.xref_set_key(
                                xref, f"{label}Font/{font}", f"{font_id[font]} 0 R"
                            )
            except Exception:
                pass
    filename = os.path.splitext(os.path.basename(file_path))[0]
    en_path = os.path.join(output_dir, f"{filename}-en.pdf")
    doc_en.save(en_path)
    timings["preprocessMs"] = int((time.time() - t) * 1000)

    t = time.time()
    model = get_yolo_model(model_path)
    timings["modelReadyMs"] = int((time.time() - t) * 1000)

    obj_patch: dict = {}
    layout: dict = {}
    rsrcmgr = PDFResourceManager(caching=True)
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

    t_translate = time.time()
    yolo_total = 0.0
    process_total = 0.0
    save_total = 0.0
    pages_translated = 0

    with open(en_path, "rb") as fp:
        page_iter = PDFPage.get_pages(
            fp,
            pages_zero_based,
            maxpages=0,
            password="",
            caching=True,
        )
        page_list = list(page_iter)
        total = len(page_list)

        for page in tqdm.tqdm(page_list, total=total, position=0):
            t0 = time.time()
            pix = doc_en[page.pageno].get_pixmap()
            image = np.frombuffer(pix.samples, np.uint8).reshape(
                pix.height, pix.width, 3
            )[:, :, ::-1]
            page_layout = model.predict(
                image,
                imgsz=int(pix.height / 32) * 32,
                device="cuda:0" if torch.cuda.is_available() else "cpu",
                verbose=False,
            )[0]
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
            layout[page.pageno] = box
            yolo_total += time.time() - t0

            page.rotate = page.rotate % 360
            page.page_xref = doc_en.get_new_xref()
            doc_en.update_object(page.page_xref, "<<>>")
            doc_en.update_stream(page.page_xref, b"")
            doc_en[page.pageno].set_contents(page.page_xref)

            t0 = time.time()
            before_keys = set(obj_patch.keys())
            interpreter.process_page(page)
            new_keys = list(set(obj_patch.keys()) - before_keys)
            process_total += time.time() - t0

            t0 = time.time()
            for obj_id in new_keys:
                doc_en.update_stream(obj_id, obj_patch[obj_id].encode())

            single = pymupdf.open()
            single.insert_pdf(doc_en, from_page=page.pageno, to_page=page.pageno)
            page_out_path = os.path.join(
                output_dir, f"page-{page.pageno + 1:04}.pdf"
            )
            # deflate=1 matches upstream pdf2zh's mono save. Single-page docs
            # serialize fast (a few ms), so this doesn't move the needle.
            single.save(page_out_path, deflate=1)
            single.close()
            save_total += time.time() - t0
            pages_translated += 1

            emit(
                proto,
                {
                    "id": job_id,
                    "event": "page",
                    "pageNumber": page.pageno + 1,
                    "file": page_out_path,
                },
            )

    device.close()
    timings["translateMs"] = int((time.time() - t_translate) * 1000)
    timings["yoloMs"] = int(yolo_total * 1000)
    timings["processPageMs"] = int(process_total * 1000)
    timings["perPageSaveMs"] = int(save_total * 1000)
    timings["pagesTranslated"] = pages_translated
    timings["sourcePageCount"] = page_count

    doc_en.close()
    try:
        os.remove(en_path)
    except OSError:
        pass

    return timings


def run_translate(job, proto, model_path):
    output_dir = job["outputDir"]
    os.makedirs(output_dir, exist_ok=True)

    for key, value in (job.get("env") or {}).items():
        os.environ[key] = str(value)

    tmp_dir = job.get("tmpDir")
    if tmp_dir:
        os.makedirs(tmp_dir, exist_ok=True)
        os.environ["TMPDIR"] = tmp_dir
        os.environ["TEMP"] = tmp_dir
        os.environ["TMP"] = tmp_dir
        tempfile.tempdir = tmp_dir

    return translate_streaming(job, proto, model_path)


def emit_warming(proto, step, total, label):
    """Announce the next phase of the warmup before paying its cost.

    The Rust side mirrors {step,totalSteps,label} into the worker status so
    the header badge / topbar pill can show "[N/M label]" — without this, a
    fresh reinstall sits on a single "PDF 引擎预热中" label for 30 s+ and
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


def main():
    proto = make_protocol_channel()

    import_started = time.time()
    try:
        # Phase split mirrors the import-time breakdown measured on M-series
        # macs: torch dominates (~70%), the CV stack pulled in by
        # doclayout_yolo (~20%), pdf2zh itself (~8%), then the model-path
        # check. The labels are user-facing so keep them plain.
        emit_warming(proto, 1, 4, "加载 PyTorch")
        import torch

        emit_warming(proto, 2, 4, "加载文档版面模型库")
        import doclayout_yolo

        emit_warming(proto, 3, 4, "加载 pdf2zh")
        from pdf2zh import pdf2zh as cli

        emit_warming(proto, 4, 4, "完成准备")
        cli.setup_log()
        model_path = os.environ.get("ROSETTA_DOCLAYOUT_MODEL")
        if not model_path or not Path(model_path).is_file():
            raise RuntimeError(
                "ROSETTA_DOCLAYOUT_MODEL is missing or does not point to a file; "
                "update the Rosetta PDF component so the DocLayout-YOLO model is bundled."
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

    mps_enabled, mps_reason = probe_and_enable_mps(torch, doclayout_yolo, model_path)

    emit(
        proto,
        {
            "event": "ready",
            "importMs": int((time.time() - import_started) * 1000),
            "mps": mps_enabled,
            "mpsReason": mps_reason,
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
