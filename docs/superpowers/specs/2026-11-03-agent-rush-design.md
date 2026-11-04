# Agent Rush Runtime — proposed build design

Status: approved by the user in chat. Subsequent user direction supersedes language choices below: all platform components are Rust, and Agent Zero is a downloaded reference, not a fork or Python runtime dependency. Implement a restricted Rust controller inspired by its context/tool lifecycle, not a claim of full upstream parity. Rust serves the product UI; HTML/CSS and minimal browser JavaScript remain native web assets. No Vultr deployment or spending yet.

## Intent and decisions

Give cooperating agents a disposable development environment and users independently collected proof of what ran. Target the supplied Blast Radius Zero criteria: Vultr hosting, Vultr inference for every agent role, isolated execution, enforced limits, cleanup, and a real containment demonstration. Winning is an aspiration, not a promised outcome.

The user authorizes end-to-end implementation, testing, a private GitHub repository and up to $300 Vultr spending. Their direct request takes precedence over attachments asking for a public repository or separate approval for every provisioning action. Keep the repository private until the user explicitly authorizes publication. Do not submit contest forms or publish their identity without authorization.

The architecture, requirements, handoff and PDF are design inputs, not evidence of deployed capabilities or independently verified event rules. Referenced documents AGENT_ZERO_REUSE.md, SECURITY_AND_VERIFICATION.md, IMPLEMENTATION_PLAN.md and VULTR_DEPLOYMENT.md were not provided; create their necessary equivalents from inspected source and real tests rather than claiming to have read them.

## Approaches

1. Single Vultr VM for control and execution: lowest cost, but exposes the controller to more consequences of worker compromise and conflicts with the proposed physical separation.
2. Two Vultr VMs with a private authenticated node manager: recommended. Preserves the supplied architecture and makes the containment demonstration understandable. Costs and operational complexity are higher.
3. Throwaway VM per execution: strongest disposable machine boundary but slower, more expensive and unnecessary for the first submission.

There is one appropriate default given the supplied architecture: option 2. This is the builder's recommendation, not an independently confirmed user selection. Start with direct CLI sandboxes and qualify nested Docker separately. Do not substitute a weaker runtime if nesting fails.

## Product and demonstration

An authenticated operator opens a task workspace, asks for a Python data-processing or small application task, observes a bounded plan and actual execution, and receives the result with artifacts. The screen shows queued/running/verifying/cleaning states, actual runtime measurements, logs and an evidence report. Include an operator-triggered containment demonstration against a synthetic canary; never attack unrelated systems or use real secrets as canaries.

The evidence report is the proposed differentiator: artifact SHA-256, image digest, runtime, enforced resource limits, worker-collected exit status, actual execution timestamps, deterministic checks and independently observed cleanup. A hash supports integrity comparison; it is not remote attestation or an independent security audit. Unmeasured values and unavailable evidence are shown as unknown, not animated or invented. Downloadable reports redact secrets and bind records to authenticated task ownership.

## Components and boundaries

- Control VM: Python/FastAPI authenticated API, durable SQLite job/event/lease ledger, bounded orchestration queue, isolated Agent Zero controller process, server-side Vultr inference gateway, React/TypeScript product UI and HTTPS reverse proxy. SQLite is adequate for one control node; no unnecessary database VM.
- Agent Zero: inspect and pin upstream commit ba9b67df0f54ac78106ae85c552ef5d2458f7c9a (observed as current upstream HEAD during discovery). Preserve MIT attribution. Expose only broker-backed task tools; remove local shell, SSH, browser and arbitrary tool-loading authority in the restricted profile. All chat, subordinate and utility model calls must use the Vultr gateway; disable unused embedding/model-dependent features unless their calls are routed and tested. Do not claim an Agent Zero integration until the real loop is exercised.
- Worker VM: private trusted node manager with host Docker authority, fixed images/runtimes and no public task interface. Only authenticated control-plane requests are accepted. mTLS on the private network; keys remain outside task containers. Persist a worker-side deadline and reconciliation journal so controller restart does not leave runaway tasks.
- Task sandbox: gVisor/runsc, non-root identity, network disabled for initial CLI tasks, read-only root, bounded tmpfs scratch, dropped capabilities, no-new-privileges, PID/CPU/memory/output/deadline caps. No host Docker socket, arbitrary bind mounts, secret variables or host paths. Copy input through bounded safe APIs. Do not inherit controller or deployment environment variables.
- Nested profiles: separate capability and image. Enable only after genuine Docker-in-gVisor qualification under the outer resource envelope. Never use host privileged Docker as a compatibility workaround. Browser and optional desktop support come after this; omit unsupported profiles from the available menu.

