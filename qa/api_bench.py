"""Latency benchmark for the control API: N requests at concurrency C per path.
Usage: python3 -I api_bench.py BASE_URL N C [path ...]"""
import concurrent.futures, json, ssl, statistics, sys, time, urllib.request

base, n, c = sys.argv[1].rstrip("/"), int(sys.argv[2]), int(sys.argv[3])
paths = sys.argv[4:] or ["/api/status", "/", "/assets/app.js"]
ctx = ssl.create_default_context()


def one(path):
    t = time.perf_counter()
    with urllib.request.urlopen(base + path, timeout=30, context=ctx if base.startswith("https") else None) as r:
        r.read()
        code = r.status
    return (time.perf_counter() - t) * 1000, code


out = {}
for path in paths:
    t0 = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(c) as pool:
        rows = list(pool.map(lambda _: one(path), range(n)))
    wall = time.perf_counter() - t0
    ms = sorted(r[0] for r in rows)
    q = lambda p: round(ms[min(len(ms) - 1, int(p * len(ms)))], 1)
    out[path] = {"n": n, "concurrency": c, "rps": round(n / wall), "p50_ms": q(0.50), "p95_ms": q(0.95),
                 "p99_ms": q(0.99), "max_ms": round(ms[-1], 1), "errors": sum(1 for r in rows if r[1] != 200)}
print(json.dumps(out, indent=1))
