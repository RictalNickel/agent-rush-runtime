# Benchmarks

Measured on 2026-11-06 with [`qa/bench/bench.py`](../qa/bench/bench.py). Raw results are in [`docs/evidence/live/benchmark-2026-11-06/`](evidence/live/benchmark-2026-11-06/) and, for the 16 vCPU round, [`benchmark-2026-11-06-16c/`](evidence/live/benchmark-2026-11-06-16c/). Print the tables yourself with `qa/bench/report.py infra|agent|matrix|soak <files>`.

Hosts:

| Name | Vultr plan | Use |
| --- | --- | --- |
| live worker | `vc2-4c-8gb` (4 vCPU, 8 GB), `AR_BOX_MAX=8` | The public instance's worker, with its control plane on a separate `vc2-2c-4gb` |
| bench | `vc2-8c-32gb` (8 vCPU, 32 GB), `AR_BOX_MAX=24`, `AR_CAPACITY_MB=28672` | A temporary VM installed from scratch with `scripts/install.sh --role all` (control and worker on one host), deleted after the run |
| bench-16 | `voc-g-16c-64gb-320s-amd` (16 dedicated vCPU, 64 GB, 320 GB disk), `AR_BOX_MAX=96`, `AR_CAPACITY_MB=59392` | Same one-host install (it took about 4 minutes and passed every check first time), deleted after about an hour |

## Box throughput (no model)

N sessions start at once. Each one runs the same commands through `POST /v1/sessions/{id}/exec`:

- `cold_start`: `true` (creates the gVisor box);
- `cpu_python`: 2 million SHA-256 rounds;
- `numpy_matmul`: four 1500×1500 matrix products;
- `disk_write_256mb`: fsync'd write to the quota-limited `/workspace`;
- `npm_install`: lodash and express from the public registry.

Times are medians in seconds. Session boxes start at 1 vCPU and 1 GiB (`AR_SESSION_BOX_MB`) and grow under load.

| Host | Sessions at once | Wall | Cold start | CPU (Python) | numpy | Disk 256 MB | npm install | Refused |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| live worker | 1 | 34.8 | 0.90 | 3.92 | 21.6 | 1.09 | 6.8 | 0 |
| live worker | 4 | 71.7 | 1.85 | 7.38 | 37.0 | 2.19 | 21.4 | 0 |
| live worker | 8 | 74.8 | 3.30 | 9.99 | 43.3 | 2.22 | 15.5 | 3 |
| bench | 1 | 28.4 | 1.01 | 2.76 | 16.6 | 1.22 | 6.4 | 0 |
| bench | 4 | 32.7 | 1.05 | 2.63 | 18.2 | 2.55 | 7.5 | 0 |
| bench | 8 | 42.3 | 1.58 | 3.08 | 20.5 | 4.08 | 10.6 | 0 |
| bench | 16 | 85.0 | 3.05 | 6.01 | 43.0 | 6.01 | 23.1 | 0 |
| bench | 24 | 130.4 | 4.94 | 9.16 | 68.8 | 7.89 | 35.3 | 1 |
| bench-16 | 16 | 16.1 | 0.78 | 0.76 | 6.46 | 0.92 | 5.06 | 0 |
| bench-16 | 32 | 30.7 | 1.27 | 1.27 | 15.7 | 1.28 | 6.76 | 0 |
| bench-16 | 48 | 43.2 | 1.76 | 1.98 | 23.6 | 1.88 | 8.74 | 1 |
| bench-16, reserve fix | 64 | 50.3 | 2.38 | 2.34 | 28.6 | 1.48 | 11.8 | 8 |
| bench-16, 512 MB boxes, 2 GB disks | 96 | 91.0 | 5.08 | 4.06 | 48.3 | 6.13 | 21.5 | 0 |

What it shows:

