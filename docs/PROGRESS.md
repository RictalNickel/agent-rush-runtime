# Implementation progress

## Independent cleanup continuation, November 4 UTC

Plan: docs/superpowers/plans/2026-11-04-cleanup-watchdog.md; base 63e2e64.

- Standing authorization reaffirmed by the user. Local execution now has full filesystem access; actual managed sidecar network policy still has no custom API hosts, and fresh Vultr/GitHub requests still return proxy 403. Do not confuse authorization, local execution permissions and applied network policy. No cloud resources or model requests created.
- Added a native trusted-runner watchdog with default preview, authenticated scope checks, private atomic heartbeat, PID/start-time/freshness guards, separate exclusive watcher lock, deadline cleanup/retry, progress persistence and interrupt cleanup attempt. Prepared an independent systemd service; none installed or started here.
- Manifest saves preserve the original scope, price, region and window, and preserve tracked resource identities. Cleanup durably closes admission before API calls. Closed deployments cannot accept new IDs or be reopened by stale snapshots; failed early cleanup remains fenced after restart.
- One fresh bounded source review found an Important completion/tracking race. Reproduced it and fixed it with deterministic completion/track and early-stop/restart regressions. Also reproduced/rejected zombie PID readiness. Full suite after fixes: 86 passed, seven real-host tests ignored; Clippy with denied warnings and formatting passed.
- Ruling: run the watcher on a trusted management runner with independent uptime/API access, separate from application/worker processes — application survival cannot carry cleanup responsibility — cost: real provider deletion and management-runner reliability remain unqualified until access works. This covers acknowledged tracked resources, not ambiguous unrecorded cloud creation; automatic provisioning remains disabled.

## Native completion continuation, November 4 UTC

Plan: docs/superpowers/plans/2026-11-04-native-port-completion.md; base 849c86e.

- Task 1: native owner-scoped durable memory save/search/delete, strict upstream tool-envelope translation, parent-bound persisted contexts and bounded history compaction implemented. Four new tests passed. No embedding/model call needed for retrieval or compaction.
- Task 2: atomic two-slot/memory admission, owner rotation, worker-advertised capacity, between-attempt-only growth, physical-memory headroom and equal CPU weights implemented. Five tests passed; the old eight-way slot over-admission and running-sandbox accounting-only growth were reproduced as failures before fixing.
- Task 3: actual production loop extracted into the Rust engine, with typed production Vultr/worker adapters. Iterative subordinate contexts, memory tools, planning, resource requests, shared twelve-call budget and interruptible cancellation implemented. Six engine tests passed, including failed execution/retry, subordinate execution, memory denial and unresolved cleanup. Current full suite after review: 74 passed, seven host tests ignored by default. Fixture providers/workers exercise the production engine but are not live Vultr acceptance.
- Ruling: memory growth admits a new envelope for the next sandbox attempt, never changes only the ledger of a running process — this keeps accounting consistent with enforced worker limits — cost: live in-place growth is unavailable until separately implemented/qualified.
- Ruling: port supported Agent Zero context/tool/memory behaviors into native Rust while retaining deny-by-default authority and Vultr-only model routing — unrestricted upstream shell/plugins conflict with the approved isolation boundary — cost: arbitrary upstream plugin parity is not claimed.
- Runtime APIs still return proxy 403 after recheck through the configured proxy. No custom network hosts are applied. GitHub creation and Vultr billing/model/deployment acceptance remain blocked; authorization does not need renewal.

Plan: docs/superpowers/plans/2026-11-03-agent-rush.md

- Design approved in chat; independent builder check found no material gaps.
- User amendment: native Rust platform, Agent Zero source reference only, no fork. Existing Python test scaffolding was removed before product implementation; Rust tests replace it.
- Existing environment is an isolated checkout on `work`; no worktree created.
- Pre-flight: API and ledger share owner-scoped job/lease records; worker receives only fixed bounded code execution requests; controller consumes worker evidence; UI consumes only authenticated API. Rust domain types will define these shared interfaces.
- Ruling: use a restricted Rust agent loop inspired by Agent Zero rather than porting every upstream plugin — user requested a Rust reference implementation; full upstream parity would include unrelated tools that violate the sandbox boundary. Cost if wrong: feature parity beyond the approved restricted product needs a separate port.
- Ruling: use Rust-served HTML/CSS with minimal browser JavaScript instead of React — follows latest all-Rust platform direction while keeping browser-native assets. Cost if wrong: a future richer browser client may need another frontend architecture.
- No paid Vultr resources created. No inference requests made. Network/secret requirements saved in the environment draft; not applied to runtime yet. GitHub API currently returns Forbidden; Git reads work.
- Agent Zero reference downloaded at /tmp/agent-rush-upstream-review, commit ba9b67df0f54ac78106ae85c552ef5d2458f7c9a. Its current Tool/Agent/ModelConfig APIs inspected; paths in supplied documents partially predate this layout.

## Current delivery checkpoint

