import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


WORKER_PATH = (
    Path(__file__).resolve().parents[1]
    / "src"
    / "managed_pdf2zh"
    / "rosetta_pdf2zh_worker.py"
)
SPEC = importlib.util.spec_from_file_location("rosetta_pdf2zh_worker", WORKER_PATH)
worker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(worker)


class FakeEngine:
    ENGINE_VERSION = "rosetta-pdf-engine-v2.1"
    _PERSISTENT_LAYOUT_CACHE_SCHEMA = 1

    def __init__(self):
        self.prepare_count = 0
        self.reset_ids = []
        self.disposed_ids = []
        self.fail_reset_ids = set()
        self.fail_collect_ids = set()
        self.persistent_hit_on_prepare = False

    def prepareRun(self, *_args, **_kwargs):
        self.prepare_count += 1
        prepared_run_id = f"run-{self.prepare_count}"
        return {
            "preparedRunId": prepared_run_id,
            "pages": [{"page": 1}],
            "persistentLayoutCacheHit": self.persistent_hit_on_prepare,
        }

    def collectUnits(self, prepared_run_id):
        if prepared_run_id in self.fail_collect_ids:
            raise RuntimeError("collect failed")
        return [{"unitId": f"unit-{prepared_run_id}", "sourceChars": 10}]

    def resetRun(self, prepared_run_id):
        self.reset_ids.append(prepared_run_id)
        if prepared_run_id in self.fail_reset_ids:
            raise RuntimeError("reset failed")

    def disposeRun(self, prepared_run_id):
        self.disposed_ids.append(prepared_run_id)

    def persistent_layout_model_signature(self, _model_path):
        return {"filename": "model.onnx", "bytes": 10, "modifiedNs": 20}


class LegacyEngine:
    def __init__(self):
        self.prepare_count = 0
        self.disposed_ids = []

    def prepareRun(self, *_args, **_kwargs):
        self.prepare_count += 1
        return {
            "preparedRunId": f"legacy-{self.prepare_count}",
            "pages": [{"page": 1}],
        }

    def collectUnits(self, prepared_run_id):
        return [{"unitId": f"unit-{prepared_run_id}", "sourceChars": 10}]

    def disposeRun(self, prepared_run_id):
        self.disposed_ids.append(prepared_run_id)


def prepare_job(cache_key):
    return {
        "id": cache_key,
        "file": f"{cache_key}.pdf",
        "cacheKey": cache_key,
        "cacheOwnerKey": cache_key,
        "pages": [1],
        "langIn": "en",
        "langOut": "zh",
        "options": {},
    }


def emitted_messages(proto):
    return [json.loads(line) for line in proto.getvalue().splitlines()]


