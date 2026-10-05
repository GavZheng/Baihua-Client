#!/usr/bin/env python3
"""Build the embedded CJK fallback font from the Source Han Sans SC variable font.
The GUI ships one static, subset font inside the binary so phones have Han
glyphs even when the system font table is unreachable.
"""

from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

FONT_DIRECTORY = Path(__file__).resolve().parent
VARIABLE_SOURCE = FONT_DIRECTORY / "SourceHanSansSC-VF.ttf"
# Intermediate file (untracked, see .gitignore): the weight-pinned instance the
# subset is cut from. Kept on disk between runs so re-subsetting is fast.
STATIC_INSTANCE = FONT_DIRECTORY / "SourceHanSansSC-Regular.ttf"
SUBSET_OUTPUT = FONT_DIRECTORY / "SourceHanSansSC-Regular-Subset.ttf"
BODY_WEIGHT = 400

# Every range the interface may need to draw. Latin first (the interface's own
# alphabet), then the punctuation and symbol blocks egui or the language files
# use, then kana (Japanese user names), fullwidth forms and the Han blocks.
CHARACTER_RANGES = [
    (0x0020, 0x007E),  # Basic Latin
    (0x00A0, 0x00FF),  # Latin-1 Supplement
    (0x0100, 0x017F),  # Latin Extended-A
    (0x2000, 0x206F),  # General Punctuation (en dash, ellipsis, quotes)
    (0x20A0, 0x20BF),  # Currency Symbols
    (0x2100, 0x214F),  # Letterlike Symbols
    (0x2190, 0x21FF),  # Arrows
    (0x2200, 0x22FF),  # Mathematical Operators
    (0x2460, 0x24FF),  # Enclosed Alphanumerics
    (0x25A0, 0x25FF),  # Geometric Shapes
    (0x2600, 0x26FF),  # Miscellaneous Symbols
    (0x3000, 0x303F),  # CJK Symbols and Punctuation
    (0x3040, 0x30FF),  # Hiragana and Katakana
    (0x4E00, 0x9FFF),  # CJK Unified Ideographs
    (0xF900, 0xFAFF),  # CJK Compatibility Ideographs
    (0xFF00, 0xFFEF),  # Halfwidth and Fullwidth Forms
]


def requested_codepoints() -> list[int]:
    codepoints: list[int] = []
    for first, last in CHARACTER_RANGES:
        codepoints.extend(range(first, last + 1))
    return codepoints


def main() -> None:
    if not VARIABLE_SOURCE.is_file():
        raise SystemExit(f"the variable font must exist: {VARIABLE_SOURCE}")

    print(f"==> instancing {VARIABLE_SOURCE.name} at wght={BODY_WEIGHT}")
    variable = TTFont(VARIABLE_SOURCE)
    instance = instantiateVariableFont(variable, {"wght": BODY_WEIGHT}, inplace=True)
    instance.save(STATIC_INSTANCE)
    print(f"    wrote {STATIC_INSTANCE.name} ({STATIC_INSTANCE.stat().st_size} bytes)")

    print("==> subsetting to the interface character ranges")
    options = subset.Options()
    options.layout_features = []            # no shaping features: egui does not run them
    options.hinting = False                 # egui rasterizes with its own hinting
    options.glyph_names = False             # post table is dropped, save the space
    options.notdef_outline = True           # keep a visible tofu box as the last resort
    options.name_IDs = ["*"]
    options.name_legacy = False
    options.name_languages = ["*"]
    options.drop_tables += [
        "BASE", "STAT", "avar", "fvar", "GDEF", "GPOS", "GSUB",
        "HVAR", "VVAR", "vhea", "vmtx",
    ]
    font = subset.load_font(str(STATIC_INSTANCE), options)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=requested_codepoints())
    subsetter.subset(font)
    subset.save_font(font, str(SUBSET_OUTPUT), options)
    print(f"    wrote {SUBSET_OUTPUT.name} ({SUBSET_OUTPUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