- Task 1: local broker/API implemented and tested; source committed. Transactional admission, ownership, idempotency, cancellation and persistence covered.
- Task 2: Rust worker/artifacts implemented. Historical bounded local suite: five passes, memory-cap failure. Runtime host preflight added; this managed host is refused. Host CPU/PID/concurrency and crash qualification remain incomplete.
- Task 3: native Rust reference implementation replaces the Python integration per user direction. Tool/context/provider/accounting tests pass; actual Vultr inference/controller/worker acceptance blocked by missing distinct inference binding and API network policy.
- Task 4: Rust-served UI implemented. Two real Chromium tests passed at 1487×1058 and 390×844, with fresh database and inference disabled. Concept/latest render inspected directly; mobile navigation and fixed network status corrected.
- Task 5: guarded read-only Vultr cost inspection and service/mTLS templates prepared. Live account/pricing, private GitHub creation and provisioning blocked. Private GitHub create attempt returned Forbidden; no source pushed. No cloud creation/deletion automation or independent billing cutoff is claimed.
- Task 6: 31 unit/API tests pass, Clippy with warnings denied passes, optimized release builds. A seven-slide progress PDF is exported. End-to-end provider/cloud demo video and submission remain outstanding.
- Repeatable local install script exercised with frozen Cargo.lock. No Python implementation remains. Agent Zero source preserved as a checksummed archive outside the repository.
- All temporary task containers removed; QA servers stopped. No paid Vultr resources/inference created. Supplied infrastructure credential scanned absent from repository files.

## Final review and rulings

- Final review: fresh read-only whole-branch review of 2d57881. Five Important findings fixed with regression coverage: uncertain-create cancellation, shutdown admission, repeated-cancel lease stranding, scheduler history starvation, and fast output-overflow success. Configured mTLS readiness was also fixed as an Important user-visible correctness issue. Final suite: 31/31 unit/API tests plus 2/2 browser tests; six host tests remain explicitly separate.
- Final: Ruling: preserve uncertain-create/crash journals and stop worker startup rather than infer cleanup from an empty snapshot — outstanding daemon creation cannot be conclusively fenced here — cost: manual operator recovery can be required.
- Final: Ruling: live Vultr inference/billing, GitHub publication, cloud provisioning/deletion and deployed TLS/systemd are not validated — required access/configuration is absent — cost: no deployed or remotely reproducible project may be claimed.
- Final: Ruling: host resource enforcement, gVisor security and crash/transport races are not independently certified by source review — the real memory failure is retained and the host refused — cost: a properly configured worker and additional real acceptance tests are required.
- Final: Ruling: generated-result semantics, full Agent Zero parity and hostile multi-tenancy are not established — the product is a restricted reference implementation with structural checks — cost: broader compatibility and security assurances require additional work.
- Final: Ruling: nested/browser/desktop, live memory growth, automatic cloud cutoff and continuous cleanup retry remain unavailable — these capabilities are not qualified or implemented — cost: they must stay out of the demo's supported-feature claims.
- Deferred minor findings: none from the final review; the single readiness finding was fixed.
- Saved install_script and start_skill plus operator/model/pricing requirement names in the environment draft. Saving confirmed; runtime application/publication and fresh-task restoration are not verified. Pending network/secrets requirements remain.
- The optimized doctor command rejects the missing MemoryLimit capability. Temporary infrastructure credential file removed before publication after a repository-wide value scan; no source or artifact contains it.

## User-selected configuration

- Selected GLM 5.3 Flash for all agent roles, Vultr-only. Official model-catalog request is blocked by proxy CONNECT 403; exact API ID/prices remain unverified and unset, with paid inference disabled. No substitute model selected.
- User delegated remaining choices, including passwords. Recorded non-secret decisions in deploy/decisions.json and docs/CONFIGURATION.md. Generated a 288-bit operator password, separate 256-bit worker token, Ed25519 SSH deployment key and P-256 private CA/service/client identities in an owner-only directory outside Git. Plaintext credential record: /workspace/.agent-rush-secrets/CREDENTIALS.md. Private keys/passwords are excluded from source and release artifacts.
- Verified generated operator login/logout against the actual Rust server, matching token configuration, owner-only permissions, SSH key pair and TLS certificate chain/hostname/purpose. Local worker remains disconnected; inference accounted spend is zero. Stopped the local test server after verification.
- Updated saved startup instructions with the selected model, credential locations, local configuration and spending choices; draft persistence confirmed. Runtime network/credential application and cloud publication remain outstanding.

## Authorized continuation

