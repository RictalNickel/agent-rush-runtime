#!/usr/bin/env python3
"""Put a live recording next to a narration panel: out/<name>/ -> out/<name>.mp4.

The recording (1440x1080) fills the left of a 1920x1080 frame. The panel on the right
shows the task, the agent's clock and tool-call count, the list of steps (done, current,
next) and a description of the current step. Step times come from the session's own
event log, so the panel follows what the agent did.

The recording plays in real time. Where a step's description needs longer to read than
the step took, the picture freezes at the start of that step, the badge says
"PAUSED TO EXPLAIN" and the agent clock stops; then it plays on. After the recording,
the agent's own screenshots are shown full frame.

    python3 compose_live.py out/live-dash narration/dashboard.json
"""
import json
import subprocess
import sys
import textwrap
from pathlib import Path

W, H, PANEL_X = 1920, 1080, 1440
X = PANEL_X + 40
FPS = 30
LEAD = 1.0  # seconds before the first step's pause starts
READ_CPS = 17  # reading speed for the step descriptions, characters per second

# ASS colours are &HBBGGRR.
TEXT, MUTED, FAINT, AMBER, GREEN = "&HEEEAE6&", "&HA79E95&", "&H6E655C&", "&H4BB0E8&", "&H8ABF5B&"
STYLE_HEADER = [
    "[V4+ Styles]",
    "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, "
    "Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, "
    "MarginL, MarginR, MarginV, Encoding",
    f"Style: P,Archivo,26,{TEXT},{TEXT},&H000000&,&H000000&,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1",
    "", "[Events]", "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"]


def ts(t):
    t = max(t, 0)
    return f"{int(t // 3600)}:{int(t % 3600 // 60):02d}:{t % 60:05.2f}"


def esc(s):
    return s.replace("{", "(").replace("}", ")")


def wrap(s, width):
    return "\\N".join(textwrap.wrap(esc(s), width))


def ass_file(path, lines):
    path.write_text("\n".join(["[Script Info]", "ScriptType: v4.00+", f"PlayResX: {W}", f"PlayResY: {H}",
                               "WrapStyle: 2", "", *STYLE_HEADER, *lines, ""]))


