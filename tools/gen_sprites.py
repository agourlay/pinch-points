#!/usr/bin/env python3
"""Procedural sprite sheet for Pinch Points.

Draws at 4x and downsamples for smooth edges. White/light shapes are meant
to be tinted by the engine (owner/kind colours); gulls, rocks, and holes
bake their palette in.
"""
from PIL import Image, ImageChops, ImageDraw

S = 4  # supersample factor
SIZE = 96

def canvas():
    img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)

def save(img, name):
    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    img.save(f"assets/sprites/{name}.png")
    print(name)

def px(*vals):
    return tuple(v * S for v in vals)

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

# --- crab: jointed legs, stalked eyes, patterned shell; tintable ------------
img, d = canvas()
# four jointed legs per side: hip -> knee -> foot
LEGS = [((34, 32), (16, 20), (6, 10)), ((30, 42), (12, 38), (2, 32)),
        ((30, 54), (12, 58), (2, 64)), ((34, 64), (16, 76), (6, 86))]
for hip, knee, foot in LEGS:
    for (a, b) in [(hip, knee), (knee, foot)]:
        d.line([px(*a), px(*b)], fill=OUTLINE, width=5 * S)
    mx, my = 96 - hip[0], hip[1]
    kx, ky = 96 - knee[0], knee[1]
    fx, fy = 96 - foot[0], foot[1]
    # mirrored on the leading side
    d.line([px(62, hip[1]), px(kx - 26, ky)], fill=OUTLINE, width=5 * S)
    d.line([px(kx - 26, ky), px(fx - 26, fy)], fill=OUTLINE, width=5 * S)
# shell: broad oval with outline
d.ellipse([px(20, 18), px(76, 78)], fill=OUTLINE)
d.ellipse([px(24, 22), px(72, 74)], fill=WHITE)
# shell pattern: dimple plus speckles
d.ellipse([px(32, 32), px(64, 64)], fill=LIGHT)
for (sx, sy) in [(34, 28), (58, 26), (30, 60), (60, 66), (46, 70)]:
    d.ellipse([px(sx, sy), px(sx + 4, sy + 4)], fill=LIGHT)
# small off-hand claw hint at the front
d.ellipse([px(66, 20), px(80, 32)], fill=OUTLINE)
d.ellipse([px(68, 22), px(78, 30)], fill=WHITE)
# googly eyes on stalks at the front (+X)
d.line([px(66, 38), px(78, 32)], fill=OUTLINE, width=3 * S)
d.line([px(66, 58), px(78, 64)], fill=OUTLINE, width=3 * S)
d.ellipse([px(74, 26), px(88, 40)], fill=OUTLINE)
d.ellipse([px(74, 56), px(88, 70)], fill=OUTLINE)
d.ellipse([px(76, 28), px(86, 38)], fill=(252, 252, 252, 255))
d.ellipse([px(76, 58), px(86, 68)], fill=(252, 252, 252, 255))
d.ellipse([px(80, 31), px(85, 36)], fill=(20, 16, 12, 255))
d.ellipse([px(80, 61), px(85, 66)], fill=(20, 16, 12, 255))
save(img, "crab")

# --- claw: open pincer, tintable --------------------------------------------
img, d = canvas()
d.ellipse([px(16, 16), px(88, 88)], fill=OUTLINE)
d.ellipse([px(22, 22), px(82, 82)], fill=WHITE)
# the pincer gap: wedge cut toward +X
d.polygon([px(52, 52), px(100, 24), px(100, 80)], fill=(0, 0, 0, 0))
d.polygon([px(52, 52), px(96, 30), px(96, 74)], fill=(0, 0, 0, 0))
save(img, "claw")

