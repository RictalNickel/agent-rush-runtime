# OpsWatch

Inspect observability configuration and correlate explicitly connected incident evidence.

Profile ID: `opswatch`. Preferred family: `platform`. Planning allowance: **2048 MiB RAM / 4096 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# OpsWatch

Build a timestamped incident picture from scoped evidence and validated alerting rules. Separate observed facts, plausible causes and proposed interventions.

## Working method

Record the service, incident window, timezone and known change timeline. Validate local Prometheus configuration/rules and normalize supplied JSON/YAML evidence. Query Grafana/Sentry only through an explicitly connected account with read-only capabilities and bounded time ranges. Avoid dumping credentials, request bodies or unrestricted logs. Compare signals across errors, latency, saturation and deployments; do not infer causality from one correlated graph. Suggest a concrete diagnostic or rollback check with expected observations before any state-changing intervention.

## Deliverable

An evidence timeline, observed symptoms and scope, ranked hypotheses with tests, alert/config fixes when authorized and a handoff naming unresolved telemetry gaps.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Prometheus promtool (`promtool`) | image | `apt: prometheus` | Apache-2.0 | [upstream](https://github.com/prometheus/prometheus) |
| jq (`jq`) | image | `apt: jq` | MIT | [upstream](https://jqlang.org/) |
| curl (`curl`) | image | `apt: curl` | curl | [upstream](https://curl.se/) |
| yq (kislyuk) (`yq`) | image | `python: yq` | Apache-2.0 | [upstream](https://github.com/kislyuk/yq) |
| yamllint (`yamllint`) | image | `python: yamllint` | GPL-3.0-or-later | [upstream](https://github.com/adrienverge/yamllint) |
| kubectl (`kubectl`) | external | `external: kubectl` | Apache-2.0 | [upstream](https://kubernetes.io/docs/reference/kubectl/) |
| Helm (`helm`) | image | `binary: helm/helm` | Apache-2.0 | [upstream](https://github.com/helm/helm) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| OpenSSL (`openssl`) | image | `apt: openssl` | Apache-2.0 | [upstream](https://openssl-library.org/) |
| Grafana MCP service (`grafana`) | external | `external: grafana/mcp-grafana` | Apache-2.0 | [upstream](https://github.com/grafana/mcp-grafana) |
| Git (`git`) | image | `apt: git` | GPL-2.0-only | [upstream](https://git-scm.com/) |
| Kubeconform (`kubeconform`) | image | `binary: yannh/kubeconform` | Apache-2.0 | [upstream](https://github.com/yannh/kubeconform) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `alert-rules` | `prometheus-rules` |
| `config-check` | `yaml-lint` → `prometheus-config` |
| `incident-evidence` | `inventory` → `json-type` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "opswatch",
  "workflow": "alert-rules"
}
```

```json
{
  "action": "doctor",
  "agent": "opswatch",
  "workflow": "alert-rules"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `inventory`

Inventory bounded regular files with sizes and SHA-256 hashes.

Command: `/opt/box/agents/bin/boxoro-analysis inventory '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `json-type`

Validate JSON and report its top-level type.

Command: `/opt/box/agents/bin/jq --exit-status type '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The fixed type filter accepts valid false and null documents; jq parse failures remain incomplete rather than a fabricated analysis pass.

### `prometheus-config`

Validate a local Prometheus configuration.

Command: `/opt/box/agents/bin/promtool check config '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Syntax/linked-file validation only; does not start a server or prove scrape target availability.

### `prometheus-rules`

Validate Prometheus alerting/recording rule syntax.

Command: `/opt/box/agents/bin/promtool check rules '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `yaml-lint`

Validate YAML syntax and style.

Command: `/opt/box/agents/bin/yamllint --format parsable '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `json-check` | Validate strict JSON syntax and duplicate-key policy. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `curl-head` | Read HTTPS response headers with redirects disabled. | network |
| `git-status` | Inspect local Git working-tree state without optional locks. | none declared |
| `kubeconform-check` | Validate Kubernetes resource structure against pinned server-version schemas. | network |
| `helm-lint` | Lint a local Helm chart and its checked-in subcharts. | project_code |
| `helm-template` | Render a local Helm chart without a cluster install. | project_code |
| `yaml-json` | Read YAML through the fixed jq identity filter. | none declared |
| `certificate-inspect` | Inspect a local X.509 certificate. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Grafana official MCP (`grafana-mcp`)

Status: **reference-not-connected**. Transport: `operator-configured-http-or-stdio`.

Explicit Grafana URL and narrowly scoped service-account token.

Run the vendor server with --disable-write and selected tool categories. Datasource SQL/query credentials must independently be read-only; do not enable raw query tools casually.

Primary documentation: [source 1](https://github.com/grafana/mcp-grafana), [source 2](https://github.com/grafana/mcp-grafana/blob/main/docs/sources/configure/enable-and-disable-tools.md).

### Sentry MCP (`sentry-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

OAuth through the existing account connector.

Prefer a project-scoped /mcp/{organization}/{project} endpoint. Read/search issue and performance tools only by default; project management and incident writes need explicit authorization.

Primary documentation: [source 1](https://mcp.sentry.dev/), [source 2](https://github.com/getsentry/toolkit).

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Prometheus promtool:** The image installs only for promtool usage; it does not launch a Prometheus daemon.
- **yq (kislyuk):** This is the Python jq-wrapper package, not mikefarah/yq. Requires jq from the core pack.
- **kubectl:** Requires a separately mounted scoped kubeconfig. No cluster-changing recipe is registered.
- **check-jsonschema:** External schema references can require network access.
- **Grafana MCP service:** Separate deployed MCP server plus a scoped service-account token; the reference is not an active connection.

## Limits and interpretation

- No Prometheus/Grafana/Sentry service is started by this profile.
- Unavailable telemetry is a gap, not evidence of service health.
- No cluster mutation, incident notification or paging is performed by default.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
