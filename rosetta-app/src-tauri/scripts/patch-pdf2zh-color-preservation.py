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


def patch_converter_scalar_layout_clamp(text: str) -> tuple[str, bool]:
    changed = False
    for item_name in ("child", "item"):
        combined = (
            f"                cx, cy = np.clip(int({item_name}.x0), 0, w - 1), "
            f"np.clip(int({item_name}.y0), 0, h - 1)\n"
        )
        split = (
            f"                cx = np.clip(int({item_name}.x0), 0, w - 1)\n"
            f"                cy = np.clip(int({item_name}.y0), 0, h - 1)\n"
        )
        replacement = (
            f"                cx = min(max(int({item_name}.x0), 0), w - 1)\n"
            f"                cy = min(max(int({item_name}.y0), 0), h - 1)\n"
        )
        if combined in text:
            text = text.replace(combined, replacement)
            changed = True
        if split in text:
            text = text.replace(split, replacement)
            changed = True
    return text, changed


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

    cache_helper = f'''_rosetta_cached_bold_font = None


def rosetta_pdf_cached_bold_font():
    global _rosetta_cached_bold_font
    if _rosetta_cached_bold_font is None:
        from babeldoc.assets.assets import get_font_and_metadata
        rosetta_bold_path, _ = get_font_and_metadata("{rosetta_bold_font_name}")
        _rosetta_cached_bold_font = Font("{rosetta_bold_font_resource_name}", rosetta_bold_path.as_posix())
    return "{rosetta_bold_font_resource_name}", _rosetta_cached_bold_font


'''
    if "def rosetta_pdf_cached_bold_font(" not in text:
        class_anchor = "class TranslateConverter(PDFConverterEx):\n"
        if class_anchor not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh converter class fragment in {target}")
        text = text.replace(class_anchor, cache_helper + class_anchor, 1)
        changed = True

    old_init = """        self.noto_name = noto_name
        self.noto = noto
"""
    legacy_bold_init = f"""        self.noto_name = noto_name
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
    cached_bold_init = f"""        self.noto_name = noto_name
        self.noto = noto
        self.rosetta_noto_bold_name = ""
        self.rosetta_noto_bold = None
        if (lang_out or "").lower() in {{"zh", "zh-cn", "zh-hans"}}:
            try:
                self.rosetta_noto_bold_name, self.rosetta_noto_bold = rosetta_pdf_cached_bold_font()
            except Exception:
                self.rosetta_noto_bold_name = ""
                self.rosetta_noto_bold = None
"""
    if legacy_bold_init in text:
        text = text.replace(legacy_bold_init, cached_bold_init, 1)
        changed = True
    elif "self.rosetta_noto_bold_name" not in text:
        if old_init not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh converter init fragment in {target}")
        text = text.replace(old_init, cached_bold_init, 1)
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


def patch_converter_text_rendering_safety(text: str) -> tuple[str, bool]:
    if "class TranslateConverter" not in text or "ops_vals: list[dict] = []" not in text:
        return text, False

    changed = False

    old_gen_op_line = """        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return f"ET q {rosetta_pdf_color_operator(color, True)}1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "
"""
    new_gen_op_line = """        def gen_op_line(x, y, xlen, ylen, linewidth, color=None):
            return f"ET q {rosetta_pdf_color_operator(color, True)}1 0 0 1 {x:f} {y:f} cm [] 0 d 0 J {linewidth:f} w 0 0 m {xlen:f} {ylen:f} l S Q BT "

        def rosetta_pdf_fill_rect(x0, y0, x1, y1, pad):
            left = min(x0, x1) - pad
            bottom = min(y0, y1) - pad
            width = abs(x1 - x0) + pad * 2
            height = abs(y1 - y0) + pad * 2
            return f"ET q 1 g {left:f} {bottom:f} {width:f} {height:f} re f Q BT "
"""
    if "def rosetta_pdf_fill_rect(" not in text:
        if old_gen_op_line not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh gen_op_line fragment in {target}")
        text = text.replace(old_gen_op_line, new_gen_op_line, 1)
        changed = True
    if "def rosetta_pdf_fill_rect(x0, pstk[id].y0, x1, pstk[id].y1, pad):" in text:
        text = text.replace(
            "def rosetta_pdf_fill_rect(x0, pstk[id].y0, x1, pstk[id].y1, pad):",
            "def rosetta_pdf_fill_rect(x0, y0, x1, y1, pad):",
        )
        changed = True
    if "ops_list.append(rosetta_pdf_fill_rect(x0, y0, x1, y1" in text:
        text = text.replace(
            "ops_list.append(rosetta_pdf_fill_rect(x0, y0, x1, y1",
            "ops_list.append(rosetta_pdf_fill_rect(x0, pstk[id].y0, x1, pstk[id].y1",
        )
        changed = True

    old_line_height = """            line_height = default_line_height

            while (lidx + 1) * size * line_height > height and line_height >= 1:
                line_height -= 0.05

            for vals in ops_vals:
                if vals["type"] == OpType.TEXT:
                    ops_list.append(gen_op_txt(vals["font"], vals["size"], vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["rtxt"], vals.get("color"), vals.get("bold", False)))
                elif vals["type"] == OpType.LINE:
                    ops_list.append(gen_op_line(vals["x"], vals["dy"] + y - vals["lidx"] * size * line_height, vals["xlen"], vals["ylen"], vals["linewidth"], vals.get("color")))
"""
    new_line_height = """            # Rosetta: erase source text under translated paragraphs and keep CJK line spacing legible.
            if str(new).strip() and self.should_translate_text(sstk[id]):
                ops_list.append(rosetta_pdf_fill_rect(x0, pstk[id].y0, x1, pstk[id].y1, max(1.0, size * 0.25)))

            line_count = lidx + 1
            line_height = default_line_height
            lang_key = self.translator.lang_out.lower()
            min_line_height = 1.2 if lang_key in {"zh", "zh-cn", "zh-hans", "zh-tw", "zh-hant", "ja", "ko"} else 1.0
            render_size = size
            if line_count > 1 and height > 0 and line_count * render_size * line_height > height:
                fit_size = height / (line_count * min_line_height)
                min_render_size = max(4.5, size * 0.72)
                render_size = max(min_render_size, min(size, fit_size))

            while line_count * render_size * line_height > height and line_height > min_line_height:
                line_height = max(min_line_height, line_height - 0.05)

            for vals in ops_vals:
                y_pos = vals["dy"] + y - vals["lidx"] * render_size * line_height
                if vals["type"] == OpType.TEXT:
                    draw_size = vals["size"]
                    if vals["font"] in {"tiro", self.noto_name, self.rosetta_noto_bold_name}:
                        draw_size = min(draw_size, render_size)
                    ops_list.append(gen_op_txt(vals["font"], draw_size, vals["x"], y_pos, vals["rtxt"], vals.get("color"), vals.get("bold", False)))
                elif vals["type"] == OpType.LINE:
                    ops_list.append(gen_op_line(vals["x"], y_pos, vals["xlen"], vals["ylen"], vals["linewidth"], vals.get("color")))
"""
    if "Rosetta: erase source text under translated paragraphs" not in text:
        if old_line_height in text:
            text = text.replace(old_line_height, new_line_height, 1)
            changed = True
    if (
        "Rosetta: erase source text under translated paragraphs" in text
        and "if str(new).strip():" in text
    ):
        text = text.replace(
            "if str(new).strip():",
            "if str(new).strip() and self.should_translate_text(sstk[id]):",
            1,
        )
        changed = True

    return text, changed


def patch_converter_centered_single_line_alignment(text: str) -> tuple[str, bool]:
    if "class TranslateConverter" not in text or "ops_vals: list[dict] = []" not in text:
        return text, False

    changed = False
    helper_marker = "def rosetta_pdf_centered_alignment_shift("
    old_helper_anchor = '''        def rosetta_pdf_fill_rect(x0, y0, x1, y1, pad):
            left = min(x0, x1) - pad
            bottom = min(y0, y1) - pad
            width = abs(x1 - x0) + pad * 2
            height = abs(y1 - y0) + pad * 2
            return f"ET q 1 g {left:f} {bottom:f} {width:f} {height:f} re f Q BT "
'''
    new_helper_anchor = old_helper_anchor + '''
        def rosetta_pdf_centered_alignment_shift(
            page_left,
            page_right,
            source_left,
            source_right,
            translated_left,
            translated_right,
            size,
            has_line_break,
            line_count,
        ):
            page_width = page_right - page_left
            source_width = source_right - source_left
            translated_width = translated_right - translated_left
            if (
                has_line_break
                or line_count != 1
                or page_width <= 0
                or source_width <= 0
                or source_width > page_width * 0.8
                or translated_width <= 0
                or translated_width > page_width
            ):
                return 0.0
            page_center = (page_left + page_right) / 2
            source_center = (source_left + source_right) / 2
            center_tolerance = max(2.0, min(12.0, size * 0.5))
            if abs(source_center - page_center) > center_tolerance:
                return 0.0
            centered_left = max(
                page_left,
                min(page_right - translated_width, source_center - translated_width / 2),
            )
            return centered_left - translated_left
'''
    if helper_marker not in text:
        if old_helper_anchor not in text:
            raise SystemExit(f"::error::could not find expected pdf2zh fill rectangle helper in {target}")
        text = text.replace(old_helper_anchor, new_helper_anchor, 1)
        changed = True

    alignment_marker = "Rosetta: preserve page-centered single-line paragraph alignment after translation."
    old_alignment_anchor = '''            # Rosetta: erase source text under translated paragraphs and keep CJK line spacing legible.
'''
    new_alignment_anchor = '''            # Rosetta: preserve page-centered single-line paragraph alignment after translation.
            alignment_shift = rosetta_pdf_centered_alignment_shift(
                ltpage.x0,
                ltpage.x1,
                x0,
                x1,
                pstk[id].x,
                x,
                size,
                brk,
                lidx + 1,
            )
            if alignment_shift:
                for vals in ops_vals:
                    vals["x"] += alignment_shift

            # Rosetta: erase source text under translated paragraphs and keep CJK line spacing legible.
'''
    if alignment_marker not in text:
        if old_alignment_anchor not in text:
            return text, changed
        text = text.replace(old_alignment_anchor, new_alignment_anchor, 1)
        changed = True

    return text, changed


def patch_converter_structural_line_breaks(text: str) -> tuple[str, bool]:
    if "class TranslateConverter" not in text or "class Paragraph:" not in text:
        return text, False

    changed = False
    line_break_placeholder = "{v900000000}"

    old_paragraph_field = """        self.brk: bool = brk  # 换行标记
        self.color = color
"""
    new_paragraph_field = """        self.brk: bool = brk  # 换行标记
        self.rosetta_line_breaks: list[tuple[int, float]] = []
        self.color = color