# --- gull: walking, folded wings, facing +X ---------------------------------
# The recognizable herring-gull cues: white body, pale grey mantle, BLACK
# wingtips crossing at the tail, yellow beak with the red spot.
GULL_WHITE = (248, 250, 252, 255)
GULL_GREY = (176, 186, 196, 255)
GULL_DARK = (52, 56, 62, 255)
BEAK = (240, 190, 60, 255)
img, d = canvas()
# tail fan with dark band
d.polygon([px(16, 40), px(2, 44), px(2, 52), px(16, 56)], fill=GULL_WHITE)
d.polygon([px(8, 42), px(2, 44), px(2, 52), px(8, 54)], fill=GULL_DARK)
# body: white teardrop
d.ellipse([px(12, 28), px(74, 68)], fill=OUTLINE)
d.ellipse([px(15, 31), px(71, 65)], fill=GULL_WHITE)
# folded-wing mantle: grey shield over the back
d.ellipse([px(18, 35), px(60, 61)], fill=GULL_GREY)
# crossed black wingtips pointing at the tail
d.polygon([px(44, 38), px(10, 44), px(26, 48), px(46, 44)], fill=GULL_DARK)
d.polygon([px(44, 58), px(10, 52), px(26, 48), px(46, 52)], fill=GULL_DARK)
# head: white, proud
d.ellipse([px(56, 32), px(86, 62)], fill=OUTLINE)
d.ellipse([px(58, 34), px(84, 60)], fill=GULL_WHITE)
# yellow beak with the red spot
d.polygon([px(82, 42), px(97, 46), px(97, 50), px(82, 54)], fill=(150, 110, 30, 255))
d.polygon([px(82, 43), px(95, 47), px(95, 49), px(82, 53)], fill=BEAK)
d.ellipse([px(88, 49), px(92, 53)], fill=(214, 70, 60, 255))
# eyes: two, slightly toward the top for the top-down read
d.ellipse([px(68, 36), px(76, 44)], fill=(255, 255, 255, 255))
d.ellipse([px(68, 52), px(76, 60)], fill=(255, 255, 255, 255))
d.ellipse([px(71, 38), px(75, 42)], fill=(24, 22, 20, 255))
d.ellipse([px(71, 54), px(75, 58)], fill=(24, 22, 20, 255))
save(img, "gull")

# --- gull in flight: wings spread wide, unmistakably a seabird ---------------
img, d = canvas()
# wings: long, angled back, grey with black tips and white mirror spots
for sign in (-1, 1):
    def wy(y):
        return 48 + sign * y
    d.polygon([px(30, wy(6)), px(16, wy(34)), px(28, wy(46)),
               px(48, wy(40)), px(50, wy(8))], fill=OUTLINE)
    d.polygon([px(32, wy(8)), px(19, wy(33)), px(28, wy(43)),
               px(46, wy(38)), px(48, wy(10))], fill=GULL_GREY)
    # black wingtip with white spot
    d.polygon([px(19, wy(33)), px(28, wy(43)), px(24, wy(46)),
               px(14, wy(36))], fill=GULL_DARK)
    spot_y0, spot_y1 = sorted((wy(37), wy(41)))
    d.ellipse([px(20, spot_y0), px(24, spot_y1)], fill=GULL_WHITE)
# tail fan with dark band
d.polygon([px(20, 42), px(4, 40), px(2, 48), px(4, 56), px(20, 54)], fill=GULL_WHITE)
d.polygon([px(8, 41), px(2, 46), px(2, 50), px(8, 55)], fill=GULL_DARK)
# body
d.ellipse([px(14, 36), px(70, 60)], fill=OUTLINE)
d.ellipse([px(17, 38), px(67, 58)], fill=GULL_WHITE)
# head + beak
d.ellipse([px(56, 34), px(84, 62)], fill=OUTLINE)
d.ellipse([px(58, 36), px(82, 60)], fill=GULL_WHITE)
d.polygon([px(80, 43), px(96, 47), px(96, 49), px(80, 53)], fill=BEAK)
d.ellipse([px(88, 48), px(92, 52)], fill=(214, 70, 60, 255))
d.ellipse([px(68, 38), px(76, 46)], fill=(255, 255, 255, 255))
d.ellipse([px(71, 40), px(75, 44)], fill=(24, 22, 20, 255))
save(img, "gull_fly")

# --- rock: faceted boulder, baked -------------------------------------------
img, d = canvas()
pts = [px(20, 78), px(10, 46), px(28, 20), px(58, 12), px(84, 30),
       px(88, 62), px(66, 84)]
d.polygon(pts, fill=(64, 62, 58, 255))
d.polygon([px(28, 24), px(56, 16), px(78, 32), px(52, 44), px(24, 44)],
          fill=(112, 110, 104, 255))
d.polygon([px(24, 46), px(52, 46), px(46, 74), px(22, 70)],
          fill=(88, 86, 82, 255))
save(img, "rock")

# --- hole: spawner burrow, baked --------------------------------------------
img, d = canvas()
d.ellipse([px(10, 18), px(86, 82)], fill=(196, 168, 120, 255))  # sand rim
d.ellipse([px(16, 24), px(80, 76)], fill=(94, 70, 44, 255))
d.ellipse([px(24, 32), px(72, 68)], fill=(44, 32, 20, 255))
save(img, "hole")

# --- castle: sandcastle keep - towers, drip-sand, shell door; tintable ------
img, d = canvas()
d.rectangle([px(12, 12), px(84, 84)], fill=OUTLINE)
d.rectangle([px(16, 16), px(80, 80)], fill=WHITE)
# battlements: notch the border on all sides
for i in range(4):
    off = 20 + i * 15
    d.rectangle([px(off, 8), px(off + 8, 18)], fill=(0, 0, 0, 0))
    d.rectangle([px(off, 78), px(off + 8, 90)], fill=(0, 0, 0, 0))
    d.rectangle([px(8, off), px(18, off + 8)], fill=(0, 0, 0, 0))
    d.rectangle([px(78, off), px(90, off + 8)], fill=(0, 0, 0, 0))
