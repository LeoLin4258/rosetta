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
    def test_patch_preserves_color_and_marks_bold_paragraphs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / "pdf2zh"
            package.mkdir()
            (package / "__init__.py").write_text("")
            converter = package / "converter.py"
            converter.write_text("""            def raw_string(fcur,cstk): # 编码字符串
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

            patched = converter.read_text()
            self.assertIn("rosetta_pdf_color_operator", patched)
            self.assertIn("rosetta_pdf_is_bold_font", patched)
            self.assertIn("rosetta_pdf_text_mode_operator(pbold,pcolor,size)", patched)
            self.assertIn('return "0 Tr "', patched)
            self.assertIn('w 2 Tr "', patched)
            self.assertNotIn("rosetta_pdf_is_bold_font(child.font)", patched)


if __name__ == "__main__":
    unittest.main()
