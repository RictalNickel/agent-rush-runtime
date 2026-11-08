#!/usr/bin/env bash
# Render the demo video to out/demo.mp4 (1920x1080, 30 fps), with a silent
# audio track so upload sites that expect one accept it.
#
#   QUALITY=l ./render.sh    fast 480p draft
set -euo pipefail
cd "$(dirname "$0")"
VENV=../manim/.venv
if [ ! -x "$VENV/bin/manim" ]; then
  echo "Needs the deck's virtualenv: run ../manim/render.sh once first." >&2
  exit 1
fi
QUALITY="${QUALITY:-h}"
"$VENV/bin/manim" render video.py DemoVideo --quality "$QUALITY" --media_dir media --disable_caching
src=$(ls -t media/videos/video/*/DemoVideo.mp4 | head -1)
mkdir -p out
# Splice the live web chat recording (out/live-session.mp4, see cut_live.sh) in after
# the demo slide, scene 5.
[ -f out/live-session.mp4 ] || ./cut_live.sh
cut=$(python3 -c "import json; print(json.load(open('media/scene_ends.json'))[4])")
ffmpeg -loglevel error -y -i "$src" -i out/live-session.mp4 -f lavfi -i anullsrc=r=48000:cl=stereo -filter_complex \
  "[0:v]trim=end=$cut,setpts=PTS-STARTPTS,fps=30,format=yuv420p[a];[1:v]fps=30,format=yuv420p[l];[0:v]trim=start=$cut,setpts=PTS-STARTPTS,fps=30,format=yuv420p[b];[a][l][b]concat=n=3:v=1:a=0[v]" \
  -map "[v]" -map 2:a -shortest -c:v libx264 -pix_fmt yuv420p -crf 20 -preset slow -c:a aac -b:a 64k -movflags +faststart out/demo.mp4
ffprobe -v error -show_entries format=duration,size -of default=nw=1 out/demo.mp4
