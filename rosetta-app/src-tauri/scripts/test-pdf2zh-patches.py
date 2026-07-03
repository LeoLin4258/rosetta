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
    def run_patch(self, converter_text: str) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / "pdf2zh"
            package.mkdir()
            (package / "__init__.py").write_text("")
            converter = package / "converter.py"
            converter.write_text(converter_text)

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
        self.assertIn('return "0 Tr "', patched)
        self.assertIn('w 2 Tr "', patched)
        self.assertNotIn("rosetta_pdf_is_bold_font(child.font)", patched)

    def test_patch_preserves_color_and_bold_for_paragraph_ops_converter(self) -> None:
        patched = self.run_patch("""class Paragraph:
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
        self.assertIn("child.graphicstate.ncolor", patched)
        self.assertIn("pstk[-1].color = child.graphicstate.ncolor", patched)
        self.assertIn('"color": pstk[id].color', patched)
        self.assertIn('"color": vch.graphicstate.ncolor', patched)
        self.assertIn('"color": l.stroking_color', patched)
        self.assertIn('vals.get("color"), vals.get("bold", False)', patched)
        self.assertIn("l.linewidth, l.stroking_color", patched)


if __name__ == "__main__":
    unittest.main()
