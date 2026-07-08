#!/usr/bin/env python3
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
PATCH_SCRIPT = SCRIPT_DIR / "patch-pdf2zh-color-preservation.py"


class Pdf2zhPatchTests(unittest.TestCase):
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
        patched = self.run_patch("""            def raw_string(fcur,cstk): # 编码字符串
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
""")

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
        self.assertIn('return "0 Tr "', patched)
        self.assertNotIn("stroke_width =", patched)
        self.assertNotIn("w 2 Tr", patched)

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


if __name__ == "__main__":
    unittest.main()