# bucket-moulded corner towers
for cx, cy in [(20, 20), (76, 20), (20, 76), (76, 76)]:
    d.ellipse([px(cx - 9, cy - 9), px(cx + 9, cy + 9)], fill=OUTLINE)
    d.ellipse([px(cx - 6, cy - 6), px(cx + 6, cy + 6)], fill=LIGHT)
# courtyard
d.rectangle([px(30, 30), px(66, 66)], fill=LIGHT)
d.rectangle([px(35, 35), px(61, 61)], fill=WHITE)
# arched gate with a scallop shell above (front = bottom of the tile)
d.rectangle([px(42, 66), px(54, 80)], fill=OUTLINE)
d.ellipse([px(42, 60), px(54, 72)], fill=OUTLINE)
d.rectangle([px(45, 69), px(51, 80)], fill=LIGHT)
d.ellipse([px(45, 63), px(51, 74)], fill=LIGHT)
# drip-sand speckles on the walls
import random as _rnd
_r = _rnd.Random("castle")
for _ in range(26):
    x = _r.randrange(18, 78)
    y = _r.randrange(18, 78)
    if 30 <= x <= 66 and 30 <= y <= 66:
        continue
    d.ellipse([px(x, y), px(x + 2, y + 2)], fill=LIGHT)
save(img, "castle")