"""
    if "self.rosetta_line_breaks" not in text and old_paragraph_field in text:
        text = text.replace(old_paragraph_field, new_paragraph_field, 1)
        changed = True

    old_break_capture = '''                        elif child.x1 < xt.x0:      # 添加换行空格并标记原文段落存在换行
                            sstk[-1] += " "
                            pstk[-1].brk = True
'''
    new_break_capture = '''                        elif child.x1 < xt.x0:      # 添加换行空格并标记原文段落存在换行
                            pstk[-1].rosetta_line_breaks.append((len(sstk[-1]), xt.x1))
                            sstk[-1] += " "
                            pstk[-1].brk = True
'''
    if "rosetta_line_breaks.append" not in text and old_break_capture in text:
        text = text.replace(old_break_capture, new_break_capture, 1)
        changed = True

    structural_marker = "Rosetta: preserve structural line breaks in list-like text blocks."
    translation_anchor = '''        ############################################################
        # B. 段落翻译
'''
    structural_block = f'''        # {structural_marker}
        def rosetta_pdf_should_preserve_source_line_breaks(paragraph, source_text):
            if len(re.findall(r"\\{{v\\d+\\}}", source_text)) >= 3:
                return False
            line_breaks = getattr(paragraph, "rosetta_line_breaks", [])
            if len(line_breaks) < 3:
                return False
            paragraph_width = paragraph.x1 - paragraph.x0
            if paragraph_width <= 0:
                return False
            short_line_gap = max(paragraph.size * 2.0, paragraph_width * 0.15)
            short_line_count = sum(
                1 for _offset, line_end in line_breaks
                if paragraph.x1 - line_end >= short_line_gap
            )
            return short_line_count >= 2 and short_line_count * 3 >= len(line_breaks) * 2

        for paragraph_id, paragraph in enumerate(pstk):
            if not rosetta_pdf_should_preserve_source_line_breaks(paragraph, sstk[paragraph_id]):
                continue
            for offset, _line_end in reversed(paragraph.rosetta_line_breaks):
                sstk[paragraph_id] = (
                    sstk[paragraph_id][:offset]
                    + "{line_break_placeholder}"
                    + sstk[paragraph_id][offset + 1:]
                )

''' + translation_anchor
    if structural_marker not in text and translation_anchor in text:
        text = text.replace(translation_anchor, structural_block, 1)
        changed = True
    old_structural_helper = '''        def rosetta_pdf_should_preserve_source_line_breaks(paragraph):
            line_breaks = getattr(paragraph, "rosetta_line_breaks", [])
'''
    new_structural_helper = '''        def rosetta_pdf_should_preserve_source_line_breaks(paragraph, source_text):
            if len(re.findall(r"\\{v\\d+\\}", source_text)) >= 3:
                return False
            line_breaks = getattr(paragraph, "rosetta_line_breaks", [])
'''
    if old_structural_helper in text:
        text = text.replace(old_structural_helper, new_structural_helper, 1)
        text = text.replace(
            "if not rosetta_pdf_should_preserve_source_line_breaks(paragraph):",
            "if not rosetta_pdf_should_preserve_source_line_breaks(paragraph, sstk[paragraph_id]):",
            1,
        )
        changed = True

    old_modifier = """                mod = 0  # 文字修饰符
                if vy_regex:  # 加载公式
"""
    new_modifier = """                mod = 0  # 文字修饰符
                rosetta_forced_line_break = False
                if vy_regex:  # 加载公式
"""
    if "rosetta_forced_line_break = False" not in text and old_modifier in text:
        text = text.replace(old_modifier, new_modifier, 1)
        changed = True

    old_placeholder_load = """                        vid = int(vy_regex.group(1).replace(" ", ""))
                        adv = vlen[vid]
"""
    new_placeholder_load = f"""                        vid = int(vy_regex.group(1).replace(" ", ""))
                        if vid == 900000000:
                            adv = 0
                            rosetta_forced_line_break = True
                        else:
                            adv = vlen[vid]