- User renewed authorization to continue all existing work. Rechecked actual runtime versus draft: seven API destinations exist in draft revision 4, but observed enforced policy still has no custom hosts. Vultr account/model catalog requests fail at proxy CONNECT 403; GitHub API returns Forbidden. No new permission question is required; network application is an external prerequisite.
- Downloaded official vultr/govultr reference at ab4d9052f7bba410fd63a3f9129e06bd5e14aae1. Its serverless-inference API exposes existing subscription keys through authenticated GET /v2/inference. A working infrastructure API route can retrieve an existing inference key; manual key entry is not necessarily required. No subscription created.
- Continuing Task 5 offline with tested secret export/model selection and exact-ownership cloud cleanup. See docs/superpowers/plans/2026-11-03-deployment-continuation.md.
- Implemented private existing-subscription credential export, strict catalog selection of GLM 5.3 Flash, owner-only deployment manifests and dry-run-first exact-resource teardown with ownership/absence checks. No subscription creation, VM provisioning or independent cutoff is claimed. No live deletion performed.
- Clarified delegated configuration: operator password already chosen privately; input/output tariffs come from the provider and are internal accounting fields. Blank values keep inference disabled. Added an explicit false-by-default tariff verification gate so arbitrary numeric values do not certify pricing.
- Current Rust checks: 52 passed, six real host tests ignored by default; Clippy with warnings denied passed. Existing two browser tests remain the latest UI evidence; no visual change in this continuation.
- Saved updated startup instructions and added www.vultr.com for official tariff lookup in the configuration draft. Save confirmed; requires publication. Actual enforced policy still has no custom allowed hosts. No new user permission is needed, but this tool cannot publish/apply the draft.
- Continuation review: fixed both Important findings with regression fixtures: account/credential-scope binding before absence checks, and independent VM cleanup despite another VM being pending/errors. Current full suite 52 passed; no new cloud calls.

- Task 4: real nine-probe native qualification command and a two-hour host/image-bound worker startup gate implemented. Actual managed-host command rejected MemoryLimit before any probe; report records zero containers created. Worker CLI rejects absent qualification. CPU/PID/neighbor probes remain not run here, not silently passed. Official Rust sandbox and Ubuntu 24.04 runtime OCI images are downloaded locally with immutable digests; native control image recipe/build/export prepared.
- Task 5 review: one fresh bounded source review found one Important context-overflow panic. Reproduced it through the real engine using an 8,000-quote task and heavily escaped worker output, including unresolved cleanup. Fixed serialized-size-aware truncation, fallible observation and receipt retention before processing. Regression RED→GREEN; full Rust suite 74/74 and Clippy with warnings denied passed. No second review claimed.
- Fresh browser checks: 2/2 passed, including authenticated context JSON download; actual execution-record screenshot inspected after the new control. Temporary QA server stopped. No paid inference/cloud actions performed.

- OCI delivery: actual native control image built/exported, source revision d5076f2, image sha256:c5251eb43c9be47c52587cd5fbe32de9e6b93040b864c06166c2daa90b7e9495. Real non-root/read-only-root container HTTP smoke passed for auth, SQLite, jobs, context/evidence, cancellation and memory; zero inference spend; exact container removed and absence verified. Raw receipt saved in docs/evidence/local/control-image-smoke.json. Execution worker remains unqualified.
- Updated the actual progress deck to eight pages with native workflows, allocation policy and current test results. Browser/PDF temporary servers stopped. All currently runnable offline native-port/image checks completed; genuine cloud/inference acceptance, qualified worker tests, GitHub publication, nested/browser/desktop implementation and a live containment recording remain outstanding.

- Actual continuous CLI denial smoke passed with an explicitly synthetic invalid credential: API failure could not arm readiness; heartbeat mode0600; SIGTERM persisted the cleanup fence and exited nonzero; process stopped; no provider deletion tested and zero resource IDs. User-mode systemd unit verification passed with the actual executable substituted; no service installed. System-mode verification lacks /run/systemd write access, so no system-startup proof is claimed.

- Rebuilt/exported the native control image from d3f90ea, image sha256:b6ae8cbf1b909af2d9e6e9e64ba7fd838a515f02142edb82aa1af6dd2da80b09; repeated the actual image smoke against the new binary, passed and verified container removal. No web layout changed. Updated the eight-page progress PDF to 86 passing Rust tests.

## 2026-11-05

- Restored the source and history from the handoff bundle into the private repository `RictalNickel/agent-rush-runtime`.
- Added a switchable inference provider (Anthropic or Vultr) and the team profile: Opus orchestrator and reviewer, Haiku coder and worker, enforced allowlists, compaction, and reserve-then-settle costing.
- Fixed docker stats sampling, which had blocked the CPU qualification check. Refusals and API errors now settle at their actual cost.
- Deployed two Vultr VMs (control and worker). The worker passed 9/9 qualification checks. Ran a live end-to-end job, a four-job load test and containment probes. Built the slide deck.
- Review pass:
  - Originality against Agent Zero: no shared text found. Removed the Agent Zero envelope decoder and aliases, and replaced `call_subordinate` with `follow_up`.
  - Code review fixes: requalification no longer restarts the worker. Review failures are advisory. A final rejection fails the job. Reviewer prompts fit the compact budget.
  - Stale documentation corrected.
- Completion pass:
  - Vultr path verified live with GLM 5.3 Flash. Pricing ($0.10/$0.35 per million tokens) read from Vultr's catalog. Per-role `reasoning_effort` fixes empty answers. Job `2fd5b26f`: 33 s, $0.003.
  - Overlapping sandboxes verified, with 429 backpressure.
  - `/api/status` went from 31 to 460 req/s (health caching, no idle worker polling).
  - Playwright 2/2 and `cargo audit` clean.
