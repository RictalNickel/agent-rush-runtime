#!/usr/bin/env python3
"""Benchmark a Boxoro instance through its /v1 API (stdlib only).

  infra   box throughput without the model: N sessions at once, each runs fixed workloads
          through POST /v1/sessions/{id}/exec (cold start, CPU, numpy, disk, npm install).
  agent   the agent itself: tasks with hidden checks, run in parallel (--suite basic or hard;
          --model a,b runs each task on both models side by side).
  soak    session churn for --minutes: --concurrency workers create, use and delete sessions.

Usage:
  AR_HOST=https://host AR_KEY=ar_... bench.py infra --levels 1,4,8,16 --out infra.json
  AR_HOST=https://host AR_KEY=ar_... bench.py agent [--model glm-5.3] [--repeat 1] --out agent.json

Every session the benchmark creates is deleted for good at the end (purge=true).
"""
import argparse, concurrent.futures as cf, hashlib, json, os, ssl, statistics, sys, threading, time, urllib.error, urllib.request

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


def soak(concurrency, minutes):
    """Session churn for a fixed time: each worker creates a session, round-trips a 1 MiB file,
    runs a short CPU command, deletes the session for good, and repeats. Rows are per minute."""
    stop, t0 = time.monotonic() + minutes * 60, time.monotonic()
    lock, log = threading.Lock(), []

    def worker(i):
        n = 0
        while time.monotonic() < stop:
            t, err, sid = time.monotonic(), None, None
            try:
                sid = create(f"bench-soak-{i}-{n}")
                blob = b"x" + os.urandom(1 << 20)
                st, res = call("PUT", f"/v1/sessions/{sid}/files?path=/workspace/blob.bin", raw=blob)
                assert st == 200, ("put", st, res)
                r = exec_(sid, "sha256sum /workspace/blob.bin && python3 -c 'import hashlib\nh=b\"x\"\n"
                               "for _ in range(200_000): h=hashlib.sha256(h).digest()'", timeout=300)
                assert r["ok"], ("exec", r)
                st, res = call("GET", f"/v1/sessions/{sid}/files?path=/workspace/blob.bin")
                assert st == 200 and hashlib.sha256(res).digest() == hashlib.sha256(blob).digest(), ("get", st)
            except Exception as e:
                err = repr(e)[:300]
            finally:
                if sid:
                    purge(sid)
            with lock:
                log.append((time.monotonic() - t0, time.monotonic() - t, err))
            n += 1

    with cf.ThreadPoolExecutor(concurrency) as pool:
        list(pool.map(worker, range(concurrency)))
    out = []
    for m in range(int(max(x[0] for x in log) // 60) + 1):
        rows = [x for x in log if m * 60 <= x[0] < (m + 1) * 60]
        ok = [x[1] for x in rows if not x[2]]
        errs = [x[2] for x in rows if x[2]]
        row = {"minute": m + 1, "cycles": len(rows), "failed": len(errs),
               "median_s": round(statistics.median(ok), 2) if ok else None,
               "p95_s": round(sorted(ok)[int(len(ok) * 0.95) - 1], 2) if len(ok) >= 20 else None,
               "first_failure": errs[0] if errs else None}
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


# ---- hard suite: longer, multi-step tasks whose hidden checks cover cases the prompt only states ----

MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]


def _etl():
    """A dirty CSV (mixed date and money formats, padded or upper-case emails, duplicate rows,
    missing amounts) and the expected clean result."""
    import datetime, random
    r = random.Random(23)
    emails = [f"{n}@example.com" for n in ["ana", "bo", "cy", "dee", "eli", "fay", "gus", "hal", "ivy", "jon"]]
    missing = set(r.sample(range(1, 301), 12))
    lines, kept = [], {}
    for i in range(1, 301):
        d = datetime.date(2026, 1, 1) + datetime.timedelta(days=r.randint(0, 364))
        email, cents = r.choice(emails), r.randint(100, 500000)
        ds = [d.isoformat(), d.strftime("%m/%d/%Y"), f"{d.day} {MONTHS[d.month - 1]} {d.year}"][r.randint(0, 2)]
        es = r.choice([email, email.upper(), f"  {email} ", email.capitalize()])
        a = cents / 100
        amt = ["", f"{a:.2f}", f'"${a:,.2f}"', f"{a:.2f} USD"][0 if i in missing else r.randint(1, 3)]
        lines.append(f"{i},{ds},{es},{amt}")
        if i not in missing:
            kept[i] = (d.isoformat(), email, cents)
    dups = r.sample([l for l in lines if not l.endswith(",")], 20)
    for l in dups:
        lines.insert(r.randint(lines.index(l) + 1, len(lines)), l)
    totals, months = {}, {}
    for d, e, c in kept.values():
        totals[e] = totals.get(e, 0) + c
        months[d[:7]] = months.get(d[:7], 0) + c
    report = {"rows_in": len(lines), "rows_out": len(kept), "duplicates_removed": 20, "missing_amount_dropped": 12,
              "total_amount": sum(c for _, _, c in kept.values()) / 100, "top_email": max(totals, key=totals.get),
              "monthly_totals": {m: c / 100 for m, c in sorted(months.items())}}
    return "id,date,email,amount\n" + "\n".join(lines) + "\n", {"rows": kept, "report": report}


ETL_CSV, ETL_EXPECTED = _etl()


def _sched():
    """A 30-task dependency graph, the same graph with a three-task cycle, and the expected plan."""
    import heapq, random
    r = random.Random(5)
    names = []
    while len(names) < 30:
        n = "".join(r.choice("abcdefghijklmnopqrstuvwxyz") for _ in range(4))
        if n not in names:
            names.append(n)
    tasks = {}
    for i, n in enumerate(names):
        tasks[n] = {"duration": r.randint(1, 9), "deps": sorted(r.sample(names[:i], min(i, r.randint(0, 3))))}
    bad = json.loads(json.dumps(tasks))
    for a, b in ((10, 9), (11, 10), (9, 11)):  # 9 -> 11 -> 10 -> 9
        bad[names[a]]["deps"] = sorted(set(bad[names[a]]["deps"]) | {names[b]})
    succ = {n: [m for m in tasks if n in tasks[m]["deps"]] for n in tasks}
    indeg = {n: len(t["deps"]) for n, t in tasks.items()}
    ready = [n for n in tasks if not indeg[n]]
    heapq.heapify(ready)
    order, es = [], {}
    while ready:
        n = heapq.heappop(ready)
        order.append(n)
        es[n] = max((es[d] + tasks[d]["duration"] for d in tasks[n]["deps"]), default=0)
        for m in succ[n]:
            indeg[m] -= 1
            if not indeg[m]:
                heapq.heappush(ready, m)
    finish = max(es[n] + tasks[n]["duration"] for n in tasks)
    ls = {}
    for n in reversed(order):
        ls[n] = min((ls[m] for m in succ[n]), default=finish) - tasks[n]["duration"]
    plan = {"order": order, "earliest_start": es, "finish": finish, "critical": sorted(n for n in tasks if ls[n] == es[n])}
    return tasks, bad, plan, sorted(names[i] for i in (9, 10, 11))


SCHED_TASKS, SCHED_BAD, SCHED_PLAN, SCHED_CYCLE = _sched()

LEDGER_FILES = {
    "ledger_proj/ledger/__init__.py": "",
    "ledger_proj/ledger/parse.py": '''from datetime import date


def parse_line(line):
    """'2026-01-03, deposit, 12.50' -> (date, kind, cents)"""
    d, kind, amount = line.split(",")
    y, m, dd = (int(p) for p in d.split("-"))
    return date(y, m, dd), kind, int(float(amount) * 100)


def parse_file(path):
    with open(path) as f:
        return [parse_line(l) for l in f if l.strip() and not l.startswith("#")]
''',
    "ledger_proj/ledger/book.py": '''SIGN = {"deposit": 1, "withdrawal": -1, "fee": -1}


def balance(entries, until=None):
    total = 0
    for d, kind, cents in entries:
        if until and d > until:
            break
        total += SIGN.get(kind, 0) * cents
    return total
''',
    "ledger_proj/ledger/report.py": '''from collections import defaultdict

from .book import SIGN


def monthly(entries):
    """{'YYYY-MM': net cents}, sorted by month"""
    out = defaultdict(int)
    for d, kind, cents in entries:
        out[d.strftime("%m")] += SIGN[kind] * cents
    return dict(sorted(out.items()))


def fmt(cents):
    return f"{cents // 100}.{cents % 100:02d}"
''',
    "ledger_proj/test_ledger.py": '''from ledger.parse import parse_line
from ledger.book import balance
from ledger.report import monthly, fmt

e = [parse_line(l) for l in ["2026-01-03, deposit, 100.00", "2026-02-01, fee, 1.50", "2025-01-09, deposit, 20.00"]]
assert e[0][1] == "deposit", e[0]
assert balance(e) == 11850, balance(e)
assert monthly(e) == {"2025-01": 2000, "2026-01": 10000, "2026-02": -150}, monthly(e)
assert fmt(12345) == "123.45"
print("ALL PASS")
''',
    "ledger_proj/SPEC.md": '''# ledger

- `parse_line` returns `(date, kind, cents)`. Fields may have spaces around them; `kind` has none.
  `cents` is exact: `"0.29"` is 29 and `"19.99"` is 1999.
- `balance(entries, until=None)` sums deposits minus withdrawals and fees, in cents. With `until`, it
  counts every entry dated on or before `until`; entries are in no particular order. An unknown kind
  raises `ValueError`.
- `monthly(entries)` maps `"YYYY-MM"` to the net cents of that month, in month order.
- `fmt(cents)` formats cents as money: `12345` is `"123.45"`, `-150` is `"-1.50"`, `-5` is `"-0.05"`.
''',
}
LEDGER_HIDDEN = '''from datetime import date
from ledger.parse import parse_line
from ledger.book import balance
from ledger.report import monthly, fmt
assert parse_line("2026-03-04 ,  withdrawal ,0.29") == (date(2026, 3, 4), "withdrawal", 29), parse_line("2026-03-04 ,  withdrawal ,0.29")
assert parse_line("2026-03-04,deposit,19.99")[2] == 1999
e = [parse_line(l) for l in ["2026-05-01, deposit, 10.00", "2026-01-01, deposit, 5.00", "2026-03-01, fee, 0.10"]]
assert balance(e, until=date(2026, 3, 31)) == 490, balance(e, until=date(2026, 3, 31))
assert balance(e) == 1490
try:
    balance([(date(2026, 1, 1), "refund", 100)])
    raise SystemExit("unknown kind accepted")
except ValueError:
    pass
assert (fmt(-150), fmt(-5), fmt(0), fmt(100)) == ("-1.50", "-0.05", "0.00", "1.00"), (fmt(-150), fmt(-5), fmt(0), fmt(100))
assert list(monthly(e)) == ["2026-01", "2026-03", "2026-05"], monthly(e)
print("HIDDEN PASS")
'''

EXPR_HIDDEN = '''import math
from expr import evaluate as e
v = {"x": 1, "a_b": 4}
cases = {"1+2*3": 7, "(1+2)*3": 9, "2^3^2": 512, "-2^2": -4, "2*-3": -6, "10/4": 2.5, "1 - 2 - 3": -4,
         "2^-1": 0.5, " 3.5 * ( 2 + x ) ": 10.5, "--3": 3, "a_b*2": 8, "8/2/2": 2}
for k, want in cases.items():
    got = e(k, v)
    assert math.isclose(got, want), (k, got, want)
for bad in ["1+", "(1", "1 2", "", "2**3", "y+1", "1)"]:
    try:
        e(bad, v)
    except ValueError:
        continue
    raise AssertionError(("no ValueError", bad))
try:
    e("1/0")
    raise AssertionError("1/0 did not raise ZeroDivisionError")
except ZeroDivisionError:
    pass
print("HIDDEN PASS")
'''

LRU_HIDDEN = '''import random, threading
from lru import LRUCache
t = [0.0]
c = LRUCache(2, ttl=10, clock=lambda: t[0])
c.put("a", 1); c.put("b", 2)
assert c.get("a") == 1
c.put("c", 3)
assert c.get("b") is None and c.get("b", "d") == "d"
assert c.get("a") == 1 and c.get("c") == 3
t[0] = 5; c.put("a", 11)
t[0] = 12
assert c.get("c") is None and c.get("a") == 11
assert len(c) == 1, len(c)
s = c.stats()
assert s["evictions"] == 1 and s["hits"] == 4 and s["misses"] == 3, s
z = LRUCache(0); z.put("x", 1)
assert z.get("x") is None and len(z) == 0
k = LRUCache(2); k.put("a", 1); k.put("b", 2); k.put("a", 3)
assert k.get("b") == 2 and k.get("a") == 3 and k.stats()["evictions"] == 0, k.stats()
big = LRUCache(50); errs = []
def work(seed):
    rr = random.Random(seed)
    try:
        for _ in range(3000):
            key = rr.randint(0, 200)
            big.put(key, key) if rr.random() < 0.5 else big.get(key)
    except Exception as ex:
        errs.append(repr(ex))
ts = [threading.Thread(target=work, args=(i,)) for i in range(8)]
[x.start() for x in ts]; [x.join() for x in ts]
assert not errs and len(big) <= 50, (errs, len(big))
print("HIDDEN PASS")
'''

KV_HIDDEN = '''import json, os, subprocess, threading, time, urllib.error, urllib.request
os.chdir("/workspace/kv")
D = "/tmp/kvcheck.json"
if os.path.exists(D):
    os.remove(D)
def start():
    p = subprocess.Popen(["python3", "server.py", "9137", D], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(50):
        try:
            urllib.request.urlopen("http://127.0.0.1:9137/kv", timeout=1)
            return p
        except urllib.error.HTTPError:
            return p
        except Exception:
            time.sleep(0.2)
    p.kill()
    raise SystemExit("server did not start")
def req(m, path, body=None):
    data = body if isinstance(body, bytes) or body is None else json.dumps(body).encode()
    r = urllib.request.Request("http://127.0.0.1:9137" + path, data=data, method=m, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(r, timeout=5) as x:
            t = x.read()
            return x.status, (json.loads(t) if t else None)
    except urllib.error.HTTPError as e:
        t = e.read()
        try:
            return e.code, (json.loads(t) if t else None)
        except ValueError:
            return e.code, None
p = start()
try:
    assert req("GET", "/kv") == (200, {"keys": []}), req("GET", "/kv")
    assert req("PUT", "/kv/a", {"x": 1})[0] == 201
    assert req("PUT", "/kv/a", [1, 2])[0] == 200
    assert req("GET", "/kv/a") == (200, [1, 2]), req("GET", "/kv/a")
    assert req("GET", "/kv/zz")[0] == 404
    assert req("PUT", "/kv/bad", b"{nope")[0] == 400
    assert req("PUT", "/kv/" + "k" * 65, {"x": 1})[0] == 400
    assert req("PUT", "/kv/a.b", {"x": 1})[0] == 400
    errs = []
    def w(i):
        try:
            s = req("PUT", f"/kv/c{i}", {"i": i})[0]
            if s != 201:
                errs.append(s)
        except Exception as ex:
            errs.append(repr(ex))
    ts = [threading.Thread(target=w, args=(i,)) for i in range(30)]
    [x.start() for x in ts]; [x.join() for x in ts]
    assert not errs, errs
    assert req("DELETE", "/kv/c0")[0] == 204
    assert req("DELETE", "/kv/c0")[0] == 404
finally:
    p.terminate(); p.wait(5)
p = start()
try:
    keys = req("GET", "/kv")[1]["keys"]
    assert keys == sorted(["a"] + [f"c{i}" for i in range(1, 30)]), keys
    assert req("GET", "/kv/c7") == (200, {"i": 7})
finally:
    p.terminate(); p.wait(5)
print("HIDDEN PASS")
'''

ETL_HIDDEN = '''import csv, json, sys
exp = json.load(open("/tmp/etl_expected.json"))
rows = list(csv.DictReader(open("/workspace/etl/clean.csv")))
want = {int(k): v for k, v in exp["rows"].items()}
got = {}
for r in rows:
    got[int(r["id"])] = (r["date"], r["email"], round(float(r["amount"]) * 100))
assert [int(r["id"]) for r in rows] == sorted(want), "ids not exactly the kept ids in order"
bad = {i: (got[i], tuple(want[i])) for i in want if got[i] != tuple(want[i])}
assert not bad, list(bad.items())[:5]
rep = json.load(open("/workspace/etl/report.json"))
e = exp["report"]
for k in ("rows_in", "rows_out", "duplicates_removed", "missing_amount_dropped", "top_email"):
    assert rep.get(k) == e[k], (k, rep.get(k), e[k])
assert abs(float(rep["total_amount"]) - e["total_amount"]) < 0.011, (rep["total_amount"], e["total_amount"])
mt = rep["monthly_totals"]
assert set(mt) == set(e["monthly_totals"]) and all(abs(float(mt[m]) - v) < 0.011 for m, v in e["monthly_totals"].items()), mt
print("HIDDEN PASS")
'''

SCHED_HIDDEN = '''import json
exp = json.load(open("/tmp/sched_expected.json"))
p = json.load(open("/workspace/sched/plan.json"))
assert p["order"] == exp["plan"]["order"], ("order", p["order"])
assert p["finish"] == exp["plan"]["finish"], ("finish", p["finish"], exp["plan"]["finish"])
assert p["earliest_start"] == exp["plan"]["earliest_start"], "earliest_start"
assert sorted(p["critical"]) == exp["plan"]["critical"], ("critical", p["critical"], exp["plan"]["critical"])
b = json.load(open("/workspace/sched/plan_bad.json"))
assert b.get("error") == "cycle" and sorted(b.get("cycle", [])) == exp["cycle"], (b, exp["cycle"])
print("HIDDEN PASS")
'''


def _hidden(name, text, run):
    """A check that writes a hidden file with a heredoc and runs it."""
    return f"cat > /tmp/{name} <<'HIDDEN_EOF'\n{text}HIDDEN_EOF\n{run}"


HARD_TASKS = {
    "expr": {
        "prompt": "Create /workspace/expr/expr.py with evaluate(text, variables=None) returning a float. Support "
                  "integer and decimal numbers, + - * /, ^ for power, unary minus, parentheses, and variable names "
                  "(letters and underscores) looked up in the variables dict. Precedence and associativity follow "
                  "Python's ** for ^ (so -2^2 is -4, 2^3^2 is 512 and 2^-1 is 0.5). Raise ValueError for malformed "
                  "input or an unknown variable and ZeroDivisionError for division by zero. Do not use eval, exec, "
                  "compile or the ast module. Write tests in /workspace/expr/test_expr.py, run them, and reply with "
                  "the result.",
        "check": _hidden("expr_hidden.py", EXPR_HIDDEN, "cd /workspace/expr && ! grep -nE '(^|[^.[:alnum:]_])(eval|exec|compile)"
                         "[[:space:]]*\\(|^[[:space:]]*(import ast|from ast)' expr.py && cp /tmp/expr_hidden.py . && "
                         "python3 expr_hidden.py; s=$?; rm -f expr_hidden.py; exit $s"),
    },
    "ledger": {
        "setup": LEDGER_FILES,
        "prompt": "/workspace/ledger_proj is a small Python package with bugs; `python3 test_ledger.py` in that folder "
                  "fails. SPEC.md says how it must behave, and the tests do not cover all of it. Fix the package so it "
                  "meets the whole spec, without editing the tests or the spec, then reply with each bug you fixed.",
        "check": _hidden("ledger_hidden.py", LEDGER_HIDDEN, "cd /workspace/ledger_proj && python3 test_ledger.py && "
                         "cp /tmp/ledger_hidden.py . && python3 ledger_hidden.py; s=$?; rm -f ledger_hidden.py; exit $s"),
    },
    "etl": {
        "setup": {"etl/raw.csv": ETL_CSV},
        "prompt": "/workspace/etl/raw.csv is messy: dates come as YYYY-MM-DD, MM/DD/YYYY or '5 Mar 2026'; amounts as "
                  "1234.50, \"$1,234.50\" or '1234.50 USD'; emails may have spaces or capitals; some rows repeat an "
                  "earlier row exactly and some have no amount. Write /workspace/etl/clean.py that drops the repeated "
                  "rows (keep the first), drops rows with no amount, and writes /workspace/etl/clean.csv with columns "
                  "id,date,email,amount (ISO date, trimmed lower-case email, amount with 2 decimals), sorted by id. "
                  "Also write /workspace/etl/report.json with rows_in (data rows in raw.csv), rows_out, "
                  "duplicates_removed, missing_amount_dropped, total_amount (sum of clean amounts), top_email (largest "
                  "total amount) and monthly_totals ({\"YYYY-MM\": total}). Run it and reply with the report.",
        "check": _hidden("etl_expected.json", json.dumps(ETL_EXPECTED) + "\n", "")
                 + _hidden("etl_hidden.py", ETL_HIDDEN, "python3 /tmp/etl_hidden.py"),
    },
    "sched": {
        "setup": {"sched/tasks.json": json.dumps(SCHED_TASKS, indent=1), "sched/tasks_bad.json": json.dumps(SCHED_BAD, indent=1)},
        "prompt": "/workspace/sched/tasks.json maps task names to {duration, deps}. Tasks run in parallel without limit, "
                  "and a task starts as soon as all its deps have finished. Write a solver /workspace/sched/plan.py and "
                  "use it to write /workspace/sched/plan.json with: order (a topological order that always takes the "
                  "alphabetically smallest ready task next), earliest_start ({task: earliest start time}), finish (when "
                  "the whole project ends) and critical (alphabetically sorted tasks with zero slack, whose delay would "
                  "delay the finish). /workspace/sched/tasks_bad.json contains a dependency cycle: write "
                  "/workspace/sched/plan_bad.json as {\"error\": \"cycle\", \"cycle\": [sorted names of every task that "
                  "is part of a cycle]}. Reply with the finish time and the cycle.",
        "check": _hidden("sched_expected.json", json.dumps({"plan": SCHED_PLAN, "cycle": SCHED_CYCLE}) + "\n", "")
                 + _hidden("sched_hidden.py", SCHED_HIDDEN, "python3 /tmp/sched_hidden.py"),
    },
    "kvapi": {
        "prompt": "Write /workspace/kv/server.py: a JSON key-value HTTP server using only the Python standard library, "
                  "started as `python3 server.py PORT DATAFILE`. PUT /kv/<key> with a JSON body stores it (201 if new, "
                  "200 if replaced); GET /kv/<key> returns the stored JSON (404 if missing); DELETE /kv/<key> answers "
                  "204, or 404 if missing; GET /kv returns {\"keys\": [sorted keys]}. A body that is not valid JSON "
                  "gets 400. Keys are 1-64 letters, digits, '-' or '_'; any other key gets 400. Data must survive a "
                  "restart (keep it in DATAFILE), and concurrent requests must not lose writes. Test it yourself, stop "
                  "your test server, and reply with how you tested it.",
        "check": _hidden("kv_hidden.py", KV_HIDDEN, "python3 /tmp/kv_hidden.py"),
    },
    "lru": {
        "prompt": "Create /workspace/lru/lru.py with class LRUCache(capacity, ttl=None, clock=time.monotonic). "
                  "get(key, default=None) returns the value and marks the key most recently used; put(key, value) "
                  "inserts or replaces and marks it most recently used, evicting the least recently used entry when "
                  "over capacity. An entry older than ttl seconds since its last put (measured with clock()) counts as "
                  "missing and is removed. len(cache) counts only live entries; capacity 0 stores nothing; the cache "
                  "must be safe to use from several threads. stats() returns {'hits', 'misses', 'evictions'}, where "
                  "evictions count only capacity evictions, not expiries. Write tests, run them, and reply with the "
                  "result.",
        "check": _hidden("lru_hidden.py", LRU_HIDDEN, "cd /workspace/lru && cp /tmp/lru_hidden.py . && python3 lru_hidden.py; "
                         "s=$?; rm -f lru_hidden.py; exit $s"),
    },
}
SUITES = {"basic": TASKS, "hard": HARD_TASKS}
ALL_TASKS = {**TASKS, **HARD_TASKS}


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
    task = ALL_TASKS[name]
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
        errors = [json.dumps(e.get("payload"))[:400] for e in ev if e.get("kind") == "error"]
        check = exec_(sid, task["check"], timeout=180)
        reply_ok = all(w in reply.lower() for w in task.get("reply_must", []))
        return {"task": name, "model": model or "default", "rep": rep, "status": s.get("status"),
                "end_reason": next((r for r in reversed(reasons) if r), None),
                "passed": check["ok"] and reply_ok and s.get("status") == "idle",
                "check": check, "reply_ok": reply_ok, "seconds": round(wall, 1),
                "tool_calls": sum(1 for e in ev if e.get("kind") == "tool_call"),
                "spend_usd": s.get("spend_usd"), "errors": errors[-3:], "reply": reply[:600]}
    finally:
        purge(sid)


def agent(model, repeat, only, suite="basic"):
    jobs = [(n, m, r) for r in range(repeat) for m in model.split(",") for n in (only or SUITES[suite])]
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
    p.add_argument("mode", choices=["infra", "agent", "soak"])
    p.add_argument("--concurrency", type=int, default=16)
    p.add_argument("--minutes", type=float, default=10)
    p.add_argument("--levels", default="1,4,8")
    p.add_argument("--model", default="", help="one model, or a comma list run side by side")
    p.add_argument("--suite", choices=list(SUITES), default="basic")
    p.add_argument("--repeat", type=int, default=1)
    p.add_argument("--tasks", help="comma list of " + ",".join(ALL_TASKS))
    p.add_argument("--label", default="")
    p.add_argument("--out", required=True)
    a = p.parse_args()
    if not HOST or not KEY:
        sys.exit("set AR_HOST and AR_KEY")
    st, status = call("GET", "/v1/status")
    started = time.time()
    if a.mode == "infra":
        res = infra([int(x) for x in a.levels.split(",")])
    elif a.mode == "soak":
        res = soak(a.concurrency, a.minutes)
    else:
        res = agent(a.model, a.repeat, a.tasks.split(",") if a.tasks else None, a.suite)
    doc = {"mode": a.mode, "label": a.label, "host": HOST, "model": a.model or None, "suite": a.suite, "started_at": int(started),
           "seconds": round(time.time() - started, 1), "status": status if st == 200 else None, "results": res}
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=1)


if __name__ == "__main__":
    main()
