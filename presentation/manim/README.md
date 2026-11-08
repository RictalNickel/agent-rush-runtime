# Pitch deck (Manim Slides)

An 8-slide deck for the Vultr hackathon pitch (Blast Radius Zero track): one idea per slide, large type, the product's dark theme. The animations total about 32 seconds. Every animation is rendered with [Manim](https://www.manim.community/). [Manim Slides](https://manim-slides.eertmans.be/) splits it into real slides that you advance with the arrow keys. Each slide's motion lasts about 2 to 5 seconds and then holds, so you set the pace.

## Files

| Path | What it is |
| --- | --- |
| `deck.py` | The deck: shared style and helpers at the top, then one method per slide: `s1_title`, `s2_problem`, `s3_plug_in`, `s4_box`, `s5_demo`, `s6_containment`, `s7_vultr`, `s8_close`. `CONTAINMENT` holds the live result shown on slide 6 |
| `render.sh` | Renders at 1920x1080, then exports the HTML deck, the PDF and the previews |
| `previews.py` | Saves the last frame of each slide to `out/previews/slide-NN.png` and prints each slide's length |
| `requirements.txt` | Pinned Python packages (Python 3.12) |
| `fonts/` | Archivo variable TTF from google/fonts, under the SIL OFL (`fonts/OFL.txt`) |
| `out/boxoro-deck.html` | The deck, with its videos and an offline copy of reveal.js in `out/boxoro-deck_assets/` |
| `out/boxoro-deck.pdf` | One page per slide, showing each slide's finished state |

The product name is the `PRODUCT` constant at the top of `deck.py`. The HTML title comes from it too.

Every number on the slides comes from `README.md`, `docs/DESIGN.md` and `docs/evidence/live/*/README.md`. The screenshots are taken from `docs/evidence/`. They are cropped and given rounded corners at render time, into `media/prepared/`.

## Render

```sh
./render.sh            # 1080p, takes about a minute
QUALITY=l ./render.sh  # 480p draft for layout checks
```

On the first run it creates `.venv` with `uv` (Python 3.12) and installs `requirements.txt`. It needs ffmpeg, cairo and pango on the system. No LaTeX is used. Two versions are held back on purpose: manim 0.22 breaks manim-slides 5.7, and Pillow 12 breaks manim 0.21's images.

`media/` and `slides/` are render caches and are gitignored. `out/` is committed (about 12 MB).

## Present

Open `out/boxoro-deck.html` in Chrome or Firefox. It works offline. Keep `out/boxoro-deck_assets/` next to it. Some browsers block video on `file://` pages. If that happens, serve the folder: `python3 -m http.server -d out 8000`, then open http://localhost:8000/boxoro-deck.html.

| Key | Action |
| --- | --- |
| Right, Space, Page Down | Next slide (plays its animation) |
| Left, Page Up | Previous slide |
| F | Full screen |
| S | Speaker view, with notes for each slide |
| O or Esc | Overview of all slides |
| B or . | Black screen (pause) |
| ? | List of shortcuts |

Clickers that send Page Up and Page Down work too. The URL hash follows the slide (`#/3`), so reloading the page keeps your place.

You can also present with the native player: `.venv/bin/manim-slides present PitchDeck`, run after rendering. Right or Space goes forward, Left goes back, R replays the slide, F toggles full screen and Q quits.

## Export

- PDF: `render.sh` writes `out/boxoro-deck.pdf`. Or run `.venv/bin/manim-slides convert PitchDeck out/boxoro-deck.pdf`.
- PowerPoint, with each slide's video embedded: `.venv/bin/manim-slides convert PitchDeck out/boxoro-deck.pptx`. This file is not committed.
- One self-contained HTML file: add `--one-file` to the HTML convert line in `render.sh`. The file is larger.

## Style

Colours and type come from `docs/UI_DESIGN.md`:
- ground `#12161b`, surface `#1a2027`, rule `#2c3540`
- text `#e6eaee`, muted `#9aa5b1`
- one amber accent `#e8b04b`
- ok `#5bbf8a`, bad `#ec6e76`

Headlines use condensed Archivo. Code uses DejaVu Sans Mono.

The motion follows two rules:
- Things arrive with a fade and a small upward rise. Images pop in from 85% scale.
- When you advance, the old slide fades out and lifts away in a stagger, while the next title rises in letter by letter. The footer number and the amber progress bar update in place.
