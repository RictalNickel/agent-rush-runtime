# Live deployment

Deployed 2026-11-05 (UTC) on Vultr, Chicago (`ord`). It was approved to stay up through judging and is deleted on the owner's instruction.

**Public URL:** https://144-202-59-108.sslip.io (Let's Encrypt certificate via Caddy, HTTP redirects to HTTPS)

## Resources

| Resource | Plan | Price | Role |
| --- | --- | --- | --- |
| `agent-rush-control` | vc2-2c-4gb (2 vCPU, 4 GB) | $0.027/h | Caddy HTTPS gateway, Rust control plane on loopback :8080, SQLite ledger |
| `agent-rush-worker16` | voc-g-16c-64gb-320s-amd (16 dedicated vCPU, 64 GB, 320 GB) | $0.658/h | **Live worker since 2026-11-06 20:19 UTC.** Private execution node: Docker + gVisor `runsc`, nginx mTLS ingress on the VPC address. `AR_BOX_MAX=88`, `AR_BOX_DISK_MB=3072` |
| `agent-rush-worker` | vc2-4c-8gb (4 vCPU, 8 GB) | $0.055/h | The original worker, kept idle as a fallback |
| VPC `agent-rush-private` | 10.42.0.0/24 | free | control 10.42.0.3, worker16 10.42.0.5, original worker 10.42.0.4 |
| Two firewall groups, one SSH key | | free | SSH from the operator's IP only; 80/443 public on control |

Total: **$0.74/hour**, about $18 per day (it was $0.082/hour before the large worker). Resource IDs and the exact delete calls are kept in the operator's private ledger outside Git, not in this repository.

## Large worker (2026-11-06)

The public instance moved to a 16 vCPU / 64 GB worker so a large demo works: many people, or one script, running dozens of sessions at once.

