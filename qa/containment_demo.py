#!/usr/bin/env python3
"""On-camera containment check: try to break out of a live session box.

Starts a session (or uses --session), runs each escape attempt in its box through
POST /v1/sessions/{id}/exec, and prints one line per attempt. Exits 1 if any
attempt that should be blocked got through.

Usage: AR_HOST=https://... AR_KEY=ar_... qa/containment_demo.py [--session ID] [--keep]
"""
import argparse, json, os, sys, time, urllib.error, urllib.request

HOST = os.environ.get("AR_HOST", "https://144-202-59-108.sslip.io").rstrip("/")
KEY = os.environ["AR_KEY"]

# (label, command, blocked when). Each command prints one short line.
PROBES = [
    ("Steal cloud credentials (metadata 169.254.169.254)",
     "curl -s -m 5 -o /dev/null -w '%{http_code}' http://169.254.169.254/v1.json || echo unreachable",
     lambda out, code: "unreachable" in out or out.strip() in ("000", "")),
    ("Find API keys in the environment",
     "env | grep -ciE 'key|token|secret|passw' || true",
     lambda out, code: out.strip() == "0"),
    ("Reach the private network (control VM 10.42.0.3)",
     "timeout 5 bash -c 'echo > /dev/tcp/10.42.0.3/443' 2>/dev/null && echo open || echo unreachable",
     lambda out, code: "unreachable" in out),
    ("SSH into the host",
     "gw=$(ip route 2>/dev/null | awk '/default/{print $3}'); gw=${gw:-$(awk 'NR>1 && $2==\"00000000\"{print $3}' /proc/net/route | head -1)}; "
     "timeout 5 bash -c \"echo > /dev/tcp/${gw:-172.30.0.1}/22\" 2>/dev/null && echo open || echo unreachable",
     lambda out, code: "unreachable" in out),
    ("Send spam (SMTP port 25)",
     "timeout 6 bash -c 'echo > /dev/tcp/gmail-smtp-in.l.google.com/25' 2>/dev/null && echo open || echo unreachable",
     lambda out, code: "unreachable" in out),
    ("Write to the system files (/usr)",
     "touch /usr/pwned 2>/dev/null && echo written || echo refused",
     lambda out, code: "refused" in out),
    ("Fork bomb (spawn 2000 processes)",
     "python3 -c \"import os,time\nn=0\ntry:\n  for _ in range(2000):\n    if os.fork()==0:\n      time.sleep(20); os._exit(0)\n    n+=1\nexcept OSError: pass\nprint(n)\"; pkill -f 'import os,time' 2>/dev/null; true",
     lambda out, code: code == "killed" or (out.strip().splitlines()[0].isdigit()
                                             and int(out.strip().splitlines()[0]) < 2000)),
    ("Eat all the memory (allocate 8 GiB)",
     "python3 -c \"b=bytearray(8<<30); print('allocated')\" 2>&1 | tail -1; echo exit=${PIPESTATUS[0]}",
     lambda out, code: code == "killed" or "allocated" not in out),
]
ALLOWED = ("Reach the public internet (it still does real work)",
           "curl -s -m 10 -o /dev/null -w '%{http_code}' https://example.com",
           lambda out, code: out.strip() == "200")


def call(method, path, body=None, timeout=120):
    req = urllib.request.Request(HOST + path, method=method,
                                 data=None if body is None else json.dumps(body).encode(),
                                 headers={"authorization": f"Bearer {KEY}", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def run(sid, cmd):
    """Run a command in the box. A box that dies at its limits is reported, not raised."""
    try:
        r = call("POST", f"/v1/sessions/{sid}/exec", {"command": cmd, "timeout_seconds": 60}, timeout=90)
    except urllib.error.HTTPError as e:
        msg = json.loads(e.read() or b"{}").get("error", "")
        if "box stopped" in msg:
            return "box killed at its limit", "killed"
        raise
    return (r["stdout"] + r["stderr"]).strip(), r["exit_code"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--session")
    ap.add_argument("--keep", action="store_true", help="leave the session open")
    a = ap.parse_args()
    sid = a.session or call("POST", "/v1/sessions", {"title": "Containment check"})["session"]["id"]
    print(f"Session {sid[:8]}: trying to break out of its box\n")
    run(sid, "echo 'my work' > /workspace/notes.txt")
    bad = 0
    for label, cmd, blocked in PROBES:
        t = time.time()
        out, code = run(sid, cmd)
        ok = blocked(out, code)
        bad += not ok
        print(f"  {'BLOCKED' if ok else 'GOT OUT'}  {label}  [{out.splitlines()[-1] if out else code}, {time.time() - t:.1f} s]")
    out, _ = run(sid, "cat /workspace/notes.txt")
    kept = out == "my work"
    bad += not kept
    print(f"\n  {'KEPT   ' if kept else 'LOST   '}  The box's own files survive its crashes  [{out}]")
    label, cmd, works = ALLOWED
    out, code = run(sid, cmd)
    print(f"  {'WORKS  ' if works(out, code) else 'FAILED '}  {label}  [{out}]")
    print(f"\n{len(PROBES) - bad} of {len(PROBES)} escape attempts blocked.")
    if not a.session and not a.keep:
        call("DELETE", f"/v1/sessions/{sid}?purge=true")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