- At 8 sessions, the 8 vCPU host runs about 3× faster per session than the 4 vCPU worker (CPU 3.1 s against 10.0 s; numpy 20.5 s against 43.3 s).
- The 8 vCPU host at 16 sessions matches the 4 vCPU worker at 4 to 8.
- The 4 vCPU worker refused 3 of 8 boxes with a clean 503 (`worker box capacity exhausted`): the old fixed 8-box ceiling plus the 6 GB memory capacity.
- The box count is now configurable (`AR_BOX_MAX`), as are the session limits (`AR_SESSION_*`); see [CONFIGURATION.md](CONFIGURATION.md).
- The 8 vCPU / 32 GB host ran 16 sessions with no refusals and 23 of 24 at once.
- The 16 dedicated-vCPU host is 6–8× faster per session than the 8 shared-vCPU host at 16 sessions (CPU 0.76 s against 6.0 s, numpy 6.5 s against 43 s). It **ran 96 sessions at once with none refused**, each still faster than on the 8 vCPU host at 24.
- How many boxes a worker admits is the smallest of three limits, and the refusals above were each of them:
  - `AR_BOX_MAX`;
  - memory: `AR_CAPACITY_MB`, less the host reserve, divided by the starting box size `AR_SESSION_BOX_MB`;
  - disk: free space divided by the workspace size `AR_BOX_DISK_MB` (each box's full quota is set aside).
- What stopped the larger hosts:
  - The worker held back a flat quarter of RAM, whatever `AR_CAPACITY_MB` said: 16 GB of 64. With 1 GiB boxes that capped the 64 GB host at 47 boxes and the 32 GB host at 23. Fixed: the reserve is now at most 2 GiB plus 1/16 of RAM (4 GiB of 32, 6 GiB of 64; unchanged at 8 GiB and below), which raised the cap to 56.
  - At 512 MB starting boxes (`AR_SESSION_BOX_MB=512`, new), the disk gate stopped it at 64: 64 × 4 GB workspaces fill a 266 GB free disk. With 2 GB workspaces all 96 started.
- **Sizing guide:**
  - About 2 busy sessions per vCPU before per-session times double (both hosts); 6 per vCPU still works, about 5× slower each.
  - `AR_BOX_MAX` of 2–6 × vCPU, and check the other two limits: `AR_CAPACITY_MB` ÷ `AR_SESSION_BOX_MB` and free disk ÷ `AR_BOX_DISK_MB`.
  - Leave `AR_CAPACITY_MB` at the RAM minus the reserve.

## Agent tasks: GLM 5.3 Flash against full GLM 5.3

Each task runs in its own session, all in parallel, and is graded by a hidden check run in the box after the agent replies (not by the agent's own claim):

| Task | What is checked |
| --- | --- |
| `data` | 500-row CSV and per-region revenue JSON; totals recomputed from the CSV |
| `code` | `word_freq` module plus its own tests; hidden cases including ties and empty input |
| `bugfix` | three bugs in `calc.py`; the original tests are restored and run |
| `sqlite` | load 400 orders into SQLite and answer five questions; every number recomputed from the CSV |
| `jsfix` | four bugs in an ES module, with the agent's own tests; hidden `node:assert` tests |
| `webapp` | build a to-do app, serve it, and add two items in Chromium; files checked, and the reply must name both items |

| Run | Model | Sessions at once | Passed | Median time per task (s) | Cost per task (USD) |
| --- | --- | --- | --- | --- | --- |
| live worker | glm-5.3-flash | 4 (original 4 tasks) | 4/4 | 7.9 | 0.0011 |
| bench | glm-5.3-flash | 12 | 11/12 | 7.8 | 0.0012 |
| bench | glm-5.3-flash | 24 | **24/24** | 12.4 | 0.0011 |
| bench | glm-5.3 (before the fix below) | 12 | 0/12 | 15.5 | 0.1009 |
| bench | glm-5.3 | 12 | **12/12** | 43.1 | 0.0075 |
| bench | glm-5.3 | 24 | 22/24 | 151.3 | 0.0087 |

Across the two comparable batches, Flash passed 35 of 36 and full GLM 5.3 passed 34 of 36. Both models made the same mistake once each, on the SQLite task: comparing `amount > 200` when SQLite had stored the CSV amounts as text, so every row matched and the count was 400 instead of 88. Full GLM 5.3 also once left a double hyphen in `slug`.

On these everyday tasks, **full GLM 5.3 was not more accurate than Flash, cost about 7× more per task, and was 5–12× slower**. Under parallel load the gap grows, because Vultr rate-limits the full model (see below). Flash stays the default; full GLM 5.3 is available per session (`"model": "glm-5.3"`) for work where you want the larger model.

### Harder tasks (`--suite hard`)

Six longer tasks, each with a written spec and a hidden check that covers cases the prompt only states in words. Reference solutions pass every check and the starting state fails it.

| Task | What the agent must do | What the hidden check covers |
| --- | --- | --- |
| `expr` | Expression evaluator without `eval` or `ast`: precedence, right-associative `^`, unary minus, variables, errors | `-2^2`, `2^3^2`, `2^-1`, `--3`, malformed input raising `ValueError`, division by zero, no `eval` |
| `ledger` | Fix a three-module package to match `SPEC.md`; the given tests cover only part of it | Exact cents (`0.29`), whitespace, unsorted `until`, unknown kinds, negative money formatting, `YYYY-MM` months |
| `etl` | Clean a messy CSV: three date formats, three money formats, duplicates, missing amounts | Every kept row, and the report's counts, total, top email and monthly totals |
| `sched` | Dependency scheduler: alphabetical topological order, earliest starts, finish, zero-slack tasks, cycle members | All of them against a precomputed plan |
| `kvapi` | Standard-library HTTP key-value server with persistence | Status codes, invalid JSON and keys, 30 concurrent writes, restart persistence |
| `lru` | Thread-safe LRU cache with TTL, an injected clock and stats | Eviction order, expiry, hit/miss/eviction counts, capacity 0, 8 threads |

On the 16 vCPU host, each task 3 times on each model, 36 sessions at once (after the output-size fix below):

| Task | glm-5.3-flash | glm-5.3 |
| --- | --- | --- |
| etl | 3/3 | 2/3 |
| expr | 3/3 | 3/3 |
| kvapi | 3/3 | 2/3 |
| ledger | 3/3 | 3/3 |
| lru | 3/3 | 3/3 |
| sched | 3/3 | 2/3 |
| **Total** | **18/18** | **15/18** |
| Median time per task | 14.3 s | 87.5 s |
| Cost per task | $0.0032 | $0.0394 |

- Full GLM 5.3's three misses:
  - `etl`: wrote `top_email` as an object instead of the address.
  - `sched`: listed only one chain of the zero-slack tasks.
  - `kvapi`: Vultr answered HTTP 504 mid-task and the session stopped. This was a provider error, not a wrong answer.
- At 48 hard tasks at once, Flash passed 43/48. All five misses were the same `sched` mistake: computing the critical tasks from one path instead of from slack.
- Even on harder work, **full GLM 5.3 was no more accurate than Flash here, and cost about 12× more and took about 6× longer**.

### 96 agent sessions at once

Flash on the 16 vCPU host, the everyday and the hard suite 8 times each, 48 sessions per batch:

| Batch | Passed | Median time per task (s) | Cost per task (USD) | Batch wall time (s) |
| --- | --- | --- | --- | --- |
| everyday × 8 | **48/48** | 18.9 | 0.0011 | 84 |
| hard × 8 | 43/48 | 114.2 | 0.0037 | 195 |

No session was refused, rate-limited or failed by the system.

## Soak: 12 minutes of session churn

48 workers each loop as fast as they can: create a session, upload a 1 MiB file, hash it and run a short CPU job in the box, download it and compare, then delete the session for good.

- **6,844 cycles**, about 570 a minute, with a median cycle of 4.2–6.4 s and a p95 of 6.8–9.7 s that did not drift.
- Control plane memory moved between 120 and 225 MB and the worker between 70 and 105 MB, sampled every 15 s, with no steady climb. Open file descriptors stayed under 110 (control) and 210 (worker). Twelve minutes is enough to rule out a fast leak, not a slow one.
- Afterwards no containers, workspace disks or session rows were left behind. Samples: [`resource-samples.csv`](evidence/live/benchmark-2026-11-06-16c/resource-samples.csv).
- 17 cycles (0.25%) failed, all within one second. Ubuntu's unattended upgrade re-executed systemd at that moment, so new boxes could not set their CPU limit over D-Bus. Docker and the services kept running, and the next cycles succeeded. Workers should schedule OS upgrades for quiet hours: see [`soak-transient-and-install.txt`](evidence/live/benchmark-2026-11-06-16c/soak-transient-and-install.txt).

### What the benchmark found and fixed

- **Rate limits.**
  - At 12 parallel sessions, Vultr answered HTTP 429 to full GLM 5.3 calls.
  - The gateway failed the turn at once and kept each call's worst-case reservation (about $0.10) as spend, although a 429 is not billed: 12 failed tasks showed $1.21.
  - The gateway now retries up to six times, using `Retry-After` or a 2–16 s backoff with jitter, and releases the reservation if the provider keeps refusing.
  - After the fix: 12/12 and then 22/24 passed, with no rate-limit failures. The rate-limit waits are why the 24-session run has longer task times.
- **Fresh-host install.** `scripts/install.sh --role all` on a new Ubuntu 24.04 VM found two bugs, both fixed:
  - The scripts were stored without the executable bit, so the documented `sudo scripts/install.sh` failed after a clone.
  - In one-host mode the control plane was pointed at root-only mTLS files it does not need, and crash-looped with `Permission denied`.
  - The second install passed 10/10 job checks and 9/9 box checks. The log is in [`install-fresh-host.log`](evidence/live/benchmark-2026-11-06/install-fresh-host.log).
- **Answers over 32 KB killed the session (Vultr path).**
  - The gateway refused any model answer over 32 KiB as "invalid model output length" and failed the session.
  - A turn that writes a whole module and its tests in one tool call is longer than that. 5 of the first 36 hard tasks failed this way, 3 of them after already solving the task.
  - The bound now matches the Anthropic path (400 KB, still inside the 16k-token output cap), and the error names the length and finish reason. The rerun passed 8/8, then 33/36.
- **Large workers could not use their memory.** See [Box throughput](#box-throughput-no-model): a flat 25% memory reserve capped a 64 GB worker at 47 boxes. The reserve is now bounded, and `AR_SESSION_BOX_MB` sets the starting box size.
- **Not fixed: provider 5xx.** A Vultr HTTP 504 ends the turn and keeps that call's reservation as spend, because the provider may have billed it. The user can resend. Retrying it safely needs a second reservation per attempt, and is left for later.
- **Workspace download and fork on real gVisor boxes:**
  - A 2 MB workspace archived and downloaded intact.
  - A fork onto full GLM 5.3 got byte-identical files and the earlier conversation, read the file it had written before, and answered on the new model.
  - `/v1/usage` split the spend by model.
  - Evidence: [`archive-fork-live.json`](evidence/live/benchmark-2026-11-06/archive-fork-live.json).

Total model spend for all benchmark and diagnosis runs: about $3.70. The first round cost $1.60, of which $1.21 was the rate-limit charges described above; the 16 vCPU round cost $2.07. The two temporary VMs cost about $0.26 and $0.63.

## Run it yourself

```sh
export AR_HOST=https://your-host AR_KEY=ar_...
python3 qa/bench/bench.py infra --levels 1,4,8 --label "my host" --out infra.json
python3 qa/bench/bench.py agent --model glm-5.3-flash --repeat 2 --label "flash" --out flash.json
python3 qa/bench/report.py agent flash.json
python3 qa/bench/bench.py agent --suite hard --model glm-5.3-flash,glm-5.3 --repeat 3 --out hard.json
python3 qa/bench/report.py matrix hard.json
python3 qa/bench/bench.py soak --concurrency 16 --minutes 5 --out soak.json
```

Every session the benchmark creates is deleted for good afterwards. `AR_BENCH_KEEP=1` keeps them for inspection.
