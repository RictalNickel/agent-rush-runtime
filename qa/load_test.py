"""Submit several real jobs at once through the public API and record queueing, timing,
outcome and cost per job. Pair with qa/worker_sampler.sh on the worker for host metrics.

Usage: AR_OPERATOR_PASSWORD=... python3 -I qa/load_test.py https://HOST OUT_DIR
"""
import concurrent.futures, http.cookiejar, json, os, sys, time, urllib.request, uuid
from datetime import datetime

base, out_dir = sys.argv[1].rstrip("/"), sys.argv[2]
TASKS = [
    ("Compute the sum of the squares of 1..=100000 using u128 and check it against the closed form n(n+1)(2n+1)/6. Write both values and whether they match to result.json.", 512),
    ("Count the words, lines and distinct words in this text: 'the quick brown fox jumps over the lazy dog the dog sleeps'. Check the counts with a second method. Write the counts to result.json.", 512),
    ("Find all Pythagorean triples with c <= 200, count them, and check each with integer arithmetic. Write the count and the first five triples to result.json.", 768),
    ("Compute the 90th Fibonacci number iteratively and with fast doubling, assert the two agree, and write the value to result.json.", 512),
]
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))


def call(method, path, body=None, headers=None):
    req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json", **(headers or {})},
                                 data=json.dumps(body).encode() if body is not None else None)
    with opener.open(req, timeout=60) as r:
        return json.load(r)


csrf = call("POST", "/api/login", {"password": os.environ["AR_OPERATOR_PASSWORD"]})["csrf"]
t0 = time.time()


def submit(task):
    prompt, mem = task
    return call("POST", "/api/jobs", {"prompt": prompt, "profile": "cli", "memory_mb": mem, "timeout_seconds": 60},
                {"x-csrf-token": csrf, "idempotency-key": str(uuid.uuid4())})["id"]


with concurrent.futures.ThreadPoolExecutor(len(TASKS)) as pool:
    ids = list(pool.map(submit, TASKS))
print("submitted", len(ids), "jobs in", round(time.time() - t0, 2), "s", flush=True)
done = {}
while len(done) < len(ids) and time.time() - t0 < 1800:
    for i in ids:
        if i not in done:
            j = call("GET", f"/api/jobs/{i}")
            if j["state"] in ("succeeded", "failed", "cancelled"):
                done[i] = j
                print(f"  {i[:8]} {j['state']} after {round(time.time() - t0)} s", flush=True)
    time.sleep(3)


def ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


rows = []
os.makedirs(out_dir, exist_ok=True)
for (prompt, mem), i in zip(TASKS, ids):
    j = done.get(i) or call("GET", f"/api/jobs/{i}")
    ev = {e["kind"]: ts(e["at"]) for e in reversed(j["events"])}  # first occurrence of each kind
    r = j.get("receipt") or {}
    calls = r.get("inference_calls", [])
    rows.append({
        "job": i[:8], "state": j["state"], "memory_mb": mem,
        "queued_s": round(ev.get("admitted", ev["queued"]) - ev["queued"], 1),
        "total_s": round(ts(j["updated_at"]) - ev["queued"], 1),
        "sandbox_ms": r.get("execution_ms"), "cleanup": r.get("cleanup_state"),
        "review": (r.get("review") or {}).get("approved"), "model_calls": r.get("model_calls"),
        "usd": round(sum(c.get("accounted_usd") or 0 for c in calls), 4), "result": r.get("result"),
    })
    with open(os.path.join(out_dir, f"load-{i}.json"), "w") as f:
        json.dump(j, f, indent=2)
summary = {"jobs": rows, "wall_s": round(time.time() - t0), "total_usd": round(sum(x["usd"] for x in rows), 4),
           "status_after": call("GET", "/api/status")}
with open(os.path.join(out_dir, "load-summary.json"), "w") as f:
    json.dump(summary, f, indent=2)
print(json.dumps({k: summary[k] for k in ("jobs", "wall_s", "total_usd")}, indent=1))
