# Boxoro Security Stack

Boxoro has a native **`security_scan` tool** and a built-in **`security-audit` skill**. They use the existing session box, owner isolation, file downloads, cancellation and REST/MCP interfaces. The optional security workspace image installs the pinned scanners. An optional control-side intelligence MCP adds CVE context, advisory lookup, password-prefix lookup and existing artifact reputation.

The scanner is a working analysis and reporting tool. It does not automatically change a project, execute its installation scripts, verify suspected API keys, or claim a clean scan proves an application secure. The agent skill supplies the review → authorized patch → tests → rescan workflow using Boxoro's existing editing and shell tools.

## Capabilities

| Area | Implemented tool | What the result establishes |
| --- | --- | --- |
| Exposed credentials | Gitleaks, with full redaction and trusted configuration | Potential secrets in the current working tree; not proof of theft or current validity |
| Vulnerable dependencies | Trivy filesystem analysis and current local advisory database | Known package/version vulnerabilities and reported fixed versions |
| Container vulnerabilities | Trivy remote registry scan of an immutable digest | Findings for the downloaded public image; no host Docker socket |
| SBOM and license inventory | Syft → sanitized CycloneDX 1.6 | Package names, versions, package URLs and available SPDX license IDs |
| Supply-chain review | Static manifest/workflow checks | Lifecycle scripts, mutable Git dependencies/actions, HTTP package resolution, missing lockfiles and Python startup code needing review |
| SAST | Semgrep CE, six original MIT-licensed rules | Narrow Python/JavaScript/TypeScript patterns; no claim of full framework or cross-file coverage |
| Lint | Ruff, ShellCheck, Hadolint, pinned Rustfmt | Python, shell, Dockerfile and Rust formatting checks without running project plugins/build scripts |
| Malware indicators | Optional ClamAV and YARA image target | Antivirus signatures and reviewed static indicators; neither establishes absence of malware |
| Local web checks | One bounded GET to a literal loopback origin | Security headers and cookie attributes for one response; no crawl, redirects or attack payloads |
| Prioritization | Optional intelligence MCP | CISA KEV, FIRST EPSS, OSV advisories; evidence is dated and can be unavailable/stale |
| Breached password screening | Optional HIBP range lookup | Hash suffixes/counts for a five-character SHA-1 prefix, compared locally; no plaintext password accepted |
| Artifact reputation | Optional VirusTotal existing-report lookup | Existing report by SHA-256 only; no upload, detonation or rescan |

Full tool/version/license decisions, primary sources and alternatives are in [SECURITY_STACK_RESEARCH.md](SECURITY_STACK_RESEARCH.md). Configure intelligence with [SECURITY_INTELLIGENCE.md](SECURITY_INTELLIGENCE.md).

## Build the workspace image

Supported deployment architecture is **Linux amd64**, matching the existing workspace's Rust toolchain. The lock records other available upstream architectures where useful; this does not make the current Boxoro base an arm64 image.

On the worker/build host with Docker and BuildKit:

```sh
# Builds the existing workspace first, then installs the core scanner stack.
scripts/build-security-image.sh --target core --tag boxoro-security:local

# Or extend an already reviewed workspace by its actual immutable reference.
scripts/build-security-image.sh \
  --base "$REVIEWED_WORKSPACE_IMAGE" \
  --target core --tag boxoro-security:local

# Larger image with ClamAV and YARA.
scripts/build-security-image.sh \
  --base "$REVIEWED_WORKSPACE_IMAGE" \
  --target malware --tag boxoro-security:malware
```

`REVIEWED_WORKSPACE_IMAGE` must be an existing local `sha256:<image ID>` or `registry/repository@sha256:<manifest digest>`. The script rejects mutable base tags supplied by callers. Without `--base`, it builds the existing digest-pinned workspace recipe locally and checks its actual image ID before and after the derived build.

The build performs checksum and size verification for downloaded scanner releases, installs Semgrep from a transitive hashed wheel lock, preserves licenses, and records versions. YARA is built from an exact source commit in a disposable build stage. Rustfmt uses the actual Rust 1.90 toolchain binary, so repository `rust-toolchain.toml` files cannot select another executable. No credential or mutable advisory/signature database is baked into the image.

The default smoke test runs non-root with a read-only root, no capabilities, no-new-privileges, resource limits and **no network**. It verifies executable versions, real synthetic-secret detection/redaction, and that a missing advisory database produces an incomplete result. Set `BOXORO_SMOKE_RUNTIME=runsc` when testing on the real gVisor worker. This smoke test does not replace Boxoro's host qualification.

