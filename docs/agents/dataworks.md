# DataWorks

Profile datasets, validate structure, inspect SQL transformations and prepare reproducible data checks.

Profile ID: `dataworks`. Preferred family: `data`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# DataWorks

Turn supplied data and transformation intent into a reproducible quality assessment with explicit units, types, keys and missing-value rules.

## Working method

Record provenance, date range, grain, expected keys and sensitive fields. Profile a bounded sample or supplied full file, then distinguish parser/type issues from business-rule violations. Check row counts, missingness, duplicate keys, numeric ranges and join cardinality with explicit assumptions. Use DuckDB/SQLite metadata and SQL linting for local assets. Pandera/pandas/Polars are optional project libraries, not fictional standalone commands. dbt adapters, destination credentials and real pipeline writes require a separate qualified environment.

For routine CSV checks use the bounded built-in profile and Frictionless validation; request csv-statistics only when csvkit's inferred types and richer statistics answer a specific question. Set the pipeline-inventory path to the folder containing existing dlt pipeline working directories.

## Deliverable

Dataset profile, declared schema/quality rules, reproducible transformation or validation code, counts before/after, rejected-row handling and unresolved provenance/coverage gaps.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| DuckDB CLI (`duckdb`) | image | `python: duckdb-cli` | MIT | [upstream](https://github.com/duckdb/duckdb-python-cli) |
| SQLite CLI (`sqlite`) | image | `apt: sqlite3` | Public-Domain | [upstream](https://www.sqlite.org/) |
| csvkit (`csvkit`) | image | `python: csvkit` | MIT | [upstream](https://github.com/wireservice/csvkit) |
| Miller (`miller`) | image | `apt: miller` | BSD-2-Clause | [upstream](https://github.com/johnkerl/miller) |
| Frictionless (`frictionless`) | image | `python: frictionless` | MIT | [upstream](https://github.com/frictionlessdata/frictionless-py) |
| dbt Core 1.x (`dbt-core`) | optional | `python: dbt-core >=1.11,<2` | Apache-2.0 | [upstream](https://github.com/dbt-labs/dbt/tree/1.latest) |
| dlt (`dlt`) | image | `python: dlt[cli]` | Apache-2.0 | [upstream](https://github.com/dlt-hub/dlt) |
| SQLFluff (`sqlfluff`) | image | `python: sqlfluff` | MIT | [upstream](https://github.com/sqlfluff/sqlfluff) |
| Pandera (`pandera`) | optional | `python: pandera` | MIT | [upstream](https://github.com/unionai-oss/pandera) |
| pandas (`pandas`) | optional | `python: pandas` | BSD-3-Clause | [upstream](https://github.com/pandas-dev/pandas) |
| Polars (`polars`) | optional | `python: polars` | MIT | [upstream](https://github.com/pola-rs/polars) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| SQLGlot (`sqlglot`) | optional | `python: sqlglot` | MIT | [upstream](https://github.com/tobymao/sqlglot) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `csv-quality` | `csv-profile` → `frictionless-validate` |
| `sql-quality` | `sqlfluff-lint` |
| `json-contract` | `json-check` → `jsonschema-validate` |
| `csv-statistics` | `csvkit-profile` |
| `pipeline-inventory` | `dlt-list-pipelines` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "dataworks",
  "workflow": "csv-quality"
}
```

```json
{
  "action": "doctor",
  "agent": "dataworks",
  "workflow": "csv-quality"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `csv-profile`

Profile UTF-8 comma CSV without emitting cell contents.

Command: `/opt/box/agents/bin/boxoro-analysis csv-profile '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Bounded to 100,000 rows and 256 columns. Reports missingness, duplicate rows and numeric summaries; no type schema or business validity is implied.

### `csvkit-profile`

Produce CSV descriptive statistics as JSON.

Command: `/opt/box/agents/bin/csvkit --json '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Use this opt-in statistical report when richer inferred types and distributions are needed. It reads the full CSV and may emit frequent/minimum/maximum values, so review sensitivity and locale/date/numeric-string interpretation. The routine csv-quality workflow omits this additional scan. No business-quality threshold is implied.

### `dlt-list-pipelines`

List pipeline working directories beneath the supplied local directory.

Command: `/opt/box/agents/bin/dlt pipeline --list-pipelines --pipelines-dir '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Set path to the directory containing pipeline working folders. An explicit --pipelines-dir avoids inspecting the fresh per-run HOME, which cannot contain project pipelines. This is local directory/trace-time inventory, not evidence of successful extraction or loading; names are not validated pipeline health. No destination access, pipeline execution, sync, drop or repair is invoked.

### `frictionless-validate`

Validate a supplied local CSV table.

Command: `/opt/box/agents/bin/frictionless validate '{input}' --type table --scheme file --format csv --trusted --json`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Forces a local file/CSV table instead of interpreting arbitrary descriptors or remote resources. --trusted permits the absolute path already checked by the workspace path guard. Findings can contain cell values; review reports before sharing. A valid inferred schema does not establish business correctness.

### `json-check`

Validate strict JSON syntax and duplicate-key policy.

Command: `/opt/box/agents/bin/boxoro-analysis json-check '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `jsonschema-validate`

Validate a supplied data file against an explicit schema.

Command: `/opt/box/agents/bin/check-jsonschema --schemafile '{schema}' '{input}'`

Effects: network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `schema` | string; path; required |
| `input` | string; path; required |

Schema references may retrieve remote resources. For strictly offline syntax validation use json-check; external schema references should be reviewed before enabling network.

### `sqlfluff-lint`

Lint a SQL file with a raw templater and explicit dialect.

Command: `/opt/box/agents/bin/sqlfluff lint --ignore-local-config --templater raw --dialect '{dialect}' --format json '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |
| `dialect` | string; default="ansi"; choices=ansi, postgres, mysql, sqlite, duckdb, bigquery, snowflake, tsql |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `sqlite-schema` | Read the schema of a local SQLite database in read-only mode. | none declared |
| `duckdb-schema` | Read local DuckDB table metadata without write access. | none declared |
| `miller-count` | Count records in a supplied CSV file. | none declared |
| `duckdb-columns` | Read DuckDB column names, types, nullability and defaults without write access. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### MCP Toolbox for Databases (`database-toolbox`)

Status: **reference-not-connected**. Transport: `operator-configured-stdio-or-http`.

Explicit database connection with read-only role and approved OAuth/IAM/service configuration.

Use reviewed predefined parameterized queries/toolsets. Avoid default arbitrary execute_sql against production and do not assume a server-side read-only flag is an authorization boundary.

Primary documentation: [source 1](https://github.com/googleapis/mcp-toolbox), [source 2](https://github.com/googleapis/mcp-toolbox/blob/main/docs/en/documentation/configuration/toolbox_mcp_auth.md).

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **DuckDB CLI:** Uses the official standalone CLI wheel, not the Python library pretending to expose a CLI. Extensions and network reads are not enabled by the fixed recipe.
- **dbt Core 1.x:** Explicitly constrained to Python dbt Core 1.x. A database adapter and project profiles are required. Current v2 distributions have separate product licensing and are not substituted silently.
- **dlt:** Open-source dlt only. dltHub commercial extras and deployments are not installed or activated.
- **SQLFluff:** Templaters/plugins can execute Python or database-dependent project code. Fixed recipe selects the raw templater.
- **Pandera:** Library integration; no CLI or passing runtime probe is claimed.
- **pandas:** Library integration; no CLI or passing runtime probe is claimed.
- **Polars:** Library integration; no CLI or passing runtime probe is claimed.
- **check-jsonschema:** External schema references can require network access.
- **SQLGlot:** Library integration; no CLI or passing runtime probe is claimed.

## Limits and interpretation

- The built-in CSV profiler is UTF-8 comma-separated and capped at 100,000 rows/256 columns.
- Inferred types and empty values need domain review.
- No database destination, data warehouse, dltHub subscription or production pipeline mutation is assumed.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
