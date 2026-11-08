#!/usr/bin/env python3
"""Screen-record one real session in the live web chat, from the first click to the agent's answer.

Needs Playwright (`pip install playwright && playwright install chromium`) and ffmpeg.
The web chat signs in with the operator cookie, so pass a short-lived `ar_session`
token in AR_SESSION (see README.md). Nothing is sped up or edited here: the output is
the session as it ran, at 30 fps.

    AR_SESSION=... python3 record_live.py --example webapp --out out/live
"""
import argparse
import asyncio
import base64
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import urlparse

from playwright.async_api import async_playwright

# A visible pointer: headless screencasts do not draw the mouse.
CURSOR = """
addEventListener('DOMContentLoaded', () => {
  const dot = document.createElement('div');
  dot.style.cssText = 'position:fixed;z-index:2147483647;left:0;top:0;width:18px;height:18px;'
    + 'margin:-9px 0 0 -9px;border-radius:50%;background:rgba(255,255,255,.85);'
    + 'box-shadow:0 0 0 2px rgba(0,0,0,.55);pointer-events:none;transition:transform .08s';
  document.body.appendChild(dot);
  addEventListener('mousemove', e => { dot.style.left = e.clientX + 'px'; dot.style.top = e.clientY + 'px'; }, true);
  addEventListener('mousedown', () => dot.style.transform = 'scale(.7)', true);
  addEventListener('mouseup', () => dot.style.transform = '', true);
});
"""


async def until(check, timeout):
    """Polls from Python: the site's CSP (script-src 'self') blocks wait_for_function."""
    deadline = time.monotonic() + timeout
    while not await check():
        if time.monotonic() > deadline:
            raise TimeoutError(f"waited {timeout}s")
        await asyncio.sleep(0.5)


async def state_of(page):
    return await page.locator("#session-state").text_content()


async def _is(page, states):
    return await state_of(page) in states


async def glide(page, selector):
    box = await page.locator(selector).bounding_box()
    await page.mouse.move(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2, steps=25)
    await asyncio.sleep(0.4)
    await page.mouse.down()
    await asyncio.sleep(0.08)
    await page.mouse.up()


async def type_text(page, text):
    """Types like a person: Shift+Enter between lines, since Enter sends."""
    lines = text.strip().split("\n")
    for i, line in enumerate(lines):
        await page.keyboard.type(line, delay=args_delay)
        if i + 1 < len(lines):
            await page.keyboard.press("Shift+Enter")


args_delay = 6  # ms per typed character


