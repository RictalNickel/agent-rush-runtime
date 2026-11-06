# Authority and evidence

This page covers the control plane, the one-shot job path and the session boxes. The session boxes have their own section at the end.

## One-shot jobs

The control API owns authentication, CSRF, task identity, idempotency, memory reservations and the event ledger. The model may choose only a validated tool object from its role's allowlist (see AGENT_TEAM.md); only the coder role may request a sandbox run. Unknown fields and tool names fail closed. It never receives Docker authority or infrastructure credentials.

The private node manager owns Docker. It launches a digest-pinned runsc container with non-root identity, no network, read-only root, no host mounts, all capabilities dropped, no-new-privileges, bounded PID/CPU/memory/output/files/scratch/time. Its Docker client uses an empty config and cleared environment so Docker cannot inject controller proxy credentials into the task. A fixed system tar binary exports bounded artifact bytes; paths are inspected, never extracted on the host.

Memory admission is transactional. Sandbox resources grow live: each run starts at 256 MiB and 0.5 vCPU, and the node manager raises its cgroup limits through `docker update` when measured use crosses 70% of memory or 85% of CPU. It never goes above the run's ceiling and never lowers a limit mid-run. The orchestrator agent proposes the ceiling with `set_limits`; trusted code clamps it to the job's admitted lease, the node's capacity and fixed maxima (1 GiB, 1 vCPU for agent runs, 120 s). Both the controller and the worker reserve the ceiling, not the starting size, so concurrent runs can never grow past the host's capacity. A memory kill is detected from cgroup `memory.events`, or from Docker's OOMKilled record when the cgroup is already gone, and reported as `memory_limit`; the next attempt then starts at the ceiling. The `scaling` qualification check proves on the real host that a run grows past its start and still dies at its ceiling. Failed or uncertain cleanup retains the lease. Unknown/late Docker creation must not be reported as verified cleanup. Worker and controller recovery reconcile known active task containers. The trusted deployment runner and node manager still need dedicated-host qualification against crashes and hostile concurrency.

`doctor` and every execution check actual Docker capabilities and reject missing memory/CPU/PID controllers, a missing cgroup driver, or runsc `--ignore-cgroups`. These are necessary checks, not a complete security qualification. Run the real boundary tests on the intended worker before public use.

The managed development host failed a real memory allocation test despite accepting Docker memory flags. Accordingly it cannot serve as a qualified execution worker. No weaker isolation fallback was added.

Session cookies are opaque random values, stored as hashes server-side, HttpOnly, SameSite=Strict and Secure by default. Mutations require the session's independent CSRF token. An API key cannot create or revoke keys or register webhooks, which need the operator login, so a leaked key cannot mint more keys. Failed logins are rate limited per client address (10 a minute; an IPv6 client by its /64) under a global ceiling of 100 a minute, and successful logins do not count. Behind Caddy the address is the last `X-Forwarded-For` entry, which Caddy writes; a client connecting directly is counted by its socket address, whatever it sends. Evidence and artifacts are owner scoped. Generated HTML is downloaded as an attachment, never rendered on the authenticated origin. CSP disallows external scripts, framing and inline scripts. TLS and private worker ingress remain deployment responsibilities.

Known limitations: one trusted operator/workspace; no hostile multi-tenant security claim; no independent security audit; structural JSON verification is not a proof of arbitrary program semantics; Docker samples are not peak-memory attestation; cloud budget estimates do not enforce a Vultr account-level hard spending cap. Do not expose this build to untrusted public task submission before deployment qualification.

Cancellation is idempotent. Event history retains at most 256 records and exports an explicit dropped_events count. Scheduler admission reads oldest queued jobs independently from the newest UI history page. Worker drain closes admission before cancelling active work. Final output/capture flags are checked after readers finish, so a fast zero exit cannot override an output violation. The readiness endpoint and scheduler share the same configured mTLS client.

