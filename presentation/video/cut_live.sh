#!/usr/bin/env bash
# Turn the raw screen recording (out/live/raw.mp4, from record_live.py) into
# out/live-session.mp4: the opening frame held 5 s for the first caption, then the
# session in real time, with live_captions.ass burned in. Nothing is sped up or cut.
set -euo pipefail
cd "$(dirname "$0")"
raw=out/live/raw.mp4
dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$raw")
total=$(python3 -c "print(round($dur + 5, 3))")
ffmpeg -loglevel error -y -i "$raw" -f lavfi -i anullsrc=r=48000:cl=stereo -filter_complex \
  "[0:v]tpad=start_duration=5:start_mode=clone,subtitles=live_captions.ass:fontsdir=../manim/fonts,fade=t=in:st=0:d=0.4,fade=t=out:st=$(python3 -c "print($total - 0.4)"):d=0.4,fps=30,format=yuv420p[v]" \
  -map "[v]" -map 1:a -t "$total" -c:v libx264 -crf 18 -preset slow -c:a aac -b:a 64k -movflags +faststart out/live-session.mp4
ffprobe -v error -show_entries format=duration,size -of default=nw=1 out/live-session.mp4
