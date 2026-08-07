import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent


def load_script(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPT_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


checkpoint = load_script("pdf_markdown_checkpoint0", "pdf_markdown_checkpoint0.py")
overlay = load_script("build_pdf_markdown_overlay", "build-pdf-markdown-overlay.py")


class CheckpointScriptTests(unittest.TestCase):
    def test_safe_resolve_rejects_parent_traversal(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(checkpoint.CheckpointError):
                checkpoint.safe_resolve(root, "../outside.pdf")

    def test_analyze_page_detects_duplicate_and_figure_overlap(self):
        text_box = {
            "boxclass": "text",
            "x0": 10,
            "y0": 10,
            "x1": 20,
            "y1": 20,
            "textlines": [{"spans": [{"text": "duplicate"}]}],
        }
        page = {
            "page_number": 1,
            "boxes": [
                text_box,
                dict(text_box),
                {"boxclass": "picture", "x0": 0, "y0": 0, "x1": 30, "y1": 30},
            ],
        }
        result = checkpoint.analyze_page(page, 1)
        self.assertEqual(result["adjacentDuplicateBodyBoxes"], 1)
        self.assertEqual(result["figureInternalTextOverlapCount"], 2)

    def test_atomic_json_replaces_complete_document(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "result.json"
            checkpoint.atomic_write_json(path, {"value": 1})
            checkpoint.atomic_write_json(path, {"value": 2})
            self.assertEqual(json.loads(path.read_text()), {"value": 2})
            self.assertEqual(list(path.parent.glob("*.tmp")), [])

    def test_extract_window_calls_to_json_one_page_at_a_time(self):
        calls = []

        class FakeEngine:
            @staticmethod
            def to_json(_source, **options):
                calls.append(options["pages"])
                page_number = options["pages"][0] + 1
                return json.dumps(
                    {
                        "filename": "source.pdf",
                        "pages": [{"page_number": page_number, "boxes": []}],
                        "use_ocr": False,
                        "force_text": False,
                        "write_images": True,
                    }
                )

        with tempfile.TemporaryDirectory() as temp:
            value, _ = checkpoint.extract_window(
                FakeEngine(), Path("source.pdf"), "fixture", [0, 1], Path(temp)
            )
        self.assertEqual(calls, [[0], [1]])
        self.assertEqual([page["page_number"] for page in value["pages"]], [1, 2])

    def test_prune_overlay_keeps_only_pinned_default_models(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            models = root / "pymupdf" / "layout" / "resources" / "onnx"
            models.mkdir(parents=True)
            for name in overlay.REQUIRED_LAYOUT_RESOURCES | {"unused.onnx"}:
                (models / name).write_bytes(b"model")
            development = root / "pymupdf" / "mupdf-devel"
            development.mkdir(parents=True)
            (development / "header.h").write_text("header")
            cache = root / "pymupdf" / "__pycache__"
            cache.mkdir()
            (cache / "module.pyc").write_bytes(b"cache")
            stats = overlay.prune_overlay(root)
            self.assertGreater(stats["removedBytes"], 0)
            self.assertEqual(
                {path.name for path in models.iterdir()}, overlay.REQUIRED_LAYOUT_RESOURCES
            )
            self.assertFalse(development.exists())
            self.assertFalse(cache.exists())

    def test_deterministic_zip_is_byte_stable(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            payload = root / "overlay"
            payload.mkdir()
            (payload / "b.txt").write_text("b")
            (payload / "a.txt").write_text("a")
            first = root / "first.zip"
            second = root / "second.zip"
            overlay.write_deterministic_zip(payload, first)
            overlay.write_deterministic_zip(payload, second)
            self.assertEqual(first.read_bytes(), second.read_bytes())


if __name__ == "__main__":
    unittest.main()
