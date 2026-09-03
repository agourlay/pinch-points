#!/usr/bin/env python3
"""The Japanese face, cut down to the characters the game actually says.

DejaVu Sans Mono draws every other language the game speaks and has no
kanji at all, so Japanese needs a second face beside it. A whole CJK font
is twenty megabytes for the thousand-odd characters this game uses, so
this cuts one down to exactly those: every non-Latin character in the
Japanese table and the Japanese column of the level tables, and nothing
else. The result is a few hundred kilobytes and is embedded in the binary
like DejaVu is.

Source: Noto Sans Mono CJK JP, SIL Open Font License 1.1, face 5 of the
Noto Sans CJK collection that Debian ships as fonts-noto-cjk. Mono
because the rest of the UI is monospaced and the widths are what the
settings gutters are measured against. Pass --ttc to point at another
copy.

Run it after adding or rewording a Japanese string: a character that is
not in the subset draws as nothing at all. `the_japanese_font_carries_every_
character_the_tables_use` in i18n fails when this has not been re-run.
"""
import argparse
import re
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets/fonts/NotoSansMonoCJKjp-Subset.otf"
SOURCES = [
    ROOT / "src/app/i18n/ja.rs",
    ROOT / "src/app/i18n/levels.rs",
    # For 日本語 itself, which is not in the Japanese table: the picker
    # draws every language's own name for itself whatever it is set to.
    ROOT / "src/app/i18n/mod.rs",
]

DEFAULT_TTC = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"
FACE = 5  # Noto Sans Mono CJK JP

# What DejaVu already draws. Anything in here is left out of the subset:
# the fallback only runs for the scripts the subset claims, and every
# Latin, Cyrillic and ASCII character the Japanese lines contain (WASD,
# Enter, the digits) is drawn by the font the rest of the UI uses.
LATIN = re.compile("[\u0000-\u04ff]")


def wanted_characters() -> set[str]:
    """Every character the Japanese strings use that DejaVu cannot draw."""
    chars: set[str] = set()
    for path in SOURCES:
        for char in path.read_text(encoding="utf-8"):
            if not LATIN.match(char):
                chars.add(char)
    return chars


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ttc", default=DEFAULT_TTC)
    args = parser.parse_args()

    chars = wanted_characters()
    if not chars:
        print("no Japanese characters found in the tables", file=sys.stderr)
        return 1

    font = TTFont(args.ttc, fontNumber=FACE, lazy=False)
    options = subset.Options()
    # Nothing here needs a shaper's tables: the game draws one horizontal
    # line at a time, with no vertical writing and no ruby.
    options.layout_features = ["kern", "liga", "vert"]
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.notdef_outline = True
    options.recalc_bounds = True
    options.drop_tables += ["vhea", "vmtx", "VORG"]
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text="".join(sorted(chars)))
    subsetter.subset(font)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    font.save(OUT)
    size = OUT.stat().st_size
    print(f"{OUT.relative_to(ROOT)}: {len(chars)} characters, {size / 1024:.0f} KiB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
