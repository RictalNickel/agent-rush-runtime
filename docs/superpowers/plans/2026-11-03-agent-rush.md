# Agent Rush Implementation Plan

## Binding amendment from the user

All platform implementation is now Rust. Agent Zero has been downloaded for source reference only; do not fork it or depend on its Python runtime. The original Python/React file paths below are superseded by Rust modules `src/{config,domain,ledger,auth,api,worker,inference,agent,deploy}.rs`, `tests/`, and Rust-served `web/` assets. Use Cargo.lock and cargo test. Preserve task behavior, trust boundaries and acceptance criteria. Execute inline without additional agent delegation. Browser HTML/CSS/JavaScript and deployment shell/config are supporting assets, not alternate backend implementations.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver an authenticated agent execution product with measured disposable gVisor execution, an evidence ledger and bounded Vultr deployment.

**Architecture:** Python control API and durable ledger own scheduling and evidence. A private authenticated worker launches pinned containers; Agent Zero can request only approved broker tools. The frontend exposes actual events and receipts.

**Tech Stack:** Python 3.12, FastAPI, SQLite, Docker/gVisor, React/TypeScript/Vite, pytest and Playwright.

**Spec:** docs/superpowers/specs/2026-11-03-agent-rush-design.md (approved by the user in chat).

## Global Constraints

- Private repository; no public GitHub publication without authorization.
- $300 ceiling; reserve $50; local-first checks; no GPU or autoscaling.
- Temporary deployment checks last at most two hours and clean up afterwards; idle VMs must not be left running.
- Generated code never executes in the controller or node-manager process.
- No secret, socket, host path or unrestricted runtime flags in task requests.
- No weaker-runtime fallback for gVisor or nesting failures.
- Real checks must distinguish executed, failed, skipped and unavailable results.
- Use the existing isolated checkout; no worktree required.

## Review Focus

- Duplicate requests and concurrent admission cannot create duplicate workers or overcommit memory (Task 1).
- Restart/cancel/disconnect cannot release capacity before cleanup or leave active sandboxes (Tasks 1–2).
- Malicious artifact names, symlinks and oversized output cannot read host files or fill its disk (Task 2).
- Model tool names, arguments and indirect subordinate calls cannot bypass policy or choose another provider (Task 3).
- Expired sessions, cross-owner IDs and hostile rendered output cannot expose another user's evidence (Tasks 1, 4).

### Task 1: Broker and authenticated durable API

**Files:** `backend/agent_rush/{config,schemas,ledger,auth,api,service}.py`, `backend/tests/test_api.py`, `pyproject.toml`, `requirements*.txt`.
**Interfaces:** `Ledger.create_job(owner, request, key)`, transactional lease admission, persisted events; FastAPI `/api/jobs`, `/api/jobs/{id}`, cancellation and evidence. Worker client has submit/status/cancel semantics with bounded requests.
- [ ] Write failing API tests for auth/CSRF, validation, cross-owner visibility, idempotency conflict and concurrent memory admission.
- [ ] Run the targeted tests and confirm failure before implementation.
- [ ] Implement SQLite WAL transactions, explicit states, conservative limits and signed sessions.
- [ ] Run tests and commit the working broker.

### Task 2: Restricted worker and actual integration

**Files:** `backend/agent_rush/{worker,runner,artifacts}.py`, `backend/tests/test_worker.py`, `backend/tests/test_sandbox_integration.py`, `sandbox/Dockerfile`.
**Interfaces:** `DockerRunner.run(job_id, code, limits)` returns a measured receipt with cleanup state; worker endpoint accepts fixed typed execution requests authenticated by mTLS at ingress plus service token.
- [ ] Write failing tests for rejected configuration, output caps, timeout, artifact traversal/symlinks, secret exclusion and cleanup failures.
- [ ] Confirm they fail; implement non-root/runsc/network-none/read-only fixed-profile execution with deadlines and recovery.
- [ ] Build digest-pinned sandbox image; run real output, artifact, timeout, filesystem, network and memory tests. Do not pass host-enforcement qualification if the managed Docker cgroup setup cannot enforce limits.
- [ ] Commit runner, tests and exact integration results.

### Task 3: Vultr inference and Agent Zero integration

**Files:** `backend/agent_rush/{inference,orchestrator,agent_zero}.py`, `integrations/agent_zero/`, `backend/tests/test_inference.py`, `backend/tests/test_agent_zero.py`, third-party notices.
**Interfaces:** bounded gateway permits only configured Vultr origin/model; controller calls broker for sandbox execution and consumes worker evidence without inventing verification.
- [ ] Write failing tests for provider restriction, malformed tool output, maximum attempts/tokens, secret redaction and disallowed tools.
- [ ] Implement gateway and restricted upstream controller with subordinate identity propagation and default-deny tool policy.
- [ ] Exercise real upstream loop locally with test transport, keeping mocked model acceptance distinct from real inference acceptance.
- [ ] With secure credentials configured, perform one actual Vultr inference roundtrip and record sanitized metadata; otherwise report external blocker.

### Task 4: Product UI and end-to-end checks

**Files:** `frontend/src/`, frontend package/lockfile, `frontend/tests/`, UI reference artifact.
**Interfaces:** same authenticated API; events and evidence originate from persisted jobs. No simulated active jobs or measurements.
- [ ] Create and inspect a visual concept; implement responsive task workspace, profile availability, real job history, evidence/artifacts and failure/cancel flows.
- [ ] Run production build, type checks and browser checks for login, task submit, error states, artifact download and small screens.
- [ ] Commit UI and screenshots of observed behavior.

### Task 5: Reproducible Vultr deploy and bounded teardown

**Files:** `deploy/`, `scripts/`, `docs/{DEPLOYMENT,SECURITY,VERIFICATION,DEMO}.md`.
**Interfaces:** trusted deployment tooling reads credentials from secure runtime bindings, queries actual prices, creates only tagged project resources, records costs/IDs and tears them down.
- [ ] Test spend/lifetime policy, inventory isolation and teardown selection with fixtures before paid operations.
- [ ] Create reproducible control/worker config, private authenticated routing, HTTPS and service recovery; test local configuration.
- [ ] Verify GitHub creation/write access and private visibility before push. Create the requested private repository if access supports it; never publish to an unverified-visibility remote.
- [ ] Once credential/network prerequisites work, query actual Vultr plans, enforce projected budget, provision only for the bounded acceptance test, run end-to-end/security checks, retain evidence and tear down temporary resources.
- [ ] Attempt nested Docker qualification only after required direct execution passes, recording unsupported results honestly.

### Task 6: Review and delivery

**Files:** `README.md`, `docs/`, `presentation/`, verification report and source notices.
- [ ] Run focused backend, real-container and frontend/browser suites. Record underlying commands and counts, with skips separated.
- [ ] Review authority boundaries, spending/cleanup behavior and evidence claims independently; resolve issues and rerun affected checks.
- [ ] Produce slide PDF and actual containment demo assets where capture is available; provide exact gaps otherwise.
- [ ] Save tested reusable installation/startup configuration, report repo commit, deployed/removed resources and costs, feature matrix, tests, benchmarks, demo assets and remaining blockers.
