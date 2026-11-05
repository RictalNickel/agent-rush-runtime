"""Drive one real job through the deployed product API and save its evidence.

Usage: AR_OPERATOR_PASSWORD=... [JOB_ID=existing] python3 -I qa/live_job.py https://HOST "task prompt" OUT_DIR
Uses only the public HTTPS API (login, CSRF, idempotent create, poll, evidence export).
"""
import http.cookiejar, json, os, sys, time, urllib.request, uuid

base, prompt, out_dir = sys.argv[1].rstrip("/"), sys.argv[2], sys.argv[3]
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))


def call(method, path, body=None, headers=None):
    req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json", **(headers or {})},
                                 data=json.dumps(body).encode() if body is not None else None)
    with opener.open(req, timeout=60) as r:
        return json.load(r)


csrf = call("POST", "/api/login", {"password": os.environ["AR_OPERATOR_PASSWORD"]})["csrf"]
job = {"id": os.environ["JOB_ID"]} if os.environ.get("JOB_ID") else call("POST", "/api/jobs", {"prompt": prompt, "profile": "cli", "memory_mb": 512, "timeout_seconds": 60},
           {"x-csrf-token": csrf, "idempotency-key": str(uuid.uuid4())})
print("job", job["id"], flush=True)
seen, started = 0, time.time()
while True:
    j = call("GET", f"/api/jobs/{job['id']}")
    for e in j.get("events", [])[seen:]:
        print(f"  [{e.get('kind')}] {str(e.get('message'))[:160]}", flush=True)
    seen = len(j.get("events", []))
    if j.get("state") in ("succeeded", "failed", "cancelled"):
        break
    if time.time() - started > 1200:
        sys.exit("gave up waiting after 20 minutes")
    time.sleep(3)
evidence = call("GET", f"/api/jobs/{job['id']}/evidence")
contexts = call("GET", f"/api/jobs/{job['id']}/contexts")
os.makedirs(out_dir, exist_ok=True)
for name, data in (("job", j), ("evidence", evidence), ("contexts", contexts)):
    with open(os.path.join(out_dir, f"{job['id']}-{name}.json"), "w") as f:
        json.dump(data, f, indent=2)
r = evidence.get("receipt") or j.get("receipt") or {}
calls = r.get("inference_calls", [])
print(json.dumps({
    "state": j.get("state"), "reason": r.get("reason"), "exit_code": r.get("exit_code"),
    "checks_passed": r.get("checks_passed"), "cleanup_state": r.get("cleanup_state"),
    "review": r.get("review"), "model_calls": r.get("model_calls"),
    "calls": [(c.get("role"), c.get("model"), c.get("effort"), c.get("input_tokens"), c.get("output_tokens"),
               c.get("cache_read_input_tokens"), round(c.get("accounted_usd") or 0, 5)) for c in calls],
    "total_usd": round(sum(c.get("accounted_usd") or 0 for c in calls), 5),
    "result": r.get("result"), "explanation": (r.get("explanation") or "")[:600],
    "seconds": round(time.time() - started),
}, indent=1))