"""
    if "if vid == 900000000:" not in text and old_placeholder_load in text:
        text = text.replace(old_placeholder_load, new_placeholder_load, 1)
        changed = True

    old_modifier_guard = '''                    if var[vid][-1].get_text() and unicodedata.category(var[vid][-1].get_text()[0]) in ["Lm", "Mn", "Sk"]:  # 文字修饰符
'''
    new_modifier_guard = '''                    if not rosetta_forced_line_break and var[vid][-1].get_text() and unicodedata.category(var[vid][-1].get_text()[0]) in ["Lm", "Mn", "Sk"]:  # 文字修饰符
'''
    if "if not rosetta_forced_line_break and var[vid]" not in text and old_modifier_guard in text:
        text = text.replace(old_modifier_guard, new_modifier_guard, 1)
        changed = True

    old_forced_break_anchor = '''                if brk and x + adv > x1 + 0.1 * size:  # 到达右边界且原文段落存在换行
'''
    new_forced_break_anchor = '''                if rosetta_forced_line_break:
                    x = x0
                    lidx += 1
                    fcur = None
                    continue
                if brk and x + adv > x1 + 0.1 * size:  # 到达右边界且原文段落存在换行
'''
    if "if rosetta_forced_line_break:" not in text and old_forced_break_anchor in text:
        text = text.replace(old_forced_break_anchor, new_forced_break_anchor, 1)
        changed = True

    return text, changed


def patch_rosetta_engine_structural_line_breaks(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    changed = False
    old_placeholder_count = '''def rosetta_placeholder_count(text: str) -> int:
    return len(re.findall(r"\\{v\\d+\\}", text))
'''
    new_placeholder_count = '''def rosetta_placeholder_count(text: str) -> int:
    return sum(
        1
        for placeholder in re.findall(r"\\{v\\d+\\}", text)
        if placeholder != "{v900000000}"
    )
'''
    if old_placeholder_count in text:
        text = text.replace(old_placeholder_count, new_placeholder_count, 1)
        changed = True

    old_source_chars = "                sourceChars=len(text),\n"
    new_source_chars = '                sourceChars=len(text.replace("{v900000000}", "")),\n'
    if old_source_chars in text:
        text = text.replace(old_source_chars, new_source_chars, 1)
        changed = True

    old_translated_chars = "            self.translated_chars += len(translated)\n"
    new_translated_chars = '            self.translated_chars += len(translated.replace("{v900000000}", ""))\n'
    if old_translated_chars in text:
        text = text.replace(old_translated_chars, new_translated_chars, 1)
        changed = True

    if changed:
        target.write_text(text, encoding="utf-8")
        print(f"[pdf2zh-pack] preserved structural PDF line breaks in {target}")
    return changed


def patch_converter_formula_text_classification(text: str) -> tuple[str, bool]:
    if "class TranslateConverter" not in text:
        return text, False

    changed = False
    if ".*Mono|.*Code|.*Ital|.*Sym|.*Math" in text:
        text = text.replace(".*Mono|.*Code|.*Ital|.*Sym|.*Math", ".*Mono|.*Code|.*Sym|.*Math")
        changed = True

    old_cls_condition = """                if (                                                                                        # 判定当前字符是否属于公式
                    cls == 0                                                                                # 1. 类别为保留区域
                    or (cls == xt_cls and len(sstk[-1].strip()) > 1 and child.size < pstk[-1].size * 0.79)  # 2. 角标字体，有 0.76 的角标和 0.799 的大写，这里用 0.79 取中，同时考虑首字母放大的情况
"""
    visual_text_gate = """        def rosetta_allow_text_like_visual_chars(ltpage: LTPage) -> bool:
            try:
                layout = self.layout[ltpage.pageid]
                h, w = layout.shape
            except Exception:
                return True
            visual_chars = []
            for item in ltpage:
                if not isinstance(item, LTChar):
                    continue
                cx = np.clip(int(item.x0), 0, w - 1)
                cy = np.clip(int(item.y0), 0, h - 1)
                if layout[cy, cx] == 0:
                    visual_chars.append(item.get_text())
            compact = " ".join("".join(visual_chars).split())
            if len(compact) < 80:
                return True
            metric_hits = len(re.findall(r"\\b(?:ODS|OIS|mIoU|FLOPs?|Param\\.?|Methods?|Year|Size|AMCM|GBST|DSCD|UNet|SegFormer|SOTA|RIND|SFIAN|ICCV|CVPR|ICIP|WACV|ECCV|arXiv)\\b", compact))
            numeric_tokens = len(re.findall(r"(?<![A-Za-z])\\d+(?:\\.\\d+)?(?:[A-Za-z%]+)?", compact))
            without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", compact)
            sentence_marks = sum(without_decimal_points.count(mark) for mark in ".;:!?")
            algorithm_hits = sum(
                1
                for token in ("Algorithm", "Input", "Output", "Initialize", "Return", "endif", "endfor")
                if token in compact
            )
            math_table_signal_hits = sum(
                1
                for token in ("LDice", "LBCE", "Dice", "BCE", "mIoU", "ODSOIS", "F1mIoU", "β:α", "alpha", "beta")
                if token in compact
            )
            compact_table_signal_hits = sum(
                1
                for token in (
                    "LayerNumODSOIS",
                    "HeadODSOIS",
                    "MethodsOD",
                    "AMCMGBSTDSCD",
                    "F1mIoU",
                    "Params",
                    "FLOPs",
                    "ModelSize",
                )
                if token in compact
            )
            dataset_table_signal_hits = sum(
                1
                for token in (
                    "DatasetCategoryTrainValTest",
                    "FarmInsects",
                    "AgriculturalPests",
                    "InsectRecognition",
                    "ForestryPest",
                    "Lidataset",
                    "IP102",
                    "QianFSD",
                    "AgriInsect",
                )
                if token in compact
            )
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if numeric_tokens >= 12 and dataset_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
                return False
            if metric_hits >= 2 and numeric_tokens >= 18 and sentence_marks <= 12:
                return False
            if numeric_tokens >= 40 and sentence_marks <= 6:
                return False
            return True

        rosetta_text_like_visual_chars_enabled = rosetta_allow_text_like_visual_chars(ltpage)

"""
    new_cls_condition = """                # Rosetta: table/legal-prose PDFs often put normal text in visual regions.
                rosetta_text_like_visual_char = (
                    cls == 0
                    and rosetta_text_like_visual_chars_enabled
                    and bool(child.get_text())
                    and (
                        child.get_text().isalnum()
                        or child.get_text().isspace()
                        or child.get_text() in ".,;:!?()[]{}<>/\\\\'\\\"-–—&$%#@*+=|"
                    )
                )
                if (                                                                                        # 判定当前字符是否属于公式
                    (cls == 0 and not rosetta_text_like_visual_char)                                         # 1. 类别为保留区域
                    or (cls == xt_cls and len(sstk[-1].strip()) > 1 and child.size < pstk[-1].size * 0.79)  # 2. 角标字体，有 0.76 的角标和 0.799 的大写，这里用 0.79 取中，同时考虑首字母放大的情况
"""
    if "rosetta_text_like_visual_char" not in text:
        if old_cls_condition in text:
            loop_anchor = """        ############################################################
        # A. 原文档解析
        for child in ltpage:
"""
            if loop_anchor in text and "def rosetta_allow_text_like_visual_chars(" not in text:
                text = text.replace(loop_anchor, visual_text_gate + loop_anchor, 1)
            text = text.replace(old_cls_condition, new_cls_condition, 1)
            changed = True
    elif "rosetta_text_like_visual_chars_enabled" not in text:
        loop_anchor = """        ############################################################
        # A. 原文档解析
        for child in ltpage:
"""
        if loop_anchor not in text:
            raise SystemExit(f"::error::could not find expected visual text gate anchor in {target}")
        if "def rosetta_allow_text_like_visual_chars(" not in text:
            text = text.replace(loop_anchor, visual_text_gate + loop_anchor, 1)
        old_visual_condition = """                rosetta_text_like_visual_char = (
                    cls == 0
                    and bool(child.get_text())
"""
        new_visual_condition = """                rosetta_text_like_visual_char = (
                    cls == 0
                    and rosetta_text_like_visual_chars_enabled
                    and bool(child.get_text())
"""
        if old_visual_condition not in text:
            raise SystemExit(f"::error::could not find expected visual text condition in {target}")
        text = text.replace(old_visual_condition, new_visual_condition, 1)
        changed = True
    old_visual_sentence_marks = '            sentence_marks = sum(compact.count(mark) for mark in ".;:!?")\n'
    new_visual_sentence_marks = '''            without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", compact)
            sentence_marks = sum(without_decimal_points.count(mark) for mark in ".;:!?")
'''
    if old_visual_sentence_marks in text:
        text = text.replace(old_visual_sentence_marks, new_visual_sentence_marks, 1)
        changed = True
    if (
        "def rosetta_allow_text_like_visual_chars(" in text
        and 'if "Algorithm" in compact and algorithm_hits >= 3:' not in text
    ):
        algorithm_gate_anchor = '''            without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", compact)
            sentence_marks = sum(without_decimal_points.count(mark) for mark in ".;:!?")
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        algorithm_gate_replacement = '''            without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", compact)
            sentence_marks = sum(without_decimal_points.count(mark) for mark in ".;:!?")
            algorithm_hits = sum(
                1
                for token in ("Algorithm", "Input", "Output", "Initialize", "Return", "endif", "endfor")
                if token in compact
            )
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        if algorithm_gate_anchor not in text:
            raise SystemExit(f"::error::could not find expected visual algorithm gate anchor in {target}")
        text = text.replace(algorithm_gate_anchor, algorithm_gate_replacement, 1)
        changed = True
    if (
        "def rosetta_allow_text_like_visual_chars(" in text
        and "math_table_signal_hits" not in text
    ):
        math_table_anchor = '''            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        math_table_replacement = '''            math_table_signal_hits = sum(
                1
                for token in ("LDice", "LBCE", "Dice", "BCE", "mIoU", "ODSOIS", "F1mIoU", "β:α", "alpha", "beta")
                if token in compact
            )
            compact_table_signal_hits = sum(
                1
                for token in (
                    "LayerNumODSOIS",
                    "HeadODSOIS",
                    "MethodsOD",
                    "AMCMGBSTDSCD",
                    "F1mIoU",
                    "Params",
                    "FLOPs",
                    "ModelSize",
                )
                if token in compact
            )
            dataset_table_signal_hits = sum(
                1
                for token in (
                    "DatasetCategoryTrainValTest",
                    "FarmInsects",
                    "AgriculturalPests",
                    "InsectRecognition",
                    "ForestryPest",
                    "Lidataset",
                    "IP102",
                    "QianFSD",
                    "AgriInsect",
                )
                if token in compact
            )
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if numeric_tokens >= 12 and dataset_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        if math_table_anchor not in text:
            raise SystemExit(f"::error::could not find expected visual math/table gate anchor in {target}")
        text = text.replace(math_table_anchor, math_table_replacement, 1)
        changed = True
    if (
        "def rosetta_allow_text_like_visual_chars(" in text
        and "compact_table_signal_hits" not in text
    ):
        compact_table_anchor = '''            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        compact_table_replacement = '''            compact_table_signal_hits = sum(
                1
                for token in (
                    "LayerNumODSOIS",
                    "HeadODSOIS",
                    "MethodsOD",
                    "AMCMGBSTDSCD",
                    "F1mIoU",
                    "Params",
                    "FLOPs",
                    "ModelSize",
                )
                if token in compact
            )
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        if compact_table_anchor not in text:
            raise SystemExit(f"::error::could not find expected visual compact-table gate anchor in {target}")
        text = text.replace(compact_table_anchor, compact_table_replacement, 1)
        changed = True
    if (
        "def rosetta_allow_text_like_visual_chars(" in text
        and "dataset_table_signal_hits" not in text
    ):
        dataset_table_anchor = '''            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        dataset_table_replacement = '''            dataset_table_signal_hits = sum(
                1
                for token in (
                    "DatasetCategoryTrainValTest",
                    "FarmInsects",
                    "AgriculturalPests",
                    "InsectRecognition",
                    "ForestryPest",
                    "Lidataset",
                    "IP102",
                    "QianFSD",
                    "AgriInsect",
                )
                if token in compact
            )
            if "Algorithm" in compact and algorithm_hits >= 3:
                return False
            if numeric_tokens >= 40 and math_table_signal_hits >= 2:
                return False
            if numeric_tokens >= 18 and compact_table_signal_hits >= 3:
                return False
            if numeric_tokens >= 12 and dataset_table_signal_hits >= 3:
                return False
            if metric_hits >= 3 and numeric_tokens >= 8 and sentence_marks <= 10:
'''
        if dataset_table_anchor not in text:
            raise SystemExit(f"::error::could not find expected visual dataset-table gate anchor in {target}")
        text = text.replace(dataset_table_anchor, dataset_table_replacement, 1)
        changed = True
    old_numeric_dense_gate = '''            if metric_hits >= 2 and numeric_tokens >= 18 and sentence_marks <= 12:
                return False
            return True
'''
    new_numeric_dense_gate = '''            if metric_hits >= 2 and numeric_tokens >= 18 and sentence_marks <= 12:
                return False
            if numeric_tokens >= 40 and sentence_marks <= 6:
                return False
            return True
'''
    if "numeric_tokens >= 40" not in text and old_numeric_dense_gate in text:
        text = text.replace(old_numeric_dense_gate, new_numeric_dense_gate, 1)
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


def patch_rosetta_engine_selected_page_window(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: prepare only selected PDF pages for translation windows."
    if marker in text:
        return False
    if "def prepareRun(" not in text or "def prepare_pdf_document(" not in text:
        return False

    changed = False
    old_prepare_doc = """    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path)
    page_count = doc.page_count
    selected_pages = normalize_pages(pages, page_count)
"""
    new_prepare_doc = f"""    # {marker}
    source_doc = pymupdf.open(str(input_path))
    try:
        page_count = source_doc.page_count
    finally:
        source_doc.close()
    selected_pages = normalize_pages(pages, page_count)
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path, selected_pages)
"""
    if old_prepare_doc in text:
        text = text.replace(old_prepare_doc, new_prepare_doc, 1)
        changed = True

    old_pdfminer_pages = """                [page - 1 for page in selected_pages],
                maxpages=0,
                password="",
                caching=True,
            )
        )
        for page, page_number in zip(pdf_pages, selected_pages):
            page_index = page_number - 1
            page.pageno = page_index
"""
    new_pdfminer_pages = """                list(range(len(selected_pages))),
                maxpages=0,
                password="",
                caching=True,
            )
        )
        for prepared_page_index, (page, page_number) in enumerate(zip(pdf_pages, selected_pages)):
            page_index = prepared_page_index
            page.pageno = page_index
"""
    if old_pdfminer_pages in text:
        text = text.replace(old_pdfminer_pages, new_pdfminer_pages, 1)
        changed = True
    else:
        old_batched_pdfminer_pages = """                [page - 1 for page in selected_pages],
                maxpages=0,
                password="",
                caching=True,
            )
        )
        batch_size = layout_batch_size(model, options)
        page_pairs = list(zip(pdf_pages, selected_pages))
        for start in range(0, len(page_pairs), batch_size):
            batch = page_pairs[start : start + batch_size]
            page_indices = [page_number - 1 for _page, page_number in batch]
            layout.update(build_layout_masks(doc, page_indices, model, options))
            for page, page_number in batch:
                page_index = page_number - 1
                page.pageno = page_index
"""
        new_batched_pdfminer_pages = """                list(range(len(selected_pages))),
                maxpages=0,
                password="",
                caching=True,
            )
        )
        batch_size = layout_batch_size(model, options)
        page_pairs = [
            (page, page_number, prepared_page_index)
            for prepared_page_index, (page, page_number) in enumerate(
                zip(pdf_pages, selected_pages)
            )
        ]
        for start in range(0, len(page_pairs), batch_size):
            batch = page_pairs[start : start + batch_size]
            page_indices = [page_index for _page, _page_number, page_index in batch]
            layout.update(build_layout_masks(doc, page_indices, model, options))
            for page, page_number, page_index in batch:
                page.pageno = page_index
"""
        if old_batched_pdfminer_pages in text:
            text = text.replace(
                old_batched_pdfminer_pages,
                new_batched_pdfminer_pages,
                1,
            )
            changed = True

    old_signature = "def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str, bold_font_path: str | None = None):\n"
    new_signature = "def prepare_pdf_document(input_path: Path, font_path: str, noto_name: str, bold_font_path: str | None = None, selected_pages: list[int] | None = None):\n"
    if old_signature in text:
        text = text.replace(old_signature, new_signature, 1)
        changed = True

    old_open = """    doc = pymupdf.open(str(input_path))
    font_list = [("tiro", None), (noto_name, font_path)]
"""
    new_open = """    source_doc = pymupdf.open(str(input_path))
    if selected_pages is None:
        doc = source_doc
    else:
        doc = pymupdf.open()
        try:
            for page_number in selected_pages:
                doc.insert_pdf(source_doc, from_page=page_number - 1, to_page=page_number - 1)
        finally:
            source_doc.close()
    font_list = [("tiro", None), (noto_name, font_path)]
