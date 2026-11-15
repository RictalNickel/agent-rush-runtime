# Boxoro Specialist Agents Implementation Plan

> **For agentic workers:** Use the parallel task ownership below; preserve unrelated files and run meaningful tests before reporting completion.

**Goal:** Deliver all 21 new specialists, extensive installable toolchains, native Boxoro integration, OpenBB finance resources, documentation and GitHub publication.

**Architecture:** Trusted catalog plus bounded workspace runner, embedded specialist skills, shared image families, separate control-side finance MCP. Existing Boxoro worker and session isolation remain the execution boundary.

**Tech Stack:** Rust 1.90, Python 3.12 standard-library runner, locked Python/Node packages, verified native tools, OpenBB V5, MCP 2025-06-18 compatibility.

**Spec:** `docs/superpowers/specs/2026-11-14-specialist-agents-design.md`

## Global Constraints

Follow the specification's binding requirements and exact catalog/request fields. Do not install credentials, execute financial trades, deploy production infrastructure, or claim unexecuted image/provider tests.

## Review Focus

- Cross-specialist operation bypass and unknown parameters.
- Untrusted paths, symlinks, project config and environment injection.
- Failed/missing tools, timeouts, process cleanup and excessive output.
- OpenBB command metadata, POST analytics, secret handling and rate/size limits.
- Install-version drift, missing executables and image-family composition.

## Task 1: Research and catalog — tooling_catalog

- [x] Verify official tool/MCP sources and package/license requirements.
- [x] Implement all 21 catalog profiles, broad typed command recipes, skills and per-agent operating guides.
- [x] Validate catalog membership, workflows, references and representative command syntax.

## Task 2: Workspace runner — runtime_design

- [x] Add regression tests for the contract and trust boundaries.
- [x] Implement catalog validation, list/describe/doctor/run/workflow, bounded subprocesses and reports.
- [x] Implement useful built-in local analyses and adapters where a raw CLI is insufficient; coordinate catalog IDs.
- [x] Run real local fixtures and tests; report limits accurately.

## Task 3: Finance — finance_research

- [x] Research current OpenBB resources, schema, provider configuration and license changes.
- [x] Implement local financial analytics plus schema-driven control-side OpenBB MCP and service examples.
- [x] Resolve finance dependencies and tests for calculations, schema dispatch, policy, authentication and evidence.
- [x] Write finance operating and provider guides.

## Task 4: Native integration and image tooling — root

- [x] Add strict Rust specialist tool, owner specialist preference, prompt/skill registry and integration tests.
- [x] Implement package lock generation, image installation, family/suite Docker targets, build and smoke scripts.
- [x] Add compatibility and dependency evidence; preserve old security integration.

## Task 5: Full integration and delivery — root + independent review

- [x] Review cross-component correctness and repair findings.
- [x] Run Rust/Python/catalog/dependency/real-tool checks available here.
- [x] Document all executed validation and deployment prerequisites.
- [x] Commit, upload all source/locks/docs/tests and open a draft PR; verify remote tree.

## Progress ledger

- Baseline: security branch `4756e5eae47a973d3f9b78bb6ae3021e5cda4e33`; default work branch `8715651e87137ce82babd91b2307a6eedfeff154`. New isolated checkout on `feat/boxoro-specialist-agents`.
- Ruling: proceed without another design approval because the user explicitly requested full implementation and the developer instructs autonomous completion.
- Ruling: use shared image families with explicit host integrations; preserve one-image-per-worker routing to avoid inventing unimplemented scheduling.

- Implemented: 21 profiles,167 tools,151 operations,10 connector records; native tool/API/MCP/config/skills and chat picker.
- Qualified: actual locked Python/npm/native CLI installations and 87 readiness probes; targeted positive/negative scanner/test/report fixtures; all-provider OpenBB discovery and real offline analytics.
- Reviewed: runtime/native/installer/finance/catalog reviews repaired concrete defects; reviewed dependency overrides patched high npm advisory chains. BackstopJS and Checkov are optional with retained advisories.
- Not executed here: Docker/BuildKit image builds, gVisor host qualification, paid/live provider queries, device/emulator and cloud/database account setup. These remain explicit deployment gates rather than completion claims.
- Integration ruling: upload the authorized feature branch and draft PR against the existing security feature branch; preserve the checkout and do not merge or deploy an unqualified image.
- Published: [draft PR #3](https://github.com/RictalNickel/agent-rush-runtime/pull/3), stacked on security PR #2. The 312-file implementation commit `8734f52808f85095b265e1072e76b3a7011ae6ca` has tree `6d3505242711955bc01053466f7a9839393736ac`, exactly matching the verified local implementation. GitHub confirms the expected head/base and a clean merge; deployment gates above remain outstanding.
