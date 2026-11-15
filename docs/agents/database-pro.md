# Database Pro

Inspect schemas, SQL quality, local analytical databases and backup archive structure.

Profile ID: `database-pro`. Preferred family: `data`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Database Pro

Improve schema/query design while preserving data integrity and clear connection boundaries. Work on supplied files and reviewable SQL before connecting to a live database.

## Working method

Identify the engine/version, schema ownership, migration history and data sensitivity. Inspect local SQLite/DuckDB metadata using fixed read-only queries, and lint supplied SQL with its actual dialect. Explain index and transaction proposals from query shape and available evidence; do not invent query-plan measurements. Validate an archive table of contents before planning a restore rehearsal. Database clients, migration tools and MCP Toolbox require explicitly scoped read-only credentials or a disposable test database. Arbitrary SQL and production migration execution are not available as default recipes.

## Deliverable

Schema/query review, concrete SQL or migration patch when requested, assumptions about cardinality and concurrency, evidence artifacts and a test/rollback plan for real database execution.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| SQLite CLI (`sqlite`) | image | `apt: sqlite3` | Public-Domain | [upstream](https://www.sqlite.org/) |
| DuckDB CLI (`duckdb`) | image | `python: duckdb-cli` | MIT | [upstream](https://github.com/duckdb/duckdb-python-cli) |
| PostgreSQL psql (`psql`) | optional | `apt: postgresql-client` | PostgreSQL | [upstream](https://www.postgresql.org/docs/current/app-psql.html) |
| PostgreSQL pg_dump (`pg-dump`) | optional | `apt: postgresql-client` | PostgreSQL | [upstream](https://www.postgresql.org/docs/current/app-pgdump.html) |
| PostgreSQL pg_restore (`pg-restore`) | image | `apt: postgresql-client` | PostgreSQL | [upstream](https://www.postgresql.org/docs/current/app-pgrestore.html) |
| pgcli (`pgcli`) | optional | `python: pgcli` | BSD-3-Clause | [upstream](https://github.com/dbcli/pgcli) |
| mycli (`mycli`) | optional | `python: mycli` | BSD-3-Clause | [upstream](https://github.com/dbcli/mycli) |
| SQLFluff (`sqlfluff`) | image | `python: sqlfluff` | MIT | [upstream](https://github.com/sqlfluff/sqlfluff) |
| SQLGlot (`sqlglot`) | optional | `python: sqlglot` | MIT | [upstream](https://github.com/tobymao/sqlglot) |
| dbmate (`dbmate`) | optional | `binary: amacneil/dbmate` | MIT | [upstream](https://github.com/amacneil/dbmate) |
| Miller (`miller`) | image | `apt: miller` | BSD-2-Clause | [upstream](https://github.com/johnkerl/miller) |
| csvkit (`csvkit`) | image | `python: csvkit` | MIT | [upstream](https://github.com/wireservice/csvkit) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `sql-quality` | `sqlfluff-lint` |
| `sqlite-schema` | `sqlite-schema` |
| `duckdb-schema` | `duckdb-schema` |
| `backup-index` | `postgres-archive-list` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "database-pro"
}
```

```json
{
  "action": "doctor",
  "agent": "database-pro"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `duckdb-schema`

Read local DuckDB table metadata without write access.

Command: `/opt/box/agents/bin/duckdb -readonly {input} -json -c SELECT table_catalog, table_schema, table_name, table_type FROM information_schema.tables ORDER BY 1,2,3;`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Fixed system-catalog query only. No extension installation, user SQL, remote CSV URL or database mutation is accepted.

### `postgres-archive-list`

Inspect a PostgreSQL archive table of contents without restoring.

Command: `/opt/box/agents/bin/pg-restore --list {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Archive readability is not proof of a working restore. Full restore rehearsal requires a disposable matching-version PostgreSQL instance and explicit separate workflow.

### `sqlfluff-lint`

Lint a SQL file with a raw templater and explicit dialect.

Command: `/opt/box/agents/bin/sqlfluff lint --ignore-local-config --templater raw --dialect {dialect} --format json {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |
| `dialect` | string; default="ansi"; choices=ansi, postgres, mysql, sqlite, duckdb, bigquery, snowflake, tsql |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `sqlite-schema`

Read the schema of a local SQLite database in read-only mode.

Command: `/opt/box/agents/bin/sqlite -readonly {input} .schema`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Schema inventory only; not a successful restore or data-integrity certification. Extension loading and user SQL are not accepted.

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
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `csvkit-profile` | Produce CSV descriptive statistics as JSON. | none declared |
| `miller-count` | Count records in a supplied CSV file. | none declared |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

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
- **PostgreSQL psql:** No arbitrary SQL or connection credential argument is registered; configure a read-only service explicitly.
- **PostgreSQL pg_dump:** Client major version must be compatible with server. Database connections and credentials remain operator-managed.
- **pgcli:** Remote connection and credentials are explicitly configured outside the default execution recipes.
- **mycli:** Requires a separately authorized database connection; arbitrary SQL execution is not registered.
- **SQLFluff:** Templaters/plugins can execute Python or database-dependent project code. Fixed recipe selects the raw templater.
- **SQLGlot:** Library integration; no CLI or passing runtime probe is claimed.
- **dbmate:** Database URL comes from a separately scoped connection. Migration up/down/drop commands are not registered.
- **check-jsonschema:** External schema references can require network access.

## Limits and interpretation

- Schema listing is not data-integrity or restore verification.
- pg_dump/pg_restore major-version compatibility must be checked.
- Remote database connections and migration writes are separately authorized integrations.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