"""
    if old_open in text:
        text = text.replace(old_open, new_open, 1)
        changed = True

    if changed:
        target.write_text(text, encoding="utf-8")
        print(f"[pdf2zh-pack] limited PDF preparation to selected pages in {target}")
    return changed


def nontranslatable_layout_helper() -> str:
    return '''def rosetta_placeholder_count(text: str) -> int:
    return len(re.findall(r"\\{v\\d+\\}", text))


def rosetta_sentence_punctuation_count(text: str) -> int:
    without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", text)
    return sum(without_decimal_points.count(mark) for mark in ".;:!?")


def is_rosetta_page_number_unit(text: str) -> bool:
    return re.fullmatch(r"\\d{1,4}", text.strip()) is not None


def is_rosetta_formula_like_unit(text: str) -> bool:
    compact = " ".join(text.split())
    placeholder_count = rosetta_placeholder_count(compact)
    if placeholder_count < 3:
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    operator_hits = len(re.findall(r"\\b(?:Partition|TopK|Gumbel|Softmax|Flatten|EM|LN|FFN|CR)\\b", compact))
    if len(compact) <= 180 and placeholder_count >= 6 and operator_hits >= 2 and rosetta_sentence_punctuation_count(compact) <= 2:
        return True
    if len(compact) > 140:
        return False
    return len(words) <= 5


def is_rosetta_table_like_unit(text: str) -> bool:
    compact = " ".join(text.split())
    if len(compact) < 80:
        return False
    metric_hits = len(
        re.findall(
            r"\\b(?:ODS|OIS|mIoU|FLOPs?|Param\\.?|Methods?|Year|Size|AMCM|GBST|DSCD|UNet|SegFormer|MFS|CSHF|SOTA|RIND|SFIAN|TITS|ICCV|CVPR|ICIP|WACV|ECCV|arXiv)\\b",
            compact,
        )
    )
    numeric_tokens = len(re.findall(r"(?<![A-Za-z])\\d+(?:\\.\\d+)?(?:[A-Za-z%]+)?", compact))
    table_symbols = len(re.findall(r"[✓×]|(?<![A-Za-z])[xX](?![A-Za-z])", compact))
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    if metric_hits >= 4 and numeric_tokens >= 8 and sentence_marks <= 8:
        return True
    if metric_hits >= 3 and table_symbols >= 4 and numeric_tokens >= 4:
        return True
    if numeric_tokens >= 18 and metric_hits >= 2 and sentence_marks <= 10:
        return True
    return False


def is_rosetta_figure_panel_label_unit(text: str) -> bool:
    compact = " ".join(text.split())
    if len(compact) < 40 or len(compact) > 260:
        return False
    if re.match(r"(?i)^fig(?:ure)?\\b", compact):
        return False
    panel_labels = re.findall(r"\\([a-z]\\)", compact, flags=re.IGNORECASE)
    if len(panel_labels) < 2:
        return False
    if not re.match(r"^\\([a-z]\\)", compact, flags=re.IGNORECASE):
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    return len(words) >= 6


def is_rosetta_diagram_label_unit(text: str, order_on_page: int) -> bool:
    compact = " ".join(text.split())
    if order_on_page > 4:
        return False
    if not compact or len(compact) > 480:
        return False
    if re.match(r"(?i)^(fig(?:ure)?|table)\\b", compact):
        return False
    if re.fullmatch(r"(?i)group\\s+[a-z0-9]+", compact):
        return True
    if re.search(r"[\\u4e00-\\u9fff]", compact) and len(compact) <= 20:
        return True

    placeholder_count = rosetta_placeholder_count(compact)
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    label_sentence_marks = rosetta_sentence_punctuation_count(re.sub(r"\\.{2,}", "", compact))
    words = re.findall(r"[A-Za-z]{2,}", compact)
    label_hits = len(
        re.findall(
            r"\\b(?:Raw|GT|Conv|DWConv|Point|Dilated|Input|Output|Concat|Upsample|Layer|Norm|softmax|dropout|Attention|Inward|Outward|Shift|Graph[A-Z]?|Focus|Features?|SCIU|BLOCK|MoveCamera|Camera|Control|Get|Upload|Process(?:ed)?|Initial|Video|Split|Combine|Resize|Frame|RIND|SFIAN|CTCrackSeg|DTrCNet|Crackmer|SCSegamba|MambaIR|CSMamba|PlainMamba|SimCrack|SCRWKV)\\b",
            compact,
        )
    )
    if "...." in compact and placeholder_count >= 1:
        return True
    if "...." in compact and label_hits >= 2 and label_sentence_marks <= 2:
        return True
    if label_hits >= 2 and len(words) <= 8 and sentence_marks == 0:
        return True
    if label_hits >= 4 and label_sentence_marks <= 3:
        return True
    if placeholder_count >= 3 and label_hits >= 2 and len(words) <= 45 and sentence_marks <= 4:
        return True
    return False


def mark_nontranslatable_layout_units(units: list[TranslationUnit]) -> None:
    for unit in units:
        if not unit.requiresTranslation:
            continue
        text = unit.sourceText.strip()
        if is_rosetta_page_number_unit(text):
            unit.requiresTranslation = False
            unit.kind = "page-number"
        elif is_rosetta_formula_like_unit(text):
            unit.requiresTranslation = False
            unit.kind = "formula"
        elif is_rosetta_table_like_unit(text):
            unit.requiresTranslation = False
            unit.kind = "table-like"
        elif is_rosetta_figure_panel_label_unit(text):
            unit.requiresTranslation = False
            unit.kind = "figure-panel-labels"
        elif is_rosetta_diagram_label_unit(text, unit.orderOnPage):
            unit.requiresTranslation = False
            unit.kind = "diagram-label"


def rosetta_nontranslatable_render_text(unit: TranslationUnit, text: str) -> str:
    if unit.kind == "duplicate-layer":
        return ""
    return text


'''


def patch_rosetta_engine_render_unit_matching(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: tolerate replay translate_many order drift."
    if "class _RenderTranslator" not in text:
        return False

    changed = False
    render_class_start = text.index("class _RenderTranslator")
    misplaced_marker = text.find(f"    # {marker}", 0, render_class_start)
    if misplaced_marker >= 0:
        misplaced_end = text.find("    def translate(self, text, *args, **kwargs):", misplaced_marker)
        if misplaced_end > misplaced_marker:
            text = text[:misplaced_marker] + text[misplaced_end:]
            changed = True

    old_init = """        self.expected_by_unit_id = {unit.unitId: unit for unit in expected_units}
        self.translations_by_unit_id = translations_by_unit_id
"""
    new_init = f"""        self.expected_by_unit_id = {{unit.unitId: unit for unit in expected_units}}
        self.expected_by_page: dict[int, list[TranslationUnit]] = {{}}
        for unit in expected_units:
            self.expected_by_page.setdefault(unit.pageNumber, []).append(unit)
        self._consumed_unit_ids: set[str] = set()
        self.translations_by_unit_id = translations_by_unit_id
"""
    if old_init in text:
        text = text.replace(old_init, new_init, 1)
        changed = True
    elif "self.expected_by_page" not in text:
        return False

    old_translate = """            unit_id = unit_id_for(self.current_page_number, order)
            expected = self.expected_by_unit_id.get(unit_id)
            if expected is None:
                raise ValueError(f"unknown translation unit requested: {unit_id}")
            if expected.sourceText != text:
                raise ValueError(f"translation unit order mismatch at {unit_id}")
            if unit_id not in self.translations_by_unit_id:
"""
    new_translate = """            unit_id = unit_id_for(self.current_page_number, order)
            expected = self._match_expected_unit(unit_id, text)
            unit_id = expected.unitId
            if unit_id not in self.translations_by_unit_id:
"""
    if old_translate in text:
        text = text.replace(old_translate, new_translate, 1)
        changed = True
    elif "expected = self._match_expected_unit(unit_id, text)" not in text:
        return False

    anchor = """    def translate(self, text, *args, **kwargs):
        return self.translate_many([text])[0]
