# Benchmarks

Measured on 2026-11-06 with [`qa/bench/bench.py`](../qa/bench/bench.py). Raw results are in [`docs/evidence/live/benchmark-2026-11-06/`](evidence/live/benchmark-2026-11-06/). Print the tables yourself with `qa/bench/report.py infra|agent <files>`.

Hosts:

| Name | Vultr plan | Use |
| --- | --- | --- |
| live worker | `vc2-4c-8gb` (4 vCPU, 8 GB), `AR_BOX_MAX=8` | The public instance's worker, with its control plane on a separate `vc2-2c-4gb` |
| bench | `vc2-8c-32gb` (8 vCPU, 32 GB), `AR_BOX_MAX=24`, `AR_CAPACITY_MB=28672` | A temporary VM installed from scratch with `scripts/install.sh --role all` (control and worker on one host), deleted after the run |

## Box throughput (no model)

N sessions start at once. Each one runs the same commands through `POST /v1/sessions/{id}/exec`:

- `cold_start`: `true` (creates the gVisor box);
- `cpu_python`: 2 million SHA-256 rounds;
- `numpy_matmul`: four 1500×1500 matrix products;
- `disk_write_256mb`: fsync'd write to the quota-limited `/workspace`;
- `npm_install`: lodash and express from the public registry.

Times are medians in seconds. Boxes start small (0.5 vCPU, 256 MiB) and grow under load, so single-session numbers include that ramp.

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

What it shows:

- At 8 sessions, the 8 vCPU host runs about 3× faster per session than the 4 vCPU worker (CPU 3.1 s against 10.0 s; numpy 20.5 s against 43.3 s).
- The 8 vCPU host at 16 sessions matches the 4 vCPU worker at 4 to 8.
- The 4 vCPU worker refused 3 of 8 boxes with a clean 503 (`worker box capacity exhausted`): the old fixed 8-box ceiling plus the 6 GB memory capacity.
- The box count is now configurable (`AR_BOX_MAX`), as are the session limits (`AR_SESSION_*`); see [CONFIGURATION.md](CONFIGURATION.md).
- The 8 vCPU / 32 GB host ran 16 sessions with no refusals and 23 of 24 at once.
- **Sizing guide:**
  - About 2 busy sessions per vCPU before per-session times double.
  - `AR_BOX_MAX` of about 2 × vCPU.
  - `AR_CAPACITY_MB` at the RAM minus 3–4 GB.

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
- **Workspace download and fork on real gVisor boxes:**
  - A 2 MB workspace archived and downloaded intact.
  - A fork onto full GLM 5.3 got byte-identical files and the earlier conversation, read the file it had written before, and answered on the new model.
  - `/v1/usage` split the spend by model.
  - Evidence: [`archive-fork-live.json`](evidence/live/benchmark-2026-11-06/archive-fork-live.json).

Total model spend for all benchmark and diagnosis runs: about $1.60, of which $1.21 was the rate-limit charges described above.

## Run it yourself

```sh
export AR_HOST=https://your-host AR_KEY=ar_...
python3 qa/bench/bench.py infra --levels 1,4,8 --label "my host" --out infra.json
python3 qa/bench/bench.py agent --model glm-5.3-flash --repeat 2 --label "flash" --out flash.json
python3 qa/bench/report.py agent flash.json
```

Every session the benchmark creates is deleted for good afterwards. `AR_BENCH_KEEP=1` keeps them for inspection.
