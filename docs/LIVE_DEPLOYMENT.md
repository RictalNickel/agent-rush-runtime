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

## Not yet done

- The live deployment uses the `anthropic` provider. The Blast Radius Zero rules require Vultr Serverless Inference for every agent call. Switching needs verified GLM 5.3 Flash pricing (`AR_INPUT_USD_PER_MILLION`, `AR_OUTPUT_USD_PER_MILLION`, `AR_PRICING_VERIFIED=true`) and `AR_INFERENCE_PROVIDER=vultr`.
- No measurements yet under concurrent load, no recorded containment demo video, no slide deck.
- The private mTLS certificates expire 2026-11-11 04:31 UTC and must be regenerated if the demo runs past that.

## Teardown

Delete both instances first, then the VPC, firewall groups and SSH key, using the IDs and delete calls in the private ledger (`~/.config/agent-rush/vultr-ledger.json` on the operator machine). Then confirm each resource returns 404. Powering a VM off does not stop billing.