"""
    helper = f'''    # {marker}
    def _match_expected_unit(self, unit_id: str, text: str) -> TranslationUnit:
        expected = self.expected_by_unit_id.get(unit_id)
        if (
            expected is not None
            and expected.unitId not in self._consumed_unit_ids
            and expected.sourceText == text
        ):
            self._consumed_unit_ids.add(expected.unitId)
            return expected
        for candidate in self.expected_by_page.get(self.current_page_number, []):
            if candidate.unitId in self._consumed_unit_ids:
                continue
            if candidate.sourceText == text:
                self._consumed_unit_ids.add(candidate.unitId)
                return candidate
        if expected is None:
            raise ValueError(f"unknown translation unit requested: {{unit_id}}")
        raise ValueError(f"translation unit order mismatch at {{unit_id}}")

'''
    render_class_start = text.index("class _RenderTranslator")
    render_class_end = text.find("\ndef prewarm", render_class_start)
    if render_class_end < 0:
        render_class_end = len(text)
    render_class_text = text[render_class_start:render_class_end]
    if helper.strip() not in render_class_text:
        anchor_index = text.find(anchor, render_class_start, render_class_end)
        if anchor_index < 0:
            raise SystemExit(f"::error::could not find expected render translator anchor in {target}")
        text = text[:anchor_index] + helper + text[anchor_index:]
        changed = True

    if changed:
        target.write_text(text, encoding="utf-8")
        print(f"[pdf2zh-pack] hardened Rosetta render unit matching in {target}")
    return changed


def duplicate_text_layer_helper() -> str:
    return '''def canonical_duplicate_text(text: str) -> str:
    normalized = (
        text.casefold()
        .replace("“", '"')
        .replace("”", '"')
        .replace("‘", "'")
        .replace("’", "'")
    )
    return "".join(char for char in normalized if char.isalnum() or char in "{}")


def duplicate_text_similarity(left: str, right: str) -> float:
    left_key = canonical_duplicate_text(left)
    right_key = canonical_duplicate_text(right)
    if not left_key or not right_key:
        return 0.0
    if left_key == right_key:
        return 1.0
    return difflib.SequenceMatcher(None, left_key, right_key, autojunk=False).ratio()


def duplicate_text_keys_match(left_key: str, right_key: str, threshold: float = 0.78) -> bool:
    if not left_key or not right_key:
        return False
    if left_key == right_key:
        return True
    matcher = difflib.SequenceMatcher(None, left_key, right_key, autojunk=False)
    if matcher.real_quick_ratio() < threshold:
        return False
    if matcher.quick_ratio() < threshold:
        return False
    return matcher.ratio() >= threshold


def mark_duplicate_text_layer_units(units: list[TranslationUnit]) -> None:
    if len(units) < 6:
        return
    canonical_keys = [canonical_duplicate_text(unit.sourceText) for unit in units]
    pair_matches: dict[tuple[int, int], bool] = {}
    best: tuple[int, int, int] | None = None
    for split in range(1, len(units)):
        pair_count = min(split, len(units) - split)
        if pair_count < 3:
            continue
        matched_pairs = 0
        matched_chars = 0
        compared_chars = 0
        for index in range(pair_count):
            duplicate_index = split + index
            duplicate = units[duplicate_index]
            compared_chars += max(1, duplicate.sourceChars)
            pair_key = (index, duplicate_index)
            if pair_key not in pair_matches:
                pair_matches[pair_key] = duplicate_text_keys_match(
                    canonical_keys[index], canonical_keys[duplicate_index]
                )
            if pair_matches[pair_key]:
                matched_pairs += 1
                matched_chars += max(1, duplicate.sourceChars)
        if matched_pairs / pair_count < 0.75:
            continue
        if matched_chars / max(1, compared_chars) < 0.75:
            continue
        if best is None or matched_chars > best[2]:
            best = (split, pair_count, matched_chars)
    if best is None:
        return
    split, pair_count, _matched_chars = best
    for index in range(pair_count):
        duplicate_index = split + index
        duplicate = units[duplicate_index]
        if pair_matches[(index, duplicate_index)]:
            duplicate.requiresTranslation = False
            duplicate.kind = "duplicate-layer"
'''


def patch_rosetta_engine_duplicate_text_layer_filter(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: suppress duplicate PDF text layers before translation."
    if marker in text:
        original = text
        duplicate_helper_start = text.find("def canonical_duplicate_text(text: str) -> str:\n")
        if duplicate_helper_start >= 0:
            duplicate_helper_end = text.find(
                "\ndef rosetta_placeholder_count(text: str) -> int:\n",
                duplicate_helper_start,
            )
            if duplicate_helper_end < 0:
                raise SystemExit(f"::error::could not find expected duplicate-layer helper end in {target}")
            text = (
                text[:duplicate_helper_start]
                + duplicate_text_layer_helper()
                + "\n"
                + text[duplicate_helper_end + 1 :]
            )
        text = text.replace(">= 0.82", ">= 0.78")
        old_sentence_count = '''def rosetta_sentence_punctuation_count(text: str) -> int:
    return sum(text.count(mark) for mark in ".;:!?")
'''
        new_sentence_count = '''def rosetta_sentence_punctuation_count(text: str) -> int:
    without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", text)
    return sum(without_decimal_points.count(mark) for mark in ".;:!?")
'''
        text = text.replace(old_sentence_count, new_sentence_count, 1)
        text = text.replace(
            '    without_decimal_points = re.sub(r"(?<=\\\\d)\\\\.(?=\\\\d)", "", text)\n',
            '    without_decimal_points = re.sub(r"(?<=\\d)\\.(?=\\d)", "", text)\n',
            1,
        )
        if "def mark_nontranslatable_layout_units(" not in text:
            helper_anchor = "def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:\n"
            if helper_anchor not in text:
                raise SystemExit(f"::error::could not find expected rosetta_engine helper anchor in {target}")
            text = text.replace(helper_anchor, nontranslatable_layout_helper() + helper_anchor, 1)
        if "def is_rosetta_formula_like_unit(" in text and "operator_hits = len(re.findall" not in text:
            old_formula_helper = '''def is_rosetta_formula_like_unit(text: str) -> bool:
    compact = " ".join(text.split())
    if len(compact) > 140:
        return False
    placeholder_count = rosetta_placeholder_count(compact)
    if placeholder_count < 3:
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    return len(words) <= 5
'''
            new_formula_helper = '''def is_rosetta_formula_like_unit(text: str) -> bool:
    compact = " ".join(text.split())
    placeholder_count = rosetta_placeholder_count(compact)
    if placeholder_count < 3:
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    operator_hits = len(re.findall(r"\\b(?:Partition|TopK|Gumbel|Softmax|Flatten|EM|LN|FFN|CR)\\b", compact))
    if len(compact) <= 180 and placeholder_count >= 6 and operator_hits >= 2 and rosetta_sentence_punctuation_count(compact) <= 2:
        return True
    if len(compact) > 140:
        return False
    return len(words) <= 5
'''
            if old_formula_helper not in text:
                raise SystemExit(f"::error::could not find expected formula-like helper anchor in {target}")
            text = text.replace(old_formula_helper, new_formula_helper, 1)
        if "def is_rosetta_figure_panel_label_unit(" not in text:
            panel_helper_anchor = "\n\ndef mark_nontranslatable_layout_units(units: list[TranslationUnit]) -> None:\n"
            if panel_helper_anchor not in text:
                raise SystemExit(f"::error::could not find expected nontranslatable layout helper anchor in {target}")
            panel_helper = '''\n\ndef is_rosetta_figure_panel_label_unit(text: str) -> bool:
    compact = " ".join(text.split())
    if len(compact) < 40 or len(compact) > 260:
        return False
    if re.match(r"(?i)^fig(?:ure)?\\b", compact):
        return False
    panel_labels = re.findall(r"\\([a-z]\\)", compact, flags=re.IGNORECASE)
    if len(panel_labels) < 2:
        return False
    if not re.match(r"^\\([a-z]\\)", compact, flags=re.IGNORECASE):
        return False
    words = re.findall(r"[A-Za-z]{2,}", compact)
    return len(words) >= 6
'''
            text = text.replace(panel_helper_anchor, panel_helper + panel_helper_anchor, 1)
        if "def is_rosetta_diagram_label_unit(" not in text:
            diagram_helper_anchor = "\n\ndef mark_nontranslatable_layout_units(units: list[TranslationUnit]) -> None:\n"
            if diagram_helper_anchor not in text:
                raise SystemExit(f"::error::could not find expected diagram label helper anchor in {target}")
            diagram_helper = '''\n\ndef is_rosetta_diagram_label_unit(text: str, order_on_page: int) -> bool:
    compact = " ".join(text.split())
    if order_on_page > 4:
        return False
    if not compact or len(compact) > 480:
        return False
    if re.match(r"(?i)^(fig(?:ure)?|table)\\b", compact):
        return False
    if re.fullmatch(r"(?i)group\\s+[a-z0-9]+", compact):
        return True
    if re.search(r"[\\u4e00-\\u9fff]", compact) and len(compact) <= 20:
        return True

    placeholder_count = rosetta_placeholder_count(compact)
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    label_sentence_marks = rosetta_sentence_punctuation_count(re.sub(r"\\.{2,}", "", compact))
    words = re.findall(r"[A-Za-z]{2,}", compact)
    label_hits = len(
        re.findall(
            r"\\b(?:Raw|GT|Conv|DWConv|Point|Dilated|Input|Output|Concat|Upsample|Layer|Norm|softmax|dropout|Attention|Inward|Outward|Shift|Graph[A-Z]?|Focus|Features?|SCIU|BLOCK|MoveCamera|Camera|Control|Get|Upload|Process(?:ed)?|Initial|Video|Split|Combine|Resize|Frame|RIND|SFIAN|CTCrackSeg|DTrCNet|Crackmer|SCSegamba|MambaIR|CSMamba|PlainMamba|SimCrack|SCRWKV)\\b",
            compact,
        )
    )
    if "...." in compact and placeholder_count >= 1:
        return True
    if "...." in compact and label_hits >= 2 and label_sentence_marks <= 2:
        return True
    if label_hits >= 2 and len(words) <= 8 and sentence_marks == 0:
        return True
    if label_hits >= 4 and label_sentence_marks <= 3:
        return True
    if placeholder_count >= 3 and label_hits >= 2 and len(words) <= 45 and sentence_marks <= 4:
        return True
    return False
'''
            text = text.replace(diagram_helper_anchor, diagram_helper + diagram_helper_anchor, 1)
        if "Attention|Inward|Outward|Shift" not in text:
            text = text.replace(
                "Attention|Shift|Graph[A-Z]?",
                "Attention|Inward|Outward|Shift|Graph[A-Z]?",
                1,
            )
        if "MoveCamera|Camera|Control|Get|Upload" not in text:
            text = text.replace(
                "SCIU|BLOCK|RIND",
                "SCIU|BLOCK|MoveCamera|Camera|Control|Get|Upload|Process(?:ed)?|Initial|Video|Split|Combine|Resize|Frame|RIND",
                1,
            )
        if 're.search(r"[\\u4e00-\\u9fff]", compact)' not in text:
            cjk_anchor = '''    if re.fullmatch(r"(?i)group\\s+[a-z0-9]+", compact):
        return True

    placeholder_count = rosetta_placeholder_count(compact)
'''
            cjk_replacement = '''    if re.fullmatch(r"(?i)group\\s+[a-z0-9]+", compact):
        return True
    if re.search(r"[\\u4e00-\\u9fff]", compact) and len(compact) <= 20:
        return True

    placeholder_count = rosetta_placeholder_count(compact)
'''
            if cjk_anchor in text:
                text = text.replace(cjk_anchor, cjk_replacement, 1)
            else:
                cjk_fallback_anchor = "    placeholder_count = rosetta_placeholder_count(compact)\n"
                cjk_fallback_replacement = '''    if re.search(r"[\\u4e00-\\u9fff]", compact) and len(compact) <= 20:
        return True
    placeholder_count = rosetta_placeholder_count(compact)
'''
                if cjk_fallback_anchor not in text:
                    raise SystemExit(f"::error::could not find expected diagram CJK-rule anchor in {target}")
                text = text.replace(cjk_fallback_anchor, cjk_fallback_replacement, 1)
        if "label_sentence_marks = rosetta_sentence_punctuation_count" not in text:
            sentence_anchor = '''    placeholder_count = rosetta_placeholder_count(compact)
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    words = re.findall(r"[A-Za-z]{2,}", compact)
'''
            sentence_replacement = '''    placeholder_count = rosetta_placeholder_count(compact)
    sentence_marks = rosetta_sentence_punctuation_count(compact)
    label_sentence_marks = rosetta_sentence_punctuation_count(re.sub(r"\\.{2,}", "", compact))
    words = re.findall(r"[A-Za-z]{2,}", compact)
'''
            if sentence_anchor not in text:
                raise SystemExit(f"::error::could not find expected diagram sentence-mark anchor in {target}")
            text = text.replace(sentence_anchor, sentence_replacement, 1)
        if "label_hits >= 2 and len(words) <= 8" not in text:
            label_short_anchor = '''    if "...." in compact and placeholder_count >= 1:
        return True
    if label_hits >= 4 and sentence_marks <= 3:
        return True
'''
            label_short_replacement = '''    if "...." in compact and placeholder_count >= 1:
        return True
    if label_hits >= 2 and len(words) <= 8 and sentence_marks == 0:
        return True
    if label_hits >= 4 and sentence_marks <= 3:
        return True
'''
            if label_short_anchor not in text:
                raise SystemExit(f"::error::could not find expected diagram label short-rule anchor in {target}")
            text = text.replace(label_short_anchor, label_short_replacement, 1)
        if '"...." in compact and label_hits >= 2' not in text:
            dot_label_anchor = '''    if "...." in compact and placeholder_count >= 1:
        return True
    if label_hits >= 2 and len(words) <= 8 and sentence_marks == 0:
'''
            dot_label_replacement = '''    if "...." in compact and placeholder_count >= 1:
        return True
    if "...." in compact and label_hits >= 2 and sentence_marks <= 2:
        return True
    if label_hits >= 2 and len(words) <= 8 and sentence_marks == 0:
'''
            if dot_label_anchor not in text:
                raise SystemExit(f"::error::could not find expected diagram dotted-label anchor in {target}")
            text = text.replace(dot_label_anchor, dot_label_replacement, 1)
        text = text.replace(
            'if "...." in compact and label_hits >= 2 and sentence_marks <= 2:',
            'if "...." in compact and label_hits >= 2 and label_sentence_marks <= 2:',
            1,
        )
        text = text.replace(
            "if label_hits >= 4 and sentence_marks <= 3:",
            "if label_hits >= 4 and label_sentence_marks <= 3:",
            1,
        )
        text = text.replace(
            "if placeholder_count >= 3 and len(words) <= 45 and sentence_marks <= 4:",
            "if placeholder_count >= 3 and label_hits >= 2 and len(words) <= 45 and sentence_marks <= 4:",
            1,
        )
        if "mark_nontranslatable_layout_units(page_units)" not in text:
            text = text.replace(
                "    mark_duplicate_text_layer_units(page_units)\n",
                "    mark_duplicate_text_layer_units(page_units)\n    mark_nontranslatable_layout_units(page_units)\n",
                1,
            )
        if 'unit.kind = "figure-panel-labels"' not in text:
            table_branch = '''        elif is_rosetta_table_like_unit(text):
            unit.requiresTranslation = False
            unit.kind = "table-like"
'''
            panel_branch = '''        elif is_rosetta_table_like_unit(text):
            unit.requiresTranslation = False
            unit.kind = "table-like"
        elif is_rosetta_figure_panel_label_unit(text):
            unit.requiresTranslation = False
            unit.kind = "figure-panel-labels"
'''
            if table_branch not in text:
                raise SystemExit(f"::error::could not find expected table-like branch in {target}")
            text = text.replace(table_branch, panel_branch, 1)
        if 'unit.kind = "diagram-label"' not in text:
            panel_branch = '''        elif is_rosetta_figure_panel_label_unit(text):
            unit.requiresTranslation = False
            unit.kind = "figure-panel-labels"
'''
            diagram_branch = '''        elif is_rosetta_figure_panel_label_unit(text):
            unit.requiresTranslation = False
            unit.kind = "figure-panel-labels"
        elif is_rosetta_diagram_label_unit(text, unit.orderOnPage):
            unit.requiresTranslation = False
            unit.kind = "diagram-label"
'''
            if panel_branch not in text:
                raise SystemExit(f"::error::could not find expected figure-panel branch in {target}")
            text = text.replace(panel_branch, diagram_branch, 1)
        if "def rosetta_nontranslatable_render_text(" not in text:
            render_text_anchor = "\n\ndef validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:\n"
            if render_text_anchor not in text:
                raise SystemExit(f"::error::could not find expected render text helper anchor in {target}")
            render_text_helper = '''\n\ndef rosetta_nontranslatable_render_text(unit: TranslationUnit, text: str) -> str:
    if unit.kind == "duplicate-layer":
        return ""
    return text
'''
            text = text.replace(render_text_anchor, render_text_helper + render_text_anchor, 1)
        old_non_required_render = '''            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if expected.requiresTranslation and expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
'''
        new_non_required_render = '''            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if not expected.requiresTranslation:
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            if expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
'''
        text = text.replace(old_non_required_render, new_non_required_render, 1)
        text = text.replace(
            '''                outputs.append("")
                continue
            translated = self.translations_by_unit_id[unit_id]
''',
            '''                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            translated = self.translations_by_unit_id[unit_id]
''',
            1,
        )
        text = text.replace(
            '''            if not expected.requiresTranslation:
                outputs.append("")
                continue
''',
            '''            if not expected.requiresTranslation:
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
''',
            1,
        )
        if text != original:
            target.write_text(text, encoding="utf-8")
            print(f"[pdf2zh-pack] refreshed duplicate text layer filtering in {target}")
            return True
        return False
    if "def collect_page_units(" not in text or "def render_one_page(" not in text:
        return False

    changed = False
    if "import difflib\n" not in text:
        if "from dataclasses import asdict, dataclass\n" not in text:
            raise SystemExit(f"::error::could not find expected rosetta_engine import anchor in {target}")
        text = text.replace(
            "from dataclasses import asdict, dataclass\n",
            "from dataclasses import asdict, dataclass\nimport difflib\n",
            1,
        )
        changed = True

    replacements = [
        (
            """            unitCount=len(collector.units),
            sourceChars=sum(unit.sourceChars for unit in collector.units),
