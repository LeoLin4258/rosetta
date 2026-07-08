#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import sys
import tempfile
import urllib.request


REQUIRED_FONTS = [
    "SourceHanSansCN-Regular.ttf",
    "SourceHanSansCN-Bold.ttf",
    "GoNotoKurrent-Regular.ttf",
]


def patch_babeldoc_cache_env() -> Path:
    spec = importlib.util.find_spec("babeldoc.const")
    if spec is None or spec.origin is None:
        raise SystemExit("::error::could not locate babeldoc.const")
    target = Path(spec.origin)
    text = target.read_text(encoding="utf-8")
    marker = "Rosetta: allow the PDF component pack to own BabelDOC assets."
    if marker in text:
        return target

    old = 'CACHE_FOLDER = Path.home() / ".cache" / "babeldoc"\n'
    new = '''# Rosetta: allow the PDF component pack to own BabelDOC assets.
_rosetta_cache_folder = os.environ.get("ROSETTA_BABELDOC_CACHE_DIR")
CACHE_FOLDER = (
    Path(_rosetta_cache_folder).expanduser()
    if _rosetta_cache_folder
    else Path.home() / ".cache" / "babeldoc"
)
'''
    if old not in text:
        raise SystemExit(f"::error::could not find BabelDOC cache fragment in {target}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"[pdf2zh-pack] patched BabelDOC cache directory in {target}", file=sys.stderr)
    return target


def verified(path: Path, sha3_256: str) -> bool:
    from babeldoc.assets.assets import verify_file

    return verify_file(path, sha3_256)


def copy_if_verified(font: str, source_dir: Path, target: Path, sha3_256: str) -> bool:
    source = source_dir / font
    if not source.is_file():
        return False
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    if verified(target, sha3_256):
        print(f"[pdf2zh-pack] copied BabelDOC font: {font} <- {source}", file=sys.stderr)
        return True
    target.unlink(missing_ok=True)
    return False


def download_font(font: str, target: Path, sha3_256: str) -> None:
    from babeldoc.assets.embedding_assets_metadata import FONT_URL_BY_UPSTREAM

    target.parent.mkdir(parents=True, exist_ok=True)
    upstreams = ["modelscope", "hf-mirror", "huggingface", "github"]
    errors: list[str] = []
    for upstream in upstreams:
        if upstream not in FONT_URL_BY_UPSTREAM:
            continue
        url = FONT_URL_BY_UPSTREAM[upstream](font)
        fd, tmp_name = tempfile.mkstemp(
            prefix=f"{font}.", suffix=".download", dir=target.parent
        )
        os.close(fd)
        tmp = Path(tmp_name)
        try:
            print(f"[pdf2zh-pack] downloading BabelDOC font {font} from {upstream}", file=sys.stderr)
            with urllib.request.urlopen(url, timeout=120) as response:
                tmp.write_bytes(response.read())
            if not verified(tmp, sha3_256):
                raise RuntimeError("sha3_256 mismatch")
            tmp.replace(target)
            return
        except Exception as error:
            errors.append(f"{upstream}: {error}")
            tmp.unlink(missing_ok=True)
    joined = "; ".join(errors)
    raise SystemExit(f"::error::failed to stage BabelDOC font {font}: {joined}")


def stage_fonts(cache_dir: Path, font_source_dir: Path | None) -> None:
    os.environ["ROSETTA_BABELDOC_CACHE_DIR"] = str(cache_dir)

    from babeldoc.assets.embedding_assets_metadata import EMBEDDING_FONT_METADATA

    fonts_dir = cache_dir / "fonts"
    fallback_source = Path.home() / ".cache" / "babeldoc" / "fonts"
    for font in REQUIRED_FONTS:
        metadata = EMBEDDING_FONT_METADATA.get(font)
        if metadata is None:
            raise SystemExit(f"::error::BabelDOC metadata does not include {font}")
        sha3_256 = metadata["sha3_256"]
        target = fonts_dir / font
        if verified(target, sha3_256):
            print(f"[pdf2zh-pack] BabelDOC font already staged: {font}", file=sys.stderr)
            continue
        if font_source_dir and copy_if_verified(font, font_source_dir, target, sha3_256):
            continue
        if fallback_source != fonts_dir and copy_if_verified(font, fallback_source, target, sha3_256):
            continue
        download_font(font, target, sha3_256)
        if not verified(target, sha3_256):
            raise SystemExit(f"::error::staged BabelDOC font failed verification: {target}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--font-source-dir", type=Path)
    parser.add_argument("--patch-cache-env-only", action="store_true")
    args = parser.parse_args()

    patch_babeldoc_cache_env()
    if args.patch_cache_env_only:
        return
    if args.cache_dir is None:
        raise SystemExit("::error::--cache-dir is required unless --patch-cache-env-only is used")
    stage_fonts(args.cache_dir, args.font_source_dir)


if __name__ == "__main__":
    main()
