#!/usr/bin/env python3
from pathlib import Path
import re
import sys

import pdf2zh


root = Path(pdf2zh.__file__).resolve().parent
target = root / "converter.py"
text = target.read_text(encoding="utf-8")
bold_expr = 're.match(r"(.*Bold|.*Medi|.*Demi|.*Black|.*Heavy|.*SemiBold|.*Semibold|.*Bd)", getattr(child.font, "fontname", "").split("+")[-1], re.IGNORECASE) is not None'
bold_list_accumulate_expr = f"pstk[-1][8] or {bold_expr}"
bold_attr_accumulate_expr = f"pstk[-1].bold or {bold_expr}"
rosetta_bold_font_resource_name = "notobold"
rosetta_bold_font_name = "SourceHanSansCN-Bold.ttf"


def normalize_text_mode_operator(text: str) -> tuple[str, bool]:
    pattern = re.compile(
        r'(?m)^(?P<indent>[ \t]*)def rosetta_pdf_text_mode_operator\(is_bold, color, size\):\n'
        r'(?P<body_indent>[ \t]+)if not is_bold:\n'
        r'[ \t]+return "0 Tr "\n'
        r'[ \t]+stroke_width = (?:'
        r'max\(0\.12, min\(0\.45, size \* 0\.018\)\)|'
        r'max\(0\.04, min\(0\.16, size \* 0\.006\)\)|'
        r'min\(0\.03, max\(0\.0, size \* 0\.0015\)\)'
        r')\n'
        r'[ \t]+return f"[^\n]*w 2 Tr "\n'
    )

    def replacement(match: re.Match[str]) -> str:
        indent = match.group("indent")
        body_indent = match.group("body_indent")
        return (
            f"{indent}def rosetta_pdf_text_mode_operator(is_bold, color, size):\n"
            f'{body_indent}return "0 Tr "\n'
        )

    return pattern.subn(replacement, text)


def patch_converter_bold_font_support(text: str) -> tuple[str, bool]:
    if "class TranslateConverter" not in text or "self.noto_name = noto_name" not in text:
        return text, False

    changed = False

    old_init = """        self.noto_name = noto_name
        self.noto = noto
"""
    new_init = f"""        self.noto_name = noto_name
        self.noto = noto
        self.rosetta_noto_bold_name = ""
        self.rosetta_noto_bold = None
        if (lang_out or "").lower() in {{"zh", "zh-cn", "zh-hans"}}:
            try:
                from babeldoc.assets.assets import get_font_and_metadata
                rosetta_bold_path, _ = get_font_and_metadata("{rosetta_bold_font_name}")
                self.rosetta_noto_bold_name = "{rosetta_bold_font_resource_name}"
                self.rosetta_noto_bold = Font(self.rosetta_noto_bold_name, rosetta_bold_path.as_posix())
            except Exception:
                self.rosetta_noto_bold_name = ""
                self.rosetta_noto_bold = None
"""
    if "self.rosetta_noto_bold_name" not in text:
        if old_init not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh converter init fragment in {target}")
        text = text.replace(old_init, new_init, 1)
        changed = True

    old_raw_string = """            if fcur == self.noto_name:
                return "".join(["%04x" % self.noto.has_glyph(ord(c)) for c in cstk])
            elif isinstance(self.fontmap[fcur], PDFCIDFont):  # 判断编码长度
"""
    new_raw_string = """            if fcur == self.noto_name:
                return "".join(["%04x" % self.noto.has_glyph(ord(c)) for c in cstk])
            if fcur == self.rosetta_noto_bold_name and self.rosetta_noto_bold is not None:
                return "".join(["%04x" % self.rosetta_noto_bold.has_glyph(ord(c)) for c in cstk])
            elif isinstance(self.fontmap[fcur], PDFCIDFont):  # 判断编码长度
"""
    if "self.rosetta_noto_bold.has_glyph" not in text:
        if old_raw_string not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh raw_string font fragment in {target}")
        text = text.replace(old_raw_string, new_raw_string, 1)
        changed = True

    if "pstk[id].bold and self.rosetta_noto_bold is not None" not in text:
        pattern = re.compile(
            r"(?m)^(?P<indent>[ \t]*)if fcur_ is None:\n"
            r"(?P=indent)[ \t]+fcur_ = self\.noto_name  # 默认非拉丁字体\n"
            r"(?P=indent)if fcur_ == self\.noto_name: # FIXME: change to CONST\n"
            r"(?P=indent)[ \t]+adv = self\.noto\.char_lengths\(ch, size\)\[0\]\n"
            r"(?P=indent)else:\n"
            r"(?P=indent)[ \t]+adv = self\.fontmap\[fcur_\]\.char_width\(ord\(ch\)\) \* size\n"
        )

        def replacement(match: re.Match[str]) -> str:
            indent = match.group("indent")
            inner = indent + "    "
            return (
                f"{indent}if fcur_ is None:\n"
                f"{inner}if pstk[id].bold and self.rosetta_noto_bold is not None:\n"
                f"{inner}    fcur_ = self.rosetta_noto_bold_name\n"
                f"{inner}else:\n"
                f"{inner}    fcur_ = self.noto_name  # 默认非拉丁字体\n"
                f"{indent}if fcur_ == self.noto_name: # FIXME: change to CONST\n"
                f"{inner}adv = self.noto.char_lengths(ch, size)[0]\n"
                f"{indent}elif fcur_ == self.rosetta_noto_bold_name and self.rosetta_noto_bold is not None:\n"
                f"{inner}adv = self.rosetta_noto_bold.char_lengths(ch, size)[0]\n"
                f"{indent}else:\n"
                f"{inner}adv = self.fontmap[fcur_].char_width(ord(ch)) * size\n"
            )

        text, count = pattern.subn(replacement, text, count=1)
        if count == 0:
            raise SystemExit(f"::error::could not find expected pdf2zh font choice fragment in {target}")
        changed = True

    return text, changed