Generated code executes only on the worker. Trusted deterministic host code may manage Docker and verify evidence; model-selected shell strings may not execute in control or node-manager processes.

## API and state model

Implement owner-scoped job creation, status/events, cancellation, artifacts, evidence and policy-limited lease growth. Server establishes owner/project identity; model output cannot override them. Validate input length, profile, resource request, idempotency key and capability against policy before admission. Return the original result for repeated identical keys; reject conflicting payloads.

Persist transitions: queued -> planning -> executing -> verifying -> cleaning -> succeeded/failed/cancelled. Terminal success requires validated output and verified cleanup. Maintain attempt IDs, maximum attempts, deadlines, heartbeats and recovery events. A failed cleanup remains visibly unresolved and prevents lease reuse until reconciliation.

Lease admission is transactional. Reserve hard memory plus node headroom, bound concurrent jobs, schedule fairly across admitted projects and expose queue reasons. CPU ceilings allow opportunistic sharing, not dedicated core guarantees. Growth is granted atomically only inside a policy ceiling and available node capacity; denial does not overcommit. Host cgroups enforce the outer envelope, including nested children. Storage and output limits are independent from memory; malicious output cannot fill the host filesystem.

Artifact ingestion accepts regular files only, with per-file/total/count quotas, path traversal and symlink rejection, and ownership checks on download. Never render generated HTML at the authenticated control origin. First release may offer downloads only.

## Authentication and public operation

Provide a configured operator login with signed HttpOnly/SameSite sessions, secure cookies under HTTPS, CSRF protection for mutations, bounded rate limits, and strict request validation. Do not expose an unauthenticated arbitrary-code endpoint to the Internet. Public visitors may view a sanitized showcase and prerecorded evidence; starting paid agent runs requires operator authentication. Never embed inference or infrastructure credentials in the frontend bundle.

Default deployment region: a US region with both selected plans and VPC support; prefer Chicago if available. Default public origin: a TLS-capable deployment hostname assigned to the control VM; use an operator-owned domain if supplied. Public HTTPS capability and trustworthy certificates must be verified, not assumed from an IP address. Admin SSH restricted to the deployment route, worker control port private only, public ingress limited to 80/443 as needed for TLS. Do not alter or remove pre-existing account firewalls/resources.

## Spending policy

Hard authorization ceiling is $300 total for this project's Vultr resources and inference. Prior to provisioning, query actual account access, current plan prices and availability. Do not treat the PDF's VM examples as quotes. Prefer approximately 2 vCPU/4 GiB control and a worker sized for the first measured task; choose the lowest adequate available plans. No GPU, managed Kubernetes, automatic scale-out or optional paid services.

Target projected infrastructure at no more than $0.30/hour combined, subject to actual available pricing; if unavailable, return a concrete costed alternative before spending. Maintain a local deployment ledger listing project-tagged resource IDs, actual prices, creation time and teardown commands. Use per-job token/attempt/concurrency limits, bound cumulative inference spend conservatively and reserve at least $50 of the $300 ceiling. If accurate inference prices/usage cannot be established, stop further paid requests rather than guessing remaining credit.

