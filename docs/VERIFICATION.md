# Observed verification

## Current status (2026-11-05, live Vultr deployment)

| Check | Observed result |
| --- | --- |
| Rust suites | 101 passed, 7 real-host tests ignored by default. Clippy `-D warnings` and rustfmt clean. Run on the Vultr worker |
| Browser suite | 2/2 Playwright tests passed (desktop flow, mobile layout) |
| Dependency audit | `cargo audit`: 228 crates, no advisories |
| Worker qualification | 9/9 real checks passed on the live worker (`evidence/live/worker-qualification-2026-11-05.json`). The earlier memory-cap failure does not reproduce on a host with enforcing cgroups |
| Public HTTPS and worker mTLS | Deployed and tested. No client certificate: 400. Wrong or missing token: 401. Raw worker port unreachable |
| Real inference, agent, worker roundtrip | Done with both providers: Anthropic (`2f6bd23b`, 138 s, $0.196) and Vultr GLM 5.3 Flash (`2fd5b26f`, 33 s, $0.003; public job `7d466fa9`, 58 s, $0.006) |
| Concurrent load | 4 jobs, 2-slot admission, all succeeded with verified cleanup. Two sandboxes overlapped for 4.7 s within their caps; a third run was refused with 429 |
| API performance | `/api/status` 460 req/s, p50 8 ms (was 31 req/s, 494 ms) |
| Containment | Probe receipt plus timeout kills recorded |
| Private repository | Created (`RictalNickel/agent-rush-runtime`) |
| Cloud spending | Two VMs at $0.082/h, plus measured model usage |

Details: [LIVE_DEPLOYMENT.md](LIVE_DEPLOYMENT.md).

## Historical checks (2026-11-04, local environment before deployment)

The table below is the earlier record from a restricted local environment and is kept for provenance. Recorded UTC timestamps are preserved in raw evidence.

| Check | Observed result |
| --- | --- |
| Rust unit/API/policy suites | 86 passed after independent-watchdog continuation; command `cargo test --locked -j4` |
| Default container test selection | Seven explicitly ignored; a default green suite does not qualify a host |
| Explicit real local container suite, before fail-closed preflight was added | Five passed; one failed (`real_memory_cap`) |
| Real containment | Rust program executed, total 42, JSON artifact collected and hashed, root write/host path/socket/credential/network probes denied, cleanup verified |
| Real timeout | Timeout observed; the last recorded 5-second deadline ended at 5007 ms execution time; total duration also includes slow vfs setup/cleanup |
| Real output cap | Passed: process output bounded to 64 KiB and task terminated |
| Real scratch cap | Passed: bounded filesystem rejected excess writes |
| Real cancellation and cleanup | Passed |
| Host memory enforcement | Failed: a 384 MiB allocation escaped a requested 256 MiB cap |
| Actual host capability diagnosis | MemoryLimit=false, CpuCfsQuota=false, PidsLimit=false, CgroupDriver=none; runsc uses --ignore-cgroups |
| New worker preflight | Unit tested; refuses this host before arbitrary execution |
| Browser flow | Two Playwright Chromium tests passed: desktop login, actual queued job, export, cancel; mobile login/layout |
| Real Vultr inference | Not run; existing-key retrieval/catalog verification blocked by network; verified pricing absent |
| Vultr account/pricing | Blocked by proxy CONNECT 403; the infrastructure key has not been validated |
| GitHub repository creation | Attempt failed with API Forbidden; no repository created or source pushed |
| Public HTTPS/mTLS deployment | Not run; service/nginx templates only |
| Cloud spending | $0 created by this project; no VMs or inference requests |
| Optimized Rust build | `cargo build --locked --release` passed |
| Rust lint/format | Clippy with warnings denied and rustfmt checks passed |

Raw earlier local receipts and the complete failing resource-suite log are in docs/evidence/local. They are historical bounded probes, not proof that the current host is safe. Since the preflight fix, the ordinary execution path refuses this host. Do not disable the fix to reproduce a green subset.

## Implemented versus qualified