def patch_cumulative_bold_marking(text: str) -> tuple[str, bool]:
    replacements = [
        (f"pstk[-1][8]={bold_expr}", f"pstk[-1][8]={bold_list_accumulate_expr}"),
        (f"pstk[-1].bold = {bold_expr}", f"pstk[-1].bold = {bold_attr_accumulate_expr}"),
    ]
    changed = False
    for old, new in replacements:
        if old in text:
            text = text.replace(old, new)
            changed = True
    return text, changed


def patch_high_level_bold_font_registration(root: Path) -> bool:
    target = root / "high_level.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: register Source Han Sans Bold for simplified Chinese PDF output."
    if marker in text:
        return False

    if "font_list.append((noto_name, font_path))" not in text:
        return False

    old = """    font_path = download_remote_fonts(lang_out.lower())
    noto_name = NOTO_NAME
    noto = Font(noto_name, font_path)
    font_list.append((noto_name, font_path))
"""
    new = f"""    font_path = download_remote_fonts(lang_out.lower())
    noto_name = NOTO_NAME
    noto = Font(noto_name, font_path)
    font_list.append((noto_name, font_path))
    # {marker}
    if lang_out.lower() in {{"zh", "zh-cn", "zh-hans"}}:
        rosetta_bold_path, _ = get_font_and_metadata("{rosetta_bold_font_name}")
        font_list.append(("{rosetta_bold_font_resource_name}", rosetta_bold_path.as_posix()))
"""
    if old not in text:
        raise SystemExit(f"::error::could not find expected high_level font_list fragment in {target}")

    target.write_text(text.replace(old, new), encoding="utf-8")
    print(f"[pdf2zh-pack] registered simplified Chinese bold font in {target}")
    return True


