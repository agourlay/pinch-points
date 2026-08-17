#!/usr/bin/env python3
"""Flag chips for the settings language row, one per Lang.

Two are drawn by hand - the Union Jack for its diagonals and the Hinomaru
for its disc - and the rest are stripes.

Drawn at 4x and downsampled, the same way gen_sprites.py works, so the
diagonals of the Union Jack come out smooth rather than stepped. Each is
3:2 with a dark hairline border, because several of these flags carry
white right up to the edge and the settings card is parchment.

The file name is the language's settings key (flag_en, flag_fr, ...), so
Art loads the set straight from ALL_LANGS without a second lookup table.

The Union Jack's saltire is drawn centred rather than counterchanged: the
chip is 21 pixels wide on screen and the offset it would need is a third
of one of them.
"""
from PIL import Image, ImageDraw

S = 4  # supersample factor
W, H = 96, 64

OUTLINE = (52, 38, 28, 255)
WHITE = (255, 255, 255, 255)


def canvas():
    img = Image.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)


def save(img, name):
    img = img.resize((W, H), Image.LANCZOS)
    img.save(f"assets/sprites/flag_{name}.png")
    print(f"flag_{name}")


def px(*vals):
    return tuple(v * S for v in vals)


def border(d):
    """A one-pixel dark edge, drawn last so nothing paints over it.

    In supersampled pixels rather than px(), which would stop a whole
    output pixel short of the far edge and leave a pale strip there.
    """
    d.rectangle([0, 0, W * S - 1, H * S - 1], outline=OUTLINE, width=S)


def bands(name, colours, vertical, weights=None):
    """A plain striped flag: the case six of the eight fall into."""
    img, d = canvas()
    weights = weights or [1] * len(colours)
    span = W if vertical else H
    total = sum(weights)
    at = 0
    last = len(colours) - 1
    for index, (colour, weight) in enumerate(zip(colours, weights)):
        # The last band runs to the edge rather than to a rounded offset,
        # so no seam of background shows through at the far side. Keyed on
        # the index: Spain's two reds are one interned tuple, so an
        # identity test would call the first band the last one.
        end = span if index == last else at + span * weight / total
        if vertical:
            d.rectangle([px(at, 0), px(end, H)], fill=colour)
        else:
            d.rectangle([px(0, at), px(W, end)], fill=colour)
        at = end
    border(d)
    save(img, name)


# --- en: the Union Jack -----------------------------------------------------
UJ_BLUE = (1, 33, 105, 255)
UJ_RED = (200, 16, 46, 255)
img, d = canvas()
d.rectangle([px(0, 0), px(W, H)], fill=UJ_BLUE)
# White saltire, then the red one inside it. Both are drawn as two wide
# lines corner to corner; the diagonal is what the supersampling is for.
for width, colour in ((13, WHITE), (5, UJ_RED)):
    d.line([px(0, 0), px(W, H)], fill=colour, width=width * S)
    d.line([px(0, H), px(W, 0)], fill=colour, width=width * S)
# The upright cross sits over the saltire: white ground, red on top.
for arm, colour in ((11, WHITE), (7, UJ_RED)):
    d.rectangle([px(0, (H - arm) / 2), px(W, (H + arm) / 2)], fill=colour)
    d.rectangle([px((W - arm) / 2, 0), px((W + arm) / 2, H)], fill=colour)
border(d)
save(img, "en")

# --- the striped six --------------------------------------------------------
bands("fr", [(0, 35, 149, 255), WHITE, (237, 41, 57, 255)], vertical=True)
bands("de", [(0, 0, 0, 255), (221, 0, 0, 255), (255, 206, 0, 255)], vertical=False)
bands("it", [(0, 140, 69, 255), (244, 245, 240, 255), (205, 33, 42, 255)], vertical=True)
bands("nl", [(174, 28, 40, 255), WHITE, (33, 70, 139, 255)], vertical=False)
bands("ru", [WHITE, (0, 57, 166, 255), (213, 43, 30, 255)], vertical=False)

# --- ja: the sun ------------------------------------------------------------
# The disc is three fifths of the height, centred, which is the flag's own
# proportion rounded to something this chip can hold.
img, d = canvas()
d.rectangle([px(0, 0), px(W, H)], fill=WHITE)
disc = H * 0.6
d.ellipse(
    [px((W - disc) / 2, (H - disc) / 2), px((W + disc) / 2, (H + disc) / 2)],
    fill=(188, 0, 45, 255),
)
border(d)
save(img, "ja")
# Spain's yellow band is twice the height of each red one, and the arms on
# the hoist are far below anything this chip can resolve.
bands(
    "es",
    [(170, 21, 27, 255), (241, 191, 0, 255), (170, 21, 27, 255)],
    vertical=False,
    weights=[1, 2, 1],
)
