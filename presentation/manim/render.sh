#!/usr/bin/env bash
# Render the deck with Manim Slides, then export the HTML deck, the PDF and
# one preview PNG per slide. Run from anywhere: ./presentation/manim/render.sh
#
#   QUALITY=l ./render.sh    fast 480p draft (default h: 1920x1080 at 30 fps)
#
# Quality is passed as "--quality h": manim-slides would read "-qh" as -q plus -h (help).
set -euo pipefail
cd "$(dirname "$0")"

SCENE=PitchDeck
QUALITY="${QUALITY:-h}"
TITLE="$(sed -n 's/^PRODUCT = "\([^"]*\)".*/\1/p' deck.py)"

if [ ! -x .venv/bin/manim-slides ]; then
  if ! command -v uv >/dev/null; then
    echo "Needs uv (https://docs.astral.sh/uv/), or make .venv with Python 3.12 and" >&2
    echo "  .venv/bin/pip install -r requirements.txt" >&2
    exit 1
  fi
  uv venv --python 3.12 .venv
  uv pip install --python .venv/bin/python -r requirements.txt
fi

.venv/bin/manim-slides render deck.py "$SCENE" --quality "$QUALITY"

mkdir -p out
rm -rf out/vultr-placeholder-test123-deck_assets out/previews  # drop videos of earlier renders
# --offline copies reveal.js next to the deck, so it presents without a network.
.venv/bin/manim-slides convert "$SCENE" out/vultr-placeholder-test123-deck.html --offline \
  -ctitle="$TITLE" -cbackground_color='#12161b' -chash=true -ccontrols=false
.venv/bin/manim-slides convert "$SCENE" out/vultr-placeholder-test123-deck.pdf
.venv/bin/python previews.py "$SCENE"

du -sh out/vultr-placeholder-test123-deck.html out/vultr-placeholder-test123-deck_assets out/vultr-placeholder-test123-deck.pdf