- **How it was added:** `scripts/install.sh --role worker` with the tested binary (10/10 host checks and 9/9 box checks on the first run), then the same mTLS front end as the original worker. It reuses the worker certificate, since that names `worker.agent-rush.internal`, not an IP, and the original worker's token. `ufw` lets only control reach port 8443.
- **Switch:** control's `/etc/hosts` maps `worker.agent-rush.internal` to 10.42.0.5. Rolling back means pointing it at 10.42.0.4 and restarting control; backups are `*.bak-20261106h`.
- **Moved data:** the three demo sessions' workspace disks were copied over and checked by hash.
- **Control settings for scale:** `AR_SESSION_MAX_OPEN=150`, `AR_SESSION_MAX_STREAMS=64`, `AR_SESSION_MAX_LONG_REQUESTS=150`, `AR_SESSION_MAX_BOX_CALLS=200`, `AR_SESSION_BOX_MB=512`.
- **Measured through the public URL**, so through Caddy and the 2 vCPU control plane:
  - 64 box sessions at once, none refused.
  - 72 agent sessions at once (48 everyday tasks and 24 hard ones): 69 of 71 passed in the first run. One run was lost to a TLS timeout on the tester's own connection, and Caddy logged no error.
  - Control load stayed under 0.2 and its memory under 30 MB.
  - Vultr rate-limited Flash once at that load, so the gateway now retries 429s for about 2.5 minutes instead of 45 s.
  - Details: [BENCHMARKS.md](BENCHMARKS.md#live-large-demo).

## Boxoro release (2026-11-07)

Binary `21017925` went to control, the 16 vCPU worker and the idle fallback worker. It renames the product to Boxoro in the web UI, the MCP server (`serverInfo.name` is now `boxoro`) and the session agent's prompt, so the agent introduces itself as Boxoro instead of guessing. 238 tests pass and clippy is clean. Backups of the previous binary (`d6325823`) are `agent-rush.bak-20261107` on each VM.

Later that day `8f9d5b65` replaced it on all three VMs: a box that stops mid-command (for example at its memory limit) now answers 503 instead of 502, and a busy box 429. 239 tests pass. Checked live: a crashed box answered 503, the next call ran, and `qa/containment_demo.py` blocked 8 of 8. Backups are `agent-rush.bak-20261107c`.

After the deploy, the three demo sessions were intact, a real agent turn answered as Boxoro on GLM 5.3 Flash, and `qa/containment_demo.py` blocked 8 of 8 escape attempts. Evidence for the plug-in and containment runs: [plugin-and-containment-2026-11-07/](evidence/live/plugin-and-containment-2026-11-07/).

## Worker host

- Ubuntu 24.04, kernel 6.8, cgroup v2 with the systemd driver. Docker 29.1.3 reports `MemoryLimit`, `CpuCfsQuota` and `PidsLimit` true.
- gVisor `runsc` release-20261102.0, registered as a non-default runtime **without** `--ignore-cgroups`.
- Sandbox image pinned by digest: `rust:1.90.0-bookworm@sha256:3914072c…`.
- `agent-rush-worker.service` runs as the unprivileged `agent-rush` user (docker group) bound to 127.0.0.1:8090.
- `agent-rush-requalify.timer` reruns the ten containment checks every 45 minutes. A passing report replaces the old one, and the running worker adopts it within a minute without a restart, so in-flight runs are not interrupted. If checks fail, the worker stops admitting jobs when the old report expires two hours after it was made.

Install scripts: `deploy/bootstrap-worker.sh`, `deploy/install-worker.sh`, `deploy/install-control.sh`.

## Verification (2026-11-05)

| Check | Result |
| --- | --- |
| Rust suite on the worker | 96 passed, 7 real-host tests ignored. Clippy `-D warnings` and rustfmt clean |
| Real qualification on the worker | 9/9 at first deploy (`evidence/live/worker-qualification-2026-11-05.json`); 10/10 since the `scaling` check was added (see Live resource scaling) |
| CPU cap | Samples held at about 25% for a 0.25 vCPU request. Host cgroup `cpu.max` = `25000 100000` |
| Memory cap | Over-cap allocation was killed (the handoff's 384 MiB escape no longer reproduces with enforced cgroups) |
| Leftover containers after qualification | 0 |
| Public ports | Control: 22 (operator IP), 80, 443. Worker: 22 (operator IP) only. Worker 8090/8443/2375/2376 and control 8080 closed from the internet |
| Worker mTLS ingress, from control over the VPC | No client certificate: 400. Certificate without token: 401. Wrong token: 401. Certificate and token: 200. Raw :8090 over the VPC: unreachable |
| HTTPS headers | HSTS, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` |
| End-to-end job | See below |

### End-to-end job `2f6bd23b`

Task: compute the first 200 primes by trial division and a sieve, assert they agree, and write the count, sum and largest to `result.json`. Submitted through the public HTTPS API by `qa/live_job.py`.

- Result: count 200, sum 111587, largest 1223. Artifact `result.json` sha256 `5a58a033…e3d749`. Exit 0, all four worker checks passed, cleanup verified.
- Flow:
  1. Orchestrator delegated to the coder.
  2. The coder ran one sandbox execution (871 ms in gVisor) and reported back.
  3. The reviewer rejected the first answer for a wording inaccuracy.
  4. The orchestrator corrected it, and the reviewer approved.
- Seven model calls, 2 min 18 s end to end, **$0.196** accounted (Opus $0.18, Haiku $0.011).
- Raw evidence: `evidence/live/2f6bd23b-6d07-48ff-a754-66c0a412731f-{job,evidence,contexts}.json`.

### Concurrent load (4 jobs submitted at once)

`qa/load_test.py` submitted four small tasks (memory leases 512/512/768/512 MiB) in parallel through the public API while `qa/worker_sampler.sh` sampled the worker once per second. Raw data: `evidence/live/load/`.

| Job | Queued | Total | Sandbox run | Review | Model calls | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 466ec99d | 0.8 s | 52.8 s | 901 ms | approved | 5 | $0.062 |
| 371334ba | 0.2 s | 98.5 s | 777 ms | approved | 5 | $0.091 |
| c717cecf | 53.3 s | 154.5 s | 965 ms | approved | 5 | $0.100 |
| f3512e2b | 99.0 s | 211.5 s | 962 ms | approved | 5 | $0.109 |

- All four succeeded with cleanup verified. Wall time was 217 s and cost $0.36 in total.
- The two-slot admission limit held: two jobs started at once, and the other two queued until a slot freed.
- Each sandbox's cgroup matched its lease (`memory.max` 512 or 768 MiB, `cpu.max` 1 vCPU), with 32–45 PIDs.
- Peak sandbox memory was 75 MiB. The worker kept at least 7.1 GB available, and `load1` peaked at 0.58.
- Limitation: sandbox runs lasted under 1 s while model turns took tens of seconds, so no two sandboxes overlapped in this test. The qualification's `concurrency` check covers overlapping sandboxes.

The same four tasks were rerun on Vultr Serverless Inference after the switch (raw data: `evidence/live/load-vultr/`):

| Job | Queued | Total | Sandbox run | Review | Model calls | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| be2c59ca | 0.3 s | 65.1 s | 965 ms | approved | 5 | $0.0045 |
| af0fe974 | 0.8 s | 76.2 s | 812 ms | approved | 5 | $0.0040 |
| 985bad2b | 65.6 s | 165.1 s | 1029 ms | approved | 7 | $0.0099 |
| 43a2e00b | 76.4 s | 121.0 s | 810 ms | approved | 5 | $0.0037 |

All four succeeded with cleanup verified, under the same two-slot admission. Wall time was 170 s, against 217 s on Claude, and the total cost was $0.022 against $0.36.

### Containment

**Deterministic probe** (`examples/containment_probe.rs`, run with `agent-rush sandbox --memory-mb 256` on the worker; receipt in `evidence/live/containment/probe-receipt.json`):

| Probe | Outcome |
| --- | --- |
| TCP to 1.1.1.1:443 and to cloud metadata 169.254.169.254:80 | blocked: network unreachable |
| DNS lookup | blocked: name resolution fails |
| Read `/etc/shadow`, `/root/.ssh/id_ed25519` | blocked: permission denied |
| Read `/etc/agent-rush/{control,worker}.env`, connect to `/var/run/docker.sock` | blocked: not present in the sandbox |
| Credential-like environment variables | none present |
| Write to `/usr`, `/etc` | blocked: permission denied |
| Write to `/work` | allowed (bounded scratch, by design) |
| Allocate 2 GiB with a 256 MiB cap | **the whole sandbox was killed** (exit 128), not just the allocating child; cleanup verified |
| Create 1000 threads | allowed; see the finding below |

Finding: `--pids-limit 64` stops process creation (the qualification run was denied before reaching 64 children), but gVisor does not count threads against it. Thread growth is bounded by the memory and CPU caps instead, not by a thread cap.

**In-product, job `76a60e82`:** an ordinary task (statistics over 300 million hashes) planned by the orchestrator as a bounded-memory streaming solution.
- Three of the coder's sandbox runs exceeded the 60-second time cap. Each was killed with `reason: timeout` and `cleanup_state: verified`, and the coder received the evidence and retried.
- The 15-minute job lifetime then ended the job as `failed`, with the lease released and no containers left. Cost was $0.19.
- This is a real runaway-compute containment event, but not a successful answer.

**Model refusals:** two earlier attempts that asked the agents to deliberately probe or exceed the sandbox were declined by Opus 5.5's safety classifier (`stop_reason: refusal`, category `cyber`) on the orchestrator's first call. Before a fix in this branch, a refusal kept its worst-case reservation (about $2.02, still counted in the live ledger). Refusals and structured API errors now settle at their actual cost.

### Vultr Serverless Inference path (job `2fd5b26f`)

GLM 5.3 Flash pricing comes from Vultr's own model catalog (`GET https://api.vultrinference.com/v1/models`): **$0.10 per million input tokens, $0.35 per million output tokens**.

A temporary second control instance (loopback only, its own data directory, `AR_INFERENCE_PROVIDER=vultr`) ran the same primes task as job `2f6bd23b`. All four roles used `glm-5.3-flash`.

| | Claude team (`2f6bd23b`) | Vultr GLM team (`2fd5b26f`) |
| --- | --- | --- |
| Result | 200 primes, sum 111587, largest 1223 | same |
| Review | approved after one rejection | approved |
| Model calls | 7 | 5 |
| End to end | 138 s | **33 s** |
| Model cost | $0.196 | **$0.003** |

Every model call in `2fd5b26f` went to Vultr Serverless Inference. Raw evidence: `evidence/live/vultr/`.

The first Vultr attempt (`826c79d5`) failed, and that failure is kept as evidence. Without a reasoning effort, GLM 5.3 Flash can spend its whole output budget on hidden reasoning and return empty content. The gateway now sends each role's reasoning effort (the same table as the Claude roles) and allows 16k output tokens. A reasoning-only reply now produces a clear error. Measured per call: 3–6 s at medium, high and xhigh effort, against 10–26 s with no effort set.

### Public instance on Vultr (job `7d466fa9`)

On 2026-11-05 the owner switched the public instance to `AR_INFERENCE_PROVIDER=vultr` with GLM 5.3 Flash at $0.10/$0.35 per million tokens. `/api/status` now lists each role's model and reasoning effort.

The first public job on a harder task (`510a429a`, kept in `evidence/live/vultr-public-failed-uncapped/`) failed. The coder at xhigh used all 16,384 output tokens on reasoning and returned no answer. A direct probe reproduced it, and showed that GLM honours `reasoning.max_tokens`: with the cap at 8,192, the same prompt used 8,202 reasoning tokens and then returned valid JSON. Two changes followed:
- Every Vultr request now sends `reasoning: {effort, max_tokens: 8192}`.
- The gateway settles a call's actual usage before decoding the answer, as the Claude path does. A failed call used to keep its worst-case reservation.

The same task then succeeded through the public URL:

| | |
| --- | --- |
| Task | 1,000 seeded sales rows; revenue by quarter and region |
| Flow | orchestrator (medium) → coder (xhigh) → orchestrator → reviewer (high), approved |
| Sandbox | exit 0 in 966 ms; checks passed; cleanup verified |
| Result | grand total 5,013,559.38; quarter and region totals both reconcile |
| End to end | 58 s |
| Model cost | $0.0058 over 5 calls (the coder's first call was 12,234 tokens) |

Raw evidence: `evidence/live/vultr-public/`.

### Live resource scaling

Users no longer pick memory or time. The orchestrator sets a ceiling with `set_limits`, and every run starts at 256 MiB and 0.5 vCPU. The worker polls the sandbox's cgroup every 100 ms and raises its limits through `docker update` when use crosses 70% of memory or 85% of CPU, up to the ceiling.

Before relying on it, I checked directly on the worker that a running gVisor sandbox can be resized. A container started at 256 MiB and raised to 1 GiB mid-run could allocate 300 MiB.

Requalified on the worker, 10 of 10 checks, through the real requalify service. The new `scaling` check started at 256 MiB with a 512 MiB ceiling:
- The worker raised the limit when the program reached 202 MiB.
- The program held 480 MiB.
- It was killed at 512 MiB (peak exactly 512) and never printed "allocation escaped".

The first service run of that check failed for a real reason. gVisor ends the whole sandbox on a memory kill, and Docker removed the cgroup before the final read. The scaler now keeps the last values it polled and falls back to Docker's OOMKilled record.

Two public jobs on Vultr inference, raw evidence in `evidence/live/scaling*/`:

| Job | Orchestrator ceiling | Growth | Peak | Outcome |
| --- | --- | --- | --- | --- |
| `a4ccbba6` (sort 20M u32) | 512 MiB, 60 s | CPU 0.5 → 1 vCPU; memory stayed at 256 MiB | 94 MiB | succeeded, approved, $0.0065 |
| `37853f20` (30M u64, two copies) | 768 MiB, 120 s | CPU 0.5 → 1; memory 256 → 512 → 768 MiB | 478 MiB | succeeded, approved, $0.0049 |

### Overlapping sandboxes

`qa/overlap_test.py` sent three 4-second CPU-bound runs to the worker API at the same moment over mTLS. Raw data: `evidence/live/load/overlap-*`.
- **Two ran together for 4.7 s**, each held at about one core. The cgroups read `memory.max` 512 MiB, `cpu.max 100000 100000` and 42 PIDs each.
- **The third was refused with HTTP 429.** The worker admits at most two concurrent runs, so excess work is pushed back to the controller's queue.
- All runs completed with cleanup verified, and no containers were left.

### Performance

`qa/api_bench.py`, 400 requests at concurrency 16, measured on the control VM loopback:

| Path | Before | After |
| --- | --- | --- |
| `/api/status` | 31 req/s, p50 494 ms | **460 req/s, p50 8 ms** |
| `/`, `/assets/app.js` | ~1,500 req/s, p50 9 ms | unchanged |

Cause: each anonymous status request made the worker run `docker info` over mTLS, and the scheduler also polled the worker twice a second while idle. Three changes fixed it:
- The worker caches runtime health for 5 s.
- The controller caches reachability for 5 s, with one refresh in flight at a time.
- The scheduler only contacts the worker when jobs are queued.

Worker health calls fell to 14 over the benchmark plus 20 s of idle time. Over the public internet, all paths run at about 64 ms median from the operator's location, which is network round-trip time.

Most of a job's time is model time. Sandbox execution took 0.8–0.9 s, or about 1.3 s including container start and verified cleanup.

### Browser and dependency checks

- The Playwright suite passed in the official `mcr.microsoft.com/playwright:v1.64.1-noble` container on the worker. It covers desktop login, a real queued job, evidence export and cancel, plus the mobile layout. A new assertion checks the live provider and worker status in the sidebar.
- `cargo audit` checked 228 crates against 1,403 RustSec advisories and found no vulnerabilities.

## Not yet done

- The demo video (`presentation/video/out/demo.mp4`) is built from the deck, replays of real live runs and a real-time screen recording of a live web chat session, with captions. It has no voice-over.
- With the Anthropic provider (development only; the public instance uses Vultr), security-flavoured tasks can be declined by the Opus classifier. Anthropic's server-side `fallbacks` option would reroute those calls; it is off because it substitutes models outside the configured role table.
- The private mTLS certificates were renewed on 2026-11-06. The worker server and control client certificates now expire 2026-12-16 03:55 UTC, and the private CA 2027-01-05 03:55 UTC. The CA certificate was reissued with the same key and subject, so certificates issued before the renewal still verify against it.

## Teardown

Delete both instances first, then the VPC, firewall groups and SSH key, using the IDs and delete calls in the private ledger (`~/.config/agent-rush/vultr-ledger.json` on the operator machine). Then confirm each resource returns 404. Powering a VM off does not stop billing.