def main(rec_dir, narration_file):
    rec = Path(rec_dir)
    marks = json.loads((rec / "marks.json").read_text())
    n = json.loads(Path(narration_file).read_text())
    frames = marks["frames"]
    r0, r_end = frames[0], marks["marks"]["end"]
    if "end_mark" in n:  # stop the clip a little after this mark instead of at the end of the recording
        r_end = min(r_end, marks["marks"][n["end_mark"]] + n.get("end_pad", 1.5))
    # Recording time of a session event: the message reached the server just after "sent".
    sent = marks["marks"]["sent"] + n.get("server_lag", 0.3)
    steps = n["steps"]
    for s in steps:
        s["r"] = max(sent + s["at"] if "at" in s else marks["marks"][s["mark"]], r0)
    steps.sort(key=lambda s: s["r"])
    calls = [sent + t for t in n["tool_calls"]]
    done_r = sent + n["done_at"]

    # Pauses: (recording time, seconds). Each step gets enough screen time to read it.
    pauses = []
    for i, s in enumerate(steps):
        nxt = steps[i + 1]["r"] if i + 1 < len(steps) else r_end
        need = len(s["detail"]) / READ_CPS + 1.0
        if need > nxt - s["r"]:
            pauses.append((s["r"], round(need - (nxt - s["r"]), 2)))
    if "max_pause" in n and sum(d for _, d in pauses) > n["max_pause"]:
        # Keep the clip's length in budget: shorten every pause by the same factor.
        k = n["max_pause"] / sum(d for _, d in pauses)
        pauses = [(at, round(d * k, 2)) for at, d in pauses]

    def out_t(r, before_pause=False):
        """Output time of recording time r (after any pause that starts at r, unless before_pause)."""
        return LEAD + (r - r0) + sum(d for at, d in pauses if at < r or (at == r and not before_pause))

    clip = out_t(r_end)
    lines = []

    def add(a, b, text, layer=0):
        if b > a:
            lines.append(f"Dialogue: {layer},{ts(a)},{ts(b)},P,,0,0,0,,{text}")

    # Static panel.
    add(0, clip, f"{{\\an7\\pos({X},78)\\c{FAINT}\\fs22}}{esc(n['where'])}")
    add(0, clip, f"{{\\an7\\pos({X},128)\\c{MUTED}\\fs22}}THE TASK")
    add(0, clip, f"{{\\an7\\pos({X},158)\\c{TEXT}\\fs26}}{wrap(n['task'], 32)}")

    # Badge: live, or paused to explain.
    live_from = 0.0
    for at, d in pauses:
        a = out_t(at, before_pause=True)
        add(live_from, a, f"{{\\an7\\pos({X},40)\\c{AMBER}\\b1\\fs24}}●  LIVE RECORDING  ·  REAL TIME")
        add(a, a + d, f"{{\\an7\\pos({X},40)\\c{MUTED}\\b1\\fs24}}❚❚  PAUSED TO EXPLAIN")
        live_from = a + d
    if live_from == 0.0:
        add(0, LEAD, f"{{\\an7\\pos({X},40)\\c{AMBER}\\b1\\fs24}}●  LIVE RECORDING  ·  REAL TIME")
    add(live_from, clip, f"{{\\an7\\pos({X},40)\\c{AMBER}\\b1\\fs24}}●  LIVE RECORDING  ·  REAL TIME")

    # Clock and tool calls: change on whole seconds of agent time and at each call.
    edges = {0.0, clip}
    for k in range(int(r_end - r0) + 2):
        edges.add(round(out_t(r0 + k), 3))
    for r in [*calls, done_r, sent, *[at for at, _ in pauses]]:
        edges.add(round(out_t(r, before_pause=True), 3))
        edges.add(round(out_t(r), 3))
    edges = sorted(e for e in edges if 0 <= e <= clip)

    def rec_t(o):
        """Recording time shown at output time o (frozen during a pause)."""
        r = r0 + o - LEAD
        for at, d in pauses:
            if r <= at:
                break
            r -= d
            if r < at:
                return at
        return max(r0, min(r, r_end))

    for a, b in zip(edges, edges[1:]):
        r = rec_t(a + 1e-3)
        agent = min(max(r - sent, 0), n["done_at"])
        clock = f"{int(agent // 60):02d}:{int(agent % 60):02d}"
        made = sum(1 for c in calls if c <= r)
        colour = GREEN if r >= done_r else TEXT
        add(a, b, f"{{\\an7\\pos({X},300)\\c{MUTED}\\fs22}}AGENT TIME")
        add(a, b, f"{{\\an7\\pos({X},326)\\c{colour}\\fs62\\b1}}{clock}")
        add(a, b, f"{{\\an7\\pos({X + 200},300)\\c{MUTED}\\fs22}}TOOL CALLS")
        add(a, b, f"{{\\an7\\pos({X + 200},326)\\c{colour}\\fs62\\b1}}{made}")

    # Step list and the current step's description.
    starts = [out_t(s["r"], before_pause=True) for s in steps] + [clip]
    for i, s in enumerate(steps):
        a, b = starts[i], starts[i + 1]
        y = 425
        for j, other in enumerate(steps):
            if j < i or (other.get("final") and j == i):
                sym, c, bold = "✓", GREEN, 0
            elif j == i:
                sym, c, bold = "●", AMBER, 1
            else:
                sym, c, bold = "○", FAINT, 0
            add(a, b, f"{{\\an7\\pos({X},{y})\\c{c}\\fs24\\b{bold}}}{sym}   {esc(other['title'])}")
            y += 33
        y += 22
        add(a, b, f"{{\\an7\\pos({X},{y})\\c{MUTED}\\fs22}}WHAT'S HAPPENING", layer=1)
        add(a, b, f"{{\\an7\\pos({X},{y + 30})\\c{TEXT}\\fs25}}{wrap(s['detail'], 34)}", layer=1)

    ass = rec / "panel.ass"
    ass_file(ass, lines)
    fonts = Path(__file__).resolve().parent.parent / "manim" / "fonts"

    # The picture: each output frame shows the newest capture at its recording time.
    main_mp4 = rec / "panel.mp4"
    enc = subprocess.Popen(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "image2pipe", "-framerate", str(FPS), "-c:v", "mjpeg",
         "-i", "-", "-vf",
         f"scale={PANEL_X}:{H}:flags=lanczos,pad={W}:{H}:0:0:color=0x12161b,drawbox=x={PANEL_X}:y=0:w=2:h={H}:color=0x262b33:t=fill,"
         f"subtitles={ass}:fontsdir={fonts},fade=t=in:st=0:d=0.4,fade=t=out:st={clip - 0.4:.2f}:d=0.4,"
         "format=yuv420p", "-c:v", "libx264", "-crf", "18", "-preset", "slow", str(main_mp4)],
        stdin=subprocess.PIPE)
    j = 0
    for k in range(int(clip * FPS)):
        r = rec_t(k / FPS)
        j = max(0, min(j, len(frames) - 1))
        while j + 1 < len(frames) and frames[j + 1] <= r:
            j += 1
        while j > 0 and frames[j] > r:
            j -= 1
        enc.stdin.write((rec / "frames" / f"{j:06d}.jpg").read_bytes())
    enc.stdin.close()
    if enc.wait():
        raise SystemExit("ffmpeg failed")

    # After the recording: the agent's own screenshots, full frame, each with a caption.
    parts = [main_mp4]
    for i, r in enumerate(n.get("results", [])):
        cap = rec / f"result{i}.ass"
        ass_file(cap, [
            f"Dialogue: 0,0:00:00.00,{ts(r['seconds'])},P,,0,0,0,,{{\\an7\\pos(120,40)\\c{AMBER}\\fs24\\b1}}"
            f"{esc(r['label'])}",
            f"Dialogue: 0,0:00:00.00,{ts(r['seconds'])},P,,0,0,0,,{{\\an7\\pos(120,76)\\fs30}}{esc(r['caption'])}"])
        seg = rec / f"result{i}.mp4"
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-loop", "1", "-framerate", str(FPS), "-t",
                        str(r["seconds"]), "-i", str(Path(narration_file).parent / r["file"]), "-vf",
                        "scale=1680:900:force_original_aspect_ratio=decrease,"
                        f"pad={W}:{H}:(ow-iw)/2:140:color=0x12161b,subtitles={cap}:fontsdir={fonts},"
                        f"fade=t=in:st=0:d=0.4,fade=t=out:st={r['seconds'] - 0.4}:d=0.4,format=yuv420p",
                        "-c:v", "libx264", "-crf", "18", str(seg)], check=True)
        parts.append(seg)

    out = rec.parent / f"{rec.name}.mp4"
    inputs = [arg for p in parts for arg in ("-i", str(p))]
    graph = ("".join(f"[{i}:v]setsar=1[p{i}];" for i in range(len(parts)))
             + "".join(f"[p{i}]" for i in range(len(parts))) + f"concat=n={len(parts)}:v=1:a=0[v]")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", *inputs, "-f", "lavfi", "-i",
                    "anullsrc=r=48000:cl=stereo", "-filter_complex", graph, "-map", "[v]", "-map",
                    f"{len(parts)}:a", "-shortest", "-c:v", "libx264", "-crf", "18", "-preset", "slow",
                    "-c:a", "aac", "-b:a", "64k", "-movflags", "+faststart", str(out)], check=True)
    paused = sum(d for _, d in pauses)
    print(f"wrote {out}: recording {r_end - r0:.1f}s, paused {paused:.1f}s in {len(pauses)} places, "
          f"panel clip {clip:.1f}s")


if __name__ == "__main__":
    main(*sys.argv[1:])
