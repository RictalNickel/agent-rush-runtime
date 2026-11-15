# Agent Foundry

Build and validate reviewable agent packages, tool contracts and evaluation scaffolds.

Profile ID: `agent-foundry`. Preferred family: `python`. Planning allowance: **2048 MiB RAM / 4096 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Agent Foundry

Turn a concrete agent job into a small package with explicit inputs, outputs, failure behavior and evaluation cases. Prefer existing runtime primitives and reviewed templates over another unbounded agent framework.

## Working method

Inspect repository conventions and the current agent interface. Record what the agent can read/change, its provider assumptions and its expected evidence. Create a new scaffold or render a reviewed local template with hooks/tasks disabled. Add examples for success, bad input, unavailable integrations and truncated coverage. Use lint/type/security checks and then run the real project tests. Treat MCP Inspector as an optional protocol diagnostic, not permission to spawn arbitrary server commands.

## Deliverable

A runnable package or patch, explicit tool/input schemas, an evaluation fixture set, dependency lock changes and a concise readiness report with exact unresolved dependencies.

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
| Copier (`copier`) | image | `python: copier` | MIT | [upstream](https://github.com/copier-org/copier) |
| Cookiecutter (`cookiecutter`) | image | `python: cookiecutter` | BSD-3-Clause | [upstream](https://github.com/cookiecutter/cookiecutter) |
| uv (`uv`) | image | `python: uv` | Apache-2.0 OR MIT | [upstream](https://github.com/astral-sh/uv) |
| Ruff (`ruff`) | image | `python: ruff` | MIT | [upstream](https://github.com/astral-sh/ruff) |
| mypy (`mypy`) | image | `python: mypy` | MIT | [upstream](https://github.com/python/mypy) |
| Bandit (`bandit`) | image | `python: bandit` | Apache-2.0 | [upstream](https://github.com/PyCQA/bandit) |
| pip-audit (`pip-audit`) | image | `python: pip-audit` | Apache-2.0 | [upstream](https://github.com/pypa/pip-audit) |
| detect-secrets (`detect-secrets`) | image | `python: detect-secrets` | Apache-2.0 | [upstream](https://github.com/Yelp/detect-secrets) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| pytest (`pytest`) | image | `python: pytest` | MIT | [upstream](https://github.com/pytest-dev/pytest) |
| tox (`tox`) | image | `python: tox` | MIT | [upstream](https://github.com/tox-dev/tox) |
| Promptfoo (`promptfoo`) | image | `npm: promptfoo` | MIT | [upstream](https://github.com/promptfoo/promptfoo) |
| MCP Inspector (`mcp-inspector`) | optional | `npm: @modelcontextprotocol/inspector` | Apache-2.0 AND MIT AND CC-BY-4.0 | [upstream](https://github.com/modelcontextprotocol/inspector) |
| Hypothesis (`hypothesis`) | optional | `python: hypothesis` | MPL-2.0 | [upstream](https://github.com/HypothesisWorks/hypothesis) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `preflight` | `inventory` → `ruff-check` → `bandit-scan` |
| `test` | `pytest-test` |
| `scaffold` | `agent-scaffold` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "agent-foundry",
  "workflow": "preflight"
}
```

```json
{
  "action": "doctor",
  "agent": "agent-foundry",
  "workflow": "preflight"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `agent-scaffold`

Create a new reviewable Python agent package.

Command: `/opt/box/agents/bin/boxoro-analysis scaffold '{destination}' '{package_name}'`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `destination` | string; output_path; required |
| `package_name` | string; identifier; default="my_agent" |

New destination only. package_name must also satisfy Python lowercase identifier rules. Generates package metadata, README, initializer and a describe entrypoint; does not install/register an agent or claim to generate a complete test suite.

### `bandit-scan`

Check Python ASTs for common security mistakes.

Command: `/opt/box/agents/bin/bandit --recursive '{path}' --format json --quiet`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The runtime qualifies JSON errors and scanned source counts: parsing/scanner errors or zero scanned source are incomplete even when Bandit exits 0. Rules cover common patterns only; a clean report is not a full security assessment.

### `inventory`

Inventory bounded regular files with sizes and SHA-256 hashes.

Command: `/opt/box/agents/bin/boxoro-analysis inventory '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `pytest-test`

Run Python tests with bounded output and JUnit evidence.

Command: `/opt/box/agents/bin/pytest -q --disable-warnings '--junitxml={artifacts}/pytest.xml' '{path}'`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Project tests/conftest/plugins execute code and can reach network or mutate their environment. Exit 5 (no tests collected) is incomplete. The runtime also checks the JUnit report so all-skipped suites and missing/invalid evidence cannot pass.

### `ruff-check`

Lint Python without automatic fixes.

Command: `/opt/box/agents/bin/ruff check --no-cache --no-fix --output-format json '{path}'`

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
| `uv-lock-check` | Verify that an existing Python lock matches project metadata offline. | project_code |
| `copier-copy` | Render a reviewed local template without template tasks. | project_code, mutation |
| `cookiecutter-copy` | Render a local Cookiecutter template with hooks disabled. | project_code, mutation |
| `ruff-format-check` | Check Python formatting and emit a diff. | none declared |
| `mypy-check` | Check Python types using the project configuration. | project_code |
| `pip-audit-requirements` | Audit an already pinned requirements file without executing pip. | network |
| `detect-secrets-file` | Check a supplied file for secrets without live verification. | none declared |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `tox-test` | Run one named test environment from the project tox configuration. | project_code, network, mutation |
| `promptfoo-eval` | Run a reviewed evaluation configuration with one concurrent request. | project_code, network, mutation |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### MCP reference SDK/server implementations (`mcp-reference`)

Status: **reference-not-connected**. Transport: `stdio`.

Local filesystem roots and network access must be explicitly scoped per server.

Use current maintained reference packages only after selecting a concrete server. This is a development reference, not a blanket entitlement to filesystem, Git or network tools.

Primary documentation: [source 1](https://github.com/modelcontextprotocol/servers), [source 2](https://github.com/modelcontextprotocol/servers-archived), [source 3](https://github.com/modelcontextprotocol/python-sdk).

### Tool-specific qualifications

- **Copier:** Template tasks and extensions may execute code; use the registered local-copy recipe only for reviewed templates.
- **mypy:** Configured plugins import Python code.
- **pip-audit:** Vulnerability lookup requires network; dependency resolution can execute build backends. Registered recipe uses --no-deps --disable-pip and requires pinned entries.
- **detect-secrets:** Uses the hook executable so findings produce a failing exit code; inventory scan alone always succeeds. Filenames and surrounding diagnostics can still be sensitive.
- **check-jsonschema:** External schema references can require network access.
- **Promptfoo:** Requires Node >=22.22; Node24 recommended. Provider calls and hooks can incur cost or execute code. No account is created; no red-team sweep by default. Reviewed exact npm override basic-ftp=6.2.6 fixes GHSA-c475-qrg2-pj4r and subsequent FTP advisories. get-uri 8.0.1 compatibility was exercised with local data/file/HTTP/FTP fixtures; lock regeneration and operation smoke remain required on upgrades.
- **MCP Inspector:** Upstream is transitioning code from MIT to Apache-2.0; non-relicensed contributions remain MIT and documentation is CC-BY-4.0. May launch configured server commands; no arbitrary subprocess/server argument passthrough is registered.
- **Hypothesis:** Library integration; no CLI or passing runtime probe is claimed. Install in the project test environment and run through pytest; this package has no general standalone test command.

## Limits and interpretation

- Scaffolding does not install/register a new agent automatically.
- Provider credentials and MCP servers must be provisioned separately.
- Framework libraries are not counted as executable capabilities without an adapter.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