async def record(args):
    out = Path(args.out)
    frames_dir = out / "frames"
    frames_dir.mkdir(parents=True, exist_ok=True)
    frames, marks = [], {}
    t0 = time.monotonic()

    def mark(name):
        marks[name] = round(time.monotonic() - t0, 2)
        print(f"{marks[name]:7.1f}s  {name}", flush=True)

    async with async_playwright() as p:
        browser = await p.chromium.launch()
        width, height = (int(v) for v in args.size.split("x"))
        context = await browser.new_context(
            viewport={"width": width, "height": height}, device_scale_factor=args.scale, color_scheme="dark")
        host = urlparse(args.url).hostname
        await context.add_cookies([{"name": "ar_session", "value": os.environ["AR_SESSION"],
                                    "domain": host, "path": "/", "secure": True, "httpOnly": True}])
        await context.add_init_script(CURSOR)
        page = await context.new_page()
        await page.goto(args.url)
        await until(page.locator("#prompt").is_enabled, 20)
        await page.mouse.move(900, 600)

        cdp = await context.new_cdp_session(page)

        def on_frame(event):
            n = len(frames)
            (frames_dir / f"{n:06d}.jpg").write_bytes(base64.b64decode(event["data"]))
            frames.append(time.monotonic() - t0)
            asyncio.ensure_future(cdp.send("Page.screencastFrameAck", {"sessionId": event["sessionId"]}))

        cdp.on("Page.screencastFrame", on_frame)
        await cdp.send("Page.startScreencast", {"format": "jpeg", "quality": 92,
                                                "maxWidth": round(width * args.scale),
                                                "maxHeight": round(height * args.scale)})
        mark("recording")
        await asyncio.sleep(2.5)
        if args.prompt_file:
            await glide(page, "#prompt")
            await type_text(page, Path(args.prompt_file).read_text())
            mark("prompt typed")
            await asyncio.sleep(1.5)
        else:
            await glide(page, f'[data-example="{args.example}"]')
            mark("example picked")
            await asyncio.sleep(3)
        await glide(page, "#run-button")
        mark("sent")
        await until(lambda: page.locator("#session-state").is_visible(), 60)
        await until(lambda: _is(page, {"Working"}), 60)
        mark("working")
        steer = Path(args.steer_file).read_text() if args.steer_file else None

        async def ready_or_steer():
            nonlocal steer
            if steer and await state_of(page) == "Working" and args.steer_when in await page.locator("#log").inner_text():
                # A message sent while the agent works reaches it before its next step.
                await glide(page, "#prompt")
                await type_text(page, steer)
                await page.keyboard.press("Enter")
                steer = None
                mark("steered")
            return await _is(page, {"Ready", "Failed", "Closed"})

        await until(ready_or_steer, args.timeout)
        if steer:  # finished before the trigger: send it as the next message instead
            await glide(page, "#prompt")
            await type_text(page, steer)
            await page.keyboard.press("Enter")
            mark("follow-up sent")
            await until(lambda: _is(page, {"Working"}), 60)
            await until(lambda: _is(page, {"Ready", "Failed", "Closed"}), args.timeout)
        state = await state_of(page)
        mark(f"finished: {state}")
        # Scroll the thread to the agent's answer and its screenshots.
        await asyncio.sleep(1.5)
        await page.mouse.move(width * 0.6, height * 0.55, steps=15)
        for _ in range(40):
            await page.mouse.wheel(0, 160)
            await asyncio.sleep(0.1)
        await asyncio.sleep(args.hold)
        mark("end")
        await cdp.send("Page.stopScreencast")
        await asyncio.sleep(0.5)
        title = await page.locator("#session-title").text_content()
        await browser.close()

    # Resample to exactly 30 fps: each output frame shows the newest capture at that moment,
    # so idle stretches keep their real length and the clip runs in real time.
    end, fps = marks["end"], 30
    picks, j = [], 0
    for k in range(int((end - frames[0]) * fps)):
        t = frames[0] + k / fps
        while j + 1 < len(frames) and frames[j + 1] <= t:
            j += 1
        picks.append(j)
    (out / "marks.json").write_text(json.dumps({"marks": marks, "frames": [round(t, 4) for t in frames],
                                                "state": state, "title": title}, indent=2))
    enc = subprocess.Popen(["ffmpeg", "-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", str(fps),
                            "-c:v", "mjpeg", "-i", "-", "-vf",
                            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                            "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p", str(out / "raw.mp4")],
                           stdin=subprocess.PIPE)
    for j in picks:
        enc.stdin.write((frames_dir / f"{j:06d}.jpg").read_bytes())
    enc.stdin.close()
    if enc.wait():
        raise SystemExit("ffmpeg failed")
    print(f"wrote {out / 'raw.mp4'} ({len(frames)} frames, {end:.0f}s, state {state})")
    return 0 if state == "Ready" else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--url", default="https://144-202-59-108.sslip.io")
    ap.add_argument("--example", default="webapp", choices=["webapp", "report", "csv", "bug"])
    ap.add_argument("--prompt-file", help="type this task instead of clicking an example")
    ap.add_argument("--steer-file", help="message to send while the agent is working")
    ap.add_argument("--steer-when", default="Opened", help="send it once the thread shows this text")
    ap.add_argument("--size", default="1280x720", help="viewport in CSS pixels")
    ap.add_argument("--scale", type=float, default=1.5, help="device pixel ratio of the capture")
    ap.add_argument("--out", default="out/live")
    ap.add_argument("--timeout", type=int, default=900, help="seconds to wait for the agent")
    ap.add_argument("--hold", type=float, default=6, help="seconds to keep recording after the answer")
    sys.exit(asyncio.run(record(ap.parse_args())))


if __name__ == "__main__":
    main()