def patch_rosetta_engine_bold_font_registration(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    changed = False
    if "from babeldoc.assets.assets import get_font_and_metadata" not in text:
        old = "from pdf2zh.converter import TranslateConverter\n"
        new = "from babeldoc.assets.assets import get_font_and_metadata\nfrom pdf2zh.converter import TranslateConverter\n"
        if old not in text:
            raise SystemExit(f"::error::could not find expected rosetta_engine import fragment in {target}")
        text = text.replace(old, new, 1)
        changed = True

    old_prepare = """    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    doc = prepare_pdf_document(input_path, font_path, noto_name)
"""
    new_prepare = f"""    font_path = download_remote_fonts(langOut.lower())
    noto_name = NOTO_NAME
    noto = pymupdf.Font(noto_name, font_path)
    rosetta_bold_font_path = None
    if langOut.lower() in {{"zh", "zh-cn", "zh-hans"}}:
        rosetta_bold_path, _ = get_font_and_metadata("{rosetta_bold_font_name}")
        rosetta_bold_font_path = rosetta_bold_path.as_posix()
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path)
"""
    if "rosetta_bold_font_path = None" not in text:
        if old_prepare not in text:
            raise SystemExit(f"::error::could not find expected rosetta_engine prepareRun font fragment in {target}")
        text = text.replace(old_prepare, new_prepare, 1)
        changed = True

    old_signature = "def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str):\n"
    new_signature = "def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str, bold_font_path: str | None = None):\n"
    if old_signature in text:
        text = text.replace(old_signature, new_signature, 1)
        changed = True

    old_font_list = """    font_list = [("tiro", None), (noto_name, font_path)]
    font_id = {}
"""
    new_font_list = f"""    font_list = [("tiro", None), (noto_name, font_path)]
    if bold_font_path:
        font_list.append(("{rosetta_bold_font_resource_name}", bold_font_path))
    font_id = {{}}
"""
    if f'font_list.append(("{rosetta_bold_font_resource_name}", bold_font_path))' not in text:
        if old_font_list not in text:
            raise SystemExit(f"::error::could not find expected rosetta_engine font_list fragment in {target}")
        text = text.replace(old_font_list, new_font_list, 1)
        changed = True

    if changed:
        target.write_text(text, encoding="utf-8")
        print(f"[pdf2zh-pack] registered Rosetta engine bold font in {target}")
    return changed


def patch_simplified_chinese_font(root: Path) -> bool:
    target = root / "high_level.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find expected pdf2zh high_level.py in {root}")

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: prefer Source Han Sans for simplified Chinese PDF output."
    if marker in text:
        return False

    old = '    font_name = LANG_NAME_MAP.get(lang, "GoNotoKurrent-Regular.ttf")\n'
    new = f'''    # {marker}
    LANG_NAME_MAP.update({{
        "zh": "SourceHanSansCN-Regular.ttf",
        "zh-cn": "SourceHanSansCN-Regular.ttf",
        "zh-hans": "SourceHanSansCN-Regular.ttf",
    }})
    font_name = LANG_NAME_MAP.get(lang, "GoNotoKurrent-Regular.ttf")
'''
    if old not in text:
        raise SystemExit(f"::error::could not find expected font map fragment in {target}")

    target.write_text(text.replace(old, new), encoding="utf-8")
    print(f"[pdf2zh-pack] patched simplified Chinese PDF font mapping in {target}")
    return True


def clear_pycache(root: Path) -> None:
    for cache_dir in root.rglob("__pycache__"):
        for child in cache_dir.iterdir():
            child.unlink()
        cache_dir.rmdir()


if "def rosetta_pdf_is_bold_font(" in text and "rosetta_pdf_is_bold_font(child.font)" not in text:
    text, changed = normalize_text_mode_operator(text)
    text, bold_font_changed = patch_converter_bold_font_support(text)
    text, cumulative_bold_changed = patch_cumulative_bold_marking(text)
    if changed or bold_font_changed or cumulative_bold_changed:
        target.write_text(text, encoding="utf-8")
        if changed:
            print(f"[pdf2zh-pack] normalized PDF faux-bold text mode in {target}")
        if bold_font_changed:
            print(f"[pdf2zh-pack] enabled simplified Chinese bold font switching in {target}")
        if cumulative_bold_changed:
            print(f"[pdf2zh-pack] made PDF paragraph bold marking cumulative in {target}")
    font_changed = patch_simplified_chinese_font(root)
    high_level_bold_changed = patch_high_level_bold_font_registration(root)
    engine_bold_changed = patch_rosetta_engine_bold_font_registration(root)
    any_changed = (
        changed
        or bold_font_changed
        or cumulative_bold_changed
        or font_changed
        or high_level_bold_changed
        or engine_bold_changed
    )
    if any_changed:
        clear_pycache(root)
    else:
        print(f"[pdf2zh-pack] color, bold, and font mapping patch already present in {target}")
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
            def rosetta_pdf_is_bold_font(font):
                fontname = getattr(font, "fontname", "").split("+")[-1]
                return re.match(r"(.*Bold|.*Medi|.*Demi|.*Black|.*Heavy|.*SemiBold|.*Semibold|.*Bd)", fontname, re.IGNORECASE) is not None
            def rosetta_pdf_text_mode_operator(is_bold, color, size):
                return "0 Tr "
            _x,_y=0,0
"""

old_replacements = [
    (
        old_raw_string,
        new_raw_string,
    ),
    (
        """                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False])
""",
        f"""                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False,child.graphicstate.ncolor,{bold_expr}])
""",
    ),
    (
        """                            pstk[-1][5]=child.font
""",
        f"""                            pstk[-1][5]=child.font
                            pstk[-1][7]=child.graphicstate.ncolor
                            pstk[-1][8]={bold_list_accumulate_expr}
""",
    ),
    (
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6] # 段落属性
""",
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6];pcolor=pstk[id][7];pbold=pstk[id][8] # 段落属性
""",
    ),
    (
        """                            ops+=f'/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
        """                            ops+=f'{rosetta_pdf_text_mode_operator(pbold,pcolor,size)}{rosetta_pdf_color_operator(pcolor)}/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
    ),
    (
        """                            ops+=f"/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
