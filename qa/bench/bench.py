#!/usr/bin/env python3
"""Benchmark a Vultr-Placeholder-Test123 instance through its /v1 API (stdlib only).

  infra   box throughput without the model: N sessions at once, each runs fixed workloads
          through POST /v1/sessions/{id}/exec (cold start, CPU, numpy, disk, npm install).
  agent   the agent itself: four tasks with objective checks, run in parallel on one model.

Usage:
  AR_HOST=https://host AR_KEY=ar_... bench.py infra --levels 1,4,8,16 --out infra.json
  AR_HOST=https://host AR_KEY=ar_... bench.py agent [--model glm-5.3] [--repeat 1] --out agent.json

Every session the benchmark creates is deleted for good at the end (purge=true).
"""
import argparse, concurrent.futures as cf, json, os, ssl, statistics, sys, time, urllib.error, urllib.request

HOST = os.environ.get("AR_HOST", "").rstrip("/")
KEY = os.environ.get("AR_KEY", "")
CTX = ssl.create_default_context()
if os.environ.get("AR_INSECURE") == "1":  # a tunnelled self-signed instance
    CTX.check_hostname, CTX.verify_mode = False, ssl.CERT_NONE


def call(method, path, body=None, raw=None, timeout=900):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(HOST + path, method=method, data=data, headers={
        "authorization": "Bearer " + KEY,
        "content-type": "application/octet-stream" if raw is not None else "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout, context=CTX) as r:
            text = r.read()
            return r.status, (json.loads(text) if text[:1] in (b"{", b"[") else text)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")[:500]


def create(title, message=None, model=None):
    body = {"title": title}
    if message:
        body["message"] = message
    if model:
        body["model"] = model
    st, res = call("POST", "/v1/sessions", body)
    if st != 201:
        raise RuntimeError(f"create {title}: {st} {res}")
    return res["session"]["id"]


def purge(sid):
    if os.environ.get("AR_BENCH_KEEP") == "1":  # keep sessions to inspect their events
        return
    call("DELETE", f"/v1/sessions/{sid}?purge=true")


def exec_(sid, command, timeout=600):
    t = time.monotonic()
    st, res = call("POST", f"/v1/sessions/{sid}/exec", {"command": command, "timeout_seconds": timeout})
    dt = time.monotonic() - t
    ok = st == 200 and isinstance(res, dict) and res.get("exit_code") == 0 and not res.get("timed_out")
    return {"ok": ok, "seconds": round(dt, 2), "status": st,
            "detail": None if ok else (res if isinstance(res, str) else {k: res.get(k) for k in ("exit_code", "stderr", "timed_out")})}


# ---- infra ---------------------------------------------------------------------------------

WORKLOADS = [
    ("cold_start", "true"),
    ("cpu_python", "python3 -c 'import hashlib\nh=b\"x\"\nfor _ in range(2_000_000): h=hashlib.sha256(h).digest()'"),
    ("numpy_matmul", "python3 -c 'import numpy as n; a=n.random.rand(1500,1500); [a@a for _ in range(4)]'"),
    ("disk_write_256mb", "dd if=/dev/zero of=/workspace/bench.bin bs=1M count=256 conv=fsync status=none && rm /workspace/bench.bin"),
    ("npm_install", "mkdir -p /tmp/np && cd /tmp/np && npm init -y >/dev/null && npm install --silent --no-audit --no-fund lodash@4.17.21 express@4.21.2 >/dev/null"),
]


def infra_one(i):
    sid = create(f"bench-infra-{i}")
    try:
        return {name: exec_(sid, cmd) for name, cmd in WORKLOADS}
    finally:
        purge(sid)


def infra(levels):
    out = []
    for n in levels:
        t = time.monotonic()
        with cf.ThreadPoolExecutor(n) as pool:
            runs = list(pool.map(infra_one, range(n)))
        wall = time.monotonic() - t
        row = {"concurrency": n, "wall_seconds": round(wall, 1), "workloads": {}}
        for name, _ in WORKLOADS:
            secs = [r[name]["seconds"] for r in runs if r[name]["ok"]]
            fails = [r[name] for r in runs if not r[name]["ok"]]
            row["workloads"][name] = {
                "ok": len(secs), "failed": len(fails),
                "median_s": round(statistics.median(secs), 2) if secs else None,
                "max_s": max(secs) if secs else None,
                "first_failure": fails[0] if fails else None}
        print(json.dumps(row), flush=True)
        out.append(row)
    return out


# ---- agent ---------------------------------------------------------------------------------

BUGGY = '''def mean(xs):
    return sum(xs) / len(xs) + 1

def median(xs):
    s = sorted(xs)
    return s[len(s) // 2]

def clamp(x, lo, hi):
    return max(hi, min(lo, x))
'''
BUG_TESTS = '''from calc import mean, median, clamp
assert mean([1, 2, 3]) == 2
assert median([3, 1, 2]) == 2
assert median([4, 1, 3, 2]) == 2.5
assert clamp(5, 0, 3) == 3 and clamp(-1, 0, 3) == 0 and clamp(2, 0, 3) == 2
print("ALL PASS")
'''

def _orders_csv():
    import random
    r = random.Random(11)
    names = ["ana", "bo", "cy", "dee", "eli", "fay", "gus", "hal"]
    rows = ["order_id,customer,date,amount"]
    for i in range(1, 401):
        rows.append(f"{i},{r.choice(names)},2026-{r.randint(1, 12):02d}-{r.randint(1, 28):02d},{r.randint(500, 25000) / 100:.2f}")
    return "\n".join(rows) + "\n"


ORDERS = _orders_csv()
JS_BUGGY = """// Utilities for a shopping cart.
export function subtotal(items) {
  return items.reduce((s, i) => s + i.price * i.qty, 1);
}
export function applyDiscount(total, code) {
  if (code === "TEN") return total * 0.1;
  if (code === "FIVEOFF") return Math.max(0, total - 5);
  return total;
}
export function slug(title) {
  return title.toLowerCase().replace(" ", "-");
}
export function chunk(arr, size) {
  const out = [];
  for (let i = 0; i <= arr.length; i += size) out.push(arr.slice(i, i + size));
  return out;
}
"""
JS_TESTS = """import assert from "node:assert/strict";
import { subtotal, applyDiscount, slug, chunk } from "./cart.mjs";
assert.equal(subtotal([{ price: 2, qty: 3 }, { price: 1.5, qty: 2 }]), 9);
assert.equal(subtotal([]), 0);
assert.equal(applyDiscount(50, "TEN"), 45);
assert.equal(applyDiscount(3, "FIVEOFF"), 0);
assert.equal(applyDiscount(20, "NOPE"), 20);
assert.equal(slug("Hello Big  World"), "hello-big-world");
assert.deepEqual(chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
assert.deepEqual(chunk([], 3), []);
console.log("ALL PASS");
"""

TASKS = {
    "data": {
        "prompt": "Using Python with random seed 7, generate /workspace/sales.csv with a header and 500 rows: "
                  "date (any day in 2026, YYYY-MM-DD), region (North, South, East or West), units (integer 1-50), "
                  "price (5.00-99.99, two decimals). Then compute total revenue (units*price) per region and write "
                  "/workspace/summary.json as {region: revenue rounded to 2 decimals}. Reply with the four totals.",
        "check": "cd /workspace && python3 -c \"import csv,json\nr={}\nfor x in csv.DictReader(open('sales.csv')): "
                 "r[x['region']]=r.get(x['region'],0)+int(x['units'])*float(x['price'])\n"
                 "s=json.load(open('summary.json'))\nrows=sum(1 for _ in open('sales.csv'))-1\n"
                 "assert rows==500, rows\nassert set(r)==set(s), (r,s)\n"
                 "assert all(abs(r[k]-s[k])<0.02 for k in r), (r,s)\"",
    },
    "code": {
        "prompt": "Create /workspace/textstats/textstats.py defining word_freq(text, top=3) that returns the `top` most "
                  "common words as a list of (word, count) tuples: words are lowercased, punctuation is ignored, ties "
                  "are broken alphabetically. Add tests in /workspace/textstats/test_textstats.py covering at least "
                  "five cases, run them, and reply with the result.",
        "check": "cd /workspace/textstats && python3 -c \"from textstats import word_freq as w\n"
                 "assert w('b a, B c a!', top=2)==[('a',2),('b',2)], w('b a, B c a!', top=2)\n"
                 "assert w('Hello hello WORLD.')==[('hello',2),('world',1)]\nassert w('', top=3)==[]\"",
    },
    "bugfix": {
        "setup": {"bugs/calc.py": BUGGY, "bugs/test_calc.py": BUG_TESTS},
        "prompt": "The tests in /workspace/bugs/test_calc.py fail (run them with `python3 test_calc.py` in that "
                  "folder). Fix the code in calc.py, not the tests, until they pass, then reply with what you changed.",
        "check": "cd /workspace/bugs && cat > /tmp/t.py <<'EOF'\n" + BUG_TESTS + "EOF\ncp /tmp/t.py ./_hidden.py && "
                 "python3 _hidden.py && rm _hidden.py",
    },
    "sqlite": {
        "setup": {"orders.csv": ORDERS},
        "prompt": "Load /workspace/orders.csv into a SQLite database /workspace/shop.db (table orders). Using SQL on that "
                  "database, write /workspace/answers.json with: top_customer (the customer with the highest total "
                  "amount), top_customer_total (that total, 2 decimals), busiest_month (YYYY-MM with the most orders; "
                  "earliest month wins a tie), average_order (mean amount, 2 decimals) and orders_over_200 (count of "
                  "orders with amount above 200). Reply with the answers.",
        "check": "cd /workspace && python3 -c \"import csv,json,collections\n"
                 "rows=list(csv.DictReader(open('orders.csv')))\nt=collections.Counter()\nm=collections.Counter()\n"
                 "for r in rows: t[r['customer']]+=float(r['amount']); m[r['date'][:7]]+=1\n"
                 "top=max(t,key=t.get)\nbm=sorted(m.items(),key=lambda x:(-x[1],x[0]))[0][0]\n"
                 "a=json.load(open('answers.json'))\nimport sqlite3; assert sqlite3.connect('shop.db').execute('select count(*) from orders').fetchone()[0]==400\n"
                 "exp={'top_customer':top,'top_customer_total':round(t[top],2),'busiest_month':bm,"
                 "'average_order':round(sum(float(r['amount']) for r in rows)/len(rows),2),"
                 "'orders_over_200':sum(float(r['amount'])>200 for r in rows)}\n"
                 "bad={k:(a.get(k),v) for k,v in exp.items() if (abs(float(a.get(k,-1))-v)>0.011 if isinstance(v,float) else a.get(k)!=v)}\n"
                 "assert not bad, bad\"",
    },
    "jsfix": {
        "setup": {"cart/cart.mjs": JS_BUGGY, "cart/package.json": '{"name":"cart","type":"module"}\n'},
        "prompt": "/workspace/cart/cart.mjs has several bugs. Write tests for every exported function with node:test "
                  "or node:assert in /workspace/cart/cart.test.mjs, find and fix all the bugs in cart.mjs (keep the "
                  "function names and signatures; applyDiscount 'TEN' means ten percent off, 'FIVEOFF' means 5 off but "
                  "never below 0; slug joins words with single hyphens), run your tests with node, and reply with "
                  "the bugs you fixed.",
        "check": "cd /workspace/cart && cat > /tmp/h.mjs <<'EOF'\n" + JS_TESTS + "EOF\ncp /tmp/h.mjs ./_hidden.mjs && "
                 "node _hidden.mjs && rm _hidden.mjs",
    },
    "webapp": {
        "prompt": "Build a small to-do web app in /workspace/todo: one index.html with inline JavaScript, items kept in "
                  "localStorage, an input with id 'new' and a button with id 'add', items listed in <ul id='items'>. "
                  "Serve it on port 8080 in the background, then use the browser to add the items 'milk' and 'eggs' "
                  "and confirm both appear in the list. Reply with what the browser showed.",
        "check": "cd /workspace/todo && grep -q localStorage index.html && grep -q \"id=.items\" index.html",
        "reply_must": ["milk", "eggs"],
    },
}


def wait_idle(sid, deadline_s=1900):
    t = time.monotonic()
    while time.monotonic() - t < deadline_s:
        st, s = call("GET", f"/v1/sessions/{sid}")
        if st == 200 and s["status"] in ("idle", "failed", "closed"):
            return s
        time.sleep(3)
    call("POST", f"/v1/sessions/{sid}/cancel")
    return {"status": "timeout"}


def events(sid):
    out, after = [], 0
    while True:
        st, res = call("GET", f"/v1/sessions/{sid}/events?after={after}&limit=1000")
        if st != 200:
            return out
        out += res["events"]
        if not res.get("more"):
            return out
        after = res["next_after"]


def agent_one(name, model, rep):
    task = TASKS[name]
    sid = create(f"bench-{name}-{model or 'default'}-{rep}", model=model)
    try:
        for path, text in task.get("setup", {}).items():
            st, res = call("PUT", f"/v1/sessions/{sid}/files?path=/workspace/{path}", raw=text.encode())
            assert st == 200, (st, res)
        t = time.monotonic()
        st, res = call("POST", f"/v1/sessions/{sid}/messages", {"text": task["prompt"]})
        assert st == 202, (st, res)
        s = wait_idle(sid)
        wall = time.monotonic() - t
        ev = events(sid)
        reply = " ".join(e.get("payload", e).get("text", "") for e in ev if e.get("kind") == "assistant")
        reasons = [e.get("payload", e).get("reason") for e in ev if e.get("kind") == "status"]
        check = exec_(sid, task["check"], timeout=120)
        reply_ok = all(w in reply.lower() for w in task.get("reply_must", []))
        return {"task": name, "model": model or "default", "rep": rep, "status": s.get("status"),
                "end_reason": next((r for r in reversed(reasons) if r), None),
                "passed": check["ok"] and reply_ok and s.get("status") == "idle",
                "check": check, "reply_ok": reply_ok, "seconds": round(wall, 1),
                "tool_calls": sum(1 for e in ev if e.get("kind") == "tool_call"),
                "spend_usd": s.get("spend_usd"), "reply": reply[:600]}
    finally:
        purge(sid)


def agent(model, repeat, only):
    jobs = [(n, model, r) for r in range(repeat) for n in (only or TASKS)]
    with cf.ThreadPoolExecutor(len(jobs)) as pool:
        futs = [pool.submit(agent_one, *j) for j in jobs]
        out = []
        for f in cf.as_completed(futs):
            try:
                row = f.result()
            except Exception as e:  # keep the other runs' results
                row = {"error": repr(e)}
            print(json.dumps({k: v for k, v in row.items() if k != "reply"}), flush=True)
            out.append(row)
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("mode", choices=["infra", "agent"])
    p.add_argument("--levels", default="1,4,8")
    p.add_argument("--model")
    p.add_argument("--repeat", type=int, default=1)
    p.add_argument("--tasks", help="comma list of " + ",".join(TASKS))
    p.add_argument("--label", default="")
    p.add_argument("--out", required=True)
    a = p.parse_args()
    if not HOST or not KEY:
        sys.exit("set AR_HOST and AR_KEY")
    st, status = call("GET", "/v1/status")
    started = time.time()
    if a.mode == "infra":
        res = infra([int(x) for x in a.levels.split(",")])
    else:
        res = agent(a.model, a.repeat, a.tasks.split(",") if a.tasks else None)
    doc = {"mode": a.mode, "label": a.label, "host": HOST, "model": a.model, "started_at": int(started),
           "seconds": round(time.time() - started, 1), "status": status if st == 200 else None, "results": res}
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=1)


if __name__ == "__main__":
    main()
