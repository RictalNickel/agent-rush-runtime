#!/usr/bin/env python3
"""Build the interactive session replay: one page with the screen recording beside the
session's full event log, chapters from the narration file, and the session's results.

    python3 build_replay.py ../video/out/live-shop ../video/narration/shop.json \
        session.json session-events.json out/

Writes out/index.html and out/recording.mp4. Every event, command and output on the page
comes from the session's own event log (GET /v1/sessions/{id}/events); the page only
lays them out and links each one to its moment in the recording.
"""
import base64
import html
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MAX_TEXT = 6000  # characters of any one command, file or output kept on the page


def clip(s):
    s = s if isinstance(s, str) else json.dumps(s, indent=2)
    return s if len(s) <= MAX_TEXT else s[:MAX_TEXT] + f"\n… ({len(s) - MAX_TEXT:,} more characters in the event log)"


def summary(call):
    tool = call.get("tool")
    if call.get("invalid"):
        return "invalid", "A malformed tool call, rejected"
    if tool == "bash":
        first = call.get("command", "").strip().splitlines()[0] if call.get("command") else ""
        return "shell", first
    if tool == "write_file":
        return "file", f"Wrote {call.get('path')}"
    if tool == "edit_file":
        return "file", f"Edited {call.get('path')}"
    if tool == "read_file":
        return "file", f"Read {call.get('path')}"
    if tool == "browser":
        a = call.get("action")
        detail = {"goto": call.get("url"), "type": f"“{call.get('text')}” into [{call.get('ref')}]",
                  "select": f"“{call.get('value')}” in [{call.get('ref')}]", "click": f"[{call.get('ref')}]",
                  "screenshot": call.get("path")}.get(a, "")
        return "browser", f"{a} {detail or ''}".strip()
    return "other", tool or "tool"


def main(rec_dir, narration_file, session_file, events_file, out_dir):
    rec, out = Path(rec_dir), Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    marks = json.loads((rec / "marks.json").read_text())
    n = json.loads(Path(narration_file).read_text())
    session = json.loads(Path(session_file).read_text())
    events = json.loads(Path(events_file).read_text())
    events = events.get("events", events)
    # Video time of a server event: the first message reached the server just after "sent".
    offset = marks["marks"]["sent"] - marks["frames"][0] + n.get("server_lag", 0.3)
    t0 = events[0].get("created_at") or events[0].get("at")

    items, pending = [], {}
    for e in events:
        t = ((e.get("created_at") or e.get("at")) - t0) / 1000
        p, kind = e["payload"], e["kind"]
        if kind in ("user", "interjection"):
            items.append({"t": t, "type": "you", "label": "Sent while working" if kind == "interjection" else "You",
                          "text": p["text"]})
        elif kind == "assistant":
            items.append({"t": t, "type": "agent", "label": "Agent's answer", "text": p["text"],
                          "files": p.get("attachments", [])})
        elif kind == "tool_call":
            call = p.get("call", p)
            group, line = summary(call)
            args = {k: v for k, v in call.items() if k not in ("tool", "invalid")}
            body = args.pop("command", None) or args.pop("content", None)
            items.append({"t": t, "type": group, "line": line, "tool": call.get("tool") or "?",
                          "args": clip(json.dumps(args, indent=2, ensure_ascii=False)) if args else "",
                          "body": clip(body) if body else "", "ok": None, "out": ""})
            pending = items[-1]
        elif kind == "tool_result" and pending:
            pending["ok"] = bool(p.get("ok"))
            pending["out"] = clip(p.get("output", ""))
            pending["dt"] = round(t - pending["t"], 2)
            pending = {}

    chapters = [{"t": s["at"], "title": s["title"], "detail": s["detail"]} for s in n["steps"] if "at" in s]
    stats = {
        "agent_seconds": n["done_at"], "model_calls": session["model_calls"], "tool_calls": len(n["tool_calls"]),
        "input_tokens": session["input_tokens"], "output_tokens": session["output_tokens"],
        "spend_usd": session["spend_usd"], "model": session["model"], "id": session["id"],
        "messages": sum(1 for i in items if i["type"] == "you"),
        "failed": sum(1 for i in items if i.get("ok") is False),
    }
    results = []
    for r in n.get("replay_results", []):
        src = Path(narration_file).parent / r["file"]
        mime = "image/png" if src.suffix == ".png" else "text/plain"
        if mime == "image/png":
            data = f"data:{mime};base64," + base64.b64encode(src.read_bytes()).decode()
            results.append({**r, "src": data})
        else:
            results.append({**r, "text": src.read_text()})

    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(rec / "raw.mp4"), "-vf", "scale=1200:900",
                    "-c:v", "libx264", "-crf", "30", "-preset", "slow", "-pix_fmt", "yuv420p", "-an",
                    "-movflags", "+faststart", str(out / "recording.mp4")], check=True)
    data = {"offset": round(offset, 3), "items": items, "chapters": chapters, "stats": stats,
            "results": results, "title": n["replay_title"], "where": n["where"], "date": n["date"],
            "story": n["replay_story"], "notes": n.get("replay_notes", [])}
    page = (HERE / "template.html").read_text()
    blob = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
    page = page.replace("/*DATA*/null", blob).replace("{{TITLE}}", html.escape(n["replay_title"]))
    (out / "index.html").write_text(page)
    print(f"wrote {out / 'index.html'} ({len(page) // 1024} KB, {len(items)} events) and recording.mp4 "
          f"({(out / 'recording.mp4').stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main(*sys.argv[1:])
