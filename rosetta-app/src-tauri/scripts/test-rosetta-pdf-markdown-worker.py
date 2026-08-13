#!/usr/bin/env python3
"""Protocol and path-boundary tests for the isolated PDF Markdown worker."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "src" / "managed_pdf_markdown" / "rosetta_pdf_markdown_worker.py"


def write_fake_runtime(root: Path, *, pymupdf_version: str = "1.28.0") -> None:
    (root / "pymupdf.py").write_text(
        f"""import os
print('vendor-import-noise')
os.write(1, b'vendor-native-import-noise\\n')
__version__ = {pymupdf_version!r}
class Model:
    _providers = ['CPUExecutionProvider']
class Wrapper:
    _model = Model()
def make_get_layout(wrapper):
    def get_layout():
        return wrapper
    return get_layout
_get_layout = make_get_layout(Wrapper())
""",
        encoding="utf-8",
    )
    (root / "pymupdf4llm.py").write_text(
        """import json
import os
print('vendor-import-noise')
os.write(1, b'vendor-native-import-noise\\n')
def to_json(source, *, pages, use_ocr, force_text, write_images, image_path):
    print('vendor-extraction-noise')
    os.write(1, b'vendor-native-extraction-noise\\n')
    assert use_ocr is False
    assert force_text is False
    assert write_images is True
    assert len(pages) == 1
    return json.dumps({'pages': [{'pageIndex': pages[0], 'boxes': []}]})
""",
        encoding="utf-8",
    )
    for distribution, version in [
        ("pymupdf4llm", "1.28.0"),
        ("pymupdf_layout", "1.28.0"),
        ("pymupdf", pymupdf_version),
    ]:
        info = root / f"{distribution}-{version}.dist-info"
        info.mkdir()
        info.joinpath("METADATA").write_text(
            f"Metadata-Version: 2.1\nName: {distribution}\nVersion: {version}\n",
            encoding="utf-8",
        )


class Worker:
    def __init__(self, runtime: Path, jobs_root: Path):
        env = os.environ.copy()
        env.update(
            {
                "PYTHONPATH": str(runtime),
                "PYTHONNOUSERSITE": "1",
                "ROSETTA_PDF_MARKDOWN_JOBS_ROOT": str(jobs_root),
            }
        )
        self.process = subprocess.Popen(
            [sys.executable, str(WORKER)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
            encoding="utf-8",
        )

    def request(self, payload: dict) -> dict:
        assert self.process.stdin and self.process.stdout
        self.process.stdin.write(json.dumps(payload) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise AssertionError("worker exited without a response")
        return json.loads(line)

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self.request({"type": "shutdown"})
            except (BrokenPipeError, AssertionError):
                pass
        self.process.wait(timeout=5)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream:
                stream.close()


class PdfMarkdownWorkerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="rosetta-pdf-markdown-worker-")
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        self.jobs = self.root / "jobs"
        self.job = self.jobs / "job-safe"
        self.output = self.job / "pdf-markdown" / ".tmp" / "run-1"
        self.runtime.mkdir()
        self.output.mkdir(parents=True)
        self.job.joinpath("source.pdf").write_bytes(b"%PDF-1.7\n")
        write_fake_runtime(self.runtime)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_hello_reports_exact_cpu_only_contract(self) -> None:
        worker = Worker(self.runtime, self.jobs)
        try:
            ready = worker.request({"type": "hello"})
            self.assertEqual(ready["type"], "ready")
            self.assertEqual(ready["protocol"], 1)
            self.assertEqual(ready["providers"], ["CPUExecutionProvider"])
            self.assertEqual(ready["integrationBoundary"], "to_json")
            self.assertTrue(ready["cpuOnly"])
            self.assertEqual(set(ready["versions"].values()), {"1.28.0"})
        finally:
            worker.close()

    def test_extract_window_uses_to_json_and_reports_bounded_progress(self) -> None:
        worker = Worker(self.runtime, self.jobs)
        try:
            worker.request({"type": "hello"})
            progress = worker.request(
                {
                    "type": "extractWindow",
                    "id": "window-1",
                    "sourcePath": str(self.job / "source.pdf"),
                    "tempDir": str(self.output),
                    "pages": [0],
                }
            )
            result = json.loads(worker.process.stdout.readline())
            self.assertEqual(progress, {"type": "windowProgress", "id": "window-1", "completed": 1, "total": 1})
            self.assertEqual(result["type"], "windowResult")
            self.assertEqual(result["pages"][0]["pageIndex"], 0)
            self.assertEqual(result["pages"][0]["json"]["pages"][0]["pageIndex"], 0)
        finally:
            worker.close()

    def test_path_escape_and_oversized_window_are_rejected_without_path_disclosure(self) -> None:
        outside = self.root / "outside"
        outside.mkdir()
        worker = Worker(self.runtime, self.jobs)
        try:
            worker.request({"type": "hello"})
            response = worker.request(
                {
                    "type": "extractWindow",
                    "id": "escape",
                    "sourcePath": str(self.job / "source.pdf"),
                    "tempDir": str(outside),
                    "pages": [0],
                }
            )
            encoded = json.dumps(response)
            self.assertEqual(response["type"], "error")
            self.assertNotIn(str(self.root), encoded)
            response = worker.request(
                {
                    "type": "extractWindow",
                    "id": "too-many",
                    "sourcePath": str(self.job / "source.pdf"),
                    "tempDir": str(self.output),
                    "pages": list(range(11)),
                }
            )
            self.assertEqual(response["code"], "invalid-request")
        finally:
            worker.close()

    def test_version_mismatch_fails_closed(self) -> None:
        bad = self.root / "bad-runtime"
        bad.mkdir()
        write_fake_runtime(bad, pymupdf_version="1.25.2")
        worker = Worker(bad, self.jobs)
        try:
            response = worker.request({"type": "hello"})
            self.assertEqual(response["type"], "error")
            self.assertEqual(response["code"], "version-mismatch")
        finally:
            worker.close()

    def test_request_size_limit_terminates_protocol_stream(self) -> None:
        worker = Worker(self.runtime, self.jobs)
        try:
            assert worker.process.stdin and worker.process.stdout
            worker.process.stdin.write("{" + "x" * (64 * 1024) + "}\n")
            worker.process.stdin.flush()
            response = json.loads(worker.process.stdout.readline())
            self.assertEqual(response["code"], "request-too-large")
            worker.process.wait(timeout=5)
        finally:
            worker.close()


if __name__ == "__main__":
    unittest.main()