If container creation is uncertain, cancellation cannot release its reservation from an instantaneous empty inventory. Unfinished runs on node restart are conservatively marked uncertain and stop worker startup for operator reconciliation. Before clearing such a journal record, the operator must fence outstanding daemon creation, confirm all named tasks are absent, and record the reconciliation; deleting the record merely to start the worker is not a verification method.

## Session boxes

### Threat model

What must be protected:

- Credentials: the operator password, API keys, the worker token, the Vultr and Anthropic inference keys, and MCP server headers.
- Other owners' sessions, files and skills.
- The worker's Docker authority, its private network and its control listener.

What is treated as hostile: the model, when it acts on instructions from web pages, fetched files, tool output or anything else it reads (prompt injection), and any code it writes and runs. Another owner trying to reach a session is also in scope.

What the design promises:

- A box cannot read credentials or reach the worker's control listener, the private network, cloud metadata or mail ports.
- A box cannot reach any service on the worker, SSH included, and cannot use more disk than its workspace size limit.
- The model cannot raise its own limits or change the egress rules or the box limits.
- One owner cannot read another owner's session.

What is not claimed:

- Anything a box is allowed to send to the public internet. The egress filter allows public destinations, including DNS.
- Protection against flaws in gVisor or Docker beyond what the nine box checks exercise.
- Multi-tenant isolation, or the correctness of anything the model produces.

### Egress policy

The rules come from `deploy/box-network.sh`. They apply to the `ar-box` subnet (172.30.0.0/24) in the `DOCKER-USER` chain, and each rule is tagged `ar-box-egress`. An earlier dump of the applied rules is in [egress-policy.txt](evidence/live/box-qualification/egress-policy.txt).

| Traffic from a box | Result |
| --- | --- |
| 0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16 (cloud metadata), 172.16.0.0/12, 192.0.0.0/24, 192.168.0.0/16, 198.18.0.0/15, 224.0.0.0/4, 240.0.0.0/4 | Rejected |
| The host's own addresses, read from `ip addr`, such as its public address, `172.17.0.1` (docker0) and the box gateway `172.30.0.1` | Rejected |
| TCP ports 25, 465 and 587 (SMTP) | Rejected |
| New connections above 50 per second per box, with a burst of 100 | Dropped |
| Everything else, including the public internet and DNS through the public resolvers 1.1.1.1 and 8.8.8.8 | Allowed |

Other network settings: boxes cannot reach each other (`enable_icc=false`), and IPv6 is disabled on `ar-box`.

Traffic from a box to the host's own addresses is delivered locally, so `DOCKER-USER` never sees it. A host input guard in the `INPUT` chain refuses all of it, including SSH and the worker's listeners. The root unit `agent-rush-box-network.service` installs the guard at every boot, and the worker starts no box without the marker that proves it is in place.

### Box checks

`agent-rush qualify-box` starts a real box, runs eight probes in it, destroys it, and then checks on the host that the box's container and workspace are gone. The report has nine checks. The worker starts boxes, and restarts parked ones, only while the report is less than two hours old (`BOX_REPORT_TTL_SECONDS`, 7200), was made for this host (a digest of its machine ID and boot ID), this image and this box network, and passes all nine checks. The guard marker for this boot must also be present. A run on a scratch box network passed all nine: [box-qualification-v2](evidence/live/box-qualification-v2/). The earlier eight-check report, [box-qualification.json](evidence/live/box-qualification/box-qualification.json), predates the host guard and the disk quota, and the worker no longer accepts it.

