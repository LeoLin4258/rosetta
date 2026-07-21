#!/usr/bin/env python3
import ast
import difflib
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
PATCH_SCRIPT = SCRIPT_DIR / "patch-pdf2zh-color-preservation.py"
DIRECTML_PATCH_SCRIPT = SCRIPT_DIR / "patch-pdf2zh-directml-layout.py"
FONT_ASSETS_SCRIPT = SCRIPT_DIR / "stage-pdf2zh-font-assets.py"


class Pdf2zhPatchTests(unittest.TestCase):
    def test_directml_patch_is_bounded_and_has_cpu_fallback(self) -> None:
        text = DIRECTML_PATCH_SCRIPT.read_text(encoding="utf-8")

        self.assertIn('return 5 if self._uses_directml else 1', text)
        self.assertIn('min(5, int(requested))', text)
        self.assertIn('falling back to CPU', text)
        self.assertIn('["CPUExecutionProvider"]', text)
        self.assertIn('grouped[pix.shape]', text)

    def test_windows_pack_uses_directml_runtime_and_patch(self) -> None:
        requirements = (
            SCRIPT_DIR / "requirements-pdf2zh-windows-amd64.txt"
        ).read_text(encoding="utf-8")
        builder = (
            SCRIPT_DIR / "build-pdf2zh-pack-windows-amd64.ps1"
        ).read_text(encoding="utf-8")

        self.assertIn("onnxruntime-directml==1.24.4", requirements)
        self.assertNotRegex(requirements, r"(?m)^onnxruntime==")
        self.assertIn("patch-pdf2zh-directml-layout.py", builder)

    def test_duplicate_text_fast_match_preserves_exact_threshold_decisions(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        helper_factory = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "duplicate_text_layer_helper"
        )
        factory_namespace = {}
        exec(
            compile(
                ast.Module(body=[helper_factory], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            factory_namespace,
        )

        class TranslationUnit:
            pass

        namespace = {"difflib": difflib, "TranslationUnit": TranslationUnit}
        exec(factory_namespace["duplicate_text_layer_helper"](), namespace)
        canonical = namespace["canonical_duplicate_text"]
        fast_match = namespace["duplicate_text_keys_match"]
        samples = [
            "",
            "Alpha beta gamma",
            "ALPHA, beta; gamma!",
            "Alpha beta delta",
            "a" * 78 + "b" * 22,
            "a" * 77 + "c" * 23,
            "completely unrelated text",
            "公式 {v1} 与 {v2}",
            "公式 {v1} 和 {v3}",
        ]
        for left in samples:
            for right in samples:
                left_key = canonical(left)
                right_key = canonical(right)
                expected = bool(left_key and right_key) and (
                    difflib.SequenceMatcher(
                        None, left_key, right_key, autojunk=False
                    ).ratio()
                    >= 0.78
                )
                self.assertEqual(fast_match(left_key, right_key), expected)

        units = []
        for index, text in enumerate(samples[:6] + samples[:6]):
            unit = TranslationUnit()
            unit.sourceText = text
            unit.sourceChars = len(text)
            unit.requiresTranslation = True
            unit.kind = "body"
            units.append(unit)
        original_canonical = namespace["canonical_duplicate_text"]
        canonical_calls = 0

        def counted_canonical(text: str) -> str:
            nonlocal canonical_calls
            canonical_calls += 1
            return original_canonical(text)

        namespace["canonical_duplicate_text"] = counted_canonical
        namespace["mark_duplicate_text_layer_units"](units)
        self.assertEqual(canonical_calls, len(units))

    def test_patch_uses_scalar_layout_coordinate_clamps(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        function = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "patch_converter_scalar_layout_clamp"
        )
        namespace = {}
        exec(
            compile(
                ast.Module(body=[function], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            namespace,
        )
        source = (
            "                cx, cy = np.clip(int(child.x0), 0, w - 1), "
            "np.clip(int(child.y0), 0, h - 1)\n"
        ) * 2 + (
            "                cx = np.clip(int(item.x0), 0, w - 1)\n"
            "                cy = np.clip(int(item.y0), 0, h - 1)\n"
        )

        patched, changed = namespace["patch_converter_scalar_layout_clamp"](source)

        self.assertTrue(changed)
        self.assertEqual(patched.count("cx = min(max(int(child.x0), 0), w - 1)"), 2)
        self.assertEqual(patched.count("cy = min(max(int(child.y0), 0), h - 1)"), 2)
        self.assertIn("cx = min(max(int(item.x0), 0), w - 1)", patched)
        self.assertIn("cy = min(max(int(item.y0), 0), h - 1)", patched)
        self.assertNotIn("np.clip(int(child.x0)", patched)
        self.assertNotIn("np.clip(int(item.x0)", patched)
        self.assertFalse(namespace["patch_converter_scalar_layout_clamp"](patched)[1])

    def test_patch_reuses_pdfminer_resource_manager_across_pages(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        function = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "patch_rosetta_engine_resource_manager_reuse"
        )
        namespace = {"Path": Path}
        exec(
            compile(
                ast.Module(body=[function], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            namespace,
        )

        fixture = """from pdfminer.pdfinterp import PDFResourceManager
from typing import Any

class _UnitCollectorTranslator:
    pass

class _PageCache:
    pass

def prepareRun():
    page_caches: dict[int, _PageCache] = {}
    with open('prepared.pdf', 'rb') as fp:
        for page, page_number in zip([], []):
            cache = collect_page_units(
                page=page,
                page_index=0,
                page_number=page_number,
                layout={},
                translator=collector,
                lang_in='en',
                lang_out='zh',
                thread=1,
                noto_name='noto',
                noto=None,
            )

def collect_page_units(
    page,
    page_index: int,
    page_number: int,
    layout: dict[int, Any],
    translator: _UnitCollectorTranslator,
    lang_in: str,
    lang_out: str,
    thread: int,
    noto_name: str,
    noto: Any,
) -> _PageCache:
    translator.set_page(page_number)
    before_count = len(translator.units)
    rsrcmgr = PDFResourceManager(caching=True)
    return _PageCache()
"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / "rosetta_engine.py"
            engine.write_text(fixture, encoding="utf-8")

            changed = namespace["patch_rosetta_engine_resource_manager_reuse"](root)
            self.assertTrue(changed)
            patched = engine.read_text(encoding="utf-8")
            self.assertIn("rsrcmgr = PDFResourceManager(caching=True)", patched)
            self.assertIn("rsrcmgr=rsrcmgr", patched)
            self.assertNotIn("    rsrcmgr = PDFResourceManager(caching=True)\n    return _PageCache()", patched)
            self.assertFalse(
                namespace["patch_rosetta_engine_resource_manager_reuse"](root)
            )

    def test_patch_shares_prepared_pdf_font_objects_across_pages(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        function = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "patch_rosetta_engine_shared_font_registration"
        )
        namespace = {"Path": Path}
        exec(
            compile(
                ast.Module(body=[function], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            namespace,
        )
        fixture = '''import re

def prepare_pdf_document(input_path, font_path, noto_name):
    doc = open_document(input_path)
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc
'''
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / "rosetta_engine.py"
            target.write_text(fixture, encoding="utf-8")

            self.assertTrue(namespace["patch_rosetta_engine_shared_font_registration"](root))
            patched = target.read_text(encoding="utf-8")
            self.assertIn("share prepared PDF font objects", patched)
            self.assertIn("doc[0].insert_font(font_name, font_file)", patched)
            self.assertIn("rosetta_pdf_register_page_fonts(doc, page.xref", patched)
            self.assertIn('doc.xref_set_key(resource_xref, font_key, "<<>>")', patched)
            self.assertFalse(namespace["patch_rosetta_engine_shared_font_registration"](root))

    def test_patch_subsets_fonts_in_single_page_artifacts(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        function = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "patch_rosetta_engine_page_artifact_font_subsetting"
        )
        namespace = {"Path": Path}
        exec(
            compile(
                ast.Module(body=[function], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            namespace,
        )
        fixture = '''def render_one_page(state, cache, artifact_path):
        single = pymupdf.open()
        single.insert_pdf(state.doc, from_page=cache.page_index, to_page=cache.page_index)
        single.save(
            artifact_path,
            deflate=bool(state.options.get("singlePageDeflate", False)),
            deflate_images=bool(state.options.get("singlePageDeflateImages", True)),
        )
'''
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / "rosetta_engine.py"
            target.write_text(fixture, encoding="utf-8")

            self.assertTrue(namespace["patch_rosetta_engine_page_artifact_font_subsetting"](root))
            patched = target.read_text(encoding="utf-8")
            self.assertIn("doc.subset_fonts(verbose=False)", patched)
            self.assertIn('state.options.get("singlePageSubsetFonts", True)', patched)
            self.assertIn("garbage=4 if subset_page_fonts else 0", patched)
            self.assertFalse(namespace["patch_rosetta_engine_page_artifact_font_subsetting"](root))

    def test_patch_adds_versioned_durable_layout_cache_contract(self) -> None:
        module = ast.parse(PATCH_SCRIPT.read_text(encoding="utf-8"))
        function = next(
            node
            for node in module.body
            if isinstance(node, ast.FunctionDef)
            and node.name == "patch_rosetta_engine_persistent_layout_cache"
        )
        namespace = {"Path": Path}
        exec(
            compile(
                ast.Module(body=[function], type_ignores=[]),
                str(PATCH_SCRIPT),
                "exec",
            ),
            namespace,
        )

        fixture = '''from dataclasses import asdict, dataclass
import io
from pathlib import Path

ENGINE_CONTRACT_VERSION = 2
ENGINE_VERSION = "rosetta-pdf-engine-v2"
_PREPARED_RUNS: dict[str, "_PreparedState"] = {}
_PRISTINE_PREPARED_PDFS: dict[str, bytes] = {}

@dataclass
class PreparedRun:
    preparedRunId: str
    sourcePageCount: int
    pages: list[int]
    unitCount: int
    sourceChars: int


@dataclass
class EngineCapabilities:
    engineVersion: str


def prepareRun(
    inputPdf,
    pages,
    langIn,
    langOut,
    options,
):
    selected_pages = normalize_pages(pages, page_count)
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path, selected_pages)
    prepared_pdf_path = scratch_dir / "prepared.pdf"
    doc.save(prepared_pdf_path)
    model = get_layout_model(_model_path_from_options(options))
    layout: dict[int, Any] = {}
    with prepared_pdf_path.open("rb"):
        for page_index in range(len(selected_pages)):
            layout[page_index] = build_layout_mask(doc, page_index, model, options)
    state = _PreparedState(
        prepared_run_id=prepared_run_id,
    )
    return asdict(
        PreparedRun(
            preparedRunId=prepared_run_id,
            sourcePageCount=page_count,
            pages=selected_pages,
            unitCount=translatable_unit_count(collector.units),
            sourceChars=translatable_source_chars(collector.units),
        )
    )
'''
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / "rosetta_engine.py"
            engine.write_text(fixture, encoding="utf-8")

            changed = namespace["patch_rosetta_engine_persistent_layout_cache"](root)
            self.assertTrue(changed)
            patched = engine.read_text(encoding="utf-8")
            self.assertIn('ENGINE_VERSION = "rosetta-pdf-engine-v2.1"', patched)
            self.assertIn("_PERSISTENT_LAYOUT_CACHE_SCHEMA = 1", patched)
            self.assertIn("def load_persistent_layout_cache(", patched)
            self.assertIn("allow_pickle=False", patched)
            self.assertIn("np.savez_compressed(", patched)
            self.assertIn("persistentLayoutCacheHit=persistent_layout_cache_hit", patched)
            self.assertFalse(
                namespace["patch_rosetta_engine_persistent_layout_cache"](root)
            )

    def converter_with_bold_helpers(self, text_mode_body: str = '            return "0 Tr "\\n') -> str:
        return f"""class TranslateConverter(PDFConverterEx):
    def __init__(
        self,
        rsrcmgr,
        lang_out: str = "",
        noto_name: str = "",
        noto = None,
    ) -> None:
        self.noto_name = noto_name
        self.noto = noto

    def receive_layout(self):
        def raw_string(fcur: str, cstk: str):
            if fcur == self.noto_name:
                return "".join(["%04x" % self.noto.has_glyph(ord(c)) for c in cstk])
            elif isinstance(self.fontmap[fcur], PDFCIDFont):  # 判断编码长度
                return ""

        def rosetta_pdf_is_bold_font(font):
            return True
        def rosetta_pdf_text_mode_operator(is_bold, color, size):
{text_mode_body.rstrip()}
        if fcur_ is None:
            fcur_ = self.noto_name  # 默认非拉丁字体
        if fcur_ == self.noto_name: # FIXME: change to CONST
            adv = self.noto.char_lengths(ch, size)[0]
        else:
            adv = self.fontmap[fcur_].char_width(ord(ch)) * size
"""

    def legacy_converter_text(self) -> str:
        return """            def raw_string(fcur,cstk): # 编码字符串
                if isinstance(self.fontmap[fcur],PDFCIDFont): # 判断编码长度
                    return "".join(["%04x" % ord(c) for c in cstk])
                else:
                    return "".join(["%02x" % ord(c) for c in cstk])
            _x,_y=0,0
                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False])
                            pstk[-1][5]=child.font
                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6] # 段落属性
                            ops+=f'/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
                            ops+=f"/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
                                ops+=f"ET q 1 0 0 1 {l.pts[0][0]+x-var[vid][0].x0:f} {l.pts[0][1]+fix+y-var[vid][0].y0:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
                    ops+=f"ET q 1 0 0 1 {l.pts[0][0]:f} {l.pts[0][1]:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
"""

    def render_order_drift_engine_text(self) -> str:
        return """# Rosetta: suppress duplicate PDF text layers before translation.
from dataclasses import dataclass
from pathlib import Path
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

@dataclass
class TranslationUnit:
    unitId: str
    pageNumber: int
    sourceText: str
    requiresTranslation: bool

def prepareRun(inputPdf: str, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
    return doc

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc

def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
    pass

class _RenderTranslator(_EngineTranslator):
    def __init__(
        self,
        lang_in: str,
        lang_out: str,
        expected_units: list[TranslationUnit],
        translations_by_unit_id: dict[str, str],
    ):
        super().__init__(lang_in, lang_out)
        self.current_page_number = 0
        self._orders_by_page: dict[int, int] = {}
        self.expected_by_unit_id = {unit.unitId: unit for unit in expected_units}
        self.translations_by_unit_id = translations_by_unit_id
        self.translated_unit_count = 0
        self.translated_chars = 0
        self.empty_translation_count = 0
        self.placeholder_mismatch_count = 0

    def set_page(self, page_number: int):
        self.current_page_number = page_number
        self._orders_by_page.setdefault(page_number, 0)
        self.translated_unit_count = 0
        self.translated_chars = 0
        self.empty_translation_count = 0
        self.placeholder_mismatch_count = 0

    def translate_many(self, texts, *args, **kwargs):
        outputs = []
        for text in list(texts):
            self._orders_by_page[self.current_page_number] += 1
            order = self._orders_by_page[self.current_page_number]
            unit_id = unit_id_for(self.current_page_number, order)
            expected = self.expected_by_unit_id.get(unit_id)
            if expected is None:
                raise ValueError(f"unknown translation unit requested: {unit_id}")
            if expected.sourceText != text:
                raise ValueError(f"translation unit order mismatch at {unit_id}")
            if unit_id not in self.translations_by_unit_id:
                if expected.requiresTranslation:
                    raise ValueError(f"missing translation for unit: {unit_id}")
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            translated = self.translations_by_unit_id[unit_id]
            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            outputs.append(translated)
        return outputs

    def translate(self, text, *args, **kwargs):
        return self.translate_many([text])[0]
"""

    def write_default_high_level(self, package: Path) -> None:
        (package / "high_level.py").write_text(
            """from babeldoc.assets.assets import get_font_and_metadata
from pymupdf import Font

def download_remote_fonts(lang: str):
    LANG_NAME_MAP = {
        **{
            la: f"SourceHanSerif{region}-Regular.ttf"
            for region, langs in {
                "CN": ["zh-cn", "zh-hans", "zh"],
                "TW": ["zh-tw", "zh-hant"],
            }.items()
            for la in langs
        },
    }
    font_name = LANG_NAME_MAP.get(lang, "GoNotoKurrent-Regular.ttf")
    return font_name

def translate_stream(lang_out: str):
    font_list = [("tiro", None)]

    font_path = download_remote_fonts(lang_out.lower())
    noto_name = NOTO_NAME
    noto = Font(noto_name, font_path)
    font_list.append((noto_name, font_path))
    return font_list
"""
        )

    def write_default_rosetta_engine(self, package: Path) -> None:
        (package / "rosetta_engine.py").write_text(
            """from pathlib import Path
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

def prepareRun(inputPdf: str, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
    return doc

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc

def rosetta_placeholder_count(text: str) -> int:
    return len(re.findall(r"\\{v\\d+\\}", text))

def collect_unit(text: str):
    return dict(
                sourceChars=len(text),
    )

def record_translation(self, translated: str):
            self.translated_chars += len(translated)
"""
        )

    def run_patch(self, converter_text: str, extra_files: bool = True) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / "pdf2zh"
            package.mkdir()
            (package / "__init__.py").write_text("")
            converter = package / "converter.py"
            converter.write_text(converter_text)
            if extra_files:
                self.write_default_high_level(package)
                self.write_default_rosetta_engine(package)

            env = os.environ.copy()
            env["PYTHONPATH"] = str(root)
            subprocess.run(
                [sys.executable, str(PATCH_SCRIPT)],
                env=env,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )

            return converter.read_text()

    def run_patch_for_package(self, converter_text: str, high_level_text: str | None = None, rosetta_engine_text: str | None = None) -> dict[str, str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / "pdf2zh"
            package.mkdir()
            (package / "__init__.py").write_text("")
            (package / "converter.py").write_text(converter_text)
            if high_level_text is None:
                self.write_default_high_level(package)
            else:
                (package / "high_level.py").write_text(high_level_text)
            if rosetta_engine_text is None:
                self.write_default_rosetta_engine(package)
            else:
                (package / "rosetta_engine.py").write_text(rosetta_engine_text)

            env = os.environ.copy()
            env["PYTHONPATH"] = str(root)
            subprocess.run(
                [sys.executable, str(PATCH_SCRIPT)],
                env=env,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )

            return {
                "converter": (package / "converter.py").read_text(),
                "high_level": (package / "high_level.py").read_text(),
                "rosetta_engine": (package / "rosetta_engine.py").read_text(),
            }

    def test_patch_preserves_color_and_marks_bold_paragraphs(self) -> None:
        patched = self.run_patch(self.legacy_converter_text())

        self.assertIn("rosetta_pdf_color_operator", patched)
        self.assertIn("rosetta_pdf_is_bold_font", patched)
        self.assertIn("rosetta_pdf_text_mode_operator(pbold,pcolor,size)", patched)
        self.assertIn("pstk[-1][8]=pstk[-1][8] or re.match", patched)
        self.assertIn('return "0 Tr "', patched)
        self.assertNotIn("stroke_width =", patched)
        self.assertNotIn("w 2 Tr", patched)
        self.assertNotIn("stroke_width = max(0.04, min(0.16, size * 0.006))", patched)
        self.assertNotIn("stroke_width = max(0.12, min(0.45, size * 0.018))", patched)
        self.assertNotIn("rosetta_pdf_is_bold_font(child.font)", patched)

    def test_patch_caches_bold_font_across_page_converters(self) -> None:
        patched = self.run_patch(self.converter_with_bold_helpers())

        self.assertIn("def rosetta_pdf_cached_bold_font", patched)
        self.assertIn(
            "self.rosetta_noto_bold_name, self.rosetta_noto_bold = rosetta_pdf_cached_bold_font()",
            patched,
        )
        self.assertEqual(patched.count('get_font_and_metadata("SourceHanSansCN-Bold.ttf")'), 1)

    def test_legacy_converter_patch_path_applies_render_order_drift_matching(self) -> None:
        files = self.run_patch_for_package(
            self.legacy_converter_text(),
            rosetta_engine_text=self.render_order_drift_engine_text(),
        )

        patched = files["rosetta_engine"]
        self.assertIn("tolerate replay translate_many order drift", patched)
        self.assertIn("expected = self._match_expected_unit(unit_id, text)", patched)
        self.assertIn("self._consumed_unit_ids", patched)
        self.assertNotIn("expected.sourceText != text", patched)

    def test_patch_preserves_color_and_bold_for_paragraph_ops_converter(self) -> None:
        patched = self.run_patch("""class TranslateConverter(PDFConverterEx):
    def __init__(
        self,
        rsrcmgr,
        lang_out: str = "",
        noto_name: str = "",
        noto = None,
    ) -> None:
        self.noto_name = noto_name
        self.noto = noto

    def receive_layout(self):
        def raw_string(fcur: str, cstk: str):
            if fcur == self.noto_name:
                return "".join(["%04x" % self.noto.has_glyph(ord(c)) for c in cstk])
            elif isinstance(self.fontmap[fcur], PDFCIDFont):  # 判断编码长度
                return ""

        if fcur_ is None:
            fcur_ = self.noto_name  # 默认非拉丁字体
        if fcur_ == self.noto_name: # FIXME: change to CONST
            adv = self.noto.char_lengths(ch, size)[0]
        else:
            adv = self.fontmap[fcur_].char_width(ord(ch)) * size

class Paragraph:
    def __init__(self, y, x, x0, x1, y0, y1, size, brk):
        self.y: float = y  # 初始纵坐标
        self.x: float = x  # 初始横坐标
        self.x0: float = x0  # 左边界
        self.x1: float = x1  # 右边界
        self.y0: float = y0  # 上边界
        self.y1: float = y1  # 下边界
        self.size: float = size  # 字体大小
        self.brk: bool = brk  # 换行标记

        def vflag(font: str, char: str):    # 匹配公式（和角标）字体
            pass
                        pstk.append(Paragraph(child.y0, child.x0, child.x0, child.x0, child.y0, child.y1, child.size, False))
                        pstk[-1].size = child.size
        def gen_op_txt(font, size, x, y, rtxt):
            return f"/{font} {size:f} Tf 1 0 0 1 {x:f} {y:f} Tm [<{rtxt}>] TJ "
        def gen_op_line(x, y, xlen, ylen, linewidth):
            return f"ET q 1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "
            ops_vals: list[dict] = []
                        ops_vals.append({
                            "type": OpType.TEXT,
                            "font": fcur,
                            "size": size,
                            "x": tx,
                            "dy": 0,
                            "rtxt": raw_string(fcur, cstk),
                            "lidx": lidx
                        })
                        ops_vals.append({
                            "type": OpType.TEXT,
                            "font": self.fontid[vch.font],
                            "size": vch.size,
                            "x": x + vch.x0 - var[vid][0].x0,
                            "dy": fix + vch.y0 - var[vid][0].y0,
                            "rtxt": raw_string(self.fontid[vch.font], vc),
                            "lidx": lidx
                        })
                            ops_vals.append({
                                "type": OpType.LINE,
                                "x": l.pts[0][0] + x - var[vid][0].x0,
                                "dy": l.pts[0][1] + fix - var[vid][0].y0,
                                "linewidth": l.linewidth,
                                "xlen": l.pts[1][0] - l.pts[0][0],
                                "ylen": l.pts[1][1] - l.pts[0][1],
                                "lidx": lidx
                            })
                ops_vals.append({
                    "type": OpType.TEXT,
                    "font": fcur,
                    "size": size,
                    "x": tx,
                    "dy": 0,
                    "rtxt": raw_string(fcur, cstk),
                    "lidx": lidx
                })
                    ops_list.append(gen_op_txt(vals["font"], vals["size"], vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["rtxt"]))
                    ops_list.append(gen_op_line(vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["xlen"], vals["ylen"], vals["linewidth"]))
                ops_list.append(gen_op_line(l.pts[0][0], l.pts[0][1], l.pts[1][0] - l.pts[0][0], l.pts[1][1] - l.pts[0][1], l.linewidth))
""")

        self.assertIn("color=None, bold=False", patched)
        self.assertIn("self.color = color", patched)
        self.assertIn("rosetta_pdf_color_operator", patched)
        self.assertIn("rosetta_pdf_text_mode_operator", patched)
        self.assertIn("self.rosetta_noto_bold_name", patched)
        self.assertIn("SourceHanSansCN-Bold.ttf", patched)
        self.assertIn("pstk[id].bold and self.rosetta_noto_bold is not None", patched)
        self.assertIn("self.rosetta_noto_bold.char_lengths", patched)
        self.assertIn("self.rosetta_noto_bold.has_glyph", patched)
        self.assertIn("child.graphicstate.ncolor", patched)
        self.assertIn("pstk[-1].color = child.graphicstate.ncolor", patched)
        self.assertIn("pstk[-1].bold = pstk[-1].bold or re.match", patched)
        self.assertIn('"color": pstk[id].color', patched)
        self.assertIn('"color": vch.graphicstate.ncolor', patched)
        self.assertIn('"color": l.stroking_color', patched)
        self.assertIn('vals.get("color"), vals.get("bold", False)', patched)
        self.assertIn("l.linewidth, l.stroking_color", patched)
        self.assertIn("rosetta_pdf_fill_rect", patched)
        self.assertIn('return "0 Tr "', patched)
        self.assertNotIn("stroke_width =", patched)
        self.assertNotIn("w 2 Tr", patched)

    def test_patch_hardens_current_converter_text_masking_and_cjk_line_spacing(self) -> None:
        patched = self.run_patch(
            self.converter_with_bold_helpers()
            + '''        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return f"ET q {rosetta_pdf_color_operator(color, True)}1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "

            ops_vals: list[dict] = []
            line_height = default_line_height

            while (lidx + 1) * size * line_height > height and line_height >= 1:
                line_height -= 0.05

            for vals in ops_vals:
                if vals["type"] == OpType.TEXT:
                    ops_list.append(gen_op_txt(vals["font"], vals["size"], vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["rtxt"], vals.get("color"), vals.get("bold", False)))
                elif vals["type"] == OpType.LINE:
                    ops_list.append(gen_op_line(vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["xlen"], vals["ylen"], vals["linewidth"], vals.get("color")))
'''
        )

        self.assertIn("rosetta_pdf_fill_rect", patched)
        self.assertIn("Rosetta: erase source text under translated paragraphs", patched)
        self.assertIn("self.should_translate_text(sstk[id])", patched)
        self.assertIn("min_line_height = 1.2", patched)
        self.assertIn("render_size = max(min_render_size, min(size, fit_size))", patched)
        self.assertIn('draw_size = min(draw_size, render_size)', patched)
        self.assertNotIn("line_height -= 0.05", patched)

    def test_patch_narrows_formula_detection_for_visual_prose_text(self) -> None:
        patched = self.run_patch(
            self.converter_with_bold_helpers()
            + '''        ############################################################
        # A. 原文档解析
        for child in ltpage:
            if re.match(                                            # latex 字体
                    r"(CM[^R]|MS.M|XY|MT|BL|RM|EU|LA|RS|LINE|LCIRCLE|TeX-|rsfs|txsy|wasy|stmary|.*Mono|.*Code|.*Ital|.*Sym|.*Math)",
                    font,
                ):
                    return True
                if (                                                                                        # 判定当前字符是否属于公式
                    cls == 0                                                                                # 1. 类别为保留区域
                    or (cls == xt_cls and len(sstk[-1].strip()) > 1 and child.size < pstk[-1].size * 0.79)  # 2. 角标字体，有 0.76 的角标和 0.799 的大写，这里用 0.79 取中，同时考虑首字母放大的情况
'''
        )

        self.assertNotIn(".*Ital|.*Sym", patched)
        self.assertIn(".*Code|.*Sym", patched)
        self.assertIn("rosetta_text_like_visual_char", patched)
        self.assertIn("rosetta_allow_text_like_visual_chars", patched)
        self.assertIn("rosetta_text_like_visual_chars_enabled", patched)
        self.assertIn("metric_hits >= 3", patched)
        self.assertIn("numeric_tokens >= 40", patched)
        self.assertIn("algorithm_hits", patched)
        self.assertIn("math_table_signal_hits", patched)
        self.assertIn("compact_table_signal_hits", patched)
        self.assertIn("dataset_table_signal_hits", patched)
        self.assertIn("numeric_tokens >= 40 and math_table_signal_hits >= 2", patched)
        self.assertIn("numeric_tokens >= 18 and compact_table_signal_hits >= 3", patched)
        self.assertIn("numeric_tokens >= 12 and dataset_table_signal_hits >= 3", patched)
        self.assertIn("DatasetCategoryTrainValTest", patched)
        self.assertIn("QianFSD", patched)
        self.assertIn('"Algorithm" in compact', patched)
        self.assertIn("without_decimal_points", patched)
        self.assertIn("(cls == 0 and not rosetta_text_like_visual_char)", patched)

    def test_patch_centers_only_page_centered_single_line_paragraphs(self) -> None:
        converter = self.converter_with_bold_helpers() + '''
        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return ""

        def rosetta_pdf_fill_rect(x0, y0, x1, y1, pad):
            left = min(x0, x1) - pad
            bottom = min(y0, y1) - pad
            width = abs(x1 - x0) + pad * 2
            height = abs(y1 - y0) + pad * 2
            return f"ET q 1 g {left:f} {bottom:f} {width:f} {height:f} re f Q BT "

            ops_vals: list[dict] = []
            # Rosetta: erase source text under translated paragraphs and keep CJK line spacing legible.
'''
        patched = self.run_patch(converter)

        marker = "        def rosetta_pdf_centered_alignment_shift("
        helper_start = patched.index(marker)
        helper_end = patched.index("            ops_vals: list[dict] = []")
        namespace: dict[str, object] = {"re": re}
        exec(patched[helper_start:helper_end].replace("        def ", "def ", 1), namespace)
        shift = namespace["rosetta_pdf_centered_alignment_shift"]

        self.assertAlmostEqual(shift(0, 612, 129.77, 482.22, 129.77, 354.75, 24, False, 1), 63.74, places=1)
        self.assertEqual(shift(0, 612, 72, 300, 72, 220, 12, False, 1), 0.0)
        self.assertEqual(shift(0, 612, 50, 562, 50, 300, 12, False, 1), 0.0)
        self.assertEqual(shift(0, 612, 129.77, 482.22, 129.77, 354.75, 24, True, 2), 0.0)
        self.assertIn("for vals in ops_vals:", patched)
        self.assertIn('vals["x"] += alignment_shift', patched)

    def test_patch_preserves_structural_but_not_soft_pdf_line_breaks(self) -> None:
        converter = self.converter_with_bold_helpers() + '''
class Paragraph:
    def __init__(self, brk, color=None):
        self.brk: bool = brk  # 换行标记
        self.color = color

        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return f"ET q {rosetta_pdf_color_operator(color, True)}1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "

                        elif child.x1 < xt.x0:      # 添加换行空格并标记原文段落存在换行
                            sstk[-1] += " "
                            pstk[-1].brk = True

        ############################################################
        # B. 段落翻译
            ops_vals: list[dict] = []
                mod = 0  # 文字修饰符
                if vy_regex:  # 加载公式
                        vid = int(vy_regex.group(1).replace(" ", ""))
                        adv = vlen[vid]
                    if var[vid][-1].get_text() and unicodedata.category(var[vid][-1].get_text()[0]) in ["Lm", "Mn", "Sk"]:  # 文字修饰符
                        mod = var[vid][-1].width
                if brk and x + adv > x1 + 0.1 * size:  # 到达右边界且原文段落存在换行
                    x = x0
'''
        files = self.run_patch_for_package(converter)
        patched = files["converter"]

        marker = "        def rosetta_pdf_should_preserve_source_line_breaks("
        helper_start = patched.index(marker)
        helper_end = patched.index("        for paragraph_id, paragraph in enumerate(pstk):")
        namespace: dict[str, object] = {"re": re}
        exec(patched[helper_start:helper_end].replace("        def ", "def ", 1), namespace)
        should_preserve = namespace["rosetta_pdf_should_preserve_source_line_breaks"]

        class ParagraphState:
            x0 = 43.0
            x1 = 400.0
            size = 14.0

        toc = ParagraphState()
        toc.rosetta_line_breaks = [(10, 120.0), (20, 215.0), (30, 400.0)]
        prose = ParagraphState()
        prose.x0 = 77.0
        prose.x1 = 535.0
        prose.rosetta_line_breaks = [(10, 530.0), (20, 535.0), (30, 526.0), (40, 531.0)]

        self.assertTrue(should_preserve(toc, "Title Page Gateway Introduction Preface Introduction"))
        self.assertFalse(should_preserve(prose, "A normal prose paragraph with visual soft wraps."))
        self.assertFalse(should_preserve(toc, "Input {v0} Conv {v1} Output {v2}"))
        self.assertIn('"{v900000000}"', patched)
        self.assertIn("if rosetta_forced_line_break:", patched)
        self.assertIn("if not rosetta_forced_line_break and var[vid]", patched)
        self.assertIn('placeholder != "{v900000000}"', files["rosetta_engine"])
        self.assertIn('sourceChars=len(text.replace("{v900000000}", ""))', files["rosetta_engine"])
        self.assertIn('self.translated_chars += len(translated.replace("{v900000000}", ""))', files["rosetta_engine"])

    def test_patch_upgrades_existing_visual_prose_gate(self) -> None:
        patched = self.run_patch(
            self.converter_with_bold_helpers()
            + '''        ############################################################
        # A. 原文档解析
        for child in ltpage:
                # Rosetta: table/legal-prose PDFs often put normal text in visual regions.
                rosetta_text_like_visual_char = (
                    cls == 0
                    and bool(child.get_text())
                    and (
                        child.get_text().isalnum()
                        or child.get_text().isspace()
                        or child.get_text() in ".,;:!?()[]{}<>/\\\\'\\\"-–—&$%#@*+=|"
                    )
                )
                if (                                                                                        # 判定当前字符是否属于公式
                    (cls == 0 and not rosetta_text_like_visual_char)                                         # 1. 类别为保留区域
'''
        )

        self.assertIn("rosetta_allow_text_like_visual_chars", patched)
        self.assertIn("rosetta_text_like_visual_chars_enabled", patched)
        self.assertIn("and rosetta_text_like_visual_chars_enabled", patched)
        self.assertIn("without_decimal_points", patched)
        self.assertIn("numeric_tokens >= 40", patched)
        self.assertIn("algorithm_hits", patched)
        self.assertIn("math_table_signal_hits", patched)
        self.assertIn("compact_table_signal_hits", patched)
        self.assertIn("dataset_table_signal_hits", patched)
        self.assertIn("numeric_tokens >= 12 and dataset_table_signal_hits >= 3", patched)

    def test_patch_upgrades_existing_visual_table_gate_for_dataset_tables(self) -> None:
        patched = self.run_patch(
            self.converter_with_bold_helpers()
            + '''        def rosetta_allow_text_like_visual_chars(ltpage: LTPage) -> bool:
            compact = "DatasetCategoryTrainValTest FarmInsects IP102 QianFSD 143 4938 705 1411"
            metric_hits = 0
            numeric_tokens = 16
            without_decimal_points = compact
            sentence_marks = 0
            algorithm_hits = 0
            math_table_signal_hits = 0
            compact_table_signal_hits = 0
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
                return False
            return True

        rosetta_text_like_visual_chars_enabled = rosetta_allow_text_like_visual_chars(ltpage)
        ############################################################
        # A. 原文档解析
        for child in ltpage:
                rosetta_text_like_visual_char = (
                    cls == 0
                    and rosetta_text_like_visual_chars_enabled
                    and bool(child.get_text())
                )
'''
        )

        self.assertIn("dataset_table_signal_hits", patched)
        self.assertIn("DatasetCategoryTrainValTest", patched)
        self.assertIn("QianFSD", patched)
        self.assertIn("numeric_tokens >= 12 and dataset_table_signal_hits >= 3", patched)
        self.assertLess(
            patched.index("dataset_table_signal_hits = sum("),
            patched.index("numeric_tokens >= 12 and dataset_table_signal_hits >= 3"),
        )

    def test_patch_replaces_existing_faux_bold_text_stroke_with_font_switch(self) -> None:
        for stroke_width in [
            "stroke_width = max(0.12, min(0.45, size * 0.018))",
            "stroke_width = max(0.04, min(0.16, size * 0.006))",
            "stroke_width = min(0.03, max(0.0, size * 0.0015))",
        ]:
            with self.subTest(stroke_width=stroke_width):
                patched = self.run_patch(
                    self.converter_with_bold_helpers(
                        f"""            if not is_bold:
                return "0 Tr "
            {stroke_width}
            return f"{{stroke_width:f}} w 2 Tr "
"""
                    )
                )

                self.assertIn('return "0 Tr "', patched)
                self.assertIn("self.rosetta_noto_bold_name", patched)
                self.assertIn("pstk[id].bold and self.rosetta_noto_bold is not None", patched)
                self.assertNotIn("stroke_width =", patched)
                self.assertNotIn("w 2 Tr", patched)
                self.assertNotIn("stroke_width = max(0.12, min(0.45, size * 0.018))", patched)
                self.assertNotIn("stroke_width = max(0.04, min(0.16, size * 0.006))", patched)

    def test_patch_uses_source_han_sans_for_simplified_chinese_pdf_output(self) -> None:
        patched = self.run_patch_for_package(
            """        def rosetta_pdf_is_bold_font(font):
            return True
        def rosetta_pdf_text_mode_operator(is_bold, color, size):
            return "0 Tr "
""",
            """def download_remote_fonts(lang: str):
    lang = lang.lower()
    LANG_NAME_MAP = {
        **{
            la: f"SourceHanSerif{region}-Regular.ttf"
            for region, langs in {
                "CN": ["zh-cn", "zh-hans", "zh"],
                "TW": ["zh-tw", "zh-hant"],
                "JP": ["ja"],
                "KR": ["ko"],
            }.items()
            for la in langs
        },
    }
    font_name = LANG_NAME_MAP.get(lang, "GoNotoKurrent-Regular.ttf")
    return font_name
"""
        )["high_level"]

        self.assertIn("prefer Source Han Sans", patched)
        self.assertIn('"zh": "SourceHanSansCN-Regular.ttf"', patched)
        self.assertIn('"zh-cn": "SourceHanSansCN-Regular.ttf"', patched)
        self.assertIn('"zh-hans": "SourceHanSansCN-Regular.ttf"', patched)
        self.assertIn('font_name = LANG_NAME_MAP.get(lang, "GoNotoKurrent-Regular.ttf")', patched)

    def test_patch_registers_bold_font_in_high_level_and_rosetta_engine(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers()
        )

        self.assertIn('font_list.append(("notobold", rosetta_bold_path.as_posix()))', files["high_level"])
        self.assertIn('get_font_and_metadata("SourceHanSansCN-Bold.ttf")', files["high_level"])
        self.assertIn("from babeldoc.assets.assets import get_font_and_metadata", files["rosetta_engine"])
        self.assertIn("rosetta_bold_font_path = None", files["rosetta_engine"])
        self.assertIn("prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path)", files["rosetta_engine"])
        self.assertIn('font_list.append(("notobold", bold_font_path))', files["rosetta_engine"])

    def test_patch_prepares_only_selected_pages_in_rosetta_engine(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            rosetta_engine_text="""from pathlib import Path
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

def prepareRun(inputPdf: str, pages: list[int] | None, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    rosetta_bold_font_path = None
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path)
    page_count = doc.page_count
    selected_pages = normalize_pages(pages, page_count)
    prepared_pdf_path = scratch_dir / "prepared.pdf"
    doc.save(prepared_pdf_path)
    with open(prepared_pdf_path, "rb") as fp:
        pdf_pages = list(
            PDFPage.get_pages(
                fp,
                [page - 1 for page in selected_pages],
                maxpages=0,
                password="",
                caching=True,
            )
        )
        for page, page_number in zip(pdf_pages, selected_pages):
            page_index = page_number - 1
            page.pageno = page_index
            layout[page_index] = build_layout_mask(doc, page_index, model, options)
    return PreparedRun(sourcePageCount=page_count, pages=selected_pages)

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str, bold_font_path: str | None = None):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    if bold_font_path:
        font_list.append(("notobold", bold_font_path))
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc
""",
        )

        patched = files["rosetta_engine"]
        self.assertIn("prepare only selected PDF pages", patched)
        self.assertIn("page_count = source_doc.page_count", patched)
        self.assertIn("selected_pages = normalize_pages(pages, page_count)", patched)
        self.assertIn("prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path, selected_pages)", patched)
        self.assertIn("list(range(len(selected_pages)))", patched)
        self.assertIn("for prepared_page_index, (page, page_number) in enumerate(zip(pdf_pages, selected_pages))", patched)
        self.assertIn("page_index = prepared_page_index", patched)
        self.assertIn("selected_pages: list[int] | None = None", patched)
        self.assertIn("doc.insert_pdf(source_doc, from_page=page_number - 1, to_page=page_number - 1)", patched)

    def test_patch_adds_pristine_prepared_run_reset_contract(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            rosetta_engine_text="""from dataclasses import dataclass
from pathlib import Path
import pymupdf
from babeldoc.assets.assets import get_font_and_metadata
from pdf2zh.converter import TranslateConverter

ENGINE_CONTRACT_VERSION = 2
_PREPARED_RUNS: dict[str, "_PreparedState"] = {}

@dataclass
class _PreparedState:
    doc: object

def _bold_registration_marker(font_list, bold_font_path):
    rosetta_bold_font_path = None
    font_list.append(("notobold", bold_font_path))

def prepareRun(inputPdf: str):
    prepared_run_id = "prepared-1"
    prepared_pdf_path = Path(inputPdf)
    state = _PreparedState(doc=pymupdf.open(str(prepared_pdf_path)))
    _PREPARED_RUNS[prepared_run_id] = state
    return {"preparedRunId": prepared_run_id}

def prepared_state(preparedRunId: str) -> _PreparedState:
    return _PREPARED_RUNS[preparedRunId]

def disposeRun(preparedRunId: str) -> None:
    state = _PREPARED_RUNS.pop(preparedRunId, None)
    if state is None:
        return
    state.doc.close()
""",
        )

        patched = files["rosetta_engine"]
        self.assertIn('_PRISTINE_PREPARED_PDFS: dict[str, bytes] = {}', patched)
        self.assertIn(
            "_PRISTINE_PREPARED_PDFS[prepared_run_id] = prepared_pdf_path.read_bytes()",
            patched,
        )
        self.assertIn("def resetRun(preparedRunId: str) -> None:", patched)
        self.assertIn("_PRISTINE_PREPARED_PDFS.pop(preparedRunId, None)", patched)

        reset_start = patched.index("def resetRun(")
        reset_end = patched.index("def disposeRun(", reset_start)

        class FakeDoc:
            def __init__(self, content: bytes):
                self.content = content
                self.closed = False

            def close(self):
                self.closed = True

        class FakePymupdf:
            @staticmethod
            def open(*, stream: bytes, filetype: str):
                self.assertEqual(filetype, "pdf")
                return FakeDoc(stream)

        old_doc = FakeDoc(b"mutated")
        state = type("State", (), {"doc": old_doc})()
        namespace = {
            "pymupdf": FakePymupdf,
            "prepared_state": lambda prepared_run_id: state,
            "_PRISTINE_PREPARED_PDFS": {"prepared-1": b"pristine"},
        }
        exec(patched[reset_start:reset_end], namespace)
        namespace["resetRun"]("prepared-1")

        self.assertTrue(old_doc.closed)
        self.assertEqual(state.doc.content, b"pristine")
        self.assertFalse(state.doc.closed)

    def test_patch_filters_duplicate_text_layers_in_rosetta_engine(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            rosetta_engine_text="""from dataclasses import asdict, dataclass
from pathlib import Path
import re
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

@dataclass
class TranslationUnit:
    unitId: str
    sourceText: str
    sourceChars: int
    kind: str
    requiresTranslation: bool

def prepareRun(inputPdf: str, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
    return asdict(
        PreparedRun(
            unitCount=len(collector.units),
            sourceChars=sum(unit.sourceChars for unit in collector.units),
        )
    )

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc

def collect_page_units():
    page_units = translator.units[before_count:]
    return _PageCache(
        units=list(page_units),
    )

def render_one_page():
    source_chars = sum(unit.sourceChars for unit in cache.units)
    if not cache.units:
        return PageResult(sourceUnitCount=0)
    missing = [unit.unitId for unit in cache.units if unit.unitId not in translations_by_unit_id]
    return PageResult(
        sourceUnitCount=len(cache.units),
    )

class _RenderTranslator:
    def translate_many(self):
            if unit_id not in self.translations_by_unit_id:
                raise ValueError(f"missing translation for unit: {unit_id}")
            translated = self.translations_by_unit_id[unit_id]
            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if expected.requiresTranslation and expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1

def failed_page_result():
    return PageResult(
        sourceUnitCount=len(cache.units),
        translatedUnitCount=translated_unit_count,
    )

def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
    pass
""",
        )

        patched = files["rosetta_engine"]
        self.assertIn("import difflib", patched)
        self.assertIn("suppress duplicate PDF text layers", patched)
        self.assertIn("mark_duplicate_text_layer_units(page_units)", patched)
        self.assertIn("mark_nontranslatable_layout_units(page_units)", patched)
        self.assertIn("is_rosetta_table_like_unit", patched)
        self.assertIn("is_rosetta_formula_like_unit", patched)
        self.assertIn("is_rosetta_page_number_unit", patched)
        self.assertIn("is_rosetta_figure_panel_label_unit", patched)
        self.assertIn("is_rosetta_diagram_label_unit", patched)
        self.assertIn("text.casefold()", patched)
        self.assertIn("char.isalnum()", patched)
        self.assertIn("matcher.real_quick_ratio()", patched)
        self.assertIn("matcher.quick_ratio()", patched)
        self.assertIn("canonical_keys = [canonical_duplicate_text", patched)
        self.assertIn("pair_matches: dict[tuple[int, int], bool]", patched)
        self.assertIn("duplicate.requiresTranslation = False", patched)
        self.assertIn('duplicate.kind = "duplicate-layer"', patched)
        self.assertIn("unitCount=translatable_unit_count(collector.units)", patched)
        self.assertIn("sourceChars=translatable_source_chars(collector.units)", patched)
        self.assertIn("if unit.requiresTranslation and unit.unitId not in translations_by_unit_id", patched)
        self.assertIn("if expected.requiresTranslation:", patched)
        self.assertIn("if not expected.requiresTranslation:", patched)
        self.assertIn("rosetta_nontranslatable_render_text(expected, text)", patched)
        self.assertNotIn('outputs.append("")', patched)
        self.assertIn('if unit.kind == "duplicate-layer":', patched)
        self.assertIn('return ""', patched)

    def test_patch_marks_table_formula_and_page_number_units_nontranslatable(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            rosetta_engine_text="""from dataclasses import asdict, dataclass
from pathlib import Path
import re
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

@dataclass
class TranslationUnit:
    unitId: str
    sourceText: str
    sourceChars: int
    kind: str
    requiresTranslation: bool

def prepareRun(inputPdf: str, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
    return asdict(
        PreparedRun(
            unitCount=len(collector.units),
            sourceChars=sum(unit.sourceChars for unit in collector.units),
        )
    )

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc

def collect_page_units():
    page_units = translator.units[before_count:]
    return _PageCache(
        units=list(page_units),
    )

def render_one_page():
    source_chars = sum(unit.sourceChars for unit in cache.units)
    if not cache.units:
        return PageResult(sourceUnitCount=0)
    missing = [unit.unitId for unit in cache.units if unit.unitId not in translations_by_unit_id]
    return PageResult(
        sourceUnitCount=len(cache.units),
    )

class _RenderTranslator:
    def translate_many(self):
            if unit_id not in self.translations_by_unit_id:
                raise ValueError(f"missing translation for unit: {unit_id}")
            translated = self.translations_by_unit_id[unit_id]
            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if expected.requiresTranslation and expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1

def failed_page_result():
    return PageResult(
        sourceUnitCount=len(cache.units),
        translatedUnitCount=translated_unit_count,
    )

def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
    pass
""",
        )

        namespace: dict[str, object] = {}
        helper_start = files["rosetta_engine"].index("def rosetta_placeholder_count")
        helper_end = files["rosetta_engine"].index("def validate_translation_keys")
        exec("from __future__ import annotations\nimport re\n" + files["rosetta_engine"][helper_start:helper_end], namespace)

        table_text = (
            "Methods Year FLOPs Param. Size RIND ICCV 2021 695.77G 59.39M 453MB "
            "SFIAN TITS 2023 84.57G 13.63M 56MB SCRWKV ICML 2026 22.78G 1.22M 28MB"
        )
        formula_text = "max {v8} 2 {v9}24{v10} {v11} 1 {v12} max {v13} 2 {v14}25{v15}"
        operator_formula_text = (
            "1{v7} [0{v8}){v9} Partition(EM({v10})){v11}6{v12} "
            "{v13} TopK( {v14} 1){v15} [0{v16}){v17}7{v18} "
            "{v19} [0{v20}){v21} Partition({v22}){v23}8{v24}"
        )
        prose_text = (
            "For the TUT dataset, our method achieves SOTA performance, with F1 and mIoU "
            "reaching 0.8428 and 0.8512, respectively."
        )
        panel_label_text = (
            "(a) Comparison with SOTA methods. (c) Segmentation results in complex "
            "interference conditions. (b) Different Enhancement Modules."
        )
        caption_text = (
            "Figure 1. Performance of SCRWKV on multi-scenario TUT dataset. "
            "(a) Comparison with SOTA methods. (b) Impact of enhancement modules."
        )
        diagram_text = (
            "Conv Point Conv {v12} Concat Dilated Conv Q1 Q2 Concat DWConv "
            "Input Output DWConv Spatial Attention Point Conv Concat"
        )
        legend_text = "Raw GT RIND SFIAN CTCrackSeg DTrCNet Crackmer SCSegamba MambaIR CSMamba PlainMamba SimCrack SCRWKV"
        deployment_text = "MoveCamera Control ...... ...... Get Input Upload ...... ...... Output Process Initial Video Split Combine Processed Video Resize"
        frame_text = "Frame 001 Frame 101 Frame 201 Frame 301 Frame 401 Frame {v0}"
        formula_intro_text = (
            "To achieve an optimal equilibrium between pixel-level classification accuracy "
            "and boundary continuity, the final crack detection objective incorporates {v0}, "
            "{v1}, {v2}, {v3}, {v4}, {v5}, and {v6}, which are defined as follows:"
        )

        self.assertTrue(namespace["is_rosetta_table_like_unit"](table_text))
        self.assertTrue(namespace["is_rosetta_formula_like_unit"](formula_text))
        self.assertTrue(namespace["is_rosetta_formula_like_unit"](operator_formula_text))
        self.assertTrue(namespace["is_rosetta_page_number_unit"]("8"))
        self.assertTrue(namespace["is_rosetta_figure_panel_label_unit"](panel_label_text))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"](diagram_text, 2))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"]("Group A", 2))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"]("Inward Shift Outward Shift", 4))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"](legend_text, 1))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"](deployment_text, 1))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"](frame_text, 1))
        self.assertTrue(namespace["is_rosetta_diagram_label_unit"]("第二篇中的", 2))
        self.assertFalse(namespace["is_rosetta_table_like_unit"](prose_text))
        self.assertFalse(namespace["is_rosetta_formula_like_unit"](formula_intro_text))
        self.assertFalse(namespace["is_rosetta_figure_panel_label_unit"](caption_text))
        self.assertFalse(namespace["is_rosetta_diagram_label_unit"](caption_text, 2))
        self.assertFalse(namespace["is_rosetta_diagram_label_unit"](formula_intro_text, 4))
        self.assertFalse(namespace["is_rosetta_diagram_label_unit"](diagram_text, 5))

    def test_patch_upgrades_existing_nontranslatable_layout_helpers(self) -> None:
        files = self.run_patch_for_package(
            """def rosetta_pdf_is_bold_font(font):
    return False
""",
            high_level_text="""# Rosetta: prefer Source Han Sans for simplified Chinese PDF output.
# Rosetta: register Source Han Sans Bold for simplified Chinese PDF output.
""",
            rosetta_engine_text='''# Rosetta: suppress duplicate PDF text layers before translation.
from babeldoc.assets.assets import get_font_and_metadata
import re

class TranslationUnit:
    pass

rosetta_bold_font_path = None
font_list = [("tiro", None)]
font_list.append(("notobold", bold_font_path))

# Rosetta: prepare only selected PDF pages for translation windows.

def is_rosetta_table_like_unit(text: str) -> bool:
    return "Methods" in text

def is_rosetta_formula_like_unit(text: str) -> bool:
    compact = " ".join(text.split())
    if len(compact) > 140:
        return False
    placeholder_count = rosetta_placeholder_count(compact)
    if placeholder_count < 3:
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    return len(words) <= 5

def is_rosetta_diagram_label_unit(text: str, order_on_page: int) -> bool:
    compact = " ".join(text.split())
    if order_on_page > 4:
        return False
    if not compact or len(compact) > 480:
        return False
    placeholder_count = rosetta_placeholder_count(compact)
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    words = re.findall(r"[A-Za-z]{2,}", compact)
    label_hits = len(
        re.findall(
            r"\\b(?:Raw|GT|Conv|DWConv|Point|Dilated|Input|Output|Concat|Upsample|Layer|Norm|softmax|dropout|Attention|Shift|Graph[A-Z]?|Focus|Features?|SCIU|BLOCK|RIND|SFIAN|CTCrackSeg|DTrCNet|Crackmer|SCSegamba|MambaIR|CSMamba|PlainMamba|SimCrack|SCRWKV)\\b",
            compact,
        )
    )
    if "...." in compact and placeholder_count >= 1:
        return True
    if label_hits >= 4 and sentence_marks <= 3:
        return True
    if placeholder_count >= 3 and len(words) <= 45 and sentence_marks <= 4:
        return True
    return False

def mark_nontranslatable_layout_units(units: list[TranslationUnit]) -> None:
    for unit in units:
        if not unit.requiresTranslation:
            continue
        text = unit.sourceText.strip()
        if text == "8":
            unit.requiresTranslation = False
            unit.kind = "page-number"
        elif is_rosetta_table_like_unit(text):
            unit.requiresTranslation = False
            unit.kind = "table-like"

def collect_page_units():
    page_units = translator.units[before_count:]
    mark_duplicate_text_layer_units(page_units)
    mark_nontranslatable_layout_units(page_units)
    return page_units

def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
    pass
''',
        )

        patched = files["rosetta_engine"]
        self.assertIn("def is_rosetta_figure_panel_label_unit", patched)
        self.assertIn("def is_rosetta_diagram_label_unit", patched)
        self.assertIn("operator_hits = len(re.findall", patched)
        self.assertIn("Partition|TopK|Gumbel|Softmax|Flatten|EM|LN|FFN|CR", patched)
        self.assertIn("Attention|Inward|Outward|Shift", patched)
        self.assertIn("MoveCamera|Camera|Control|Get|Upload", patched)
        self.assertIn('re.search(r"[\\u4e00-\\u9fff]", compact)', patched)
        self.assertIn("label_hits >= 2 and len(words) <= 8", patched)
        self.assertIn('"...." in compact and label_hits >= 2', patched)
        self.assertIn("placeholder_count >= 3 and label_hits >= 2", patched)
        self.assertNotIn("placeholder_count >= 3 and len(words) <= 45", patched)
        self.assertIn("unit.kind = \"figure-panel-labels\"", patched)
        self.assertIn("unit.kind = \"diagram-label\"", patched)
        self.assertIn("def rosetta_nontranslatable_render_text", patched)

    def test_patch_upgrades_existing_nonrequired_render_to_preserve_source_text(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            high_level_text="""# Rosetta: prefer Source Han Sans for simplified Chinese PDF output.
# Rosetta: register Source Han Sans Bold for simplified Chinese PDF output.
""",
            rosetta_engine_text='''# Rosetta: suppress duplicate PDF text layers before translation.
from babeldoc.assets.assets import get_font_and_metadata

class TranslationUnit:
    pass

rosetta_bold_font_path = None
font_list = [("tiro", None)]
font_list.append(("notobold", bold_font_path))

# Rosetta: prepare only selected PDF pages for translation windows.

def rosetta_nontranslatable_render_text(unit: TranslationUnit, text: str) -> str:
    if unit.kind == "duplicate-layer":
        return ""
    return text

def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
    pass

class _RenderTranslator:
    def translate_many(self, texts, *args, **kwargs):
        outputs = []
        for text in list(texts):
            unit_id = "p0001-u0001"
            expected = self.expected_by_unit_id[unit_id]
            if unit_id not in self.translations_by_unit_id:
                if expected.requiresTranslation:
                    raise ValueError(f"missing translation for unit: {unit_id}")
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            translated = self.translations_by_unit_id[unit_id]
            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if not expected.requiresTranslation:
                outputs.append("")
                continue
            outputs.append(translated)
        return outputs
''',
        )

        patched = files["rosetta_engine"]
        self.assertIn("outputs.append(rosetta_nontranslatable_render_text(expected, text))", patched)
        self.assertNotIn('outputs.append("")', patched)

    def test_patch_tolerates_render_translate_many_order_drift(self) -> None:
        files = self.run_patch_for_package(
            self.converter_with_bold_helpers(),
            rosetta_engine_text="""from pathlib import Path
import pymupdf
from pdf2zh.converter import TranslateConverter
from pdf2zh.high_level import NOTO_NAME, download_remote_fonts

def prepareRun(inputPdf: str, langOut: str):
    input_path = Path(inputPdf)
    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
    return doc

def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):
    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
    return doc

class TranslationUnit:
    pass

class _RenderTranslator(_EngineTranslator):
    def __init__(
        self,
        lang_in: str,
        lang_out: str,
        expected_units: list[TranslationUnit],
        translations_by_unit_id: dict[str, str],
    ):
        super().__init__(lang_in, lang_out)
        self.current_page_number = 0
        self._orders_by_page: dict[int, int] = {}
        self.expected_by_unit_id = {unit.unitId: unit for unit in expected_units}
        self.translations_by_unit_id = translations_by_unit_id
        self.translated_unit_count = 0
        self.translated_chars = 0
        self.empty_translation_count = 0
        self.placeholder_mismatch_count = 0

    def set_page(self, page_number: int):
        self.current_page_number = page_number
        self._orders_by_page.setdefault(page_number, 0)
        self.translated_unit_count = 0
        self.translated_chars = 0
        self.empty_translation_count = 0
        self.placeholder_mismatch_count = 0

    def translate_many(self, texts, *args, **kwargs):
        outputs = []
        for text in list(texts):
            self._orders_by_page[self.current_page_number] += 1
            order = self._orders_by_page[self.current_page_number]
            unit_id = unit_id_for(self.current_page_number, order)
            expected = self.expected_by_unit_id.get(unit_id)
            if expected is None:
                raise ValueError(f"unknown translation unit requested: {unit_id}")
            if expected.sourceText != text:
                raise ValueError(f"translation unit order mismatch at {unit_id}")
            if unit_id not in self.translations_by_unit_id:
                if expected.requiresTranslation:
                    raise ValueError(f"missing translation for unit: {unit_id}")
                outputs.append("")
                continue
            translated = self.translations_by_unit_id[unit_id]
            outputs.append(translated)
        return outputs

    def translate(self, text, *args, **kwargs):
        return self.translate_many([text])[0]
""",
        )

        patched = files["rosetta_engine"]
        self.assertIn("tolerate replay translate_many order drift", patched)
        self.assertIn("self.expected_by_page", patched)
        self.assertIn("self._consumed_unit_ids", patched)
        self.assertIn("def _match_expected_unit", patched)
        self.assertIn("expected = self._match_expected_unit(unit_id, text)", patched)
        self.assertNotIn("expected.sourceText != text", patched)

    def test_release_pack_builders_apply_pdf_converter_patch(self) -> None:
        builders = [
            SCRIPT_DIR / "build-pdf2zh-pack-macos-arm64.sh",
            SCRIPT_DIR / "build-pdf2zh-pack-windows-amd64.ps1",
            SCRIPT_DIR / "stage-pdf2zh-pack-local.sh",
        ]

        for builder in builders:
            with self.subTest(builder=builder.name):
                text = builder.read_text()
                self.assertIn("patch-pdf2zh-color-preservation.py", text)

    def test_macos_pack_builders_pin_tencentcloud_tmt_import_compatible_version(self) -> None:
        builders = [
            SCRIPT_DIR / "build-pdf2zh-pack-macos-arm64.sh",
            SCRIPT_DIR / "stage-pdf2zh-pack-local.sh",
        ]

        for builder in builders:
            with self.subTest(builder=builder.name):
                text = builder.read_text()
                self.assertIn("tencentcloud-sdk-python-tmt==3.1.121", text)

    def test_release_pack_builders_stage_babeldoc_font_assets(self) -> None:
        builders = [
            SCRIPT_DIR / "build-pdf2zh-pack-macos-arm64.sh",
            SCRIPT_DIR / "build-pdf2zh-pack-windows-amd64.ps1",
            SCRIPT_DIR / "stage-pdf2zh-pack-local.sh",
        ]

        for builder in builders:
            with self.subTest(builder=builder.name):
                text = builder.read_text()
                self.assertIn("stage-pdf2zh-font-assets.py", text)
                self.assertIn("ROSETTA_BABELDOC_CACHE_DIR", text)
                self.assertIn("SourceHanSansCN-Regular.ttf", text)
                self.assertIn("SourceHanSansCN-Bold.ttf", text)
                self.assertIn("GoNotoKurrent-Regular.ttf", text)

    def test_local_archive_checks_current_onnx_model_and_fonts(self) -> None:
        text = (SCRIPT_DIR / "archive-pdf2zh-pack-local.sh").read_text()

        self.assertIn("doclayout_yolo_docstructbench_imgsz1024.onnx", text)
        self.assertNotIn("doclayout_yolo_docstructbench_imgsz1024.pt", text)
        self.assertIn("assets/babeldoc/fonts/$font_name", text)

    def test_font_asset_script_patches_babeldoc_cache_folder_env(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / "babeldoc"
            package.mkdir()
            (package / "__init__.py").write_text("")
            (package / "const.py").write_text(
                """import os
from pathlib import Path

CACHE_FOLDER = Path.home() / ".cache" / "babeldoc"
"""
            )

            env = os.environ.copy()
            env["PYTHONPATH"] = str(root)
            subprocess.run(
                [sys.executable, str(FONT_ASSETS_SCRIPT), "--patch-cache-env-only"],
                env=env,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )

            patched = (package / "const.py").read_text()
            self.assertIn("ROSETTA_BABELDOC_CACHE_DIR", patched)
            self.assertIn("allow the PDF component pack to own BabelDOC assets", patched)


if __name__ == "__main__":
    unittest.main()
