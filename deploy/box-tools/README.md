# Box tools

Helpers installed at `/opt/box/bin` in the workspace image (`deploy/workspace.Dockerfile`) and on the box user's `PATH`. Each one runs inside the session's gVisor box, as uid 1000, with only the box's filtered internet. None of them holds or needs a credential.

## The box environment

| Path | What it is | Lifetime |
| --- | --- | --- |
| `/workspace` | the session's own disk (a fixed-size ext4 image), the working directory | until the session closes, or 7 days without a call |
| `/home/agent` (`$HOME`) | tmpfs, 1 GiB: caches and dotfiles (`~/.cache`, `~/.cargo`, `~/.npm`) | until the box is parked or restarted |
| `/tmp` | tmpfs, 1 GiB | until the box is parked or restarted |
| everything else | the image, read-only | |

Both tmpfs mounts allow executables (`nosuid,nodev`, not `noexec`), so compiled packages work wherever they are installed. The gVisor box is the sandbox.

User-level installs land on `/workspace`, so they survive parking:
- `pip install --user <pkg>`, or plain `pip install <pkg>` (it falls back to a user install), goes to `PYTHONUSERBASE=/workspace/.local`. Scripts go to `/workspace/.local/bin`, which is on `PATH`. A virtualenv (`python3 -m venv /workspace/.venv`) works too.
- `npm install -g <pkg>` goes to `NPM_CONFIG_PREFIX=/workspace/.npm-global`, with `/workspace/.npm-global/bin` on `PATH`.
- `cargo install` goes to `~/.cargo/bin` (on `PATH`); it does not survive parking.

`PIP_NO_CACHE_DIR=1` keeps pip from filling `$HOME` with a second copy of every wheel.

Preinstalled, so the basics need no install:
- Python 3.12 with numpy, pandas, matplotlib (`MPLBACKEND=Agg`, so `savefig` works without a display) and Pillow
- Node and npm
- a minimal Rust toolchain
- pandoc, LibreOffice (Writer, Calc, Impress), ImageMagick with librsvg (`rsvg-convert`; ImageMagick reads SVG through it), ffmpeg, poppler-utils, qpdf
- git, curl, wget, jq, ripgrep, fd, build-essential

A command runs as `bash -lc` would run it, but its text is passed on stdin, never as a process argument, so `pkill -f '<part of the command>'` cannot kill the shell running it. A command that kills its own shell anyway (`kill 0`, say) still gets a result: exit code 128+signal, the output captured so far, and a note on stderr.

## browse

Drives one headless Chromium that persists between calls. The first call starts it with a CDP endpoint on `127.0.0.1:9222` and its profile under `/tmp/browser`. It runs with `--no-sandbox` because the box is the sandbox.

```
browse '{"action":"goto","url":"https://example.com"}'
browse snapshot
browse '{"action":"click","ref":3}'
```

Each call prints one JSON object with `ok`, the page `url` and `title`, and the action's result. On failure it prints `{"ok":false,"error":…}` and exits 1.

| Action | Arguments | Result |
| --- | --- | --- |
| `goto` | `url` | `status` |
| `snapshot` | | `text`: the visible page as an indented tree, interactive elements tagged `[ref=N]` (at most 40 KB) |
| `click` | `ref` or `selector` | |
| `type` | `ref` or `selector`, `text`, `submit?` | fills the field; `submit` presses Enter |
| `press` | `key` | |
| `select` | `ref` or `selector`, `value` | `selected` |
| `scroll` | `dy` (pixels, default 600) | |
| `screenshot` | `path?`, `full_page?` | `path` (default `/workspace/.screenshots/shot-<ms>.png`) |
| `eval` | `js` | `result` |
| `back` | | |
| `tabs` | `select?` (index), `new?` (url) | `tabs` |
| `console` | `clear?` | `messages` logged by the page since the last call |
| `close` | | stops the browser |

Refs come from the latest `snapshot` and stay valid until the page changes. A call that has not finished after 90 s gives up with an error, so the browser can never hang a command.

## websearch

```
websearch '<query>' [n]
```

Prints `[{"title","url","snippet"}]` from DuckDuckGo's HTML pages (no key). If the provider answers with a bot challenge, it says so and exits 3; it does not try to get around it.

## fetch

```
fetch <url> [--max N] [--raw]
```

Prints a page as readable text: the main content as markdown-ish text with links kept. JSON and plain text are printed as they are, and PDFs go through `pdftotext`. Output is capped at N characters (default 20000).

## convert

```
convert <in> <out>
```

Picks the converter from the two extensions:
- documents: pandoc (`md`, `html`, `docx`, `odt`, `rst`, `epub`, …), LibreOffice for office formats and anything to PDF
- PDF: to `txt`, `png` or `jpg` through poppler
- images: ImageMagick; SVG is rendered by librsvg (`svg` to `png` or `pdf` directly, to other image formats through a PNG)
- audio and video: ffmpeg

An unsupported pair exits 2 with the reason.
