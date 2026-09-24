#!/usr/bin/env sh
# Make the landing page's WebP pictures from the PNG screenshots.
#
# docs/screenshots holds the PNGs the README and the guide show; the page
# serves these copies instead, a fraction of the size. Run it after
# retaking a screenshot and commit what it writes: the Pages workflow only
# copies site/media, so a PNG changed without this keeps its old picture on
# the site.
#
# The hero video (site/media/hero.webm and hero.mp4) is recorded by hand;
# with ffmpeg on the PATH, its poster is remade from the first frame here too.
#
# Needs python3 with Pillow.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
media="$root/site/media"

python3 - "$root/docs/screenshots" "$media" <<'PY'
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
  python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).convert("RGB").save(sys.argv[2], quality=82, method=6)' \
    "$tmp/poster.png" "$media/hero-poster.webp"
  printf 'wrote %s\n' "$media/hero-poster.webp"
fi
