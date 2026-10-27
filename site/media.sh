#!/usr/bin/env sh
# Make the landing page's WebP pictures from the PNG screenshots.
#
# docs/screenshots holds the PNGs the README and the guide show; the page
# serves these copies instead, a fraction of the size. Run it after
# retaking a screenshot and commit what it writes: the Pages workflow only
# copies site/media, so a PNG changed without this keeps its old picture on
# the site.
#
# The hero video (site/media/hero.webm and hero.mp4) is recorded by hand:
# a release build with PINCH_WINDOW=1280x720 PINCH_ON_TOP=1
# PINCH_SKIRMISH=arena PINCH_AUTOPILOT=normal PINCH_AUTOPILOT_WALK=1 (the
# cursor walks to each arrow it places), a plain round on the classic
# beach, filmed with `ffmpeg -f x11grab` at the window's corner and cut to
# a stretch no lure or Crab Monopoly takes over. Without PINCH_ON_TOP the
# grab films whatever window sits in front. With ffmpeg on the PATH, its
# poster is remade from the first frame here too.
#
# Needs uv, which fetches Pillow for the two Python steps.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
media="$root/site/media"

uv run --no-project --with pillow python - "$root/docs/screenshots" "$media" <<'PY'
import sys
from PIL import Image
shots, media = sys.argv[1], sys.argv[2]
for name in ("lure", "old_mill", "stages", "menu", "lobby"):
    Image.open(f"{shots}/{name}.png").convert("RGB").save(
        f"{media}/{name}.webp", quality=82, method=6)
    print(f"wrote {media}/{name}.webp")
PY

if command -v ffmpeg >/dev/null 2>&1; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  ffmpeg -hide_banner -loglevel error -y -i "$media/hero.mp4" -frames:v 1 "$tmp/poster.png"
  uv run --no-project --with pillow python -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).convert("RGB").save(sys.argv[2], quality=82, method=6)' \
    "$tmp/poster.png" "$media/hero-poster.webp"
  printf 'wrote %s\n' "$media/hero-poster.webp"
fi
