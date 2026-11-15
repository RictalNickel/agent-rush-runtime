# Code Architect

Inspect dependency boundaries, complexity, types, duplication and static architecture risks.

Profile ID: `code-architect`. Preferred family: `python`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Code Architect

Explain structural risks in terms of concrete change cost, ownership and behavior. Prefer a small coherent refactor over an abstract rewrite.

## Working method

Map entrypoints, modules and dependency direction before proposing boundaries. Use language-aware type/lint tools and dependency graphs, then inspect the affected code to validate findings. Treat complexity and duplication reports as evidence for review, not automatic design verdicts. Identify cycles, implicit state, error propagation and tests that constrain the refactor. Respect project conventions and compare behavior before and after. Config/plugins execute project code, so review them and ensure the isolated tool environment matches installed dependencies.

The dependency-cycle recipe includes JS/JSX/TS/TSX/MJS/CJS files. Point it at the relevant source subtree and review alias/configuration coverage; an empty cycle list is not a source-coverage count.

## Deliverable

An architecture map, evidence-backed issues, a bounded refactor proposal or patch, compatibility risks and the exact behavioral checks used to validate it.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Ruff (`ruff`) | image | `python: ruff` | MIT | [upstream](https://github.com/astral-sh/ruff) |
| mypy (`mypy`) | image | `python: mypy` | MIT | [upstream](https://github.com/python/mypy) |
| TypeScript (`typescript`) | image | `npm: typescript` | Apache-2.0 | [upstream](https://github.com/microsoft/TypeScript) |
| ESLint (`eslint`) | image | `npm: eslint` | MIT | [upstream](https://github.com/eslint/eslint) |
| Semgrep CE (`semgrep`) | image | `python: semgrep` | LGPL-2.1-or-later | [upstream](https://github.com/semgrep/semgrep) |
| Lizard (`lizard`) | image | `python: lizard` | MIT | [upstream](https://github.com/terryyin/lizard) |
| Radon (`radon`) | image | `python: radon` | MIT | [upstream](https://github.com/rubik/radon) |
| dependency-cruiser (`dependency-cruiser`) | image | `npm: dependency-cruiser` | MIT | [upstream](https://github.com/sverweij/dependency-cruiser) |
| Madge (`madge`) | image | `npm: madge` | MIT | [upstream](https://github.com/pahen/madge) |
| jscpd (`jscpd`) | image | `npm: jscpd` | MIT | [upstream](https://github.com/kucherenko/jscpd) |
| Graphviz (`graphviz`) | image | `apt: graphviz` | EPL-1.0 | [upstream](https://graphviz.org/) |
| cloc (`cloc`) | image | `apt: cloc` | GPL-2.0-or-later | [upstream](https://github.com/AlDanial/cloc) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Git (`git`) | image | `apt: git` | GPL-2.0-only | [upstream](https://git-scm.com/) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `python-architecture` | `ruff-check` → `mypy-check` → `radon-complexity` |
| `dependency-check` | `dependency-cruiser-check` → `madge-cycles` |
| `duplication` | `jscpd-scan` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "code-architect",
  "workflow": "python-architecture"
}
```

```json
{
  "action": "doctor",
  "agent": "code-architect",
  "workflow": "python-architecture"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `dependency-cruiser-check`

Validate module boundaries with a supplied rule config.

Command: `/opt/box/agents/bin/dependency-cruiser --config '{config}' --output-type json '{path}'`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `config` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `jscpd-scan`

Report duplicate code with an explicit maximum percentage.

Command: `/opt/box/agents/bin/jscpd --reporters json --output '{artifacts}/jscpd' --threshold '{threshold}' '{path}'`

Effects: project_code, mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `threshold` | number; default=5; min=0; max=100 |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `madge-cycles`

Find JavaScript/TypeScript dependency cycles.

Command: `/opt/box/agents/bin/madge --circular --json --extensions js,jsx,ts,tsx,mjs,cjs '{path}'`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Explicit extensions include JavaScript, JSX, TypeScript, TSX and ESM/CommonJS variants when a directory is supplied. Qualified cycles exit 1. Scope the source directory; aliases, project configuration, unresolved imports and parser coverage still require review. An empty cycle list alone does not prove that every intended source file was analyzed.

### `mypy-check`

Check Python types using the project configuration.

Command: `/opt/box/agents/bin/mypy --no-incremental --cache-dir=/dev/null --show-error-codes '{path}'`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `radon-complexity`

Report Python cyclomatic complexity as JSON.

Command: `/opt/box/agents/bin/radon cc --json '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Descriptive metric only; no arbitrary numeric threshold is silently treated as an architecture gate.

### `ruff-check`

Lint Python without automatic fixes.

Command: `/opt/box/agents/bin/ruff check --no-cache --no-fix --output-format json '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

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
| `ruff-format-check` | Check Python formatting and emit a diff. | none declared |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `git-status` | Inspect local Git working-tree state without optional locks. | none declared |
| `typescript-check` | Type-check a supplied tsconfig without emitting files. | project_code, mutation |
| `eslint-check` | Lint project source without fixing it. | project_code |
| `lizard-complexity` | Report cyclomatic complexity with a bounded warning threshold. | none declared |
| `cloc-count` | Count source lines excluding common dependency/build directories. | none declared |
| `graphviz-render` | Render a supplied DOT graph as SVG. | mutation |
| `semgrep-scan` | Run community static analysis with a supplied local rule file. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **mypy:** Configured plugins import Python code.
- **ESLint:** Config, resolvers and plugins execute project JavaScript; automatic fixes disabled.
- **Semgrep CE:** Community engine only; commercial engines, registry rule licenses and hosted services are separate. License expression follows the resolved 1.185.0 PyPI metadata. Resolved 1.185.0 pins mcp 1.29.0 and OpenTelemetry 1.37.x with known moderate advisories. The registered operation is local scan with metrics/version checks disabled; no MCP client/server or telemetry endpoint is exposed. This limited use does not erase the dependency findings; see SPECIALIST_DEPENDENCIES.md.
- **check-jsonschema:** External schema references can require network access.

## Limits and interpretation

- Static graphs may miss dynamic imports, reflection and generated code.
- Complexity/line counts are descriptive signals, not quality certification.
- Missing dependencies/plugins or parse coverage must remain visible.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
