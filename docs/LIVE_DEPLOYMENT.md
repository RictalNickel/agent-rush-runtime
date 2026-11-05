# Live deployment

Deployed 2026-11-05 (UTC) on Vultr, Chicago (`ord`). It was approved to stay up through judging and is deleted on the owner's instruction.

**Public URL:** https://144-202-59-108.sslip.io (Let's Encrypt certificate via Caddy, HTTP redirects to HTTPS)

## Resources

| Resource | Plan | Price | Role |
| --- | --- | --- | --- |
| `agent-rush-control` | vc2-2c-4gb (2 vCPU, 4 GB) | $0.027/h | Caddy HTTPS gateway, Rust control plane on loopback :8080, SQLite ledger |
| `agent-rush-worker` | vc2-4c-8gb (4 vCPU, 8 GB) | $0.055/h | Private execution node: Docker + gVisor `runsc`, nginx mTLS ingress on the VPC address |
| VPC `agent-rush-private` | 10.42.0.0/24 | free | control 10.42.0.3, worker 10.42.0.4 |
| Two firewall groups, one SSH key | | free | SSH from the operator's IP only; 80/443 public on control |

Total: **$0.082/hour**, about $2 per day. Resource IDs and the exact delete calls are kept in the operator's private ledger outside Git, not in this repository.

## Worker host

- Ubuntu 24.04, kernel 6.8, cgroup v2 with the systemd driver. Docker 29.1.3 reports `MemoryLimit`, `CpuCfsQuota` and `PidsLimit` true.
- gVisor `runsc` release-20261102.0, registered as a non-default runtime **without** `--ignore-cgroups`.
- Sandbox image pinned by digest: `rust:1.90.0-bookworm@sha256:3914072c…`.
- `agent-rush-worker.service` runs as the unprivileged `agent-rush` user (docker group) bound to 127.0.0.1:8090.
- `agent-rush-requalify.timer` reruns the nine containment checks every 90 minutes. It installs the new report and restarts the worker only if every check passes; otherwise the worker stops admitting jobs when the old report expires.

Install scripts: `deploy/bootstrap-worker.sh`, `deploy/install-worker.sh`, `deploy/install-control.sh`.

## Verification (2026-11-05)

| Check | Result |
| --- | --- |
| Rust suite on the worker | 96 passed, 7 real-host tests ignored. Clippy `-D warnings` and rustfmt clean |
| Real qualification on the worker | 9/9 passed: containment, timeout, output, memory, scratch, pids, cpu, cancel, concurrency (`evidence/live/worker-qualification-2026-11-05.json`) |
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

## Not yet done

- The live deployment uses the `anthropic` provider. The Blast Radius Zero rules require Vultr Serverless Inference for every agent call. Switching needs verified GLM 5.3 Flash pricing (`AR_INPUT_USD_PER_MILLION`, `AR_OUTPUT_USD_PER_MILLION`, `AR_PRICING_VERIFIED=true`) and `AR_INFERENCE_PROVIDER=vultr`.
- No recorded demo video and no slide deck yet.
- Security-flavoured tasks can be declined by the Opus classifier. Anthropic's server-side `fallbacks` option would reroute those calls to another model; it is off because it substitutes models outside the configured role table.
- The private mTLS certificates expire 2026-11-11 04:31 UTC and must be regenerated if the demo runs past that.

## Teardown

Delete both instances first, then the VPC, firewall groups and SSH key, using the IDs and delete calls in the private ledger (`~/.config/agent-rush/vultr-ledger.json` on the operator machine). Then confirm each resource returns 404. Powering a VM off does not stop billing.
