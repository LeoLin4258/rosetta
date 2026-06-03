#!/usr/bin/env python3
from pathlib import Path
import sys

import pdf2zh


root = Path(pdf2zh.__file__).resolve().parent
target = root / "converter.py"
text = target.read_text()

if "def rosetta_pdf_color_operator(" in text:
    print(f"[pdf2zh-pack] color preservation patch already present in {target}")
    raise SystemExit(0)

old_raw_string = """            def raw_string(fcur,cstk): # 编码字符串
                if isinstance(self.fontmap[fcur],PDFCIDFont): # 判断编码长度
                    return "".join(["%04x" % ord(c) for c in cstk])
                else:
                    return "".join(["%02x" % ord(c) for c in cstk])
            _x,_y=0,0
"""
new_raw_string = """            def raw_string(fcur,cstk): # 编码字符串
                if isinstance(self.fontmap[fcur],PDFCIDFont): # 判断编码长度
                    return "".join(["%04x" % ord(c) for c in cstk])
                else:
                    return "".join(["%02x" % ord(c) for c in cstk])
            def rosetta_pdf_color_operator(color, stroking=False):
                if color is None:
                    return ""
                suffix = "G" if stroking else "g"
                if isinstance(color, (int, float)):
                    return f"{float(color):f} {suffix} "
                if isinstance(color, (list, tuple)):
                    values = [float(value) for value in color]
                    if len(values) == 1:
                        return f"{values[0]:f} {suffix} "
                    if len(values) == 3:
                        operator = "RG" if stroking else "rg"
                        return f"{values[0]:f} {values[1]:f} {values[2]:f} {operator} "
                    if len(values) == 4:
                        operator = "K" if stroking else "k"
                        return f"{values[0]:f} {values[1]:f} {values[2]:f} {values[3]:f} {operator} "
                return ""
            _x,_y=0,0
"""

replacements = [
    (
        old_raw_string,
        new_raw_string,
    ),
    (
        """                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False])
""",
        """                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False,child.graphicstate.ncolor])
""",
    ),
    (
        """                            pstk[-1][5]=child.font
""",
        """                            pstk[-1][5]=child.font
                            pstk[-1][7]=child.graphicstate.ncolor
""",
    ),
    (
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6] # 段落属性
""",
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6];pcolor=pstk[id][7] # 段落属性
""",
    ),
    (
        """                            ops+=f'/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
        """                            ops+=f'{rosetta_pdf_color_operator(pcolor)}/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
    ),
    (
        """                            ops+=f"/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
""",
        """                            ops+=f"{rosetta_pdf_color_operator(vch.graphicstate.ncolor)}/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
""",
    ),
    (
        """                                ops+=f"ET q 1 0 0 1 {l.pts[0][0]+x-var[vid][0].x0:f} {l.pts[0][1]+fix+y-var[vid][0].y0:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
""",
        """                                ops+=f"ET q {rosetta_pdf_color_operator(l.stroking_color, True)}1 0 0 1 {l.pts[0][0]+x-var[vid][0].x0:f} {l.pts[0][1]+fix+y-var[vid][0].y0:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
""",
    ),
    (
        """                    ops+=f"ET q 1 0 0 1 {l.pts[0][0]:f} {l.pts[0][1]:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
""",
        """                    ops+=f"ET q {rosetta_pdf_color_operator(l.stroking_color, True)}1 0 0 1 {l.pts[0][0]:f} {l.pts[0][1]:f} cm [] 0 d 0 J {l.linewidth:f} w 0 0 m {l.pts[1][0]-l.pts[0][0]:f} {l.pts[1][1]-l.pts[0][1]:f} l S Q BT "
""",
    ),
]

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"::error::could not find expected pdf2zh converter fragment in {target}")
    text = text.replace(old, new)

target.write_text(text)
print(f"[pdf2zh-pack] patched PDF text color preservation in {target}")

for cache_dir in root.rglob("__pycache__"):
    for child in cache_dir.iterdir():
        child.unlink()
    cache_dir.rmdir()
