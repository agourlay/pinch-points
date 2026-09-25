#!/usr/bin/env sh
# Render the link-preview card (card.html) to site/media/card.png.
#
# Run it after retaking docs/screenshots/turf_war.png, which the card
# shows, and commit the PNG: the Pages workflow publishes it as is.
#
# Needs a Chromium; set CHROME to one when it is not on the PATH as
# chromium or google-chrome. The card names Noto Sans and Noto Color Emoji,
# so render it where those are installed or the type will drift.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out="$here/../media/card.png"
chrome=${CHROME:-$(command -v chromium || command -v google-chrome || true)}
[ -n "$chrome" ] || { echo "card.sh: no Chromium found; set CHROME" >&2; exit 1; }

"$chrome" --headless --no-sandbox --disable-gpu --hide-scrollbars \
  --allow-file-access-from-files --force-device-scale-factor=1 \
  --window-size=1200,630 --virtual-time-budget=2000 \
  --screenshot="$out" "file://$here/card.html" >/dev/null 2>&1
printf 'wrote %s\n' "$out"
