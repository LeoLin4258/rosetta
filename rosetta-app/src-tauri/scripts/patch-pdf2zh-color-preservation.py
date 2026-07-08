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
            if str(new).strip():
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

    return text, changed


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
    new_cls_condition = """                # Rosetta: table/legal-prose PDFs often put normal text in visual regions.
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
                    or (cls == xt_cls and len(sstk[-1].strip()) > 1 and child.size < pstk[-1].size * 0.79)  # 2. 角标字体，有 0.76 的角标和 0.799 的大写，这里用 0.79 取中，同时考虑首字母放大的情况
"""
    if "rosetta_text_like_visual_char" not in text:
        if old_cls_condition in text:
            text = text.replace(old_cls_condition, new_cls_condition, 1)
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


def patch_rosetta_engine_duplicate_text_layer_filter(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    if not target.is_file():
        return False

    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: suppress duplicate PDF text layers before translation."
    if marker in text:
        original = text
        old_canonical = '''def canonical_duplicate_text(text: str) -> str:
    normalized = (
        text.lower()
        .replace("“", '"')
        .replace("”", '"')
        .replace("‘", "'")
        .replace("’", "'")
    )
    return re.sub(r"[^a-z0-9{}]+", "", normalized)
'''
        new_canonical = '''def canonical_duplicate_text(text: str) -> str:
    normalized = (
        text.casefold()
        .replace("“", '"')
        .replace("”", '"')
        .replace("‘", "'")
        .replace("’", "'")
    )
    return "".join(char for char in normalized if char.isalnum() or char in "{}")
'''
        text = text.replace(old_canonical, new_canonical, 1)
        text = text.replace(">= 0.82", ">= 0.78")
        old_non_required_render = '''            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if expected.requiresTranslation and expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
'''
        new_non_required_render = '''            if not isinstance(translated, str):
                raise ValueError(f"translation is not a string for unit: {unit_id}")
            if not expected.requiresTranslation:
                outputs.append("")
                continue
            if expected.sourceText.strip() and not translated.strip():
                self.empty_translation_count += 1
'''
        text = text.replace(old_non_required_render, new_non_required_render, 1)
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
                outputs.append("")
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
                outputs.append("")
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


def canonical_duplicate_text(text: str) -> str:
    normalized = (
        text.casefold()
        .replace("“", '"')
        .replace("”", '"')
        .replace("‘", "'")
        .replace("’", "'")
    )
    return "".join(char for char in normalized if char.isalnum() or char in "{{}}")


def duplicate_text_similarity(left: str, right: str) -> float:
    left_key = canonical_duplicate_text(left)
    right_key = canonical_duplicate_text(right)
    if not left_key or not right_key:
        return 0.0
    if left_key == right_key:
        return 1.0
    return difflib.SequenceMatcher(None, left_key, right_key, autojunk=False).ratio()


def mark_duplicate_text_layer_units(units: list[TranslationUnit]) -> None:
    if len(units) < 6:
        return
    best: tuple[int, int, int] | None = None
    for split in range(1, len(units)):
        pair_count = min(split, len(units) - split)
        if pair_count < 3:
            continue
        matched_pairs = 0
        matched_chars = 0
        compared_chars = 0
        for index in range(pair_count):
            duplicate = units[split + index]
            compared_chars += max(1, duplicate.sourceChars)
            if duplicate_text_similarity(units[index].sourceText, duplicate.sourceText) >= 0.78:
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
        duplicate = units[split + index]
        if duplicate_text_similarity(units[index].sourceText, duplicate.sourceText) >= 0.78:
            duplicate.requiresTranslation = False
            duplicate.kind = "duplicate-layer"


'''
    if helper_anchor not in text:
        raise SystemExit(f"::error::could not find expected rosetta_engine helper anchor in {target}")
    text = text.replace(helper_anchor, helper + helper_anchor, 1)
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


def clear_pycache(root: Path) -> None:
    for cache_dir in root.rglob("__pycache__"):
        for child in cache_dir.iterdir():
            child.unlink()
        cache_dir.rmdir()


if "def rosetta_pdf_is_bold_font(" in text and "rosetta_pdf_is_bold_font(child.font)" not in text:
    text, changed = normalize_text_mode_operator(text)
    text, bold_font_changed = patch_converter_bold_font_support(text)
    text, cumulative_bold_changed = patch_cumulative_bold_marking(text)
    text, rendering_safety_changed = patch_converter_text_rendering_safety(text)
    text, formula_text_changed = patch_converter_formula_text_classification(text)
    if changed or bold_font_changed or cumulative_bold_changed or rendering_safety_changed or formula_text_changed:
        target.write_text(text, encoding="utf-8")
        if changed:
            print(f"[pdf2zh-pack] normalized PDF faux-bold text mode in {target}")
        if bold_font_changed:
            print(f"[pdf2zh-pack] enabled simplified Chinese bold font switching in {target}")
        if cumulative_bold_changed:
            print(f"[pdf2zh-pack] made PDF paragraph bold marking cumulative in {target}")
        if rendering_safety_changed:
            print(f"[pdf2zh-pack] hardened translated PDF text masking and CJK line spacing in {target}")
        if formula_text_changed:
            print(f"[pdf2zh-pack] narrowed PDF formula classification for prose text in {target}")
    font_changed = patch_simplified_chinese_font(root)
    high_level_bold_changed = patch_high_level_bold_font_registration(root)
    engine_bold_changed = patch_rosetta_engine_bold_font_registration(root)
    selected_window_changed = patch_rosetta_engine_selected_page_window(root)
    duplicate_layer_changed = patch_rosetta_engine_duplicate_text_layer_filter(root)
    any_changed = (
        changed
        or bold_font_changed
        or cumulative_bold_changed
        or rendering_safety_changed
        or formula_text_changed
        or font_changed
        or high_level_bold_changed
        or engine_bold_changed
        or selected_window_changed
        or duplicate_layer_changed
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
text, _ = patch_converter_text_rendering_safety(text)
text, _ = patch_converter_formula_text_classification(text)

target.write_text(text, encoding="utf-8")
print(f"[pdf2zh-pack] patched PDF text color and bold preservation in {target}")
patch_simplified_chinese_font(root)
patch_high_level_bold_font_registration(root)
patch_rosetta_engine_bold_font_registration(root)
patch_rosetta_engine_selected_page_window(root)
patch_rosetta_engine_duplicate_text_layer_filter(root)
clear_pycache(root)
