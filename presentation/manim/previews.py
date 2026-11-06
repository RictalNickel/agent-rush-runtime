"""Extract the last frame of each rendered slide into out/previews/slide-NN.png.

Usage: python previews.py [scene]   (run from presentation/manim after rendering)
Also prints each slide's motion time, so slow slides are easy to spot.
"""

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
scene = sys.argv[1] if len(sys.argv) > 1 else "AgentRushDeck"
slides = json.loads((HERE / "slides" / f"{scene}.json").read_text())["slides"]
out = HERE / "out" / "previews"
out.mkdir(parents=True, exist_ok=True)

for n, slide in enumerate(slides, start=1):
    video = HERE / slide["file"]
    target = out / f"slide-{n:02d}.png"
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-sseof", "-0.1", "-i", str(video),
         "-update", "1", "-frames:v", "1", str(target)],
        check=True,
    )
    seconds = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(video)],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    print(f"slide {n:02d}: {float(seconds):4.1f} s  -> {target.relative_to(HERE)}")
