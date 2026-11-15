# CloudPilot

Review infrastructure plans, Kubernetes configuration and cloud deployment prerequisites.

Profile ID: `cloudpilot`. Preferred family: `platform`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# CloudPilot

Make infrastructure changes understandable before they touch an account or cluster. Prefer static validation and reproducible rendered manifests as the first evidence.

## Working method

Identify the target environment, provider/cluster versions, resource ownership and expected cost-impact areas. Inspect formatting and static policy, validate Kubernetes manifests against the matching schema version and render local charts/overlays without installing them. Keep providers, modules and plugins pinned; review network access for schema/module retrieval. Cloud accounts, kubeconfig and deployment credentials must be scoped outside the workspace. Plans/apply, cluster writes and account creation are not registered default commands; escalate only the concrete reviewed change through the existing authorized deployment process.

## Deliverable

Reviewed infrastructure changes, rendered manifests, schema/policy findings, version prerequisites, impact assumptions and explicit deploy/rollback steps for the owner.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| OpenTofu (`opentofu`) | image | `binary: opentofu/opentofu` | MPL-2.0 | [upstream](https://github.com/opentofu/opentofu) |
| TFLint (`tflint`) | image | `binary: terraform-linters/tflint` | MPL-2.0 | [upstream](https://github.com/terraform-linters/tflint) |
| Checkov (`checkov`) | optional | `python: checkov` | Apache-2.0 | [upstream](https://github.com/bridgecrewio/checkov) |
| Conftest (`conftest`) | image | `binary: open-policy-agent/conftest` | Apache-2.0 | [upstream](https://github.com/open-policy-agent/conftest) |
| Kubeconform (`kubeconform`) | image | `binary: yannh/kubeconform` | Apache-2.0 | [upstream](https://github.com/yannh/kubeconform) |
| Helm (`helm`) | image | `binary: helm/helm` | Apache-2.0 | [upstream](https://github.com/helm/helm) |
| Kustomize (`kustomize`) | image | `binary: kubernetes-sigs/kustomize` | Apache-2.0 | [upstream](https://github.com/kubernetes-sigs/kustomize) |
| yamllint (`yamllint`) | image | `python: yamllint` | GPL-3.0-or-later | [upstream](https://github.com/adrienverge/yamllint) |
| actionlint (`actionlint`) | image | `binary: rhysd/actionlint` | MIT | [upstream](https://github.com/rhysd/actionlint) |
| ShellCheck (`shellcheck`) | image | `apt: shellcheck` | GPL-3.0-or-later | [upstream](https://github.com/koalaman/shellcheck) |
| AWS CLI (`aws-cli`) | external | `external: awscli` | Apache-2.0 | [upstream](https://github.com/aws/aws-cli) |
| kubectl (`kubectl`) | external | `external: kubectl` | Apache-2.0 | [upstream](https://kubernetes.io/docs/reference/kubectl/) |
| Syft (`syft`) | image | `binary: anchore/syft` | Apache-2.0 | [upstream](https://github.com/anchore/syft) |
| Trivy (`trivy`) | image | `binary: aquasecurity/trivy` | Apache-2.0 | [upstream](https://github.com/aquasecurity/trivy) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `infra-preflight` | `tofu-fmt-check` → `trivy-config` |
| `kubernetes-check` | `kubeconform-check` |
| `chart-check` | `helm-lint` → `helm-template` |
| `checkov-qualified` | `checkov-scan` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "cloudpilot",
  "workflow": "infra-preflight"
}
```

```json
{
  "action": "doctor",
  "agent": "cloudpilot",
  "workflow": "infra-preflight"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `checkov-scan`

Scan local Terraform, Kubernetes and Dockerfile configuration.

Command: `/opt/box/agents/bin/checkov --directory '{path}' --output json --quiet --skip-download --download-external-modules false --framework terraform kubernetes dockerfile`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Project config/custom checks can execute Python. Disables external-module download and Prisma remediation-guide calls; commercial SCA services are not enabled.

### `helm-lint`

Lint a local Helm chart and its checked-in subcharts.

Command: `/opt/box/agents/bin/helm lint '{path}' --strict --with-subcharts`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `helm-template`

Render a local Helm chart without a cluster install.

Command: `/opt/box/agents/bin/helm template boxoro '{path}'`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

No dependency-update, install, upgrade, hooks or kubeconfig argument is accepted. Rendered YAML is evidence for separate schema/policy checks.

### `kubeconform-check`

Validate Kubernetes resource structure against pinned server-version schemas.

Command: `/opt/box/agents/bin/kubeconform -strict -summary -output json -kubernetes-version '{kubernetes_version}' '{input}'`

Effects: network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |
| `kubernetes_version` | string; default="1.34.0"; choices=1.32.0, 1.33.0, 1.34.0, 1.35.0 |

Schema retrieval requires network/cache. The chosen version must match the target cluster; CRDs need reviewed additional schemas and are not silently skipped.

### `tofu-fmt-check`

Check infrastructure source formatting without initialization or apply.

Command: `/opt/box/agents/bin/opentofu fmt -check -recursive '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[3]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Formatting only; does not evaluate providers, estimate costs, plan or apply infrastructure.

### `trivy-config`

Scan infrastructure files for misconfiguration.

Command: `/opt/box/agents/bin/trivy config --config '' --ignorefile '' --disable-telemetry --skip-version-check --skip-check-update --include-non-failures --format json --exit-code 1 '{path}'`

Effects: network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Uses embedded checks when the private check cache is empty; check-bundle updates, project trivy.yaml and ignore-file suppression are disabled. Real offline Terraform fixtures confirmed rule coverage. Terraform module references may require network and must be reviewed. The runtime qualifies JSON MisconfSummary coverage; fatal/missing reports and zero checked configurations are incomplete, not pass.

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
| `syft-sbom` | Generate CycloneDX JSON for the supplied workspace tree. | none declared |
| `actionlint-check` | Validate supplied GitHub Actions workflow YAML. | none declared |
| `shellcheck-check` | Lint a supplied shell script without executing it. | none declared |
| `tflint-check` | Lint Terraform/OpenTofu with the existing reviewed plugin setup. | project_code, network |
| `conftest-policy` | Evaluate a supplied Rego policy against a local data file. | project_code, network |
| `kustomize-render` | Render a local Kubernetes overlay without exec plugins. | network |
| `yaml-lint` | Validate YAML syntax and style. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### AWS official MCP toolkits (`aws-mcp`)

Status: **reference-not-connected**. Transport: `operator-configured-stdio-or-http`.

Explicit AWS credential/profile and least-privilege account policy outside the workspace.

Select a named official service server or the current Agent Toolkit for AWS successor. Documentation/read-only diagnostics first; no automatic cloud resource creation.

Primary documentation: [source 1](https://github.com/awslabs/mcp).

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Checkov:** The fixed recipe disables result-guide download and external module downloads. Project configuration may name custom Python checks. Excluded from default images after the 2026-11-15 OSV audit: Checkov 3.4.5 pins asteval 1.0.6 with sandbox-escape findings and includes ecdsa with unpatched Minerva timing leakage (GHSA-wj6h-64fc-37mp). Review a supported patched dependency tree before enabling.
- **AWS CLI:** AWS credentials and account selection are operator-managed; CLI v2 distribution uses its own installer. No account is created or infrastructure modified.
- **kubectl:** Requires a separately mounted scoped kubeconfig. No cluster-changing recipe is registered.

## Limits and interpretation

- No cloud credentials, cloud accounts or cluster access are inferred.
- Formatting does not validate provider semantics or estimate a bill.
- No apply, destroy, Helm install/upgrade or kubectl mutation is registered.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
