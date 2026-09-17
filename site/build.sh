#!/usr/bin/env sh
# Assemble the landing page into ./_site, ready to serve or publish.
#
# The screenshots are not duplicated in this directory: they live in
# docs/screenshots, where the README and the guide already point at them,
# and are copied in here so the published site has its own copy at a path
# that does not climb out of the site root.
#
# Used by .github/workflows/pages.yml and by hand:
#
#   site/build.sh && python3 -m http.server -d _site
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$root/_site"

rm -rf "$out"
mkdir -p "$out/screenshots"

cp "$root/site/index.html" "$root/site/style.css" "$out/"
cp "$root/docs/screenshots/"*.png "$out/screenshots/"

# Pages runs Jekyll over a branch unless told not to, which would drop any
# file or directory whose name begins with an underscore.
touch "$out/.nojekyll"

printf 'built %s\n' "$out"