The build writes `security-image-build.json`, including the actual local `image_id`, observed `repo_digests`, source commit/dirty state, base identity and lock hashes. **A Docker image ID is a configuration digest; it is not a registry manifest digest.** Use the exact local image ID on the Docker host where it was built. For registry distribution, publish through your existing release process, verify signatures/provenance, and use the actual returned `RepoDigest`. No image is published by the script.

If a managed build environment needs an additional proxy CA, `BOXORO_BUILD_CA_FILE` passes it as a transient BuildKit secret and requires an explicit `--base`; prepare the existing workspace base separately because that Dockerfile does not consume the secret. TLS verification remains enabled. See [INSTALL.md](INSTALL.md) for the base worker prerequisites. Digest pinning controls what is selected; it does not make apt repositories or the full base build bit-for-bit reproducible.

## Activate on a Boxoro worker

Configure the worker's existing `/etc/agent-rush/worker.env` through your deployment process:

```dotenv
AR_BOX_IMAGE=sha256:<actual-local-image-id-from-build-output>
# Suggested dedicated security worker ceilings, subject to actual host capacity:
AR_BOX_CEILING_MB=8192
AR_BOX_DISK_MB=12288
```

These values are starting budgets, not measured guarantees. The core can run within smaller limits on small projects; malware signatures, Java advisory data and installed-dependency scans need more space. ClamAV alone recommends substantial memory/disk resources. Match `AR_CAPACITY_MB`, concurrency and physical host memory to the selected profile, and keep the existing admission checks. Never silently raise a running deployment's limits.

Run the existing box network and qualification procedures with this image, then restart the worker using the established deployment instructions. Use fresh sessions to verify the new image and correct qualification evidence. A worker currently has one `AR_BOX_IMAGE`; this change does **not** introduce per-session image selection or a second unreviewed orchestration path. A separate security worker/deployment is useful when its resources should differ from general-purpose boxes.

The updated base workspace also contains the helper and rules. On a base image without scanner binaries, `doctor` identifies the missing tools and scans remain incomplete.

## Use the agent

In Boxoro chat, or as a session message sent through REST/MCP:

> Load the security-audit skill. Audit `/workspace/app` using the standard profile, report coverage and evidence, then investigate high-priority findings. Prepare minimal fixes in a separate work copy, run the project's required tests and rescan. Keep raw credentials out of reports.

The agent first calls:

```json
{"tool":"skill","action":"load","name":"security-audit"}
```

Then it can use these exact tool envelopes:

```json
{"tool":"security_scan","action":"doctor"}
```

```json
{"tool":"security_scan","action":"scan","path":"app","profile":"standard","offline":true,"timeout_seconds":540}
```

```json
{"tool":"security_scan","action":"update_databases","offline":false,"timeout_seconds":540}
```

```json
{"tool":"security_scan","action":"web","target":"http://127.0.0.1:3000","timeout_seconds":30}
```

Image scanning requires an actual `registry/repository@sha256:<64 lowercase hex characters>` and `offline:false`; it pulls that immutable public image directly from a registry. It never mounts Docker/containerd sockets or receives private registry credentials.

### Request contract

| Action | Accepted fields beyond `action` | Behavior |
| --- | --- | --- |
| `doctor` | `timeout_seconds` | File/executable availability, expected lock, installed version manifest, visible resource limits and database status |
| `scan` | `path`, `profile`, `offline`, `timeout_seconds` | Read-only source analysis via a private bounded snapshot |
| `image` | `image`, `offline`, `timeout_seconds` | Immutable public registry image vulnerability scan |
| `web` | `target`, `timeout_seconds` | One HTTP response from `127.0.0.1` or `[::1]` at an explicit port |
| `update_databases` | `offline:false`, `timeout_seconds` | Explicit public Trivy/Java DB and optional ClamAV signature downloads |

Defaults: `path:"."`, `profile:"standard"`, `offline:true`, `timeout_seconds:540`. Timeout range is **10–540 seconds**, with worker overhead still below the existing 600-second exec ceiling. Unknown/duplicate fields, fields irrelevant to the action, arbitrary flags/commands, mutable image tags and external web targets are rejected. A `null` optional field means absent. The web URL cannot contain credentials, a non-root path, query or fragment.

### Profiles