| Capability | Implementation | Acceptance |
| --- | --- | --- |
| Rust agent loop, team roles, contexts | Implemented: orchestrator, coder, worker and reviewer with enforced allowlists | Unit tests; live roundtrips passed with both providers |
| Authenticated product UI | Implemented, Rust-served HTML/CSS and browser JS | Desktop/mobile core flow tested |
| Durable jobs/idempotency/owner scoping | Implemented, SQLite WAL | Tested |
| Atomic memory admission/reservation | Implemented | Concurrency/restart tests pass |
| Memory growth | Atomic admission for the next sandbox attempt | Engine/ledger tests pass; live in-place updates are not exposed or claimed |
| Private worker/token auth | Implemented | Deployed mTLS and token rejection tested live |
| Fixed gVisor execution and safe artifacts | Implemented | Live worker qualification 9/9 |
| Recovery/reconciliation | Implemented, conservative lease retention | Crash/transport race qualification incomplete; no production claim |
| Inference gateway and cost reservations | Implemented: Vultr Serverless Inference or Anthropic, selected by the operator | Both verified live; the public instance runs Vultr (GLM 5.3 Flash) |
| Read-only cost planning | Implemented with guarded lifetime/rate budgets | Fixture tests; actual plan query blocked |
| Infrastructure provisioning/independent teardown | Provisioning disabled; native tracked-resource watchdog and manual teardown implemented | Fixture tests and denial-path CLI smoke pass; real provider deletion/runner uptime unverified |
| Nested Docker, browser/desktop profiles | Not enabled | Not qualified |
| Agent Zero compatibility | Not a goal | Design reference only; see reference/DESIGN_LINEAGE.md |
| Private repository publication | Done | Private now; public before submission |
| Presentation | Slide deck built from live evidence (`presentation/deck/`) | PDF export pending |
| Real live demo video/submission | Not recorded/submitted | Outstanding |

## Measurement limits

The local daemon uses vfs storage, so Rust image container creation can take tens of seconds. Record total duration separately from execution_ms. Raw runs are illustrative samples, not a reproducible Vultr benchmark. Docker resource sampling returned no usable samples on this host; no utilization, peak-memory or CPU-isolation numbers are invented. CPU/PID/concurrent-neighbor and nested-runtime qualification remain unrun.

## UI fidelity and browser evidence

Reference: /workspace/generated_images/exec-1a96790f-a803-4167-85fd-008bdf0d7716.png, 1487×1058. Browser plugin unavailable; regular Playwright with installed Chromium used the same desktop dimensions and a 390×844 mobile viewport.

Compared five areas: true-white/cool-gray/pine palette; sidebar and two-column composition; title/control typography; fine borders/radii/spacing; actual form/table/dialog interactions. Refined the sidebar width, title sizing and outline icons to match the reference. Intentional extensions: login/error dialogs, genuine queued/unavailable state, refresh/export/cancel controls, and cloud status shown as unverified rather than claiming account-wide idle state. Mobile navigation wraps so no control is clipped. Above-the-fold product copy follows the concept except these functional status additions.

Screenshots and temporary QA outputs remain outside committed source under /tmp. The final UI comparison and latest test results must be refreshed after any subsequent visual change.

## Final review fixes

One fresh read-only whole-branch review identified five Important defects and one readiness-reporting defect. The readiness defect was promoted for fixing because it would mislead users of an mTLS deployment. Added regression coverage for idempotent repeated cancellation, bounded history with explicit dropped-event counts, queued jobs surviving more than 100 newer terminal records, rejection during worker drain, preservation of uncertain creation during cancellation, final output-overflow/capture reconciliation, and configured worker trust not being ignored. All 31 unit/API tests and both fresh-database browser tests passed after the fixes. No second review or independent security audit is claimed.

Uncertain creation and crashes with an unfinished run now require operator fencing/reconciliation before worker restart. An empty inventory snapshot does not resolve possible late Docker creation. This conservative stop retains capacity and can require manual recovery; it is preferable to a false cleanup receipt. Experimental crash/transport race qualification remains outstanding.

## Configuration and deployment continuation

The Rust suite now has 52 passing tests, including existing-key export permissions/redaction, exact GLM 5.3 Flash catalog matching, missing-price behavior, explicit tariff-verification gating, resource ownership/TTL/budget validation, pending deletion and cleanup persistence. Clippy with warnings denied and format checks pass. These fixture checks do not establish that the provider offers the selected model or any particular tariff.

The official Vultr SDK reference at ab4d9052f7bba410fd63a3f9129e06bd5e14aae1 documents retrieval of existing inference subscription keys via the account API. The new helper never creates a subscription or prints its key. Model discovery uses the separate inference credential and a fixed Vultr origin. No actual API request has succeeded, no cloud resource has been deleted, and no inferred rate has been enabled.

