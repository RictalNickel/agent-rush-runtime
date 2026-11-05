"""Submit several CPU-bound sandbox runs to the private worker at the same moment and
verify they overlap, stay within their leases and are all cleaned up.

Run on the control VM as root (reads the mTLS client identity and worker token):
  python3 -I overlap_test.py N OUT.json
"""
import concurrent.futures, json, os, ssl, sys, time, urllib.request, uuid
from datetime import datetime

N, OUT = int(sys.argv[1]), sys.argv[2]
env = dict(l.strip().split("=", 1) for l in open("/etc/agent-rush/control.env") if "=" in l and not l.startswith("#"))
BASE, TOKEN = env["AR_WORKER_URL"], env["AR_WORKER_TOKEN"]
ctx = ssl.create_default_context(cafile=env["AR_WORKER_CA_PATH"])
ctx.load_cert_chain(env["AR_WORKER_IDENTITY_PATH"])

CODE = r'''
use std::time::{Duration, Instant};
fn main() {
    let start = Instant::now();
    let mut x: u64 = 0x9E3779B97F4A7C15;
    let mut rounds: u64 = 0;
    while start.elapsed() < Duration::from_secs(4) {
        for _ in 0..100_000 { x ^= x << 13; x ^= x >> 7; x ^= x << 17; }
        rounds += 1;
    }
    std::fs::create_dir_all("/work/artifacts").unwrap();
    std::fs::write("/work/artifacts/result.json", format!("{{\"rounds\":{rounds},\"x\":{x}}}")).unwrap();
    println!("rounds={rounds}");
}
'''


def call(method, path, body=None):
    req = urllib.request.Request(BASE + path, method=method, data=json.dumps(body).encode() if body else None,
                                 headers={"Authorization": "Bearer " + TOKEN, "Content-Type": "application/json"})
    with urllib.request.urlopen(req, context=ctx, timeout=30) as r:
        return r.status, json.load(r)


def run(i):
    rid = f"overlap-{uuid.uuid4()}"
    t0 = time.time()
    try:
        status, _ = call("POST", "/v1/runs", {"id": rid, "code": CODE, "limits": {"memory_mb": 512, "timeout_seconds": 60, "cpu_millis": 1000}})
    except urllib.error.HTTPError as e:
        return {"i": i, "admitted": False, "http": e.code, "detail": e.read().decode()[:200]}
    while time.time() - t0 < 240:
        _, v = call("GET", f"/v1/runs/{rid}")
        if v.get("state") == "finished":
            r = v["receipt"]
            return {"i": i, "admitted": True, "id": rid, "reason": r.get("reason"), "exit": r.get("exit_code"),
                    "cleanup": r.get("cleanup_state"), "start": r.get("execution_started_at"),
                    "end": r.get("execution_finished_at"), "exec_ms": r.get("execution_ms"),
                    "duration_ms": r.get("duration_ms"), "result": r.get("result"),
                    "cpu_samples": [s["docker_stats"].get("CPUPerc") for s in r.get("resource_samples", [])][-6:]}
        time.sleep(0.25)
    return {"i": i, "admitted": True, "id": rid, "reason": "client_timeout"}


t = time.time()
with concurrent.futures.ThreadPoolExecutor(N) as pool:
    results = list(pool.map(run, range(N)))
wall = round(time.time() - t, 1)
ts = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
done = [r for r in results if r.get("start")]
overlap = 0.0
if len(done) >= 2:
    latest_start = max(ts(r["start"]) for r in done)
    earliest_end = min(ts(r["end"]) for r in done)
    overlap = round(earliest_end - latest_start, 2)
summary = {"submitted": N, "wall_s": wall, "all_runs_overlap_s": overlap, "runs": results}
json.dump(summary, open(OUT, "w"), indent=2)
print(json.dumps({k: summary[k] for k in ("submitted", "wall_s", "all_runs_overlap_s")}))
for r in results:
    print({k: r.get(k) for k in ("i", "admitted", "http", "reason", "exit", "cleanup", "exec_ms", "duration_ms", "result", "cpu_samples")})
