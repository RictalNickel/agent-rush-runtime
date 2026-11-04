# Observed verification

This report distinguishes the implemented local system from a deployable/qualified hackathon submission. Recorded UTC timestamps are preserved in raw evidence; user-facing planning dates use America/Chicago.

## Checks

| Check | Observed result |
| --- | --- |
| Rust unit/API/policy suites | 52 passed after configuration/deployment continuation; command `cargo test --locked -j4` |
| Default container test selection | Six explicitly ignored; a default green suite does not qualify a host |
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
| Native Rust controller/tool/context lifecycle | Implemented, bounded main/subordinate contexts | Unit tests; real provider/controller/worker roundtrip outstanding |
| Authenticated product UI | Implemented, Rust-served HTML/CSS and browser JS | Desktop/mobile core flow tested |
| Durable jobs/idempotency/owner scoping | Implemented, SQLite WAL | Tested |
| Atomic memory admission/reservation | Implemented | Concurrency/restart tests pass |
| Memory growth | Ledger primitive | Live host update deliberately not exposed or claimed |
| Private worker/token auth | Implemented | Authentication tests pass; deployed mTLS tests outstanding |
| Fixed gVisor execution and safe artifacts | Implemented | Real local containment passed; host enforcement qualification failed |
| Recovery/reconciliation | Implemented, conservative lease retention | Crash/transport race qualification incomplete; no production claim |
| Vultr-only inference and cost reservations | Implemented, no alternate provider | Validation/accounting tests; live API blocked |
| Read-only cost planning | Implemented with guarded lifetime/rate budgets | Fixture tests; actual plan query blocked |
| Infrastructure provisioning/independent teardown | Provisioning/watchdog not implemented; guarded manual teardown implemented | Fixture tests only; live account/management route and independent cleanup required first |
| Nested Docker, browser/desktop profiles | Not enabled | Not qualified |
| Full Agent Zero port/plugin parity | Not claimed | Upstream downloaded as reference only |
| Private repository publication | Blocked/unverified | No source published |
| Presentation PDF | Prepared from actual local evidence/status | Must refresh after cloud acceptance |
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
