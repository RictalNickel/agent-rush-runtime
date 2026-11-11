# Boxoro Security Stack Implementation Plan

> **For agentic workers:** Use the subagent-driven-development workflow for independent implementation and review. The user authorized execution; proceed without a separate design approval pause.

**Goal:** Deliver the native Boxoro security agent, fixed scanner runner, installable image, optional intelligence MCP, research and operating documentation.

**Architecture:** Reuse Boxoro sessions, authentication, worker boxes and external MCP. A strict native tool invokes a fixed Python helper in its box. All reports are bounded workspace artifacts; privileged intelligence is a separate optional service.

**Tech Stack:** Rust 1.90; Python 3 standard library; pinned Linux scanner binaries; gVisor workspace images; Streamable HTTP MCP.

**Spec:** `docs/superpowers/specs/2026-11-10-security-stack-design.md`

## Global Constraints

- No new Rust runtime dependencies; preserve Boxoro worker isolation and owner-scoped access.
- Helper `/opt/box/bin/security-scan`; strict JSON; default offline true; timeout 10–540 seconds.
- Actions: `doctor`, `scan`, `image`, `web`, `update_databases`.
- Profiles: `quick`, `standard`, `secrets`, `dependencies`, `sast`, `lint`, `supply_chain`, `iac`, `malware`.
- No secrets in images, model outputs, or scanner logs; no source/binary uploads.
- Missing, timed-out, stale or failed checks block a passing result.
- Repository work is on `feat/boxoro-security-stack`, based on `8715651e87137ce82babd91b2307a6eedfeff154`.

## Review Focus

1. Path escapes, malicious symlinks and special files must not expose files outside the source root.
2. Scanner configuration or environment from the repository must not alter execution or leak secrets.
3. Missing tools, nonzero exit codes, invalid output and stale databases must remain incomplete coverage.
4. Timeout and cancellation must not leave child processes running or unbounded report files.
5. Security findings and MCP responses must not disclose raw credentials or claim patch/malware guarantees.

## Task 1: Native agent integration

**Files:** `src/tools.rs`, `src/session.rs`, `src/agent_config.rs`, `src/skills.rs`, `tests/sessions.rs`, `deploy/security/SECURITY_SKILL.md`.

**Interfaces:** Decode `security_scan` to a typed call; serialize action/options into a single JSON argument for `/usr/bin/python3 -I /opt/box/bin/security-scan`. No scanner execution on control. Register security-audit skill using an embedded source asset.

- [x] Add failing tests for strict action/field/profile validation, disabled tool, unsafe target, digest-only image and shell-safe dispatch.
- [x] Implement registry, parsing, prompt documentation, box dispatch and built-in skill.
- [x] Run targeted Rust tests; report baseline/toolchain blockers.

## Task 2: Scanner runner and reports

**Files:** `deploy/box-tools/security-scan`, `deploy/security/boxoro_security/*.py`, `deploy/security/rules/*`, `qa/test_security_scan.py`.

**Interfaces:** One JSON CLI argument as described in the spec. Emit a small JSON summary to stdout and persist JSON/Markdown/SARIF plus remediation under `/workspace/.security/reports/<id>/`. Explicit status and gate, scanner coverage and no raw secrets.

- [x] Add failing behavior tests for containment, snapshot/output limits, redaction, profiles, exit codes and incomplete gates.
- [x] Implement command registry, safe snapshots/subprocesses, normalized parsers, database operations and reports.
- [x] Add original Semgrep/YARA rules, manifest checks and bounded loopback web checks.
- [x] Run Python tests and real available-scanner smoke tests.

## Task 3: Image and installation

**Files:** `deploy/security.Dockerfile`, `deploy/security/tools.lock.json`, `deploy/security/requirements.lock`, `scripts/build-security-image.sh`, supporting installer/smoke scripts.

**Interfaces:** Base workspace image digest required by build script; binaries in `/opt/security/bin`, Semgrep in `/opt/security/venv`; runner module/rules in `/opt/box/security`; cache in `/workspace/.security/cache`. Keep uid 1000 and existing runtime permissions.

- [x] Lock current release checksums and Python dependencies from official sources.
- [x] Implement build-time verified installation and optional malware layer, database seeding guidance and version manifest.
- [x] Add build/image verification with exact provenance/limitations; run what this environment supports.

## Task 4: Optional intelligence MCP and research

**Files:** `deploy/security/intel_server.py`, `deploy/security/mcp-servers.example.json`, `deploy/security/security-intel.service`, `qa/test_security_intel.py`, `docs/SECURITY_STACK.md`, `docs/SECURITY_STACK_RESEARCH.md`.

**Interfaces:** Authenticated loopback Streamable HTTP, strict fixed tools, bounded upstream fetch with fixed HTTPS hosts and no redirects. Credentials remain server-side. Compatible with current Boxoro MCP negotiation.

- [x] Add failing tests for auth, strict schemas, non-disclosure and upstream failures.
- [x] Implement constrained optional bridge and configuration/service examples.
- [x] Record primary-source research, tool/coverage/license/resource matrices, API privacy, DAST and remediation procedures.
- [x] Run bridge tests and document no-key versus configured-service coverage.

## Task 5: Integration review and delivery

**Files:** README/API/configuration/installation docs and CI dispatch checks as needed.

- [x] Run full Rust suite, fmt/clippy, Python suite and meaningful smoke scans (one documented /tmp environment exclusion; see validation record).
- [x] Review the complete diff independently; fix important findings and retest.
- [ ] Commit cohesive changes, push a feature branch and create a draft pull request if connector permissions allow; do not merge or deploy.
- [ ] Report verified results, unresolved infrastructure requirements and startup steps.

## Deployment status

Implementation and host-level checks are complete; image build, YARA compilation, live database coverage and gVisor qualification require the documented build/worker environment. See `docs/SECURITY_STACK_VALIDATION.md`. No image digest or deployment success is fabricated.