""",
        """                            ops+=f"0 Tr {rosetta_pdf_color_operator(vch.graphicstate.ncolor)}/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
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

color_only_replacements = [
    (
        """                return ""
            _x,_y=0,0
""",
        """                return ""
            def rosetta_pdf_is_bold_font(font):
                fontname = getattr(font, "fontname", "").split("+")[-1]
                return re.match(r"(.*Bold|.*Medi|.*Demi|.*Black|.*Heavy|.*SemiBold|.*Semibold|.*Bd)", fontname, re.IGNORECASE) is not None
            def rosetta_pdf_text_mode_operator(is_bold, color, size):
                return "0 Tr "
            _x,_y=0,0
""",
    ),
    (
        """                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False,child.graphicstate.ncolor])
""",
        f"""                            pstk.append([child.y0,child.x0,child.x0,child.x0,child.size,child.font,False,child.graphicstate.ncolor,{bold_expr}])
""",
    ),
    (
        """                            pstk[-1][7]=child.graphicstate.ncolor
""",
        f"""                            pstk[-1][7]=child.graphicstate.ncolor
                            pstk[-1][8]={bold_list_accumulate_expr}
""",
    ),
    (
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6];pcolor=pstk[id][7] # 段落属性
""",
        """                tx=x=pstk[id][1];y=pstk[id][0];lt=pstk[id][2];rt=pstk[id][3];ptr=0;size=pstk[id][4];font=pstk[id][5];lb=pstk[id][6];pcolor=pstk[id][7];pbold=pstk[id][8] # 段落属性
""",
    ),
    (
        """                            ops+=f'{rosetta_pdf_color_operator(pcolor)}/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
        """                            ops+=f'{rosetta_pdf_text_mode_operator(pbold,pcolor,size)}{rosetta_pdf_color_operator(pcolor)}/{fcur} {size:f} Tf 1 0 0 1 {tx:f} {y:f} Tm [<{raw_string(fcur,cstk)}>] TJ '
""",
    ),
    (
        """                            ops+=f"{rosetta_pdf_color_operator(vch.graphicstate.ncolor)}/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
""",
        """                            ops+=f"0 Tr {rosetta_pdf_color_operator(vch.graphicstate.ncolor)}/{self.fontid[vch.font]} {vch.size:f} Tf 1 0 0 1 {x+vch.x0-var[vid][0].x0:f} {fix+y+vch.y0-var[vid][0].y0:f} Tm [<{raw_string(self.fontid[vch.font],vc)}>] TJ "
""",
    ),
]

repair_broken_bold_replacements = [
    (
        "rosetta_pdf_is_bold_font(child.font)",
        bold_expr,
    ),
]

