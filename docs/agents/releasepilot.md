# ReleasePilot

Assemble release readiness evidence across source, CI, containers and supply chain.

Profile ID: `releasepilot`. Preferred family: `platform`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# ReleasePilot

Turn release intent into a reviewable readiness packet. Preserve the exact source revision, artifact identity, validation status and rollback assumptions.

## Working method

Inspect the tree and intended release scope. Check workflow/Dockerfile/shell syntax, secrets and license metadata; generate an SBOM and audit it with a fresh advisory database when available. Distinguish tool execution success from a release gate and record ignored or oversized files. Require project tests and reproducible artifact hashes for the actual candidate. Signing, release creation, registry pushes, merge and deployment need the user-authorized external workflow; they are not implied by a successful readiness report. Use official GitHub MCP read-only toolsets unless a separate action is explicitly authorized.

## Deliverable

A release checklist tied to the source revision, SBOM and scan evidence, changelog/compatibility notes, unresolved blockers and a concrete rollback or forward-fix plan.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Git (`git`) | image | `apt: git` | GPL-2.0-only | [upstream](https://git-scm.com/) |
| Syft (`syft`) | image | `binary: anchore/syft` | Apache-2.0 | [upstream](https://github.com/anchore/syft) |
| Grype (`grype`) | image | `binary: anchore/grype` | Apache-2.0 | [upstream](https://github.com/anchore/grype) |
| Trivy (`trivy`) | image | `binary: aquasecurity/trivy` | Apache-2.0 | [upstream](https://github.com/aquasecurity/trivy) |
| Gitleaks (`gitleaks`) | image | `binary: gitleaks/gitleaks` | MIT | [upstream](https://github.com/gitleaks/gitleaks) |
| actionlint (`actionlint`) | image | `binary: rhysd/actionlint` | MIT | [upstream](https://github.com/rhysd/actionlint) |
| Hadolint (`hadolint`) | image | `binary: hadolint/hadolint` | GPL-3.0-only | [upstream](https://github.com/hadolint/hadolint) |
| ShellCheck (`shellcheck`) | image | `apt: shellcheck` | GPL-3.0-or-later | [upstream](https://github.com/koalaman/shellcheck) |
| REUSE (`reuse`) | image | `python: reuse` | Apache-2.0 AND CC0-1.0 AND CC-BY-SA-4.0 AND GPL-3.0-or-later | [upstream](https://codeberg.org/fsfe/reuse-tool) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| Cosign (`cosign`) | optional | `binary: sigstore/cosign` | Apache-2.0 | [upstream](https://github.com/sigstore/cosign) |
| GitHub CLI (`github-cli`) | external | `external: gh` | MIT | [upstream](https://github.com/cli/cli) |
| pytest (`pytest`) | image | `python: pytest` | MIT | [upstream](https://github.com/pytest-dev/pytest) |
| Ruff (`ruff`) | image | `python: ruff` | MIT | [upstream](https://github.com/astral-sh/ruff) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `source-release-check` | `gitleaks-dir` → `reuse-lint` → `syft-sbom` |
| `container-check` | `hadolint-check` |
| `workflow-check` | `actionlint-check` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "releasepilot"
}
```

```json
{
  "action": "doctor",
  "agent": "releasepilot"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `actionlint-check`

Validate supplied GitHub Actions workflow YAML.

Command: `/opt/box/agents/bin/actionlint -oneline {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Does not execute workflow jobs. ShellCheck is an optional analysis subprocess from the trusted image.

### `gitleaks-dir`

Scan local files for secrets with full redaction.

Command: `/opt/box/agents/bin/gitleaks dir {path} --no-banner --no-color --redact=100 --report-format json --report-path {artifacts}/gitleaks.json --max-target-megabytes 10`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Files larger than 10 MiB are skipped and must be reported as a coverage limitation. Secret values are redacted; paths and descriptions remain private.

### `hadolint-check`

Lint a supplied Dockerfile.

Command: `/opt/box/agents/bin/hadolint --format json {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `reuse-lint`

Validate source license/copyright metadata.

Command: `/opt/box/agents/bin/reuse --root {path} lint`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

REUSE metadata validation does not decide legal compatibility, ownership or license sufficiency.

### `syft-sbom`

Generate CycloneDX JSON for the supplied workspace tree.

Command: `/opt/box/agents/bin/syft scan dir:{path} --output cyclonedx-json`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Filesystem package detection is incomplete for dynamic builds and vendored artifacts. An SBOM is inventory, not a vulnerability or provenance pass.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `json-check` | Validate strict JSON syntax and duplicate-key policy. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `ruff-check` | Lint Python without automatic fixes. | none declared |
| `ruff-format-check` | Check Python formatting and emit a diff. | none declared |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `pytest-test` | Run Python tests with bounded output and JUnit evidence. | project_code, network, mutation |
| `git-status` | Inspect local Git working-tree state without optional locks. | none declared |
| `grype-sbom` | Audit a supplied SBOM with a high-severity failure threshold. | network |
| `trivy-config` | Scan infrastructure files for misconfiguration. | network |
| `shellcheck-check` | Lint a supplied shell script without executing it. | none declared |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Sentry MCP (`sentry-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

OAuth through the existing account connector.

Prefer a project-scoped /mcp/{organization}/{project} endpoint. Read/search issue and performance tools only by default; project management and incident writes need explicit authorization.

Primary documentation: [source 1](https://mcp.sentry.dev/), [source 2](https://github.com/getsentry/toolkit).

### Tool-specific qualifications

- **Gitleaks:** Upstream is feature-complete and remains on security maintenance; registered modern dir subcommand needs >=8.19.
- **REUSE:** Original tool code is GPL-3.0-or-later; borrowed code, configuration and documentation have Apache-2.0, CC0-1.0 and CC-BY-SA-4.0 terms. Preserve the package license files. A passing lint does not decide legal license compatibility.
- **check-jsonschema:** External schema references can require network access.
- **Cosign:** Keyless verification requires trust root, transparency-log material and network policy; no signing or registry-write operation is registered.
- **GitHub CLI:** Requires user-authorized GitHub credentials. No publish, merge, release or message-sending recipe is registered.

## Limits and interpretation

- Default commands do not publish, merge, tag, sign or deploy.
- Cosign needs a reviewed trust/identity policy and remains optional.
- An SBOM or REUSE lint is not itself a vulnerability, provenance or legal compatibility certification.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
