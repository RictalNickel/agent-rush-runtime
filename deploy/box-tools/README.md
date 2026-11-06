# Box tools

Helpers installed at `/opt/box/bin` in the workspace image (`deploy/workspace.Dockerfile`) and on the box user's `PATH`. Each one runs inside the session's gVisor box, as uid 1000, with only the box's filtered internet. None of them holds or needs a credential.

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
- images: ImageMagick
- audio and video: ffmpeg

An unsupported pair exits 2 with the reason.