# --- sand tiles: baked speckled sand, two brightness variants ----------------
import random
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
img, d = canvas()
rng = random.Random("kelp")
for i, x0 in enumerate((22, 36, 50, 64, 76)):
    sway = rng.randrange(-8, 9)
    top = rng.randrange(10, 26)
    d.line([px(x0, 88), px(x0 + sway // 2, 56), px(x0 + sway, top)],
           fill=(26, 84, 44, 255), width=7 * S)
    d.line([px(x0, 88), px(x0 + sway // 2, 56), px(x0 + sway, top)],
           fill=(44, 128, 66, 255), width=4 * S)
    # frond tips
    d.ellipse([px(x0 + sway - 5, top - 6), px(x0 + sway + 5, top + 4)],
              fill=(62, 156, 82, 255))
save(img, "kelp")

# --- pool: shallow water with ripples, baked ---------------------------------
img, d = canvas()
d.rounded_rectangle([px(4, 4), px(92, 92)], radius=26 * S, fill=(70, 130, 168, 235))
d.rounded_rectangle([px(9, 9), px(87, 87)], radius=22 * S, fill=(96, 160, 196, 255))
for r, alpha in ((30, 140), (18, 180)):
    d.ellipse([px(48 - r, 48 - r), px(48 + r, 48 + r)],
              outline=(214, 236, 246, alpha), width=2 * S)
d.ellipse([px(30, 26), px(52, 40)], fill=(196, 226, 240, 120))
save(img, "pool")

# --- log: turnstile driftwood, horizontal with a pivot knob ------------------
img = Image.new("RGBA", (SIZE * S, SIZE * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([px(4, 36), px(92, 60)], radius=11 * S, fill=(74, 58, 44, 255))
d.rounded_rectangle([px(7, 39), px(89, 57)], radius=9 * S, fill=(122, 96, 68, 255))
d.rounded_rectangle([px(7, 39), px(89, 46)], radius=8 * S, fill=(148, 120, 88, 255))
rng = random.Random("log")
for _ in range(5):
    x0 = rng.randrange(14, 66)
    y0 = rng.randrange(42, 54)
    d.line([px(x0, y0), px(x0 + rng.randrange(8, 18), y0)],
           fill=(96, 74, 52, 255), width=2 * S)
# pivot knob
d.ellipse([px(38, 38), px(58, 58)], fill=(52, 38, 28, 255))
d.ellipse([px(42, 42), px(54, 54)], fill=(178, 148, 108, 255))
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

# --- crab frame B: alternate leg pose for the walk cycle ---------------------
img, d = canvas()
LEGS_B = [((34, 32), (14, 26), (4, 18)), ((30, 42), (14, 34), (2, 26)),
          ((30, 54), (14, 62), (2, 70)), ((34, 64), (14, 70), (4, 80))]
for hip, knee, foot in LEGS_B:
    for (a, b) in [(hip, knee), (knee, foot)]:
        d.line([px(*a), px(*b)], fill=OUTLINE, width=5 * S)
    kx, ky = 96 - knee[0], knee[1]
    fx, fy = 96 - foot[0], foot[1]
    d.line([px(62, hip[1]), px(kx - 26, ky)], fill=OUTLINE, width=5 * S)
    d.line([px(kx - 26, ky), px(fx - 26, fy)], fill=OUTLINE, width=5 * S)
d.ellipse([px(20, 18), px(76, 78)], fill=OUTLINE)
d.ellipse([px(24, 22), px(72, 74)], fill=WHITE)
d.ellipse([px(32, 32), px(64, 64)], fill=LIGHT)
for (sx, sy) in [(34, 28), (58, 26), (30, 60), (60, 66), (46, 70)]:
    d.ellipse([px(sx, sy), px(sx + 4, sy + 4)], fill=LIGHT)
d.ellipse([px(66, 20), px(80, 32)], fill=OUTLINE)
d.ellipse([px(68, 22), px(78, 30)], fill=WHITE)
d.line([px(66, 38), px(78, 32)], fill=OUTLINE, width=3 * S)
d.line([px(66, 58), px(78, 64)], fill=OUTLINE, width=3 * S)
d.ellipse([px(74, 26), px(88, 40)], fill=OUTLINE)
d.ellipse([px(74, 56), px(88, 70)], fill=OUTLINE)
d.ellipse([px(76, 28), px(86, 38)], fill=(252, 252, 252, 255))
d.ellipse([px(76, 58), px(86, 68)], fill=(252, 252, 252, 255))
d.ellipse([px(80, 31), px(85, 36)], fill=(20, 16, 12, 255))
d.ellipse([px(80, 61), px(85, 66)], fill=(20, 16, 12, 255))
save(img, "crab_b")

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

# --- keep_ring: the outer curtain wall a tier-1 castle grows, tintable -------
# Replaces a plain coloured square. Hollow, so the keep sits inside it.
# Three deep merlons a side rather than a row of fine notches: at 64 px on
# the board a fine notch is a dashed border, not a battlement.
img, d = canvas()
# The dark keel is kept thin and the coloured band wide: the outline is
# tinted along with everything else, and a wall that is mostly outline
# comes out black whoever owns it.
d.rounded_rectangle([px(2, 2), px(94, 94)], radius=10 * S, fill=OUTLINE)
d.rounded_rectangle([px(5, 5), px(91, 91)], radius=8 * S, fill=WHITE)
d.rounded_rectangle([px(17, 17), px(79, 79)], radius=6 * S, fill=(0, 0, 0, 0))
for off in (20, 42, 64):
    for box in [(off, 0, off + 12, 9), (off, 87, off + 12, 96),
                (0, off, 9, off + 12), (87, off, 96, off + 12)]:
        d.rectangle([px(box[0], box[1]), px(box[2], box[3])], fill=(0, 0, 0, 0))
# a lit inner lip so the wall has a thickness to it rather than reading flat
d.rounded_rectangle([px(17, 17), px(79, 79)], radius=6 * S,
                    outline=LIGHT, width=2 * S)
save(img, "keep_ring")

# --- turret: a bucket-moulded corner tower seen from above, tintable ---------
img, d = canvas()
d.ellipse([px(6, 6), px(90, 90)], fill=OUTLINE)
d.ellipse([px(11, 11), px(85, 85)], fill=WHITE)
d.ellipse([px(24, 24), px(72, 72)], fill=LIGHT)
d.ellipse([px(34, 30), px(62, 52)], fill=WHITE)
# crenellation nicks around the rim
for i in range(8):
    a = i * 45
    import math as _m
    cx = 48 + 39 * _m.cos(_m.radians(a))
    cy = 48 + 39 * _m.sin(_m.radians(a))
    d.ellipse([px(cx - 6, cy - 6), px(cx + 6, cy + 6)], fill=(0, 0, 0, 0))
save(img, "turret")

# --- moat: the ring of water a tier-3 castle digs, baked ---------------------
img, d = canvas()
d.rounded_rectangle([px(0, 0), px(96, 96)], radius=16 * S, fill=(46, 96, 132, 255))
d.rounded_rectangle([px(4, 4), px(92, 92)], radius=14 * S, fill=(78, 142, 180, 255))
d.rounded_rectangle([px(7, 7), px(89, 89)], radius=12 * S, fill=(104, 172, 206, 255))
# ripple highlights around the ring
for inset, alpha in ((11, 150), (16, 100)):
    d.rounded_rectangle([px(inset, inset), px(96 - inset, 96 - inset)],
                        radius=10 * S, outline=(216, 238, 248, alpha), width=2 * S)
# the island the keep stands on: nothing, so the sand shows through
d.rounded_rectangle([px(21, 21), px(75, 75)], radius=8 * S, fill=(0, 0, 0, 0))
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