class PrepareCacheTests(unittest.TestCase):
    def setUp(self):
        worker._prepare_cache.clear()
        worker._persistent_cache_owner_keys.clear()
        self.engine = FakeEngine()
        self.proto = io.StringIO()
        self.env = patch.dict(os.environ, {"ROSETTA_PDF_PREPARE_CACHE_ENTRIES": "2"})
        self.env.start()

    def tearDown(self):
        worker._prepare_cache.clear()
        worker._persistent_cache_owner_keys.clear()
        self.env.stop()

    def prepare(self, cache_key):
        worker.run_prepare(prepare_job(cache_key), self.proto, self.engine)

    def test_switching_back_to_a_prepared_pdf_is_a_cache_hit(self):
        self.prepare("a")
        self.prepare("b")
        self.prepare("a")

        self.assertEqual(self.engine.prepare_count, 2)
        self.assertEqual(self.engine.reset_ids, ["run-1"])
        completed = [
            message
            for message in emitted_messages(self.proto)
            if message.get("event") == "stage" and message.get("status") == "completed"
        ]
        self.assertTrue(completed[-1]["details"]["cacheHit"])
        self.assertEqual(completed[-1]["details"]["cacheEntryCount"], 2)
        self.assertEqual(completed[-1]["details"]["cachedOwnerKeys"], ["b", "a"])

    def test_capacity_evicts_the_least_recently_used_run(self):
        self.prepare("a")
        self.prepare("b")
        self.prepare("a")
        self.prepare("c")

        self.assertEqual(self.engine.disposed_ids, ["run-2"])
        self.assertEqual(list(worker._prepare_cache), ["a", "c"])
        self.assertEqual(worker.cached_owner_keys(), ["a", "c"])

    def test_disk_layout_restore_is_reported_as_a_cache_hit(self):
        self.engine.persistent_hit_on_prepare = True
        self.prepare("a")

        completed = [
            message
            for message in emitted_messages(self.proto)
            if message.get("event") == "stage" and message.get("status") == "completed"
        ][-1]
        prepared = [
            message
            for message in emitted_messages(self.proto)
            if message.get("event") == "prepared_pdf_window"
        ][-1]
        self.assertTrue(completed["details"]["cacheHit"])
        self.assertEqual(completed["details"]["cacheTier"], "disk")
        self.assertTrue(prepared["cacheHit"])
        self.assertEqual(prepared["cacheTier"], "disk")

    def test_worker_startup_discovers_valid_job_local_disk_cache(self):
        with tempfile.TemporaryDirectory() as root_text:
            root = Path(root_text)
            job_dir = root / "job-1"
            entry = job_dir / "pdf-prepare-cache" / "v1" / "entry"
            entry.mkdir(parents=True)
            (job_dir / "pdf_source.json").write_text(
                json.dumps({"sourceFingerprint": "source-1"}), encoding="utf-8"
            )
            (entry / "layout.npz").write_bytes(b"fixture")
            (entry / "manifest.json").write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "engineVersion": "rosetta-pdf-engine-v2.1",
                        "sourceFingerprint": "source-1",
                        "model": {
                            "filename": "model.onnx",
                            "bytes": 10,
                            "modifiedNs": 20,
                        },
                        "layoutFile": "layout.npz",
                    }
                ),
                encoding="utf-8",
            )

            with patch.dict(
                os.environ, {"ROSETTA_PDF_PREPARE_CACHE_JOBS_ROOT": root_text}
            ):
                owners = worker.discover_persistent_cache_owner_keys(
                    self.engine, "model.onnx"
                )

            self.assertEqual(owners, ["job-1"])

    def test_explicit_dispose_removes_only_the_matching_run(self):
        self.prepare("a")
        self.prepare("b")
        worker.run_dispose(
            {"id": "dispose", "preparedRunId": "run-1"}, self.proto, self.engine
        )

        self.assertEqual(list(worker._prepare_cache), ["b"])
        self.assertEqual(self.engine.disposed_ids, ["run-1"])
        disposed = emitted_messages(self.proto)[-1]
        self.assertEqual(disposed["cachedOwnerKeys"], ["b"])

    def test_reset_failure_discards_bad_entry_and_prepares_again(self):
        self.prepare("a")
        self.engine.fail_reset_ids.add("run-1")
        self.prepare("a")

        self.assertEqual(self.engine.prepare_count, 2)
        self.assertEqual(self.engine.disposed_ids, ["run-1"])
        self.assertEqual(worker._prepare_cache["a"]["preparedRunId"], "run-2")

    def test_collect_failure_disposes_the_new_run_without_caching_it(self):
        self.engine.fail_collect_ids.add("run-1")

        with self.assertRaisesRegex(RuntimeError, "collect failed"):
            self.prepare("a")

        self.assertEqual(self.engine.disposed_ids, ["run-1"])
        self.assertEqual(len(worker._prepare_cache), 0)

    def test_legacy_engine_without_reset_keeps_conservative_single_run_behavior(self):
        legacy = LegacyEngine()
        worker.run_prepare(prepare_job("a"), self.proto, legacy)
        worker.run_prepare(prepare_job("b"), self.proto, legacy)
        worker.run_prepare(prepare_job("a"), self.proto, legacy)

        self.assertEqual(legacy.prepare_count, 3)
        self.assertEqual(legacy.disposed_ids, ["legacy-1", "legacy-2"])
        self.assertEqual(len(worker._prepare_cache), 1)

    def test_configured_capacity_is_clamped(self):
        with patch.dict(os.environ, {"ROSETTA_PDF_PREPARE_CACHE_ENTRIES": "0"}):
            self.assertEqual(worker.prepare_cache_max_entries(), 1)
        with patch.dict(os.environ, {"ROSETTA_PDF_PREPARE_CACHE_ENTRIES": "100"}):
            self.assertEqual(worker.prepare_cache_max_entries(), 32)
        with patch.dict(os.environ, {"ROSETTA_PDF_PREPARE_CACHE_ENTRIES": "invalid"}):
            self.assertEqual(worker.prepare_cache_max_entries(), 6)


if __name__ == "__main__":
    unittest.main()