| Profile | Checks |
| --- | --- |
| `quick` | Gitleaks, manifest/workflow heuristics |
| `standard` | Secrets, dependency vulnerabilities, IaC, SAST, SBOM, supply-chain heuristics and applicable linters |
| `secrets` | Gitleaks working-tree scan |
| `dependencies` | Trivy dependency vulnerabilities, Syft SBOM |
| `sast` | Original Semgrep rules |
| `lint` | Applicable Ruff, ShellCheck, Hadolint and Rustfmt checks |
| `supply_chain` | SBOM, manifest/workflow heuristics, dependency vulnerabilities |
| `iac` | Trivy misconfiguration analysis, Dockerfile lint |
| `malware` | ClamAV, YARA and manifest heuristics; includes installed dependency directories within snapshot limits |

Scanners run sequentially against a shared deadline. For a large repository, scan bounded subprojects/profiles in separate calls. A deadline-exhausted check is recorded as such. No background job is claimed to have completed after the caller returned.

## Databases, network and privacy

Run `update_databases` explicitly before dependency/malware scans. Caches are under `/workspace/.security/cache`, belong to this session and survive parking because they are in `/workspace`. They are not writable shared caches between owners. Missing or older-than-24-hours main Trivy/ClamAV metadata blocks a passing result. Java archive coverage additionally requires the real Java database, schema 1, a valid published `NextUpdate`, and a build age of at most 72 hours. Registry image scans require both Trivy databases because their contents are not known beforehand. ClamAV freshness comes from the daily signature header's build timestamp. Download/copy times alone cannot make old content fresh.

The updater checks the resulting artifacts and metadata after a successful process exit; exit code zero without usable databases is incomplete. A cache preflight refuses symlinks, hard-linked files, special files and foreign-owned entries before invoking a scanner. Database downloads need ordinary public DNS/HTTPS connectivity from the worker. Sanitized scanner environments do not inherit ambient proxy variables or credentials; configure any required egress and trusted CA at the worker infrastructure layer.

Default scans do not update databases, fetch registry Semgrep rules, send scanner telemetry, validate secrets with providers, or execute project lifecycle scripts. The trusted Syft configuration disables network/package-license enrichment. `offline:false` on a source scan may permit scanner metadata resolution; it still does not implicitly refresh advisory databases. Explicit image scans download the requested digest; explicit database updates download public feeds.

Boxoro's existing egress filter blocks private/metadata/host networks but permits public internet. Scanner offline flags and fixed arguments are **defense in depth, not a network firewall**. A stricter no-egress job policy must be enforced by the worker/network layer. The general-purpose `bash` tool remains available, so these wrapper restrictions are not a new isolation boundary against an agent with arbitrary shell access in its own box.

No intelligence credentials enter the box. The optional MCP service runs as a separate low-privilege process on the control host, accepts only fixed identifier-based operations and has its own auth/concurrency/time/output bounds. It does not inherit gVisor's worker egress protection. See [SECURITY_INTELLIGENCE.md](SECURITY_INTELLIGENCE.md) for that trust boundary and exact upstream destinations.

## Evidence and report semantics

Each scan saves these files in `/workspace/.security/reports/<random-id>/`:

- `report.json`: normalized findings, coverage, source snapshot hash, rule hashes, expected tool manifest hash, installed tool versions when available, timestamps and database status.
- `report.md`: readable coverage and findings.
- `report.sarif`: SARIF 2.1.0 with stable fingerprints and unsuccessful invocation metadata for incomplete scans.
- `remediation.md`: prioritized actions and verification requirements.
- `sbom.cdx.json`: sanitized CycloneDX inventory when the SBOM check completes.

The small tool response gives relative paths. Use existing `read_file`, `reply.attachments`, REST file downloads or MCP file tools. Download needed reports before closing the session; existing retention/deletion rules still apply. Reports contain sensitive security metadata even though raw secrets and source snippets are excluded; share them only with the intended project owners.

`gate` is:

- **`pass`**: selected checks completed or were inapplicable, the snapshot had no unplanned coverage gaps, and no high/critical findings were found.
- **`fail`**: selected coverage completed and at least one high/critical finding exists.
- **`incomplete`**: a selected scanner/database was missing, stale, failed, timed out, returned invalid/oversized output, honored an inline suppression, returned an unknown severity, or the source snapshot had an unplanned gap. Findings found before the failure remain in the report.

A tool invocation may exit successfully while its report gate is `fail` or `incomplete`. **CI must inspect `gate`, not only the helper's process exit code.** A pass applies to the selected profile's limited coverage, not to the application's overall security. Medium/low findings remain visible even if the gate passes.

