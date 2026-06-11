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
#   stdout (worker -> Rosetta): {"id": "...", "event": "ready|done|error", ...}
#
# pdf2zh's own logging / tqdm progress keeps flowing on stderr, where the
# Rust side parses it exactly like the CLI path. The real stdout is reserved
# for protocol lines: fd 1 is duplicated for the protocol channel and then
# redirected to stderr so stray library prints can't corrupt the protocol.

import json
import os
import sys
import tempfile
import time
import traceback


def make_protocol_channel():
    proto = os.fdopen(os.dup(1), "w", buffering=1)
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


def run_translate(job):
    output_dir = job["outputDir"]
    os.makedirs(output_dir, exist_ok=True)
    os.chdir(output_dir)

    for key, value in (job.get("env") or {}).items():
        os.environ[key] = str(value)

    tmp_dir = job.get("tmpDir")
    if tmp_dir:
        os.makedirs(tmp_dir, exist_ok=True)
        os.environ["TMPDIR"] = tmp_dir
        os.environ["TEMP"] = tmp_dir
        os.environ["TMP"] = tmp_dir
        tempfile.tempdir = tmp_dir

    pages = job.get("pages")
    pages_zero_based = [int(p) - 1 for p in pages] if pages else None

    from pdf2zh import pdf2zh as cli

    cli.extract_text(
        files=[job["file"]],
        pages=pages_zero_based,
        lang_in=job.get("langIn", "auto"),
        lang_out=job.get("langOut", "auto"),
        service=job.get("service", "google"),
        thread=int(job.get("thread", 4)),
    )


def main():
    proto = make_protocol_channel()

    import_started = time.time()
    try:
        import torch
        import doclayout_yolo
        from huggingface_hub import hf_hub_download
        from pdf2zh import pdf2zh as cli

        cli.setup_log()
        model_path = hf_hub_download(
            repo_id="juliozhao/DocLayout-YOLO-DocStructBench",
            filename="doclayout_yolo_docstructbench_imgsz1024.pt",
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
            run_translate(job)
            emit(proto, {"id": job_id, "event": "done"})
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