The user additionally requires using machines and money only when needed and pausing or shutting down containers and VMs. Build and run policy/unit/UI checks locally first. Create Vultr resources only when a specific deployment acceptance test is ready to run. Stop and remove task containers on every exit path; stop VMs promptly when their active check completes. Because stopped VMs can still be billed, temporary test deployments should then be deleted after evidence and reproducible configuration are retained; this applies only to newly created project resources. Do not leave a public demo running unattended merely for convenience. Any later continuously available demo needs an explicit time window and budget allocation from the user.

Every temporary deployment has a maximum two-hour test window, shortened to fit remaining funds, with independent scheduled deletion where supported and a manual teardown path. Do not provision if cleanup cannot be performed through the available authenticated route. Keep inference disabled when no authorized test is running. Report that a hard all-account billing cap cannot be guaranteed by application counters. Never delete unrelated resources or resources with unpreserved user data. Do not spend to compensate for unavailable credentials.

## Required evidence and acceptance

1. Backend tests cover authentication/ownership, policy, atomic admission under concurrency, idempotency, cancellation, timeouts, crash reconciliation, safe artifacts and secret redaction.
2. Real gVisor integration proves generated Python execution, known output/artifact hashes, blocked host/socket/canary access, disabled egress, PID/output/scratch/deadline/memory limits and removed worker inventory after success/failure/cancel/restart.
3. Actual Vultr inference generates a task through the Agent Zero path. Record provider/model/usage metadata without keys; mocks cannot pass this acceptance check.
4. UI build and browser tests cover real task progress, authentication, evidence downloads, error/cancel paths and small screens. No fake results presented as live.
5. Deploy and test HTTPS, external reachability, private worker interface, startup recovery and end-to-end containment on Vultr. Current environment integration is useful but does not qualify remote hosting.
6. Measure cold/warm startup and concurrent task behavior with raw samples, image/runtime/host details, sample counts and methodology. No performance guarantees from tiny samples.
7. Deliver README, dependency locks, deployment/teardown guide, upstream notices, security limitations, demo script/recording and slide PDF. Verify live event rules when network access permits; do not claim submission happened without a receipt.

## Staging and optional scope

First complete direct execution end to end with the real controller and evidence. Then qualify nested app+database execution and admitted memory growth; include them in the final feature matrix only if tests pass. Headless browser and dormant desktop are later optional capabilities. Multi-node scaling, hostile multi-tenant guarantees, automatic rescue from OOM, arbitrary external browser actions and independent nested-child resource guarantees are out of scope for the initial release.

## Current discovery and external prerequisites

- Existing /workspace/Hackathon is an empty Git checkout with no local changes or commits; remote Git reads work. Repository visibility and write/create permissions are not verified: GitHub REST calls currently return Forbidden under the running network policy.
- Local managed Docker responds when executed with the sandbox exception; runsc is registered. Its configuration includes --ignore-cgroups, so local Docker availability alone does not establish memory/CPU enforcement. Host resource qualification is still required.
- No VULTR_API_KEY or VULTR_INFERENCE_API_KEY binding is present in the current runtime. A key was pasted in chat; do not reproduce it in files, commands or logs. Request a rotated infrastructure credential through secure environment settings. The inference credential is distinct and must be supplied separately.
- Saved a configuration draft adding the two secure credential requirements and destinations api.vultr.com, api.vultrinference.com, api.github.com, lablab.ai, community.vultr.com, gvisor.dev and storage.googleapis.com. Saving is confirmed; applying/publishing is not. Existing package-manager presets are preserved.

## Builder discretion

Choose implementation structure, current compatible pinned dependency versions, UI details, test tooling and safe task fixtures. Preserve the selected trust boundaries and private-repository requirement. Record supported versus unsupported behavior truthfully. Budget, credential access, public visibility and public task authority cannot be silently expanded.