The snapshot allows at most 20,000 regular files, 25 MiB per file and 256 MiB total. It uses descriptor-based reads, never follows source symlinks or reads FIFOs/devices, and records skipped files. Planned generated/dependency directory exclusions are listed; malware includes installed dependency directories but still has limits. Repository `.gitleaksignore`, `.semgrepignore` and Trivy ignore files are omitted from the scan copy. Gitleaks, Semgrep, Ruff and Hadolint receive explicit ignore-disabling options. ShellCheck, Trivy/tfsec and Rustfmt inline suppression directives are surfaced as incomplete coverage rather than silently accepted or rewritten. Source files are never modified.

The combined finding list is limited to 10,000 findings and an 8 MiB normalized payload, retaining severity-prioritized findings when the byte limit is reached. A limit breach is incomplete coverage. Sanitized SBOM components have a separate 8 MiB payload limit. These bounds keep emitted artifacts within Boxoro's 25 MiB download limit. Scanner stdout/stderr and output files are also bounded. TERM/INT/HUP cancellation reaps the scanner process group before unwinding; abrupt SIGKILL still requires the box lifecycle to terminate remaining processes. Temporary raw scanner outputs and source copies are removed before a successful tool response.

The working-tree scanner intentionally excludes Git history. A separate full-history review must use a complete clone, an independently reviewed invocation and the same credential-free isolation. A shallow clone is insufficient evidence of historical secret absence. Package SBOMs and signature databases also have ecosystem/format limits; encrypted archives, unsupported languages, runtime-only payloads and business-logic defects need other evidence.

## Patch and verify

The security agent uses existing Boxoro tools to carry out authorized remediation:

1. Inspect the exact finding, affected versions, advisory and exposure. Enrich CVEs with KEV/EPSS when configured; keep forecast, severity and observed exploitation distinct.
2. Retain the baseline report and prepare a separate work copy. Do not run dependency scripts from unknown source with credentials.
3. Select the smallest supported fixed version or code change. Update the manifest **and the resolved lockfile**. Review transitive changes, licenses and new lifecycle scripts.
4. Run project-defined lint/type/build/tests in the isolated work copy after inspecting their commands. Clippy, npm scripts, build.rs, setup.py and project ESLint plugins can execute code; they are not part of read-only scanning.
5. Add an appropriate regression check for the vulnerability, rescan with the same profile and fresh data, and compare coverage. Report unexecuted checks.
6. Present a reviewable diff, evidence and compatibility risks. Follow the existing protected-branch/deployment process.

For an exposed credential, patching the file is insufficient: the owner must revoke/rotate it, assess use and possible compromise, and clean history where appropriate. Do not test unknown credentials against a provider. An antivirus hit is an investigation/quarantine signal; the tool does not automatically delete evidence.

Experimental OSV guided remediation and Renovate/Dependabot are useful extensions; their exact role, permissions and limitations are documented in the research. This implementation does not claim every dependency can be safely patched automatically or that all vulnerabilities have available fixes.

## Broader testing and extensions

ZAP and Nuclei are researched optional DAST extensions, not silently enabled attack scanners. A ZAP baseline still spiders and generates requests. Before broader tests, define exact target origins/paths, exclusions, methods, rate/concurrency, test identities, environment, expiration and the permitted scan profile. Enforce those constraints outside the prompt, including redirects/discovered links; use an approved staging target. Do not mount a host container socket into a Boxoro workspace to run another scanner container.

ASVS 5.0 provides the manual/control-verification layer: authentication/session handling, object/function authorization, tenant separation, injection/output handling, business logic, cryptography, privacy, rate limits, configuration and audit behavior. The included scanners cover only parts of that work. Supply-chain assurance also includes upstream provenance/signatures, maintained releases, release-account protection and review of newly introduced install-time code.

## Maintenance and verification

Keep `tools.lock.json` and `requirements.lock` under code review. Refresh release versions/checksums from official sources, review upstream security advisories and license changes, regenerate Python hashes with `lock-python.py`, build a new image, scan the image itself, run smoke tests and worker qualification, and admit the actual digest. Pinning is compatible with updates only when this refresh process is maintained. Database freshness is a separate daily concern.

Local checks:

```sh
python3 -m unittest discover -s qa -p 'test_security_*.py' -v
cargo test --locked
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
bash -n scripts/build-security-image.sh qa/security-image-smoke.sh
```

The Python tests exercise behavior with real subprocesses/local HTTP services and bounded provider fixtures. They do not certify live paid accounts, all upstream scanner capabilities, a built image, or gVisor containment. Only actual image smoke and host qualification evidence establishes those deployment checks. See [SECURITY_STACK_VALIDATION.md](SECURITY_STACK_VALIDATION.md) for the evidence and environmental gaps from this implementation.
