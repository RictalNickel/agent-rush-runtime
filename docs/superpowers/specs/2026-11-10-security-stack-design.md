# Boxoro Security Stack design

## Intent

Build a researched, usable application-security agent inside Boxoro. It must detect exposed secrets, vulnerable dependencies, suspicious supply-chain patterns, insecure code and configuration, lint defects, and malware indicators; produce evidence and remediation plans; and support rechecking fixes. It must preserve Boxoro's existing gVisor, owner isolation, credential separation, resource limits, and qualification requirements.

The user explicitly authorized research and implementation. Implementation choices below are engineering decisions, not additional user requirements. No production deployment or external target scanning is implied by this change.

## Existing architecture and selected approach

At baseline `8715651e87137ce82babd91b2307a6eedfeff154`, the Rust control plane decodes built-in tools in `src/tools.rs`, executes them through `src/session.rs`, exposes sessions over REST and MCP, and indexes built-in skills in `src/skills.rs`. Workspace helpers live in `deploy/box-tools`; the worker selects a single immutable workspace image with `AR_BOX_IMAGE`. Boxes run as uid 1000 with a read-only root and writable `/workspace`. Exec requests have a 600-second ceiling. Remote MCP servers are operator-registered HTTP services called by the control plane.

Alternatives considered: a prompt-only custom tool would lack reliable coverage and output semantics; a standalone scanner service would duplicate authentication/job ownership and move source outside the box. Select a native `security_scan` tool, a built-in security skill, and an optional extended workspace image, using the existing session lifecycle and external MCP interface. Keep intelligence APIs in an optional narrowly scoped control-side MCP bridge, not inside workspace credentials.

## Components

1. **Native tool**: strict `security_scan` envelope. Actions `doctor`, `scan`, `image`, `web`, `update_databases`. Optional `path`, `profile`, `image`, `target`, `offline`, `timeout_seconds`. Profiles `quick`, `standard`, `secrets`, `dependencies`, `sast`, `lint`, `supply_chain`, `iac`, `malware`. Default offline true and timeout 540 seconds (range 10–540); worker exec allows 30 seconds overhead. `scan` defaults to `.` and `standard`; `image` requires a digest reference; `web` accepts only a literal loopback HTTP origin with an explicit port. Reject irrelevant fields. The control plane calls `/usr/bin/python3 -I /opt/box/bin/security-scan` with one shell-quoted JSON argument; it never executes a scanner itself.
2. **Runner**: Python standard library, fixed executable paths and argument arrays, scrubbed environment, no caller-defined commands or scanner flags. Validate all inputs again. Copy regular source files to a bounded private snapshot, skip symlinks and special files visibly, never run repository scripts, and keep reports outside the scanned tree. Sequential subprocesses have wall-clock, output, and process-group limits. All external scanners use trusted local configuration. Offline is best-effort scanner configuration, not a replacement for the worker's network controls.
3. **Coverage**: Gitleaks redacted secret detection; Trivy dependency/IaC/image scans; Syft CycloneDX inventory; Semgrep CE with original local rules; Ruff, ShellCheck and Hadolint linting; ClamAV and original YARA rules in the malware profile; built-in manifest/workflow risk analysis and local HTTP-header checks. Profiles disclose exactly what was selected and skipped. Optional tools and advanced DAST integrations are documented separately.
4. **Results**: JSON, Markdown and SARIF reports with normalized severity, rule/scanner, location, package/current/fixed versions, source references, fingerprints, source snapshot identity, scanner/database evidence, coverage, and errors. Strip raw secret values, code snippets and untrusted scanner output from normalized reports. Missing, stale, timed-out, malformed or truncated checks cannot produce a passing gate. Provide a prioritized remediation plan and explicit limitations. No report promises absence of malware or compromise.
5. **Image**: derive a security image from the existing workspace digest. Download pinned release artifacts with recorded SHA-256 checks; lock Python scanner dependencies; original rules avoid redistributing restricted Semgrep registry rules. Do not embed API keys, mount a container-engine socket or change worker containment. Database updates are explicit, bounded operations with freshness metadata. Document memory/disk requirements, package versions and licenses. Build script emits the actual image digest; never invent one or claim an unbuilt image is published.
6. **Intelligence MCP**: optional authenticated loopback HTTP bridge, fixed upstream URLs and allowlisted tools for CVE enrichment, hash-only malware reputation, and privacy-preserving password-prefix checks. Credential-bearing services are opt-in and use control-side environment variables. Never submit source, binaries or plaintext secrets. No unrestricted proxy, upload, command, shell, or arbitrary URL tool. Include a compatible `AR_MCP_SERVERS_FILE` example and tests.
7. **Remediation**: the agent inspects findings, proposes minimal dependency/code changes, applies authorized fixes in an isolated work copy, runs project checks there, and rescans. It never executes lifecycle scripts or dependency resolution in the control plane. Missing tests and unverified fixes remain explicit. Production changes use the existing operator workflow.

## Scope and constraints

- Core scanning needs no paid service or new Rust dependency.
- Default scans are read-only with respect to source and do not verify credentials against providers.
- External DAST requires a separately approved target scope. The built-in web check is loopback-only, does not crawl, follow redirects, or send attack payloads.
- Git-history coverage must be distinguished from working-tree coverage; a shallow checkout cannot represent full history.
- The general Boxoro `bash` tool is still available. The runner's restrictions are defense in depth, not a new isolation boundary against an agent that can run arbitrary commands inside its box.
- Current worker disk defaults are insufficient for reliable full malware/signature operation; use a larger dedicated security worker and sequential scans. Do not silently widen worker limits.
- Database caches are per-session, with no cross-owner writable cache.
- All unsupported ecosystems and rule-pack limitations are visible; ASVS/business-logic review cannot be replaced by scanners.

## Verification

Test strict decoding/disabled-tool handling and box dispatch; snapshot path/symlink containment and source preservation; secret redaction; subprocess timeouts/output limits; scanner exit-code semantics; stale/missing databases; malformed findings; offline command construction; SARIF structure; HTTP redirect/target limits; MCP authentication/argument/privacy restrictions; and report gates. Run the existing Rust suite and clippy, Python unit tests, helper smoke tests, and container build/qualification when the environment supports it. Record exact blockers instead of claiming unavailable tests passed.