paragraph_ops_replacements = [
    (
        """class Paragraph:
    def __init__(self, y, x, x0, x1, y0, y1, size, brk):
        self.y: float = y  # 初始纵坐标
        self.x: float = x  # 初始横坐标
        self.x0: float = x0  # 左边界
        self.x1: float = x1  # 右边界
        self.y0: float = y0  # 上边界
        self.y1: float = y1  # 下边界
        self.size: float = size  # 字体大小
        self.brk: bool = brk  # 换行标记
""",
        """class Paragraph:
    def __init__(self, y, x, x0, x1, y0, y1, size, brk, color=None, bold=False):
        self.y: float = y  # 初始纵坐标
        self.x: float = x  # 初始横坐标
        self.x0: float = x0  # 左边界
        self.x1: float = x1  # 右边界
        self.y0: float = y0  # 上边界
        self.y1: float = y1  # 下边界
        self.size: float = size  # 字体大小
        self.brk: bool = brk  # 换行标记
        self.color = color
        self.bold: bool = bold
""",
    ),
    (
        """        def vflag(font: str, char: str):    # 匹配公式（和角标）字体
""",
        """        def rosetta_pdf_color_operator(color, stroking=False):
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

        def rosetta_pdf_is_bold_font(font):
            fontname = getattr(font, "fontname", "").split("+")[-1]
            return re.match(r"(.*Bold|.*Medi|.*Demi|.*Black|.*Heavy|.*SemiBold|.*Semibold|.*Bd)", fontname, re.IGNORECASE) is not None

        def rosetta_pdf_text_mode_operator(is_bold, color, size):
            return "0 Tr "

        def vflag(font: str, char: str):    # 匹配公式（和角标）字体
""",
    ),
    (
        """                        pstk.append(Paragraph(child.y0, child.x0, child.x0, child.x0, child.y0, child.y1, child.size, False))
""",
        f"""                        pstk.append(Paragraph(child.y0, child.x0, child.x0, child.x0, child.y0, child.y1, child.size, False, child.graphicstate.ncolor, {bold_expr}))
""",
    ),
    (
        """                        pstk[-1].size = child.size
""",
        f"""                        pstk[-1].size = child.size
                        pstk[-1].color = child.graphicstate.ncolor
                        pstk[-1].bold = {bold_attr_accumulate_expr}
""",
    ),
    (
        """        def gen_op_txt(font, size, x, y, rtxt):
            return f"/{font} {size:f} Tf 1 0 0 1 {x:f} {y:f} Tm [<{rtxt}>] TJ "
""",
        """        def gen_op_txt(font, size, x, y, rtxt, color=None, bold=False):
            return f"{rosetta_pdf_text_mode_operator(bold, color, size)}{rosetta_pdf_color_operator(color)}/{font} {size:f} Tf 1 0 0 1 {x:f} {y:f} Tm [<{rtxt}>] TJ "
""",
    ),
    (
        """        def gen_op_line(x, y, xlen, ylen, linewidth):
            return f"ET q 1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "
""",
        """        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return f"ET q {rosetta_pdf_color_operator(color, True)}1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "
""",
    ),
    (
        """                            "rtxt": raw_string(fcur, cstk),
                            "lidx": lidx
""",
        """                            "rtxt": raw_string(fcur, cstk),
                            "lidx": lidx,
                            "color": pstk[id].color,
                            "bold": pstk[id].bold
""",
    ),
    (
        """                            "rtxt": raw_string(self.fontid[vch.font], vc),
                            "lidx": lidx
""",
        """                            "rtxt": raw_string(self.fontid[vch.font], vc),
                            "lidx": lidx,
                            "color": vch.graphicstate.ncolor,
                            "bold": False
""",
    ),
    (
        """                                "ylen": l.pts[1][1] - l.pts[0][1],
                                "lidx": lidx
""",
        """                                "ylen": l.pts[1][1] - l.pts[0][1],
                                "lidx": lidx,
                                "color": l.stroking_color
""",
    ),
    (
        """                    "rtxt": raw_string(fcur, cstk),
                    "lidx": lidx
""",
        """                    "rtxt": raw_string(fcur, cstk),
                    "lidx": lidx,
                    "color": pstk[id].color,
                    "bold": pstk[id].bold
""",
    ),
    (
        """                    ops_list.append(gen_op_txt(vals["font"], vals["size"], vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["rtxt"]))
""",
        """                    ops_list.append(gen_op_txt(vals["font"], vals["size"], vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["rtxt"], vals.get("color"), vals.get("bold", False)))
""",
    ),
    (
        """                    ops_list.append(gen_op_line(vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["xlen"], vals["ylen"], vals["linewidth"]))
""",
        """                    ops_list.append(gen_op_line(vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["xlen"], vals["ylen"], vals["linewidth"], vals.get("color")))
""",
    ),
    (
        """                ops_list.append(gen_op_line(l.pts[0][0], l.pts[0][1], l.pts[1][0] - l.pts[0][0], l.pts[1][1] - l.pts[0][1], l.linewidth))
""",
        """                ops_list.append(gen_op_line(l.pts[0][0], l.pts[0][1], l.pts[1][0] - l.pts[0][0], l.pts[1][1] - l.pts[0][1], l.linewidth, l.stroking_color))
""",
    ),
]

if "def rosetta_pdf_is_bold_font(" in text and "rosetta_pdf_is_bold_font(child.font)" in text:
    replacements = repair_broken_bold_replacements
elif (
    "class Paragraph:" in text
    and "ops_vals: list[dict] = []" in text
    and "def rosetta_pdf_color_operator(" not in text
):
    replacements = paragraph_ops_replacements
elif "def rosetta_pdf_color_operator(" in text:
    replacements = color_only_replacements
else:
    replacements = old_replacements

for old, new in replacements:
    if old not in text:
        raise SystemExit(f"::error::could not find expected pdf2zh converter fragment in {target}")
    text = text.replace(old, new)

text, _ = normalize_text_mode_operator(text)
text, _ = patch_converter_bold_font_support(text)
text, _ = patch_cumulative_bold_marking(text)

target.write_text(text, encoding="utf-8")
print(f"[pdf2zh-pack] patched PDF text color and bold preservation in {target}")
patch_simplified_chinese_font(root)
patch_high_level_bold_font_registration(root)
patch_rosetta_engine_bold_font_registration(root)
clear_pycache(root)
