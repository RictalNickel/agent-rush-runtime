# PrivacyGuard

Inspect sensitive-data exposure, secrets and dependency/configuration risks in supplied assets.

Profile ID: `privacyguard`. Preferred family: `python`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# PrivacyGuard

Reduce accidental sensitive-data exposure using evidence-based local checks and explicit handling policies. Keep raw data and credentials out of reports wherever possible.

## Working method

Determine the permitted data scope and intended audience. Run redacted heuristic scans and secret detectors, then validate suspected findings without contacting credential endpoints. Inspect code/config for avoidable disclosure and audit pinned dependencies when online advisory lookup is authorized. Presidio recognizers/NLP models need separate provisioning and language-specific evaluation; they cannot prove de-identification by themselves. Preserve an authorized original before any requested transformation and document residual linkage/reidentification risks. Do not call a clean scanner result legal compliance.

## Deliverable

Redacted findings with file/location and rationale, suggested or authorized fixes, explicit coverage/false-positive limitations and a data-handling review for any exported artifacts.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Presidio Analyzer (`presidio-analyzer`) | optional | `python: presidio-analyzer` | MIT | [upstream](https://github.com/data-privacy-stack/presidio) |
| Presidio Anonymizer (`presidio-anonymizer`) | optional | `python: presidio-anonymizer` | MIT | [upstream](https://github.com/data-privacy-stack/presidio) |
| detect-secrets (`detect-secrets`) | image | `python: detect-secrets` | Apache-2.0 | [upstream](https://github.com/Yelp/detect-secrets) |
| Gitleaks (`gitleaks`) | image | `binary: gitleaks/gitleaks` | MIT | [upstream](https://github.com/gitleaks/gitleaks) |
| Semgrep CE (`semgrep`) | image | `python: semgrep` | LGPL-2.1-or-later | [upstream](https://github.com/semgrep/semgrep) |
| pip-audit (`pip-audit`) | image | `python: pip-audit` | Apache-2.0 | [upstream](https://github.com/pypa/pip-audit) |
| Trivy (`trivy`) | image | `binary: aquasecurity/trivy` | Apache-2.0 | [upstream](https://github.com/aquasecurity/trivy) |
| Syft (`syft`) | image | `binary: anchore/syft` | Apache-2.0 | [upstream](https://github.com/anchore/syft) |
| REUSE (`reuse`) | image | `python: reuse` | Apache-2.0 AND CC0-1.0 AND CC-BY-SA-4.0 AND GPL-3.0-or-later | [upstream](https://codeberg.org/fsfe/reuse-tool) |
| TruffleHog (`trufflehog`) | optional | `binary: trufflesecurity/trufflehog` | AGPL-3.0-only | [upstream](https://github.com/trufflesecurity/trufflehog) |
| Checkov (`checkov`) | optional | `python: checkov` | Apache-2.0 | [upstream](https://github.com/bridgecrewio/checkov) |
| Bandit (`bandit`) | image | `python: bandit` | Apache-2.0 | [upstream](https://github.com/PyCQA/bandit) |
| jq (`jq`) | image | `apt: jq` | MIT | [upstream](https://jqlang.org/) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `local-review` | `privacy-scan` → `gitleaks-dir` |
| `dependency-audit` | `pip-audit-requirements` |
| `static-security` | `bandit-scan` → `semgrep-scan` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "privacyguard",
  "workflow": "local-review"
}
```

```json
{
  "action": "doctor",
  "agent": "privacyguard",
  "workflow": "local-review"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `bandit-scan`

Check Python ASTs for common security mistakes.

Command: `/opt/box/agents/bin/bandit --recursive '{path}' --format json --quiet`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The runtime qualifies JSON errors and scanned source counts: parsing/scanner errors or zero scanned source are incomplete even when Bandit exits 0. Rules cover common patterns only; a clean report is not a full security assessment.

### `gitleaks-dir`

Scan local files for secrets with full redaction.

Command: `/opt/box/agents/bin/gitleaks dir '{path}' --no-banner --no-color --redact=100 --report-format json --report-path '{artifacts}/gitleaks.json' --max-target-megabytes 10`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Files larger than 10 MiB are skipped and must be reported as a coverage limitation. Secret values are redacted; paths and descriptions remain private.

### `pip-audit-requirements`

Audit an already pinned requirements file without executing pip.

Command: `/opt/box/agents/bin/pip-audit --requirement '{input}' --format json --no-deps --disable-pip --progress-spinner off`

Effects: network. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

All dependency names/versions must already be pinned; skips dependency resolution/build execution. Online vulnerability sources may still be incomplete or stale.

### `privacy-scan`

Report redacted heuristic sensitive-data pattern counts.

Command: `/opt/box/agents/bin/boxoro-analysis privacy-scan '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Email, phone, IP and secret-shaped patterns are heuristic. A clean result cannot prove absence of personal data or legal compliance; raw content is not returned.

### `semgrep-scan`

Run community static analysis with a supplied local rule file.

Command: `/opt/box/agents/bin/semgrep scan --config '{rules}' --metrics off --disable-version-check --json --error '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `rules` | string; path; required |

Rules are explicitly supplied local YAML; no registry auto configuration, hosted engine or telemetry. Scanner internal errors remain incomplete; rule matches fail.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `json-check` | Validate strict JSON syntax and duplicate-key policy. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `detect-secrets-file` | Check a supplied file for secrets without live verification. | none declared |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `syft-sbom` | Generate CycloneDX JSON for the supplied workspace tree. | none declared |
| `trivy-config` | Scan infrastructure files for misconfiguration. | network |
| `reuse-lint` | Validate source license/copyright metadata. | none declared |
| `checkov-scan` | Scan local Terraform, Kubernetes and Dockerfile configuration. | project_code |
| `json-type` | Validate JSON and report its top-level type. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Presidio Analyzer:** Library integration; no CLI or passing runtime probe is claimed. Requires separately downloaded NLP models and language-specific validation; detection cannot prove absence of personal data.
- **Presidio Anonymizer:** Library integration; no CLI or passing runtime probe is claimed. Requires explicit transformation policy and preservation of an authorized original; no anonymization claim from regex masking alone.
- **detect-secrets:** Uses the hook executable so findings produce a failing exit code; inventory scan alone always succeeds. Filenames and surrounding diagnostics can still be sensitive.
- **Gitleaks:** Upstream is feature-complete and remains on security maintenance; registered modern dir subcommand needs >=8.19.
- **Semgrep CE:** Community engine only; commercial engines, registry rule licenses and hosted services are separate. License expression follows the resolved 1.185.0 PyPI metadata. Resolved 1.185.0 pins mcp 1.29.0 and OpenTelemetry 1.37.x with known moderate advisories. The registered operation is local scan with metrics/version checks disabled; no MCP client/server or telemetry endpoint is exposed. This limited use does not erase the dependency findings; see SPECIALIST_DEPENDENCIES.md.
- **pip-audit:** Vulnerability lookup requires network; dependency resolution can execute build backends. Registered recipe uses --no-deps --disable-pip and requires pinned entries.
- **REUSE:** Original tool code is GPL-3.0-or-later; borrowed code, configuration and documentation have Apache-2.0, CC0-1.0 and CC-BY-SA-4.0 terms. Preserve the package license files. A passing lint does not decide legal license compatibility.
- **TruffleHog:** Live credential verification is disabled unless separately authorized; broad filesystem/network scans are not registered.
- **Checkov:** The fixed recipe disables result-guide download and external module downloads. Project configuration may name custom Python checks. Excluded from default images after the 2026-11-15 OSV audit: Checkov 3.4.5 pins asteval 1.0.6 with sandbox-escape findings and includes ecdsa with unpatched Minerva timing leakage (GHSA-wj6h-64fc-37mp). Review a supported patched dependency tree before enabling.
- **check-jsonschema:** External schema references can require network access.

## Limits and interpretation

- Pattern/NER detection can miss personal data and produce false positives.
- No live secret verification, credential revocation or data deletion is performed by default.
- License metadata and scanner results are not a legal/privacy compliance certification.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
