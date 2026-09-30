#!/usr/bin/env python3
"""Procedural sprite sheet for Pinch Points.

Draws at 4x the output size and downsamples for smooth edges. White/light
shapes are meant to be tinted by the engine (owner/kind colours); gulls,
rocks, and holes bake their palette in.

Every shape is written in a 96-unit design square (SIZE); `px` scales it to
the canvas. The file itself is OUT pixels square, twice the design size:
a tile is 64 world units in a 1280x720 layout, so 96 pixels is exactly a
tile on a 1080p screen, and the game opens fullscreen. At 1440p a tile is
128 pixels and at 4K 192, and a 96-pixel sprite came out soft on both.
The engine draws every sprite at an explicit size, so the resolution of
the file never moves anything on screen.
"""
import math
import random

from PIL import Image, ImageChops, ImageDraw

SIZE = 96  # design units: every coordinate below is in this square
OUT = 192  # pixels in the saved file: a tile on a 4K screen
S = OUT * 4 // SIZE  # canvas pixels per design unit, 4x supersampled

def canvas():
    img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)

def save(img, name):
    img = img.resize((OUT, OUT), Image.LANCZOS)
    img.save(f"assets/sprites/{name}.png")
    print(name)

def px(*vals):
    return tuple(v * S for v in vals)


def blob(cx, cy, r, seed, wobble=0.10, points=96):
    """An irregular closed outline around (cx, cy), in design units."""
    rng = random.Random(seed)
    waves = [(k, rng.uniform(0.5, 1.0) * wobble / math.sqrt(k),
              rng.uniform(0.0, 2.0 * math.pi)) for k in (2, 3, 5)]
    pts = []
    for i in range(points):
        a = 2.0 * math.pi * i / points
        f = 1.0 + sum(amp * math.sin(k * a + ph) for k, amp, ph in waves)
        pts.append(px(cx + r * f * math.cos(a), cy + r * f * math.sin(a)))
    return pts

OUTLINE = (52, 38, 28, 255)
WHITE = (255, 255, 255, 255)
LIGHT = (235, 235, 235, 255)