| Check | What it establishes |
| --- | --- |
| `metadata_blocked` | Cloud metadata at 169.254.169.254 on ports 80 and 443 is unreachable |
| `private_blocked` | Private, CGNAT and host-gateway addresses are unreachable. The same targets were reachable from the host, so the refusal is the filter's doing |
| `host_services_blocked` | Each host port the probe covers, on every host address (SSH, DNS, web, Docker's API, Postgres, Redis, node-exporter and the worker's listeners), is refused by the host input guard. A silent drop fails the check, and so does a missing guard marker |
| `smtp_blocked` | SMTP ports 587, 465 and 25 on Gmail's servers are refused, while the host reaches the same ports |
| `public_https_allowed` | `https://example.com` answers 200 from inside the box |
| `no_credentials` | No secret-like environment variable names, no secret-like file paths and no host sockets are visible. The worker token's value is not in the environment. The mounts are recorded |
| `non_root_readonly` | The box runs as uid 1000 with no effective capabilities, `no_new_privs` set, no sudo, and a read-only root. `/workspace` is writable |
| `disk_quota_enforced` | Writing past the workspace size limit fails with ENOSPC, and the space comes back once the file is removed. Tested on a 512 MiB workspace |
| `volume_destroyed` | After destroy, the box's container and its workspace are gone (checked on the host) |

### What the agent can and cannot edit

| The agent can | The agent cannot |
| --- | --- |
| Set its system prompt addendum (up to 8 KiB) | Change the turn, step, time or spend limits |
| Disable built-in tools, except `reply` | Change the box memory or CPU ceilings, or the egress rules |
| Define up to 20 custom tools, each a bash template that runs in the box | Read or change credentials, the worker token or MCP headers |
| Use the MCP servers the operator listed, within each server's `allowed_tools` | Add an MCP server, or call one the operator did not list |
| Set reasoning effort (`low`, `medium`, `high`) | Give a custom tool a built-in name, or disable `reply` |
| Save and delete its own skills | Change the inference provider or model |
| Revert its configuration to an earlier version, which is saved as a new version | Change its configuration without a version record. Every change is saved as a version and a `config_change` event |

### Known limits

- **Parked workspaces stay on the worker.** An idle box's files are kept until its session closes or seven days pass with no call (`AR_BOX_RETENTION_SECONDS`). Anything a box holds is at rest on the worker for that long. `/tmp` and `/home/agent` are tmpfs mounts capped at 1 GiB each.
- **Lost workspaces start empty.** If a workspace is gone (deleted after retention, or lost), the next message gets a new, empty box, and the session records an error event that says so.
- **The guard is checked periodically, not continuously.** The root unit reapplies the host input guard at each boot, and the requalification timer reruns the script and the box checks every 45 minutes. A firewall change that opens box traffic to the host is not seen until the next run. The run without the guard found five host ports open from a box, SSH among them ([box-qualification-v2](evidence/live/box-qualification-v2/)). After any firewall change, check that the INPUT chain still starts with the guard (`iptables -S INPUT`, looking for `ar-box-egress`).
- **Prompt injection can mislead the agent but cannot reach secrets.** A web page or file can steer the agent inside its box. It cannot reach credentials or private networks. It can still send anything it can read to a public destination, including through DNS, and it can call the MCP servers the operator enabled. Keep secrets out of boxes and MCP servers you do not trust out of the agent's configuration.
- **Fixed session limits.** The turn and session caps (30 minutes, $0.50 per turn, $5 per session) and the per-owner caps (20 open sessions, 8 live streams, 8 long requests) are constants in code, not deployment settings.
- **Qualification is a fixed set of probes on one host.** The nine box checks and ten job checks prove what they test. They are not an escape-resistance proof, a performance test or an audit.
- **Voice has no authentication.** The voice service binds to localhost and must not be exposed. See [VOICE.md](VOICE.md).
- **Single operator.** The limits in the one-shot section apply here too: one trusted operator, no multi-tenant security claim, and no independent security audit.

## Security testing

The 2026-11-06 audit is in [security-audit-2026-11-06](evidence/live/security-audit-2026-11-06/). Its source review covers authentication and owner scoping, CSRF, cookies and headers, the files API, box configuration, SSRF through webhooks and MCP servers, UI rendering, request caps, `unsafe` code and `cargo audit`, with a result for each. It found two low-severity issues, both fixed with regression tests: the login limit was global, so any client could lock the operator out (now per client address under a global ceiling), and remote MCP servers over https were not checked for private addresses (now guarded like webhooks). The live probes of the deployed instance are recorded in the same directory. This is an internal review, not an independent audit.
