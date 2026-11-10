# Debian security workspace for Boxoro (prototype)

This feature branch adds an **optional security-focused persistent session image**, a native Boxoro `SKILL.md`, a CLI scan dispatcher, and a broad capability catalog. It does not replace the original one-shot Rust compiler sandbox, change the live worker, or install anything on Vultr.

## Scope and coverage

The catalog at [catalog.json](catalog.json) maps **21 capability groups** to representative tools for source analysis, linters, transitive dependency vulnerabilities, SBOM and provenance, credential exposure, malware, binary analysis, fuzzing, web/API tests, TLS, IAM/cloud, containers/Kubernetes, digital forensics and AI-agent security.

**Catalog entry does not mean installed, enabled, verified, safe to execute, or guaranteed detection.** An industry-wide “90% coverage” claim cannot be justified just by counting tools. Measure coverage by threat class, supported target language, test fixtures, detected seeded findings, false positives, and reproducibility. For each scan, log skipped and unavailable checks.

## Build

From a trusted checkout of this branch, with Docker installed:

```bash
docker build --platform linux/amd64 -f deploy/security-workspace.Dockerfile -t agent-rush-security:local .
docker image inspect agent-rush-security:local --format '{{.Id}}'
```

The image uses the digest-pinned Rust 1.90 Debian Bookworm base already specified in `deploy/images.lock.json`; build dependencies currently use version ranges. Before production, pin them by immutable artifacts/hashes, scan the image, generate an SBOM, and review licenses. This prototype has **not** been built or executed on the intended worker.

## Integration with the runtime

The existing worker's `AR_BOX_IMAGE` is a **single image configured per worker**, with qualification records tied to that image digest. For a separate security-capable worker, build and reference the new image using an immutable local digest, then run the current `agent-rush qualify-box` on the intended worker, and only start sessions when the qualification and egress rules pass.

Do **not** merely change `AR_BOX_IMAGE` in a running general worker: you must preserve existing sessions and rerun qualification. Running a general workspace and a security workspace on the same worker requires an additional trusted, validated per-session image-selection feature; this branch does **not** implement it. Assign a dedicated worker/deployment for the security profile for now.

The native skill can be registered through `POST /v1/skills` (body `{markdown: <SKILL.md text>}`) or the MCP `manage_skills` tool. Skills describe agent behavior; installing a skill alone does not add binaries or change containment permissions.

## Run a local scan

Inside a prepared security workspace:

```bash
security-scan /workspace/project --profile fast
security-scan /workspace/project --profile all --output /workspace/security-results/report.json
```

Only local source directories below `/workspace` are accepted. The dispatcher records tool exit status, bounded output, tool-unavailable skips, and timeouts, with results in a file mode 0600. It is **offline-first**, not a guarantee of zero networking: some third-party tools may check remote services unless separately configured. Use a network-disabled environment for strictly offline analysis; run signature/database updates in a reviewed build job, not as an agent-side action.

The current Dockerfile includes basic Python/SAST, C/C++, shell, file, YARA and ClamAV tooling. The catalog includes many optional tools that are **not installed**; the CLI marks them unavailable. Some scanners need licensed rules, databases, access to external providers, a larger memory allowance, or a dedicated VM. ClamAV without an updated signature database is incomplete.

## Safety boundaries

- The worker should still use gVisor, UID 1000, read-only root, no host socket, dropped capabilities, no-new-privileges, bounded cgroups, quota and current network guard.
- Only scan files and services belonging to the user or with documented authorization. No active internet reconnaissance in the default profile.
- Actual credential breach monitoring requires explicit authorized integrations and never transmitting recovered credential values to third-party services.
- Suspected malware static analysis may run in this workspace; dynamic execution requires a dedicated offline analysis VM. Ordinary gVisor boxes allow public egress, so they are unsuitable for running unknown malware.
- Scanner output may contain secrets; inspect and redact locally before sharing outside the worker. The current prototype does not guarantee perfect redaction.

## Validation before merge

1. Run Python unit tests and syntax checks for `security/bin/security-scan`.
2. Build and inspect the security image with reviewed dependency locks and SBOM.
3. Test each installed tool's CLI and JSON parsing against fixtures with known results.
4. Verify no output contains fixture secrets, test output caps, path traversal, symlink escapes, timeouts and missing tools.
5. Re-run **all** existing gVisor worker containment/qualification checks on the real host and network, including credentials, egress, quotas, disk and cleanup.
6. Confirm the new image doesn't unintentionally replace the existing general-purpose workspace or alter its default behavior.
7. Record actual coverage for each tool/target rather than estimating “90%”.

This is a broad extensible foundation, not a production-ready vulnerability assessment product.