# --- arrow: fat shaft + big triangular head, pointing +X ---------------------
img, d = canvas()
def arrow_poly(inset):
    i = inset
    return [px(8 + i, 40 + i), px(52 - i//2, 40 + i), px(52 - i//2, 22 + i),
            px(90 - i, 48), px(52 - i//2, 74 - i), px(52 - i//2, 56 - i),
            px(8 + i, 56 - i)]
# A fat dark keel under a bright face: the arrow is the player's only verb
# and it is drawn on bright sand, so the outline carries the contrast and
# the fill carries the owner's colour.
# The outline is grown *outward* rather than inset. Inset, it ate the
# shaft: the shaft is sixteen pixels tall on this canvas, so seven pixels
# of border each side left two pixels of owner colour down the middle and
# the arrow came out black.
d.polygon(arrow_poly(-5), fill=OUTLINE)
d.polygon(arrow_poly(1), fill=WHITE)
d.polygon(arrow_poly(6), fill=LIGHT)
save(img, "arrow")

# --- arrow, worn: the same post after a round of weather ---------------------
# Wear used to be spelled with transparency alone, which cost the arrow the
# one thing it cannot spare: contrast against the sand. So a worn post keeps
# its ink and loses its edges - splintered bites out of the shaft and head,
# and two cracks across the face.
img, d = canvas()
d.polygon(arrow_poly(-5), fill=OUTLINE)
d.polygon(arrow_poly(1), fill=WHITE)
# splinter bites: wedges of nothing chewed out of the silhouette
for pts in [
    [(10, 38), (22, 40), (16, 50), (8, 46)],
    [(30, 56), (44, 58), (38, 68), (28, 62)],
    [(64, 30), (74, 36), (62, 42)],
    [(76, 58), (86, 52), (84, 64)],
]:
    d.polygon([px(*pt) for pt in pts], fill=(0, 0, 0, 0))
# cracks: dark hairlines across what is left
d.line([px(20, 42), px(40, 54)], fill=OUTLINE, width=3 * S)
d.line([px(46, 34), px(58, 58)], fill=OUTLINE, width=3 * S)
save(img, "arrow_worn")

# --- crab: a crab from above, facing +X, tintable -----------------------------
# Drawn right-handed: the crab's left is up the image (+Y in the engine) and
# its right is down. A left-handed crab is the same sprite mirrored.
#
# The old one was a disc with eight legs spread evenly round it, which at
# board size read as a beetle, and its claw was a separate dot. This is a
# carapace wider than it is long, legs grouped down its two sides, eyes on
# stalks at the front, and a small claw on the off hand. The big claw is
# the `claw` sprite, drawn in the same frame and tinted by handedness: the
# one thing a player reads a crab by is the side its big claw is on.
CARAPACE = [(24, 36), (30, 27), (42, 21), (56, 21), (64, 28), (68, 40), (68, 56),
            (64, 68), (56, 75), (42, 75), (30, 69), (24, 60)]
# hip on the carapace's edge -> knee -> foot, for the top side; the bottom
# side mirrors them. Short and swept back, as a crab's walking legs are:
# long ones, fanned out, were what made the old one read as a spider.
LEGS_A = [((30, 31), (22, 19), (12, 12)), ((37, 25), (31, 13), (22, 6)),
          ((45, 22), (43, 10), (36, 4)), ((53, 22), (56, 10), (52, 4))]
LEGS_B = [((30, 31), (20, 22), (9, 18)), ((37, 25), (29, 15), (18, 10)),
          ((45, 22), (45, 11), (40, 3)), ((53, 22), (58, 12), (58, 5))]


def crab_legs(d, legs, mirror):
    for hip, knee, foot in legs:
        pts = [hip, knee, foot]
        if mirror:
            pts = [(x, 96 - y) for (x, y) in pts]
        d.line([px(*q) for q in pts], fill=OUTLINE, width=4 * S, joint="curve")


def crab_body(legs):
    img, d = canvas()
    crab_legs(d, legs, mirror=False)
    crab_legs(d, legs, mirror=True)
    # small off-hand claw, on the crab's left: an arm and a closed pincer
    d.line([px(62, 30), px(72, 22)], fill=OUTLINE, width=6 * S)
    d.ellipse([px(68, 13), px(82, 25)], fill=OUTLINE)
    d.ellipse([px(70, 15), px(80, 23)], fill=WHITE)
    # carapace: outline, face, and a paler ridge down the middle
    d.polygon([px(*q) for q in CARAPACE], fill=OUTLINE)
    inner = [(44 + (x - 44) * 0.86, 48 + (y - 48) * 0.86) for (x, y) in CARAPACE]
    d.polygon([px(*q) for q in inner], fill=WHITE)
    d.ellipse([px(34, 34), px(58, 62)], fill=LIGHT)
    for (sx, sy) in [(32, 32), (50, 27), (32, 60), (50, 65)]:
        d.ellipse([px(sx, sy), px(sx + 4, sy + 4)], fill=LIGHT)
    # eyes on stalks at the front, close together
    for ey in (42, 54):
        d.line([px(64, ey), px(72, ey + (ey - 48) * 0.5)], fill=OUTLINE, width=3 * S)
    for (ex, ey) in ((69, 34), (69, 54)):
        d.ellipse([px(ex, ey), px(ex + 10, ey + 10)], fill=OUTLINE)
        d.ellipse([px(ex + 2, ey + 2), px(ex + 8, ey + 8)], fill=(252, 252, 252, 255))
        d.ellipse([px(ex + 4, ey + 3), px(ex + 8, ey + 7)], fill=(20, 16, 12, 255))
    return img


save(crab_body(LEGS_A), "crab")

# --- claw: the big claw, in the crab's own frame, tintable -------------------
# Drawn over the crab at the same size and place, so the two never come
# apart: an arm out of the carapace's front right, and a pincer twice the
# small claw's size, open, pointing ahead.
img, d = canvas()
d.line([px(60, 64), px(70, 72)], fill=OUTLINE, width=8 * S)
# the palm, then two fingers reaching forward with a gap between them
d.ellipse([px(62, 66), px(86, 90)], fill=OUTLINE)
d.polygon([px(78, 66), px(95, 70), px(93, 77), px(80, 77)], fill=OUTLINE)
d.polygon([px(78, 81), px(93, 81), px(95, 88), px(78, 90)], fill=OUTLINE)
d.ellipse([px(65, 69), px(83, 87)], fill=WHITE)
d.polygon([px(79, 69), px(92, 72), px(90, 75), px(80, 75)], fill=WHITE)
d.polygon([px(80, 83), px(90, 83), px(92, 86), px(79, 87)], fill=WHITE)
d.ellipse([px(68, 72), px(76, 78)], fill=LIGHT)
save(img, "claw")

# --- gull: a herring gull from above, facing +X -------------------------------
# The cues a gull is read by, from above: a white body tapering to the
# tail, a pale grey mantle, black wingtips with white spots, a round white
# head and a yellow beak with the red spot. The first drawing had those
# cues on an oval with big cartoon eyes, and flew on stiff straight wings
# like a paper plane.
GULL_WHITE = (248, 250, 252, 255)
GULL_GREY = (176, 186, 196, 255)
GULL_GREY_DEEP = (150, 160, 172, 255)
GULL_DARK = (40, 44, 50, 255)
BEAK = (240, 190, 60, 255)
BEAK_DARK = (150, 110, 30, 255)
GULL_EDGE = (52, 56, 62, 255)


def teardrop(x_tail, x_nose, fat_at, half, tail_half, y=48, steps=24):
    """A body outline: round at the nose, tapering to the tail."""
    top, bottom = [], []
    for i in range(steps + 1):
        t = i / steps
        x = x_tail + t * (x_nose - x_tail)
        if x < fat_at:
            u = (x - x_tail) / (fat_at - x_tail)
            w = tail_half + (half - tail_half) * math.sin(u * math.pi / 2)
        else:
            u = (x - fat_at) / (x_nose - fat_at)
            w = half * math.sqrt(max(0.0, 1 - u * u))
        top.append(px(x, y - w))
        bottom.append(px(x, y + w))
    return top + bottom[::-1]


def gull_head(d, cx, r, eyes=True):
    d.ellipse([px(cx - r - 2, 48 - r - 2), px(cx + r + 2, 48 + r + 2)], fill=GULL_EDGE)
    d.ellipse([px(cx - r, 48 - r), px(cx + r, 48 + r)], fill=GULL_WHITE)
    # the beak, a little hooked at the tip, with the red spot
    d.polygon([px(cx + r - 2, 44), px(cx + r + 11, 46), px(cx + r + 13, 49),
               px(cx + r + 10, 51), px(cx + r - 2, 52)], fill=BEAK_DARK)
    d.polygon([px(cx + r - 1, 45), px(cx + r + 10, 47), px(cx + r + 11, 49),
               px(cx + r + 9, 50), px(cx + r - 1, 51)], fill=BEAK)
    d.ellipse([px(cx + r + 7, 49), px(cx + r + 10, 52)], fill=(214, 70, 60, 255))
    if eyes:
        # small and dark, on the sides of the head where a gull's are
        for ey in (48 - r * 0.62, 48 + r * 0.62):
            d.ellipse([px(cx + 2, ey - 1.8), px(cx + 5.6, ey + 1.8)], fill=(24, 22, 20, 255))


# walking: wings folded along the back, their black tips crossing past the tail
img, d = canvas()
for sign in (-1, 1):
    d.polygon([px(44, 48 + sign * 6), px(6, 48 + sign * 2.5), px(4, 48 + sign * 5.5),
               px(26, 48 + sign * 9)], fill=GULL_DARK)
    d.ellipse([px(8, 48 + sign * 4 - 1.6), px(11.2, 48 + sign * 4 + 1.6)], fill=GULL_WHITE)
d.polygon([px(20, 42), px(6, 44), px(6, 52), px(20, 54)], fill=GULL_WHITE)
d.polygon(teardrop(12, 80, 50, 17, 5), fill=GULL_EDGE)
d.polygon(teardrop(14, 78, 50, 15, 3.5), fill=GULL_WHITE)
# the mantle: grey folded wings, parted down the back
d.polygon(teardrop(18, 66, 46, 13, 4), fill=GULL_GREY)
d.line([px(22, 48), px(58, 48)], fill=GULL_GREY_DEEP, width=2 * S)
gull_head(d, 72, 11)
save(img, "gull")


# One wing, outward from the body along +y, as (x, y): the leading edge
# from the shoulder out to a forward wrist and back to the tip, then the
# trailing edge home, scalloped by the flight feathers.
WING = [(55, 7), (58, 16), (59, 24), (55, 31), (46, 39), (32, 47),
        (28, 45), (33, 41), (33, 37), (37, 33), (37, 29), (41, 25),
        (40, 21), (44, 17), (43, 12), (46, 7)]
# The black of the tip, and where its white mirror spot sits.
WING_TIP = [(46, 39), (32, 47), (28, 45), (33, 41), (41, 35)]
WING_SPOT = (34, 43)


def gull_in_flight(span, sweep, name):
    """Wings at `span` of their reach, tips moved forward by `sweep`."""
    img, d = canvas()
    for sign in (-1, 1):
        def at(x, y):
            return px(x + sweep * y / 45, 48 + sign * y * span)
        d.polygon([at(x, y) for (x, y) in WING], fill=GULL_EDGE)
        inner = [(47 + (x - 47) * 0.88, 7 + (y - 7) * 0.94) for (x, y) in WING]
        d.polygon([at(x, y) for (x, y) in inner], fill=GULL_GREY)
        d.polygon([at(x, y) for (x, y) in WING_TIP], fill=GULL_DARK)
        # a darker line where the arm meets the hand
        d.line([at(53, 10), at(52, 22), at(46, 33)], fill=GULL_GREY_DEEP, width=S)
        sx, sy = at(*WING_SPOT)
        r = 1.9 * S
        d.ellipse([sx - r, sy - r, sx + r, sy + r], fill=GULL_WHITE)
    d.polygon([px(22, 42), px(8, 40), px(6, 48), px(8, 56), px(22, 54)], fill=GULL_EDGE)
    d.polygon([px(22, 43.5), px(9.5, 42), px(8, 48), px(9.5, 54), px(22, 52.5)], fill=GULL_WHITE)
    d.polygon(teardrop(16, 78, 48, 11, 4), fill=GULL_EDGE)
    d.polygon(teardrop(18, 76, 48, 9.5, 3), fill=GULL_WHITE)
    gull_head(d, 72, 9.5)
    save(img, name)


gull_in_flight(1.0, 0, "gull_fly")
# the upstroke: wings raised, so from above they span less and reach forward
gull_in_flight(0.62, 10, "gull_fly_b")

# --- rock: faceted boulder, baked -------------------------------------------
# Lit from the upper left, the way every shadow on the beach falls to the
# lower right (`layout::SUN`): a pale top, two shaded flanks, and a dark
# keel so it sits on the sand. It was three flat greys, which read as a
# paper cut-out beside the ponds and the castles.
img, d = canvas()
ROCK_EDGE = (44, 42, 40, 255)
silhouette = [px(18, 76), px(10, 52), px(18, 30), px(36, 16), px(60, 14),
              px(80, 26), px(88, 50), px(82, 72), px(62, 86), px(34, 86)]
d.polygon(silhouette, fill=ROCK_EDGE)
body = [px(22, 73), px(15, 52), px(22, 32), px(38, 20), px(59, 18),
        px(77, 29), px(84, 50), px(78, 69), px(60, 81), px(36, 81)]
d.polygon(body, fill=(92, 89, 84, 255))                      # the shaded flank
d.polygon([px(22, 32), px(38, 20), px(59, 18), px(77, 29), px(70, 46),
           px(46, 52), px(24, 48)], fill=(142, 138, 130, 255))  # the lit top
d.polygon([px(15, 52), px(22, 32), px(24, 48), px(46, 52), px(40, 74),
           px(22, 73)], fill=(114, 110, 104, 255))           # the near face
d.polygon([px(46, 52), px(70, 46), px(84, 50), px(78, 69), px(60, 81),
           px(40, 74)], fill=(76, 73, 69, 255))              # the far face, in shade
# a highlight along the top's front ridge, a crack, lichen, a pebble
d.line([px(26, 46), px(46, 50), px(68, 45)], fill=(170, 166, 156, 255),
       width=2 * S, joint="curve")
d.line([px(52, 22), px(48, 32), px(54, 40)], fill=(96, 92, 86, 255), width=2 * S)
for (lx, ly, r) in [(34, 30, 3), (40, 26, 2), (64, 34, 2.5)]:
    d.ellipse([px(lx - r, ly - r), px(lx + r, ly + r)], fill=(150, 164, 120, 255))
d.ellipse([px(80, 78), px(90, 86)], fill=ROCK_EDGE)
d.ellipse([px(81, 79), px(88, 84)], fill=(118, 114, 108, 255))
save(img, "rock")

# --- hole: spawner burrow, baked --------------------------------------------
# A burrow dug into the beach: a mound of thrown-out sand with clods round
# it, and a mouth with depth, its far wall catching the light from the
# upper left and its near wall in shadow. It was three flat ovals.
img, d = canvas()
rng = random.Random("hole")
for _ in range(9):
    a = rng.uniform(0, 2 * math.pi)
    r = rng.uniform(33, 38)
    cx, cy = 48 + r * math.cos(a), 50 + r * 0.8 * math.sin(a)
    k = rng.uniform(2.0, 3.6)
    d.ellipse([px(cx - k, cy - k), px(cx + k, cy + k)], fill=(176, 148, 104, 255))
d.polygon(blob(48, 50, 33, "hole-mound", wobble=0.09), fill=(188, 160, 114, 255))
d.polygon(blob(46, 47, 28, "hole-mound-top", wobble=0.08), fill=(206, 180, 134, 255))
d.polygon(blob(48, 50, 22, "hole-mouth", wobble=0.07), fill=(70, 52, 34, 255))
# the far wall, lower right, lit; the depths; the near wall's shadow
d.polygon(blob(51, 53, 17.5, "hole-far", wobble=0.07), fill=(112, 86, 58, 255))
d.polygon(blob(46, 47, 15, "hole-deep", wobble=0.07), fill=(30, 22, 14, 255))
d.arc([px(28, 31), px(68, 70)], 190, 290, fill=(44, 32, 20, 255), width=3 * S)
save(img, "hole")

# --- castle: sandcastles in three-quarter view, tintable --------------------
# The front of the tile is the bottom of the image: every piece shows a lit
# top and a shaded face below it, the way the rocks do. The engine tints
# them sand dyed with the owner's colour, and hangs the owner's own colour
# off them as flags and a banner, so a castle is a sandcastle first and
# whose it is second.
#
# `castle` is the keep, which every castle has. `wall_back` and
# `wall_front` are the curtain wall a first tier throws up, in two pieces
# so the keep stands inside it rather than on top of it, and `turret` is
# the corner tower later tiers add. `castle_trim` is the keep's banner and
# door arch, tinted in the owner's colour at full strength.
FACE = (214, 214, 214, 255)   # a tower's shaded front
GRAIN = (190, 190, 190, 255)  # sand grains pressed into the face


def tower(d, cx, top, bottom, rx, ry, merlons, grains):
    """A bucket-moulded tower: top ellipse at `top`, face down to `bottom`."""
    mw = rx * 2 / (merlons * 2 - 1)  # merlon width, with gaps as wide
    def rim_y(x):
        t = max(0.0, 1 - ((x - cx) / rx) ** 2)
        return top + ry * math.sqrt(t)
    # silhouette, grown a little, in outline
    g = 2.5
    d.ellipse([px(cx - rx - g, bottom - ry - g), px(cx + rx + g, bottom + ry + g)], fill=OUTLINE)
    d.rectangle([px(cx - rx - g, top), px(cx + rx + g, bottom)], fill=OUTLINE)
    d.ellipse([px(cx - rx - g, top - ry - g), px(cx + rx + g, top + ry + g)], fill=OUTLINE)
    for i in range(merlons):
        x0 = cx - rx + i * 2 * mw
        y = rim_y(x0 + mw / 2)
        d.rectangle([px(x0 - g, y - 7 - g), px(x0 + mw + g, y)], fill=OUTLINE)
    # face, then the flat top, then the merlons standing on its front rim
    d.ellipse([px(cx - rx, bottom - ry), px(cx + rx, bottom + ry)], fill=FACE)
    d.rectangle([px(cx - rx, top), px(cx + rx, bottom)], fill=FACE)
    for (gx, gy) in grains:
        d.ellipse([px(cx + gx * rx, top + gy * (bottom - top)),
                   px(cx + gx * rx + 2.2, top + gy * (bottom - top) + 2.2)], fill=GRAIN)
    d.ellipse([px(cx - rx, top - ry), px(cx + rx, top + ry)], fill=WHITE)
    d.ellipse([px(cx - rx * 0.62, top - ry * 0.55), px(cx + rx * 0.62, top + ry * 0.55)],
              fill=LIGHT)
    for i in range(merlons):
        x0 = cx - rx + i * 2 * mw
        y = rim_y(x0 + mw / 2)
        d.rectangle([px(x0, y - 7), px(x0 + mw, y)], fill=FACE)
        d.rectangle([px(x0, y - 7), px(x0 + mw, y - 4.5)], fill=WHITE)


GRAINS = [(-0.6, 0.3), (-0.2, 0.62), (0.35, 0.25), (0.6, 0.7), (-0.75, 0.8), (0.1, 0.9)]

img, d = canvas()
tower(d, 48, 30, 76, 26, 10, merlons=4, grains=GRAINS)
# door and a window slit, pressed into the face
d.rounded_rectangle([px(42, 66), px(54, 88)], radius=6 * S, fill=OUTLINE)
d.rectangle([px(46, 46), px(50, 56)], fill=OUTLINE)
save(img, "castle")

img, d = canvas()
# the owner's banner, hung on the face beside the door, and the door's arch
d.polygon([px(29, 44), px(37, 44), px(37, 64), px(33, 60), px(29, 64)], fill=OUTLINE)
d.polygon([px(30.5, 45.5), px(35.5, 45.5), px(35.5, 61), px(33, 58.5), px(30.5, 61)], fill=WHITE)
d.arc([px(40, 64), px(56, 80)], 180, 360, fill=WHITE, width=2 * S)
save(img, "castle_trim")

# --- the curtain wall, back and front ----------------------------------------
# A square of wall round the keep: the back and the two sides in one
# piece, drawn behind it, and the front, with its gate, drawn before it.
WALL = 9       # how thick the wall's top is, in design units
WALL_FACE = 9  # how tall its front face stands
L, R, T, B = 6, 90, 14, 78  # the wall's footprint, outer edges


def merlon_row(d, x0, x1, y, n):
    step = (x1 - x0) / (n * 2 - 1)
    for i in range(n):
        xa = x0 + i * 2 * step
        d.rectangle([px(xa, y - 5), px(xa + step, y)], fill=FACE)
        d.rectangle([px(xa, y - 5), px(xa + step, y - 3)], fill=WHITE)


img, d = canvas()
# back wall: its top, and its face seen over the courtyard
d.rectangle([px(L - 2, T - 7), px(R + 2, T + WALL + 2)], fill=OUTLINE)
d.rectangle([px(L, T), px(R, T + WALL)], fill=WHITE)
merlon_row(d, L, R, T, 7)
# side walls: long tops running down to the front wall
for x in (L, R - WALL):
    d.rectangle([px(x - 2, T), px(x + WALL + 2, B + 2)], fill=OUTLINE)
    d.rectangle([px(x, T), px(x + WALL, B)], fill=WHITE)
    d.rectangle([px(x + WALL * 0.35, T + WALL), px(x + WALL * 0.65, B - 2)], fill=LIGHT)
d.rectangle([px(L, T), px(R, T + WALL)], fill=WHITE)
merlon_row(d, L, R, T, 7)
save(img, "wall_back")

img, d = canvas()
# front wall: its top, then its face down to the sand, then the gate
d.rectangle([px(L - 2, B - 7), px(R + 2, B + WALL_FACE + 2)], fill=OUTLINE)
d.rectangle([px(L, B), px(R, B + WALL_FACE)], fill=FACE)
d.rectangle([px(L, B - WALL + 4), px(R, B)], fill=WHITE)
merlon_row(d, L, R, B - WALL + 4, 7)
for gx in (0.15, 0.3, 0.7, 0.85):
    x = L + gx * (R - L)
    d.ellipse([px(x, B + 3), px(x + 2.2, B + 5.2)], fill=GRAIN)
d.rounded_rectangle([px(41, B - 2), px(55, B + WALL_FACE + 2)], radius=5 * S, fill=OUTLINE)
save(img, "wall_front")

# --- turret: a corner tower, the keep in miniature ----------------------------
img, d = canvas()
tower(d, 48, 30, 72, 30, 12, merlons=3, grains=GRAINS[:4])
d.rectangle([px(45, 46), px(51, 58)], fill=OUTLINE)
save(img, "turret")

# --- sand tiles: baked speckled sand, two brightness variants ----------------
for name, base, speck_dark, speck_light in [
    ("sand_a", (237, 217, 176, 255), (219, 196, 152, 255), (247, 233, 203, 255)),
    ("sand_b", (230, 207, 161, 255), (211, 187, 141, 255), (242, 226, 192, 255)),
]:
    rng = random.Random(name)  # deterministic per variant
    img = Image.new("RGBA", (SIZE * S, SIZE * S), base)
    d = ImageDraw.Draw(img)
    for _ in range(210):  # fine grain
        x, y = rng.randrange(SIZE * S), rng.randrange(SIZE * S)
        r = rng.randrange(1 * S, 2 * S)
        col = speck_dark if rng.random() < 0.6 else speck_light
        d.ellipse([x - r, y - r, x + r, y + r], fill=col)
    for _ in range(4):  # a few pebbles/shell chips
        x, y = rng.randrange(8 * S, 88 * S), rng.randrange(8 * S, 88 * S)
        r = rng.randrange(2 * S, 4 * S)
        col = (208, 186, 148, 255) if rng.random() < 0.5 else (245, 238, 220, 255)
        d.ellipse([x - r, y - r, x + r, y + r], fill=col)
    save(img, name)

# --- shadow: soft radial blob, black with alpha falloff ----------------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
steps = 24
for i in range(steps):
    t = i / (steps - 1)          # 0 outer .. 1 inner
    r = int((46 - 34 * t) * S)
    a = int(6 + 110 * t * t)
    d.ellipse([px(48, 48)[0] - r, px(48, 48)[1] - r,
               px(48, 48)[0] + r, px(48, 48)[1] + r], fill=(0, 0, 0, a))
save(img, "shadow")

# --- plank: driftwood wall segment filling the canvas (the engine squashes
# --- the square texture to wall proportions) ---------------------------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([px(1, 4), px(95, 92)], radius=14 * S, fill=(74, 58, 44, 255))
d.rounded_rectangle([px(3, 12), px(93, 84)], radius=12 * S, fill=(104, 82, 60, 255))
d.rounded_rectangle([px(3, 12), px(93, 38)], radius=10 * S, fill=(128, 104, 78, 255))  # top light
rng = random.Random("plank")
for _ in range(7):  # grain streaks
    x0 = rng.randrange(8, 60)
    y0 = rng.randrange(30, 76)
    d.line([px(x0, y0), px(x0 + rng.randrange(10, 26), y0)],
           fill=(86, 66, 48, 255), width=3 * S)
save(img, "plank")

# --- bracket: cursor corner brackets, tintable -------------------------------
img, d = canvas()
t = 7   # arm thickness
l = 26  # arm length
for cx, cy, sx, sy in [(4, 4, 1, 1), (92, 4, -1, 1), (4, 92, 1, -1), (92, 92, -1, -1)]:
    d.rectangle([px(min(cx, cx + sx * l), min(cy, cy + sy * t)),
                 px(max(cx, cx + sx * l), max(cy, cy + sy * t))], fill=WHITE)
    d.rectangle([px(min(cx, cx + sx * t), min(cy, cy + sy * l)),
                 px(max(cx, cx + sx * t), max(cy, cy + sy * l))], fill=WHITE)
save(img, "bracket")

# --- crown: golden leader marker for the side panels, baked ------------------
img, d = canvas()
GOLD = (244, 196, 48, 255)
GOLD_DARK = (196, 148, 24, 255)
CROWN_OUTLINE = (92, 64, 20, 255)
# band
d.rectangle([px(14, 62), px(82, 82)], fill=CROWN_OUTLINE)
d.rectangle([px(17, 65), px(79, 79)], fill=GOLD_DARK)
# three points
d.polygon([px(14, 66), px(14, 22), px(34, 48), px(48, 16), px(62, 48),
           px(82, 22), px(82, 66)], fill=CROWN_OUTLINE)
d.polygon([px(18, 60), px(18, 32), px(35, 53), px(48, 25), px(61, 53),
           px(78, 32), px(78, 60)], fill=GOLD)
# jewels
for x in (26, 48, 70):
    d.ellipse([px(x - 4, 68), px(x + 4, 76)], fill=(214, 60, 60, 255))
save(img, "crown")

# --- kelp: seaweed clump, baked ----------------------------------------------
# Fronds, not stalks: each a ribbon that widens out of its holdfast and
# tapers to a tip, curving as it goes, with a pale midrib and the odd air
# bladder, over a damp patch of sand. It was five straight lines with a
# blob on each, which read as grass.
img, d = canvas()
rng = random.Random("kelp")
d.ellipse([px(16, 76), px(80, 92)], fill=(150, 128, 92, 110))   # the damp patch


def frond(x0, lean, top, width, phase):
    """A tapering ribbon from (x0, 86) up to its tip: it leans by `lean`,
    snakes as it rises, and its edges ruffle, the way a kelp blade does."""
    steps = 28
    left, right, rib = [], [], []
    for i in range(steps + 1):
        t = i / steps
        y = 86 - t * (86 - top)
        x = x0 + lean * t * t + 4.0 * t * math.sin(t * 2.0 * math.pi * 1.2 + phase)
        swell = 0.55 + 0.45 * math.sin(math.pi * min(1.0, t * 1.5))
        ruffle = 1.0 + 0.18 * math.sin(t * 2.0 * math.pi * 4.0 + phase)
        half = width * (1.0 - t) ** 0.7 * swell * ruffle
        left.append(px(x - half, y))
        right.append(px(x + half, y))
        rib.append(px(x, y))
    return left + right[::-1], rib


for x0, lean, top, width, phase in [(28, -16, 26, 8.5, 0.0), (43, -5, 12, 9.5, 2.1),
                                    (56, 9, 17, 9.0, 4.0), (69, 17, 32, 7.5, 1.0)]:
    shape, rib = frond(x0, lean, top, width, phase)
    d.polygon(shape, fill=(20, 70, 38, 255))
    inner, _ = frond(x0, lean, top + 2, width - 2.0, phase)
    d.polygon(inner, fill=(46, 130, 66, 255))
    d.line(rib[1:-2], fill=(118, 186, 110, 255), width=int(1.5 * S), joint="curve")
    # an air bladder a third of the way up
    bx, by = rib[9]
    r = 2.6 * S
    d.ellipse([bx - r, by - r, bx + r, by + r], fill=(150, 170, 70, 255))
save(img, "kelp")

# --- pool: an irregular pond, drawn in layers ---------------------------------
# It was a rounded square with two rings in it, which read as a button on
# the sand rather than as water. A pool is a blob instead: a circle whose
# radius wanders by a few low harmonics, seeded, so it is the same pond
# every run.
WET_SAND = (196, 168, 122, 255)
SHALLOW = (112, 176, 204, 255)
DEEP = (84, 146, 184, 255)

# The board draws pools from `puddle`, white and tinted, one layer at a
# time: wet sand, shallow water, deep water. Layers of one flat colour
# merge where they overlap, so neighbouring pool tiles, bridged by a
# stretched puddle between them, read as one pond with one shoreline.
img, d = canvas()
d.polygon(blob(48, 48, 40, "puddle"), fill=WHITE)
save(img, "puddle")

# `ripple`: a thin ring with the same wander, which the board swells and
# fades across the water, slowly, so the pond is never quite still.
img, d = canvas()
d.line(blob(48, 48, 40, "ripple", wobble=0.05) + [blob(48, 48, 40, "ripple", wobble=0.05)[0]],
       fill=WHITE, width=3 * S, joint="curve")
save(img, "ripple")

# `pool` is the same pond baked in its colours, for the places that draw a
# single one as an icon: the editor's brush and the menu's beach.
img, d = canvas()
d.polygon(blob(48, 48, 44, "puddle"), fill=WET_SAND)
d.polygon(blob(48, 48, 38, "puddle"), fill=SHALLOW)
d.polygon(blob(50, 50, 24, "deep"), fill=DEEP)
d.line(blob(46, 46, 28, "ripple", wobble=0.05) + [blob(46, 46, 28, "ripple", wobble=0.05)[0]],
       fill=(214, 236, 246, 150), width=2 * S, joint="curve")
d.ellipse([px(30, 28), px(46, 36)], fill=(214, 236, 246, 130))
save(img, "pool")

# --- log: turnstile driftwood, horizontal with a pivot peg -------------------
# Sea-bleached driftwood, a sun-faded brown, tapering unevenly, with grain along
# it, knots, cut ends showing their rings, and a wooden peg lashed through
# the middle that it swings on. It was a rounded bar with a dot on it.
img, d = canvas()
DRIFT_EDGE = (66, 52, 40, 255)
DRIFT = (152, 126, 98, 255)
DRIFT_LIT = (188, 164, 132, 255)
DRIFT_SHADE = (116, 94, 72, 255)


def log_outline(inset):
    """Top edge left to right, bottom edge back: thicker at the left end."""
    top = [(5, 40), (20, 37), (40, 38), (60, 39), (78, 40), (91, 42)]
    bottom = [(91, 55), (78, 57), (60, 58), (40, 59), (20, 60), (5, 57)]
    return [px(x + (inset if x < 48 else -inset), y + inset) for x, y in top] + \
           [px(x + (inset if x < 48 else -inset), y - inset) for x, y in bottom]


d.polygon(log_outline(0), fill=DRIFT_EDGE)
d.polygon(log_outline(2.5), fill=DRIFT)
d.polygon([px(8, 42), px(22, 40), px(42, 41), px(62, 42), px(88, 44),
           px(88, 47), px(60, 46), px(40, 45), px(20, 45), px(8, 46)], fill=DRIFT_LIT)
d.polygon([px(8, 54), px(22, 56), px(42, 55), px(62, 54), px(88, 52),
           px(88, 54), px(60, 56), px(40, 57), px(20, 58), px(8, 56)], fill=DRIFT_SHADE)
rng = random.Random("log")
for _ in range(7):
    x0 = rng.uniform(12, 70)
    y0 = rng.uniform(44, 55)
    d.line([px(x0, y0), px(x0 + rng.uniform(8, 18), y0 + rng.uniform(-1, 1))],
           fill=DRIFT_SHADE, width=int(1.5 * S))
for kx, ky in ((24, 50), (70, 47)):
    d.ellipse([px(kx - 3, ky - 2), px(kx + 3, ky + 2)], fill=DRIFT_EDGE)
    d.ellipse([px(kx - 1.6, ky - 1), px(kx + 1.6, ky + 1)], fill=DRIFT_SHADE)
# cut ends, rings and all
for ex, ey, rx, ry in ((6, 48.5, 3.4, 9.5), (90, 48.5, 2.8, 7.2)):
    d.ellipse([px(ex - rx, ey - ry), px(ex + rx, ey + ry)], fill=(204, 186, 158, 255))
    d.ellipse([px(ex - rx * 0.55, ey - ry * 0.55), px(ex + rx * 0.55, ey + ry * 0.55)],
              outline=DRIFT_SHADE, width=S)
# the pivot: a peg through the middle, lashed with rope
d.ellipse([px(40, 40), px(56, 57)], fill=(92, 70, 48, 255))
for i in range(4):
    y = 43 + i * 3.4
    d.line([px(41, y), px(55, y + 1.2)], fill=(214, 196, 150, 255), width=int(1.4 * S))
d.ellipse([px(43.5, 43.5), px(52.5, 52.5)], fill=DRIFT_EDGE)
d.ellipse([px(45, 45), px(51, 51)], fill=(150, 116, 80, 255))
d.ellipse([px(45.8, 45.6), px(48.4, 48)], fill=(196, 164, 120, 255))
save(img, "log")

# --- puff: soft white cloud for sand/dust/bubble bursts, tintable ------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
rng = random.Random("puff")
for _ in range(9):
    cx = rng.randrange(28, 68)
    cy = rng.randrange(28, 68)
    r = rng.randrange(10, 22) * S
    d.ellipse([cx * S - r, cy * S - r, cx * S + r, cy * S + r],
              fill=(255, 255, 255, 90))
d.ellipse([px(30, 30), px(66, 66)], fill=(255, 255, 255, 140))
save(img, "puff")

# --- star: four-point sparkle, tintable --------------------------------------
img, d = canvas()
d.polygon([px(48, 6), px(56, 40), px(90, 48), px(56, 56),
           px(48, 90), px(40, 56), px(6, 48), px(40, 40)], fill=WHITE)
d.polygon([px(48, 26), px(52, 44), px(70, 48), px(52, 52),
           px(48, 70), px(44, 52), px(26, 48), px(44, 44)],
          fill=(255, 255, 255, 255))
save(img, "star")

# --- foam: scalloped white edge strip (horizontal, top edge) -----------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
for i in range(6):
    cx = 8 + i * 16
    d.ellipse([px(cx - 9, 30), px(cx + 9, 54)], fill=(255, 255, 255, 200))
d.rectangle([px(0, 0), px(96, 40)], fill=(255, 255, 255, 200))
save(img, "foam")

# --- post: small driftwood knob for wall junctions ---------------------------
img, d = canvas()
d.ellipse([px(22, 22), px(74, 74)], fill=(58, 44, 32, 255))
d.ellipse([px(28, 28), px(68, 68)], fill=(96, 74, 52, 255))
d.ellipse([px(34, 34), px(54, 54)], fill=(126, 100, 72, 255))
save(img, "post")

# --- crab frame B: the other half of the walk cycle ------------------------
save(crab_body(LEGS_B), "crab_b")

# --- wet: sand-to-water gradient strip (fades downward) ----------------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
for row in range(SIZE * S):
    a = int(150 * (1.0 - row / (SIZE * S)))
    d.line([(0, row), (SIZE * S, row)], fill=(120, 100, 70, a))
save(img, "wet")

# --- cloud: soft lumpy cumulus, side-scrolling menu sky ----------------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
rng = random.Random("cloud")
# flat-ish base with puffy lobes on top
for (cx, cy, r) in [(20, 58, 14), (36, 50, 19), (54, 46, 21), (72, 54, 15),
                    (46, 58, 18), (62, 58, 14)]:
    rr = r * S
    d.ellipse([cx * S - rr, cy * S - rr, cx * S + rr, cy * S + rr],
              fill=(255, 255, 255, 235))
d.rectangle([px(10, 56), px(82, 66)], fill=(255, 255, 255, 235))
# a whisper of shading along the base
d.rectangle([px(12, 62), px(80, 66)], fill=(210, 218, 230, 180))
save(img, "cloud")

# --- boat: little sailboat in side view, sailing +X --------------------------
img, d = canvas()
HULL = (122, 78, 44, 255)
HULL_DARK = (92, 58, 32, 255)
SAIL = (245, 242, 230, 255)
MAST = (70, 50, 34, 255)
# hull: trapeze with a keel line
d.polygon([px(10, 62), px(86, 62), px(74, 80), px(22, 80)], fill=HULL)
d.polygon([px(10, 62), px(86, 62), px(83, 67), px(13, 67)], fill=HULL_DARK)
# mast
d.rectangle([px(46, 14), px(50, 62)], fill=MAST)
# main sail (leaning forward) and jib
d.polygon([px(50, 16), px(50, 58), px(82, 58)], fill=SAIL)
d.polygon([px(44, 22), px(44, 58), px(20, 58)], fill=(230, 224, 206, 255))
# tiny pennant
d.polygon([px(46, 14), px(46, 8), px(58, 11)], fill=(214, 60, 50, 255))
save(img, "boat")

# --- moat: the ring of water a tier-3 castle digs, baked ---------------------
# The pools' pond, dug round a castle: the same wet-sand shore, shallow and
# deep water and wandering outline, with the island the castle stands on
# left dry in the middle. It was a rounded square of water, a button round
# the castle the way the pools were buttons on the sand. It keeps the
# square's corners, since the wall inside it is square, but its edges
# wander like a pond's. The wall covers most of it; what shows is the shore
# and a band of water, so the ripples are drawn in that band.
def squircle(cx, cy, r, seed, wobble=0.04, points=128, n=4.0):
    """A rounded square whose edge wanders, in design units."""
    rng = random.Random(seed)
    waves = [(k, rng.uniform(0.5, 1.0) * wobble / math.sqrt(k),
              rng.uniform(0.0, 2.0 * math.pi)) for k in (3, 5, 7)]
    pts = []
    for i in range(points):
        a = 2.0 * math.pi * i / points
        c, s_ = math.cos(a), math.sin(a)
        base = r / (abs(c) ** n + abs(s_) ** n) ** (1.0 / n)
        f = 1.0 + sum(amp * math.sin(k * a + ph) for k, amp, ph in waves)
        pts.append(px(cx + base * f * c, cy + base * f * s_))
    return pts


img, d = canvas()
d.polygon(squircle(48, 48, 46, "moat-shore"), fill=WET_SAND)
d.polygon(squircle(48, 48, 43, "moat-water"), fill=SHALLOW)
d.polygon(squircle(48, 48, 41, "moat-deep", wobble=0.03), fill=DEEP)
ring = squircle(48, 48, 41.5, "moat-ripple", wobble=0.02)
# broken arcs rather than a whole ring: water catching the light
for start in range(6, len(ring), 32):
    d.line(ring[start:start + 9], fill=(214, 236, 246, 110), width=int(1.5 * S), joint="curve")
d.polygon(squircle(48, 48, 37, "moat-island", wobble=0.03), fill=WET_SAND)
d.polygon(squircle(48, 48, 35, "moat-island", wobble=0.03), fill=(0, 0, 0, 0))
save(img, "moat")

# --- feather: what is left when a gull gets a crab, tintable -----------------
img, d = canvas()
# vane: a leaf pointing +X, split by the quill
d.polygon([px(6, 48), px(38, 26), px(78, 38), px(92, 48),
           px(78, 58), px(38, 70)], fill=(226, 230, 236, 255))
d.polygon([px(10, 48), px(40, 32), px(76, 42), px(86, 48)],
          fill=(250, 251, 253, 255))
# quill
d.line([px(8, 48), px(92, 48)], fill=(178, 184, 192, 255), width=3 * S)
# barb notches along the trailing edge
for i in range(5):
    x = 30 + i * 12
    d.line([px(x, 52), px(x - 6, 66)], fill=(0, 0, 0, 0), width=3 * S)
# a dark tip, the way a herring gull's primaries end
d.polygon([px(78, 38), px(92, 48), px(78, 58)], fill=(78, 84, 92, 255))
save(img, "feather")

# --- ramp: a vertical alpha ramp, opaque at the top --------------------------
# One tintable texture for every soft gradient in the game: the sky and sea
# bands in the menu (which used to meet in hard seams), the inner shadow
# under the plank frame, and the wash at the end of a round.
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
for row in range(SIZE * S):
    a = int(255 * (1.0 - row / (SIZE * S - 1)))
    d.line([(0, row), (SIZE * S, row)], fill=(255, 255, 255, a))
save(img, "ramp")

# --- vignette: clear in the middle, dark at the edges, baked -----------------
# Stretched over the board to sink its corners a little; the sand is flat
# lit and without this the beach has no centre. Built from a radial ramp
# (black at the centre, white at the rim) used as the alpha channel, so the
# falloff is smooth to the corners rather than stepped in rings.
#
# The last few pixels are then taken back down to nothing. A radial ramp is
# at its *strongest* where the square texture stops - half strength along
# the straight edges, full at the corners - so without this the sprite ends
# in a hard rectangle of shadow drawn over the beach beyond the board, which
# is exactly the seam it was added to avoid.
mask = Image.radial_gradient("L").resize((SIZE * S, SIZE * S), Image.BILINEAR)
mask = mask.point(lambda v: int(130 * (v / 255.0) ** 2.2))
fade = Image.new("L", (SIZE * S, SIZE * S), 0)
d = ImageDraw.Draw(fade)
edge = 9 * S            # how deep the taper runs in from the border
steps = 24
for i in range(steps + 1):
    # Largest and darkest first, each smaller rectangle painting over it a
    # little brighter: the border ends at nothing and the inside is whole.
    t = i / steps
    inset = int(edge * t)
    d.rectangle(
        [inset, inset, SIZE * S - 1 - inset, SIZE * S - 1 - inset],
        fill=int(255 * t),
    )
mask = ImageChops.multiply(mask, fade)
img = Image.new("RGBA", (SIZE * S, SIZE * S), (8, 12, 20, 255))
img.putalpha(mask)
save(img, "vignette")

# --- ring: a thin bright circle, tintable ------------------------------------
# The shape every "something happened here" gets: a shockwave that swells
# out of a tile and thins away. Drawn hollow so it reads as a wave rather
# than a disc growing over the board.
img, d = canvas()
d.ellipse([px(6, 6), px(90, 90)], outline=WHITE, width=7 * S)
d.ellipse([px(13, 13), px(83, 83)], outline=(255, 255, 255, 110), width=3 * S)
save(img, "ring")