""",
            """            unitCount=translatable_unit_count(collector.units),
            sourceChars=translatable_source_chars(collector.units),
""",
        ),
        (
            """    page_units = translator.units[before_count:]
    return _PageCache(
""",
            f"""    page_units = translator.units[before_count:]
    mark_duplicate_text_layer_units(page_units)
    mark_nontranslatable_layout_units(page_units)
    return _PageCache(
""",
        ),
        (
            """    source_chars = sum(unit.sourceChars for unit in cache.units)
    if not cache.units:
""",
            """    source_units = translatable_page_units(cache.units)
    source_chars = translatable_source_chars(cache.units)
    if not source_units:
""",
        ),
        (
            """    missing = [unit.unitId for unit in cache.units if unit.unitId not in translations_by_unit_id]
""",
            """    missing = [
        unit.unitId
        for unit in cache.units
        if unit.requiresTranslation and unit.unitId not in translations_by_unit_id
    ]
""",
        ),
        (
            """        sourceUnitCount=len(cache.units),
""",
            """        sourceUnitCount=translatable_unit_count(cache.units),
""",
        ),
        (
            """        sourceUnitCount=len(cache.units),
        translatedUnitCount=translated_unit_count,
""",
            """        sourceUnitCount=translatable_unit_count(cache.units),
        translatedUnitCount=translated_unit_count,
""",
        ),
        (
            """            if unit_id not in self.translations_by_unit_id:
                raise ValueError(f"missing translation for unit: {unit_id}")
            translated = self.translations_by_unit_id[unit_id]
""",
            """            if unit_id not in self.translations_by_unit_id:
                if expected.requiresTranslation:
                    raise ValueError(f"missing translation for unit: {unit_id}")
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            translated = self.translations_by_unit_id[unit_id]
""",
        ),
        (
            """            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if expected.requiresTranslation and expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
""",
            """            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if not expected.requiresTranslation:
                outputs.append(rosetta_nontranslatable_render_text(expected, text))
                continue
            if expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
""",
        ),
    ]

    for old, new in replacements:
        if old not in text:
            raise SystemExit(f"::error::could not find expected duplicate-layer fragment in {target}")
        text = text.replace(old, new, 1)
        changed = True

    helper_anchor = """def validate_translation_keys(units: list[TranslationUnit], translations: dict[str, str]) -> None:
"""
    helper = f'''# {marker}
def translatable_page_units(units: list[TranslationUnit]) -> list[TranslationUnit]:
    return [unit for unit in units if unit.requiresTranslation]


def translatable_unit_count(units: list[TranslationUnit]) -> int:
    return len(translatable_page_units(units))


def translatable_source_chars(units: list[TranslationUnit]) -> int:
    return sum(unit.sourceChars for unit in units if unit.requiresTranslation)


{duplicate_text_layer_helper()}
'''
    if helper_anchor not in text:
        raise SystemExit(f"::error::could not find expected rosetta_engine helper anchor in {target}")
    text = text.replace(helper_anchor, helper + nontranslatable_layout_helper() + helper_anchor, 1)
    changed = True

    if changed:
        target.write_text(text, encoding="utf-8")
        print(f"[pdf2zh-pack] enabled duplicate text layer filtering in {target}")
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


def patch_rosetta_engine_prepared_cache(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find rosetta_engine.py in {root}")

    text = target.read_text(encoding="utf-8")
    marker = '_PRISTINE_PREPARED_PDFS: dict[str, bytes] = {}'
    if marker in text:
        return False
    if "ENGINE_CONTRACT_VERSION = 2" not in text:
        return False

    prepared_runs_anchor = '_PREPARED_RUNS: dict[str, "_PreparedState"] = {}\n'
    if prepared_runs_anchor not in text:
        raise SystemExit(f"::error::could not find prepared-run registry in {target}")
    text = text.replace(
        prepared_runs_anchor,
        prepared_runs_anchor + marker + "\n",
        1,
    )

    register_anchor = "    _PREPARED_RUNS[prepared_run_id] = state\n"
    if register_anchor not in text:
        raise SystemExit(f"::error::could not find prepared-run registration in {target}")
    text = text.replace(
        register_anchor,
        register_anchor
        + "    _PRISTINE_PREPARED_PDFS[prepared_run_id] = prepared_pdf_path.read_bytes()\n",
        1,
    )

    dispose_anchor = """def disposeRun(preparedRunId: str) -> None:
    state = _PREPARED_RUNS.pop(preparedRunId, None)
    if state is None:
        return
"""
    if dispose_anchor not in text:
        raise SystemExit(f"::error::could not find disposeRun in {target}")
    reset_and_dispose = """def resetRun(preparedRunId: str) -> None:
    state = prepared_state(preparedRunId)
    pristine_pdf = _PRISTINE_PREPARED_PDFS[preparedRunId]
    fresh_doc = pymupdf.open(stream=pristine_pdf, filetype="pdf")
    old_doc = state.doc
    state.doc = fresh_doc
    try:
        old_doc.close()
    except Exception:
        pass


def disposeRun(preparedRunId: str) -> None:
    state = _PREPARED_RUNS.pop(preparedRunId, None)
    _PRISTINE_PREPARED_PDFS.pop(preparedRunId, None)
    if state is None:
        return
"""
    text = text.replace(dispose_anchor, reset_and_dispose, 1)

    target.write_text(text, encoding="utf-8")
    print(f"[pdf2zh-pack] enabled reusable pristine prepared runs in {target}")
    return True


def patch_rosetta_engine_persistent_layout_cache(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find rosetta_engine.py in {root}")

    text = target.read_text(encoding="utf-8")
    marker = "_PERSISTENT_LAYOUT_CACHE_SCHEMA = 1"
    if marker in text:
        old_version = 'ENGINE_VERSION = "rosetta-pdf-engine-v2"'
        new_version = 'ENGINE_VERSION = "rosetta-pdf-engine-v2.1"'
        if old_version in text:
            target.write_text(text.replace(old_version, new_version, 1), encoding="utf-8")
            print(f"[pdf2zh-pack] versioned durable layout cache in {target}")
            return True
        return False
    if "ENGINE_CONTRACT_VERSION = 2" not in text or "def prepareRun(" not in text:
        return False
    if (
        "selected_pages = normalize_pages(pages, page_count)" not in text
        or (
            "layout[page_index] = build_layout_mask" not in text
            and "layout.update(build_layout_masks" not in text
        )
        or "class EngineCapabilities:" not in text
    ):
        return False

    old_version = 'ENGINE_VERSION = "rosetta-pdf-engine-v2"'
    new_version = 'ENGINE_VERSION = "rosetta-pdf-engine-v2.1"'
    if old_version not in text and new_version not in text:
        raise SystemExit(f"::error::could not find rosetta_engine version in {target}")
    text = text.replace(old_version, new_version, 1)

    if "import json\n" not in text:
        import_anchor = "import io\n"
        if import_anchor not in text:
            raise SystemExit(f"::error::could not find rosetta_engine import anchor in {target}")
        text = text.replace(import_anchor, import_anchor + "import json\n", 1)

    registry_anchor = '_PRISTINE_PREPARED_PDFS: dict[str, bytes] = {}\n'
    if registry_anchor not in text:
        raise SystemExit(f"::error::could not find prepared PDF registry in {target}")
    text = text.replace(
        registry_anchor,
        registry_anchor
        + "_PERSISTENT_LAYOUT_CACHE_SCHEMA = 1\n"
        + "_PERSISTENT_LAYOUT_CACHE_MAX_ENTRIES = 12\n"
        + "_PERSISTENT_LAYOUT_CACHE_MAX_BYTES = 256 * 1024 * 1024\n",
        1,
    )

    prepared_run_anchor = """    sourceChars: int


@dataclass
class EngineCapabilities:
"""
    prepared_run_replacement = """    sourceChars: int
    persistentLayoutCacheHit: bool = False


@dataclass
class EngineCapabilities:
"""
    if prepared_run_anchor not in text:
        raise SystemExit(f"::error::could not find PreparedRun fields in {target}")
    text = text.replace(prepared_run_anchor, prepared_run_replacement, 1)

    prepare_anchor = "\ndef prepareRun(\n"
    if prepare_anchor not in text:
        raise SystemExit(f"::error::could not find prepareRun anchor in {target}")
    helpers = r'''

def persistent_layout_model_signature(model_path: str) -> dict[str, Any]:
    path = Path(model_path)
    stat = path.stat()
    return {
        "filename": path.name,
        "bytes": stat.st_size,
        "modifiedNs": stat.st_mtime_ns,
    }


def load_persistent_layout_cache(
    cache_dir: Path | None,
    cache_key: str,
    source_fingerprint: str,
    selected_pages: list[int],
    model_path: str,
) -> dict[int, Any] | None:
    if cache_dir is None or not cache_key:
        return None
    manifest_path = cache_dir / "manifest.json"
    layout_path = cache_dir / "layout.npz"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        expected = {
            "schemaVersion": _PERSISTENT_LAYOUT_CACHE_SCHEMA,
            "engineVersion": ENGINE_VERSION,
            "cacheKey": cache_key,
            "sourceFingerprint": source_fingerprint,
            "pages": selected_pages,
            "model": persistent_layout_model_signature(model_path),
        }
        if any(manifest.get(key) != value for key, value in expected.items()):
            return None
        expected_names = {f"page_{index}" for index in range(len(selected_pages))}
        with np.load(layout_path, allow_pickle=False) as archive:
            if set(archive.files) != expected_names:
                return None
            layout = {}
            for index in range(len(selected_pages)):
                mask = archive[f"page_{index}"]
                if mask.ndim != 2 or mask.dtype.kind not in "biuf":
                    return None
                layout[index] = np.ascontiguousarray(mask)
        now = time.time()
        os.utime(cache_dir, (now, now))
        return layout
    except Exception:
        return None


def persistent_layout_cache_entry_size(cache_dir: Path) -> int:
    total = 0
    try:
        for child in cache_dir.iterdir():
            if child.is_file():
                total += child.stat().st_size
    except OSError:
        return 0
    return total


def prune_persistent_layout_cache(cache_root: Path, keep_dir: Path) -> None:
    try:
        entries = [child for child in cache_root.iterdir() if child.is_dir()]
    except OSError:
        return
    rows = []
    for entry in entries:
        try:
            modified_ns = entry.stat().st_mtime_ns
        except OSError:
            modified_ns = 0
        rows.append((entry, modified_ns, persistent_layout_cache_entry_size(entry)))
    rows.sort(key=lambda row: row[1], reverse=True)
    total_bytes = sum(row[2] for row in rows)
    kept = len(rows)
    for entry, _modified_ns, entry_bytes in reversed(rows):
        if kept <= _PERSISTENT_LAYOUT_CACHE_MAX_ENTRIES and total_bytes <= _PERSISTENT_LAYOUT_CACHE_MAX_BYTES:
            break
        if entry == keep_dir:
            continue
        shutil.rmtree(entry, ignore_errors=True)
        total_bytes -= entry_bytes
        kept -= 1


def write_persistent_layout_cache(
    cache_dir: Path | None,
    cache_key: str,
    source_fingerprint: str,
    selected_pages: list[int],
    model_path: str,
    layout: dict[int, Any],
) -> None:
    if cache_dir is None or not cache_key:
        return
    cache_dir.mkdir(parents=True, exist_ok=True)
    token = uuid.uuid4().hex
    layout_tmp = cache_dir / f".layout-{token}.npz"
    manifest_tmp = cache_dir / f".manifest-{token}.json"
    layout_path = cache_dir / "layout.npz"
    manifest_path = cache_dir / "manifest.json"
    now_ms = int(time.time() * 1000)
    try:
        np.savez_compressed(
            layout_tmp,
            **{f"page_{index}": layout[index] for index in range(len(selected_pages))},
        )
        manifest = {
            "schemaVersion": _PERSISTENT_LAYOUT_CACHE_SCHEMA,
            "engineVersion": ENGINE_VERSION,
            "cacheKey": cache_key,
            "sourceFingerprint": source_fingerprint,
            "pages": selected_pages,
            "model": persistent_layout_model_signature(model_path),
            "layoutFile": "layout.npz",
            "createdAt": now_ms,
            "updatedAt": now_ms,
        }
        manifest_tmp.write_text(
            json.dumps(manifest, ensure_ascii=False, sort_keys=True),
            encoding="utf-8",
        )
        os.replace(layout_tmp, layout_path)
        os.replace(manifest_tmp, manifest_path)
        now = time.time()
        os.utime(cache_dir, (now, now))
        prune_persistent_layout_cache(cache_dir.parent, cache_dir)
    except Exception:
        for temporary in (layout_tmp, manifest_tmp):
            try:
                temporary.unlink()
            except OSError:
                pass

'''
    text = text.replace(prepare_anchor, helpers + prepare_anchor, 1)

    selected_pages_anchor = """    selected_pages = normalize_pages(pages, page_count)
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path, selected_pages)
"""
    selected_pages_replacement = """    selected_pages = normalize_pages(pages, page_count)
    model_path = _model_path_from_options(options)
    persistent_cache_text = str(options.get("persistentLayoutCacheDir") or "").strip()
    persistent_cache_dir = Path(persistent_cache_text) if persistent_cache_text else None
    persistent_cache_key = str(options.get("persistentLayoutCacheKey") or "")
    persistent_source_fingerprint = str(options.get("persistentSourceFingerprint") or "")
    persistent_layout = load_persistent_layout_cache(
        persistent_cache_dir,
        persistent_cache_key,
        persistent_source_fingerprint,
        selected_pages,
        model_path,
    )
    persistent_layout_cache_hit = persistent_layout is not None
    doc = prepare_pdf_document(input_path, font_path, noto_name, rosetta_bold_font_path, selected_pages)
"""
    if selected_pages_anchor not in text:
        raise SystemExit(f"::error::could not find selected page prepare anchor in {target}")
    text = text.replace(selected_pages_anchor, selected_pages_replacement, 1)

    model_anchor = """    model = get_layout_model(_model_path_from_options(options))
    layout: dict[int, Any] = {}
"""
    model_replacement = """    model = get_layout_model(model_path)
    layout: dict[int, Any] = {}
"""
    if model_anchor not in text:
        raise SystemExit(f"::error::could not find layout model anchor in {target}")
    text = text.replace(model_anchor, model_replacement, 1)

    layout_anchor = "            layout[page_index] = build_layout_mask(doc, page_index, model, options)\n"
    layout_replacement = """            if persistent_layout is None:
                layout[page_index] = build_layout_mask(doc, page_index, model, options)
            else:
                layout[page_index] = persistent_layout[page_index]
"""
    batched_layout_anchor = "            layout.update(build_layout_masks(doc, page_indices, model, options))\n"
    batched_layout_replacement = """            if persistent_layout is None:
                layout.update(build_layout_masks(doc, page_indices, model, options))
            else:
                layout.update(
                    {page_index: persistent_layout[page_index] for page_index in page_indices}
                )
"""
    if layout_anchor in text:
        text = text.replace(layout_anchor, layout_replacement, 1)
    elif batched_layout_anchor in text:
        text = text.replace(batched_layout_anchor, batched_layout_replacement, 1)
    else:
        raise SystemExit(f"::error::could not find layout inference anchor in {target}")

    state_anchor = """    state = _PreparedState(
"""
    state_replacement = """    if not persistent_layout_cache_hit:
        write_persistent_layout_cache(
            persistent_cache_dir,
            persistent_cache_key,
            persistent_source_fingerprint,
            selected_pages,
            model_path,
            layout,
        )

    state = _PreparedState(
"""
    if state_anchor not in text:
        raise SystemExit(f"::error::could not find prepared state anchor in {target}")
    text = text.replace(state_anchor, state_replacement, 1)

    result_anchor = """            sourceChars=translatable_source_chars(collector.units),
        )
"""
    result_replacement = """            sourceChars=translatable_source_chars(collector.units),
            persistentLayoutCacheHit=persistent_layout_cache_hit,
        )
"""
    if result_anchor not in text:
        raise SystemExit(f"::error::could not find PreparedRun result anchor in {target}")
    text = text.replace(result_anchor, result_replacement, 1)

    target.write_text(text, encoding="utf-8")
    print(f"[pdf2zh-pack] enabled durable layout cache in {target}")
    return True


def patch_rosetta_engine_resource_manager_reuse(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find rosetta_engine.py in {root}")

    text = target.read_text(encoding="utf-8")
    if "rsrcmgr=rsrcmgr" in text:
        return False
    if "def prepareRun(" not in text or "def collect_page_units(" not in text:
        return False

    prepare_anchor = "    page_caches: dict[int, _PageCache] = {}\n"
    if prepare_anchor not in text:
        return False
    text = text.replace(
        prepare_anchor,
        prepare_anchor + "    rsrcmgr = PDFResourceManager(caching=True)\n",
        1,
    )

    call_anchor = "                translator=collector,\n"
    if call_anchor not in text:
        return False
    text = text.replace(
        call_anchor,
        call_anchor + "                rsrcmgr=rsrcmgr,\n",
        1,
    )

    signature_anchor = "    translator: _UnitCollectorTranslator,\n    lang_in: str,\n"
    if signature_anchor not in text:
        return False
    text = text.replace(
        signature_anchor,
        "    translator: _UnitCollectorTranslator,\n    rsrcmgr: PDFResourceManager,\n    lang_in: str,\n",
        1,
    )

    local_manager = "    before_count = len(translator.units)\n    rsrcmgr = PDFResourceManager(caching=True)\n"
    if local_manager not in text:
        return False
    text = text.replace(
        local_manager,
        "    before_count = len(translator.units)\n",
        1,
    )

    target.write_text(text, encoding="utf-8")
    print(f"[pdf2zh-pack] reused pdfminer resource manager across prepared pages in {target}")
    return True


def patch_rosetta_engine_shared_font_registration(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find rosetta_engine.py in {root}")

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: share prepared PDF font objects across page resources."
    if marker in text:
        return False
    if "def prepare_pdf_document(" not in text:
        return False

    old_registration = '''    font_id = {}
    for page in doc:
        for font_name, font_file in font_list:
            font_id[font_name] = page.insert_font(font_name, font_file)
'''
    new_registration = '''    font_id = {}
    if doc.page_count:
        for font_name, font_file in font_list:
            font_id[font_name] = doc[0].insert_font(font_name, font_file)
        for page in doc:
            rosetta_pdf_register_page_fonts(doc, page.xref, font_list, font_id)
'''
    if old_registration not in text:
        return False
    text = text.replace(old_registration, new_registration, 1)

    helper = f'''# {marker}
def rosetta_pdf_xref_id(value: str) -> int:
    return int(re.search(r"(\\d+) 0 R", value).group(1))


def rosetta_pdf_page_font_resource_target(doc, page_xref: int) -> tuple[int, str]:
    resources = doc.xref_get_key(page_xref, "Resources")
    if resources[0] == "xref":
        resource_xref = rosetta_pdf_xref_id(resources[1])
        font_key = "Font"
    else:
        resource_xref = page_xref
        if resources[0] == "null":
            doc.xref_set_key(page_xref, "Resources", "<<>>")
        font_key = "Resources/Font"

    fonts = doc.xref_get_key(resource_xref, font_key)
    if fonts[0] == "xref":
        return rosetta_pdf_xref_id(fonts[1]), ""
    if fonts[0] == "null":
        doc.xref_set_key(resource_xref, font_key, "<<>>")
    return resource_xref, f"{{font_key}}/"


def rosetta_pdf_register_page_fonts(doc, page_xref: int, font_list, font_id) -> None:
    resource_xref, font_key_prefix = rosetta_pdf_page_font_resource_target(doc, page_xref)
    for font_name, _font_file in font_list:
        target_key = f"{{font_key_prefix}}{{font_name}}"
        if doc.xref_get_key(resource_xref, target_key)[0] == "null":
            doc.xref_set_key(resource_xref, target_key, f"{{font_id[font_name]}} 0 R")


'''
    prepare_anchor = "def prepare_pdf_document("
    text = text.replace(prepare_anchor, helper + prepare_anchor, 1)
    target.write_text(text, encoding="utf-8")
    print(f"[pdf2zh-pack] shared prepared PDF font objects across pages in {target}")
    return True


def patch_rosetta_engine_page_artifact_font_subsetting(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        raise SystemExit(f"::error::could not find rosetta_engine.py in {root}")

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: subset fonts in durable single-page artifacts."
    if marker in text:
        return False
    if "def render_one_page(" not in text:
        return False

    helper = f'''# {marker}
def rosetta_pdf_subset_page_fonts(doc) -> bool:
    try:
        doc.subset_fonts(verbose=False)
        return True
    except Exception:
        return False


'''
    render_anchor = "def render_one_page("
    text = text.replace(render_anchor, helper + render_anchor, 1)

    insert_anchor = '''        single = pymupdf.open()
        single.insert_pdf(state.doc, from_page=cache.page_index, to_page=cache.page_index)
        single.save(
'''
    insert_replacement = '''        single = pymupdf.open()
        single.insert_pdf(state.doc, from_page=cache.page_index, to_page=cache.page_index)
        subset_page_fonts = bool(state.options.get("singlePageSubsetFonts", True)) and rosetta_pdf_subset_page_fonts(single)
        single.save(
'''
    if insert_anchor not in text:
        return False
    text = text.replace(insert_anchor, insert_replacement, 1)

    save_anchor = '''            artifact_path,
            deflate=bool(state.options.get("singlePageDeflate", False)),
'''
    save_replacement = '''            artifact_path,
            garbage=4 if subset_page_fonts else 0,
            deflate=bool(state.options.get("singlePageDeflate", False)),
'''
    if save_anchor not in text:
        return False
    text = text.replace(save_anchor, save_replacement, 1)

    target.write_text(text, encoding="utf-8")
    print(f"[pdf2zh-pack] subset fonts in durable single-page artifacts in {target}")
    return True


def clear_pycache(root: Path) -> None:
    for cache_dir in root.rglob("__pycache__"):
        for child in cache_dir.iterdir():
            child.unlink()
        cache_dir.rmdir()


if "def rosetta_pdf_is_bold_font(" in text and "rosetta_pdf_is_bold_font(child.font)" not in text:
    text, changed = normalize_text_mode_operator(text)
    text, scalar_clamp_changed = patch_converter_scalar_layout_clamp(text)
    text, bold_font_changed = patch_converter_bold_font_support(text)
    text, cumulative_bold_changed = patch_cumulative_bold_marking(text)
    text, rendering_safety_changed = patch_converter_text_rendering_safety(text)
    text, centered_alignment_changed = patch_converter_centered_single_line_alignment(text)
    text, structural_line_breaks_changed = patch_converter_structural_line_breaks(text)
    text, formula_text_changed = patch_converter_formula_text_classification(text)
    if changed or scalar_clamp_changed or bold_font_changed or cumulative_bold_changed or rendering_safety_changed or centered_alignment_changed or structural_line_breaks_changed or formula_text_changed:
        target.write_text(text, encoding="utf-8")
        if changed:
            print(f"[pdf2zh-pack] normalized PDF faux-bold text mode in {target}")
        if scalar_clamp_changed:
            print(f"[pdf2zh-pack] optimized scalar PDF layout coordinate clamps in {target}")
        if bold_font_changed:
            print(f"[pdf2zh-pack] enabled simplified Chinese bold font switching in {target}")
        if cumulative_bold_changed:
            print(f"[pdf2zh-pack] made PDF paragraph bold marking cumulative in {target}")
        if rendering_safety_changed:
            print(f"[pdf2zh-pack] hardened translated PDF text masking and CJK line spacing in {target}")
        if centered_alignment_changed:
            print(f"[pdf2zh-pack] preserved centered single-line PDF paragraph alignment in {target}")
        if structural_line_breaks_changed:
            print(f"[pdf2zh-pack] preserved structural line breaks in translated PDF paragraphs in {target}")
        if formula_text_changed:
            print(f"[pdf2zh-pack] narrowed PDF formula classification for prose text in {target}")
    font_changed = patch_simplified_chinese_font(root)
    high_level_bold_changed = patch_high_level_bold_font_registration(root)
    engine_bold_changed = patch_rosetta_engine_bold_font_registration(root)
    selected_window_changed = patch_rosetta_engine_selected_page_window(root)
    duplicate_layer_changed = patch_rosetta_engine_duplicate_text_layer_filter(root)
    render_matching_changed = patch_rosetta_engine_render_unit_matching(root)
    engine_structural_line_breaks_changed = patch_rosetta_engine_structural_line_breaks(root)
    prepared_cache_changed = patch_rosetta_engine_prepared_cache(root)
    persistent_layout_cache_changed = patch_rosetta_engine_persistent_layout_cache(root)
    resource_manager_changed = patch_rosetta_engine_resource_manager_reuse(root)
    shared_font_registration_changed = patch_rosetta_engine_shared_font_registration(root)
    page_artifact_subsetting_changed = patch_rosetta_engine_page_artifact_font_subsetting(root)
    any_changed = (
        changed
        or scalar_clamp_changed
        or bold_font_changed
        or cumulative_bold_changed
        or rendering_safety_changed
        or centered_alignment_changed
        or structural_line_breaks_changed
        or formula_text_changed
        or font_changed
        or high_level_bold_changed
        or engine_bold_changed
        or selected_window_changed
        or duplicate_layer_changed
        or render_matching_changed
        or engine_structural_line_breaks_changed
        or prepared_cache_changed
        or persistent_layout_cache_changed
        or resource_manager_changed
        or shared_font_registration_changed
        or page_artifact_subsetting_changed
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
text, _ = patch_converter_scalar_layout_clamp(text)
text, _ = patch_converter_bold_font_support(text)
text, _ = patch_cumulative_bold_marking(text)
text, _ = patch_converter_text_rendering_safety(text)
text, _ = patch_converter_centered_single_line_alignment(text)
text, _ = patch_converter_structural_line_breaks(text)
text, _ = patch_converter_formula_text_classification(text)

target.write_text(text, encoding="utf-8")
print(f"[pdf2zh-pack] patched PDF text color and bold preservation in {target}")
patch_simplified_chinese_font(root)
patch_high_level_bold_font_registration(root)
patch_rosetta_engine_bold_font_registration(root)
patch_rosetta_engine_selected_page_window(root)
patch_rosetta_engine_duplicate_text_layer_filter(root)
patch_rosetta_engine_render_unit_matching(root)
patch_rosetta_engine_structural_line_breaks(root)
patch_rosetta_engine_prepared_cache(root)
patch_rosetta_engine_persistent_layout_cache(root)
patch_rosetta_engine_resource_manager_reuse(root)
patch_rosetta_engine_shared_font_registration(root)
patch_rosetta_engine_page_artifact_font_subsetting(root)
clear_pycache(root)