Cleanup defaults to preview. Execution requires exact resource IDs, deployment-specific labels/tags, region and creation-time checks, then confirms absence. An authenticated account/credential hash is checked before accepting any absence response. Both independent VMs are attempted even if one is pending/errors; dependencies wait until all VMs are confirmed absent. Attached VPC/firewall resources are rejected. This is a trusted manual helper, not an independent billing cutoff or provisioning workflow.

A fresh bounded review of this continuation identified two Important issues: unbound-account 404 responses and pending instance deletion delaying the second VM. Both were reproduced with failing regression tests and fixed. Added coverage also verifies that one instance lookup error does not prevent attempting the other instance, while dependency cleanup stays blocked. This review is source-level evidence, not a live cloud or independent security qualification.

## Native port, orchestration and allocation continuation

The current Rust suite passes 74 tests. Seven host tests are explicitly ignored by default; the added native host-qualification test requires all nine fixed real probes. New fixture coverage exercises the production engine's iterative subordinates/continuation, shared call limits, memory save/search/delete, between-attempt memory requests, failed execution/retry, cancellation and unresolved cleanup. Atomic admission now checks both slots and RAM in the same transaction, and scheduling rotates queued owners. These fixtures do not establish provider-generated task quality.

Worker startup requires a host/image-matching qualification report no older than two hours. Passing flags are recomputed from their recorded receipts. Expiry blocks new admission. Actual `agent-rush qualify` on the managed host returned `qualified=false`, `containers_created=0`, and `Host cannot enforce required MemoryLimit; worker refused`; raw report is docs/evidence/local/native-qualification.json. CPU, PID and overlapping-neighbor enforcement remain unverified, as all probes were refused here.

One fresh review of this continuation found an Important panic from serialized JSON expansion in the context buffer. The actual-engine regression failed before fixing and now passes with both clean and unresolved worker receipts. Observation failures now follow normal finalization; receipt evidence is retained first. History shortening preserves the fixed policy/original task and records truncation counts. Full suite, denied-warning Clippy and format checks passed after fixing.

The current desktop/mobile Chromium suite again passes both tests, including the added authenticated context download. No browser plugin is available, so regular Playwright and installed Chromium were used. The actual updated execution record was visually inspected. No model, sandbox or cloud execution is simulated as live by these UI checks.

The native Ubuntu-based control OCI image was built and exported, then actually started with UID/GID 65532, read-only root, dropped capabilities and bounded scratch requests. Real HTTP smoke checks passed for authentication, job/context/evidence operations, memory tools and logout; no inference/worker calls were configured. Container deletion was verified. Raw evidence: docs/evidence/local/control-image-smoke.json. The host still reports unsupported resource controllers, so these requested container flags do not establish worker resource enforcement. The current progress PDF has eight pages and updated real evidence; it is not a completed contest submission.

## Independent watchdog continuation

Full Rust suite: 86 passed, seven real-host tests ignored. Added deadline/explicit-stop cleanup, scope checks, pending/error handling, private atomic heartbeat, process/freshness/one-shot guards, exclusive watcher ownership and saved partial progress. Manifest timing/scope/estimate/region and tracked identities cannot be silently changed by ordinary updates. Cleanup closes admission durably before API operations, so completion cannot race successful tracking or rearm after a failed early stop.

One fresh bounded source review found an Important completion/tracking race. Both its deterministic regression and early-stop/restart regression failed before fixing, then passed with the full suite and denied-warning Clippy. Zombie PID readiness was also reproduced and rejected. Existing default polling is 15 seconds; longer permitted polling intervals can intentionally produce conservative gaps against the 30-second readiness freshness threshold.

The actual continuous CLI failure-path smoke used a deliberately invalid synthetic credential. It observed API-check failure, an owner-only error heartbeat, no false arming, preserved admission closure on SIGTERM and a graceful nonzero exit. It had zero resource IDs and tested no provider deletion; raw evidence is docs/evidence/local/watchdog-denial-smoke.json. The systemd template passed user-mode `systemd-analyze verify` with the actual executable path substituted, but it was not installed or started. System-mode validation cannot write /run/systemd in this environment. Real provider cleanup, service restart and management-runner uptime still require live acceptance.

No web source changed in this continuation; the previous two browser tests remain applicable. No paid Vultr resource/model call, live watcher or extra VM was started. See CLEANUP_WATCHDOG.md for operational limits and remaining provisioning-intent work.
