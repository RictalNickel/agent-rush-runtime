# RestoreOps

Inspect backup artifacts and compare restored data with explicit recovery evidence.

Profile ID: `restoreops`. Preferred family: `platform`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# RestoreOps

Make recovery evidence concrete without assuming a readable archive is a successful restore. Preserve the original and operate on explicitly authorized copies.

## Working method

Record backup timestamp, source scope, encryption/recovery-key availability, format/version and target recovery objective. Inventory and inspect archives before extraction; check for traversal, links and ownership surprises. Run repository/format integrity checks with no repair/prune/delete flags. Compare a separately prepared restored tree against its source by content. A full restore rehearsal needs a disposable target and service-level verification; document that separately if the environment cannot perform it. Keep passphrases private and never print or commit them.

## Deliverable

Backup manifest and integrity evidence, comparison report, supported format/version requirements, recovery steps and explicit proof or lack of proof for recovery time and service correctness.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Restic (`restic`) | image | `apt: restic` | BSD-2-Clause | [upstream](https://github.com/restic/restic) |
| BorgBackup (`borg`) | image | `apt: borgbackup` | BSD-3-Clause | [upstream](https://github.com/borgbackup/borg) |
| rclone (`rclone`) | image | `apt: rclone` | MIT | [upstream](https://github.com/rclone/rclone) |
| rsync (`rsync`) | image | `apt: rsync` | GPL-3.0-or-later | [upstream](https://rsync.samba.org/) |
| age (`age`) | image | `apt: age` | BSD-3-Clause | [upstream](https://github.com/FiloSottile/age) |
| GnuPG (`gpg`) | image | `apt: gnupg` | GPL-3.0-or-later | [upstream](https://gnupg.org/) |
| SQLite CLI (`sqlite`) | image | `apt: sqlite3` | Public-Domain | [upstream](https://www.sqlite.org/) |
| PostgreSQL pg_dump (`pg-dump`) | optional | `apt: postgresql-client` | PostgreSQL | [upstream](https://www.postgresql.org/docs/current/app-pgdump.html) |
| PostgreSQL pg_restore (`pg-restore`) | image | `apt: postgresql-client` | PostgreSQL | [upstream](https://www.postgresql.org/docs/current/app-pgrestore.html) |
| Zstandard (`zstd`) | image | `apt: zstd` | BSD-3-Clause OR GPL-2.0-only | [upstream](https://github.com/facebook/zstd) |
| GNU tar (`tar`) | image | `apt: tar` | GPL-3.0-or-later | [upstream](https://www.gnu.org/software/tar/) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| jq (`jq`) | image | `apt: jq` | MIT | [upstream](https://jqlang.org/) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `archive-review` | `inventory` → `tar-list` |
| `compare-restore` | `rclone-check` |
| `restic-structure` | `restic-check` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "restoreops"
}
```

```json
{
  "action": "doctor",
  "agent": "restoreops"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `inventory`

Inventory bounded regular files with sizes and SHA-256 hashes.

Command: `/opt/box/agents/bin/boxoro-analysis inventory {path}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `rclone-check`

Compare source files to a local restored tree using content reads.

Command: `/opt/box/agents/bin/rclone check {source} {destination} --one-way --download --combined {artifacts}/rclone-check.txt`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `source` | string; path; required |
| `destination` | string; path; required |

Local paths only. Extra destination files are ignored by --one-way; mismatched or absent source files fail. Does not sync, delete or change data files.

### `restic-check`

Verify local repository structure without repair or deletion.

Command: `/opt/box/agents/bin/restic --repo {repository} --password-file {password_file} --no-lock check`

Effects: none declared. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `repository` | string; path; required |
| `password_file` | string; path; required |

Structural verification only; reading all backup payload bytes requires an explicit separate rehearsal. Repository must not be modified concurrently.

### `tar-list`

List archive members before a separately reviewed restore.

Command: `/opt/box/agents/bin/tar --list --file {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Listing is not content integrity or path-safety approval. No extraction command is registered; inspect absolute paths, traversal, symlinks and ownership before any restore.

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
| `json-type` | Validate JSON and report its top-level type. | none declared |
| `sqlite-schema` | Read the schema of a local SQLite database in read-only mode. | none declared |
| `postgres-archive-list` | Inspect a PostgreSQL archive table of contents without restoring. | none declared |
| `restic-snapshots` | List snapshots in a supplied local repository. | none declared |
| `borg-check` | Verify local Borg 1.x repository consistency without repair. | mutation |
| `rsync-dry-run` | Report checksum differences between two local directory trees. | none declared |
| `age-encrypt` | Encrypt a local file to a supplied public age recipient. | mutation |
| `gpg-key-inspect` | Inspect a supplied public OpenPGP key in batch mode. | mutation |
| `zstd-test` | Test a supplied zstd archive without extracting. | none declared |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Restic:** Encryption password supplied by operator secret store. No destructive forget/prune/restore command is registered by default.
- **BorgBackup:** Ubuntu Noble ships Borg 1.x; do not mix repository formats with Borg 2.x. Integrity checks may be expensive.
- **rclone:** Cloud remotes and credentials are not configured automatically; the registered check uses local workspace paths.
- **age:** Recipient keys are public; secret identity keys are not accepted by a registered default recipe.
- **GnuPG:** Registered inspection uses no keyserver, trust-database writes or decrypt operation.
- **PostgreSQL pg_dump:** Client major version must be compatible with server. Database connections and credentials remain operator-managed.

## Limits and interpretation

- Default recipes do not restore over live data, prune, forget, delete or repair repositories.
- Structural checks and archive listings do not prove recoverability.
- Encrypted backups require authorized key material; missing keys are an incomplete result.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
