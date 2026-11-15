# Specialist runtime and local analyses

The native `specialist` tool calls `/usr/bin/python3 -I /opt/box/bin/specialist` inside the existing owner-scoped workspace box. One reviewed catalog supplies specialist identities, registered operations, typed parameters, workflows, tool installation metadata and source references. Selecting a specialist supplies guidance; it does not change the worker image or its isolation policy.

## Requests and results

The helper accepts one JSON argument. Native tool calls use the same fields plus the outer `"tool":"specialist"` discriminator.

```json
{"action":"list"}
```

```json
{"action":"describe","agent":"testforge"}
```

```json
{"action":"describe","agent":"testforge","workflow":"python-gate"}
```

```json
{"action":"doctor","agent":"testforge","operation":"pytest-test","timeout_seconds":60}
```

```json
{"action":"run","agent":"testforge","operation":"pytest-test","path":"app","parameters":{},"allow_project_code":true,"allow_network":true,"allow_mutation":true,"timeout_seconds":120}
```

Use `describe` to obtain the exact operation and workflow names available to a specialist. `run` requires `agent` and `operation`; `workflow` requires `agent` and `workflow`. `describe` and `doctor` require an agent. `list` does not take one. `path`, `parameters` and effect declarations apply only to `run` and `workflow`. The request deadline is an integer from 10 to 540 seconds and defaults to 540.

For `describe` and `doctor`, optionally supply **one** `operation` or `workflow` selector. Selectors are mutually exclusive and must belong to the selected specialist. A scoped description contains only the selected operation contracts and tools, plus the union of `required_effects`; it omits the already-loadable skill and unrelated connector definitions. A scoped doctor probes each needed tool once, even when several workflow steps share it. The `scope` object records the selected operations/tools and excluded profile tools. A scoped pass certifies only those readiness probes. Omit selectors for the original full-profile behavior. Public HTTP/MCP profile-discovery endpoints still return the full catalog definition; these selectors apply to the native tool and helper.

`path` can be an existing regular file or directory within the workspace. Relative paths start at `/workspace`. For a file target, the command's working directory is its parent; `{path}` in the registered command still identifies the file. Typed relative path parameters are resolved against this working directory. Parent traversal, symlink components, special files, paths outside the workspace and specialist/security report storage are rejected. Output-path parameters must also remain inside the workspace.

A registered operation may constrain `target_type` to `file` or `directory`; omitted values mean `any`. The runner checks every selected step before creating reports or launching commands. The recursive Markdown operation requires a directory and runs a fixed `**/*.md` glob from that directory. Its single-file counterpart uses CLI2's `:{input}` literal-file form. Brackets and other glob metacharacters in a selected path therefore cannot broaden built-in Markdown file selection to a sibling path.

Parameters are validated against the operation's scalar schema. Unknown keys, wrong types, out-of-range values, unregistered enum values, control characters and option-shaped strings beginning with `-` are rejected. There is no request field for shell text, an executable, arbitrary arguments, an environment, package installation or a new catalog. Duplicate JSON keys and nonfinite numbers, including a floating-point overflow such as `1e999`, are rejected. Requests are limited to 32 KiB.

The helper's transport exit status is zero when it returns a valid execution result, including findings or missing coverage. Inspect `gate`:

| Gate | Meaning |
| --- | --- |
| `pass` | All selected checks completed with registered success exit codes and any operation-specific evidence checks. Read the operation notes: some commands record descriptive metrics without enforcing a quality threshold. |
| `fail` | A completed command returned a registered findings exit code or its validated native report contained failures. |
| `incomplete` | A tool was missing, blocked, unavailable, errored, timed out, was cancelled, or exceeded an output/artifact limit. This is not a passing assessment. |

Checks include operation/tool IDs, status, exit code, duration, source references, operation notes and paths to private logs. Native stdout and stderr are not placed directly in the summary. `reports.json` points to the complete structured execution report and `reports.markdown` to a human-readable report. Paths are relative to `/workspace`, under `.specialists/reports/<run-id>/`. Temporary per-run homes are removed when execution finishes.

Ten operation IDs also validate bounded native evidence. `bandit-scan` requires scanned Python source and no parser/scanner errors. `pytest-test`, `vitest-test` and `playwright-test` require a positive executed-test count; zero tests and all-skipped suites are incomplete even if the CLI exits zero. Reported collection, setup or global errors also make coverage incomplete. `trivy-config` requires a positive count of successful or failed configuration checks, and distinguishes actual findings from operational errors that share its findings exit code. Its report includes non-failures, so the validator uses explicit summary counts rather than counting every report entry as a failure.

Both Markdown recipes require a positive `Linting` file count and a consistent issue summary; an empty directory cannot pass with zero matched files. `i18next-status` requires completed analysis and a positive translation-key count; `i18next-unused` requires its completed summary. Both reject the locked CLI's source-parse, skipped-file and extraction-plugin failure diagnostics, even when other keys produce a 100% coverage summary. Invalid i18next configurations are incomplete rather than translation findings. `buf-breaking` requires compatibility diagnostics for its findings exit and rejects `COMPILE` diagnostics because compatibility never ran. Buf's `buf-lint` treats invalid source syntax as a contract-lint finding, while configuration/plugin execution failures remain incomplete. These parsers are qualified against the locked tool versions; review their report formats when updating dependencies.

The `qualification` object records the relevant counts. Missing, malformed, changed or larger-than-8-MiB qualification reports cannot pass. These checks do not establish that the project selected enough tests or appropriate quality thresholds, and commands without a specific evidence validator retain their documented CLI exit semantics.

A workflow runs sequentially under one shared deadline. It stops at the first findings or incomplete result and reports the remaining operation IDs. Shared parameters are passed only to steps that explicitly declare them; each step validates its own values. A workflow is not a transaction and does not roll back earlier project changes.

## Effect declarations and actual isolation

Registered operations declare any expected `project_code`, `network` and `mutation` effects. Every declared effect requires its matching request flag to be `true`. The flags document intended activity and do not independently grant user authorization. The owner/session policy and the existing worker remain the authority.

Project tests, executable configurations, plugins, build scripts and macros can run arbitrary project code. That code can write files or open connections regardless of the command's expected effect labels. CLI settings and these flags do not create an offline or read-only sandbox. The worker's gVisor/network policy and resource quotas enforce the actual boundary. Do not run untrusted project code on the assumption that a linter name makes it inert.

Buf lint/breaking configurations can launch local plugins, so those operations declare project-code execution. Markdownlint-cli2 can honor project `fix:true`; both recipes declare mutation as well as project code and use `--no-globs` to prevent project configuration from silently expanding the requested file set. Review executable/fixing configuration as part of choosing the operation.

Commands run without a shell, using the image-owned path `/opt/box/agents/bin/<tool-id>` and reviewed argv templates. A clean environment removes inherited secrets, proxies, `PYTHONPATH`, `NODE_OPTIONS`, Git user configuration and caller-specific tool settings. It supplies the pinned Node path `/opt/box/node/bin`, the image browser directory `/ms-playwright`, private home/cache/temp directories, noninteractive settings and telemetry suppression for supported tools. Provider credentials for OpenBB are kept in the separately configured trusted Finance service, outside workspace boxes.

HTTPie's `HTTPIE_CONFIG_DIR` is fixed to the image-owned `boxoro_agents/httpie-config/` directory. The configuration disables update-warning requests and has no inherited default options; the registered HTTPS request also requires certificate verification.

The runner defaults to `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1`. Isolated global pytest, Vitest, Playwright and similar installations do not automatically share every dependency in an application's `.venv` or `node_modules`. Missing imports, plugins, matching browser revisions, adapters or configuration are incomplete coverage. Install the application's dependencies in its documented environment. When that environment cannot use the registered global command, use the existing ordinary `bash` tool for an explicitly authorized project-specific test command. The native runner does not silently switch to an executable supplied by the project or install missing dependencies.

For tools using Chrome launcher or Puppeteer, `CHROME_PATH` and `PUPPETEER_EXECUTABLE_PATH` point to the image-owned `/opt/box/chromium` link. The fixed Pa11y and Mermaid recipes load JSON browser configurations from `boxoro_agents/browser-config/`; these use the existing container boundary and disable Chromium's nested sandbox. TLS certificate acceptance is not disabled. Project-driven BackstopJS, Pa11y CI and other custom browser configs still need compatible launch settings. Axe requires a matching ChromeDriver in addition to Chromium and remains an optional integration until qualified. Installed browser binaries alone are not an end-to-end browser test.

## Bounds and cancellation

| Boundary | Limit |
| --- | --- |
| Total request deadline | 10–540 seconds, including sequential workflow steps |
| Individual command | Its registered limit, capped by the remaining request budget |
| Doctor probe | At most 5 seconds, capped by the remaining request budget |
| Captured stdout / stderr | 8 MiB each; exceeding either terminates the process group and reports incomplete coverage |
| Child-created regular file | 256 MiB per file through `RLIMIT_FSIZE`; the worker volume supplies the total disk quota |
| Published native artifact | 25 MiB per file |
| Native artifact inventory | At most 1,024 entries, 64 nested directories and 100 MiB of accepted files per operation |
| Workflow length | At most 64 registered operations |

Output artifact links, special files, hard-linked files, changed files, oversized files and excess entries are not published in the artifact manifest. Their omission makes coverage incomplete. Collection uses no-follow directory descriptors, including the artifact root; report writes also refuse symlink ancestors. Private parent directories protect reports; native files can still contain source data, credentials printed by project code, browser traces or personal information. Review them before sharing.

The subprocess runs in a new process group. Timeout, output overflow, TERM, INT and HUP handling reap that group. Normal completion also removes descendants remaining in that group. This is process-group cleanup, not a separate containment mechanism for a deliberately daemonizing program; abrupt termination and intentionally escaped processes still depend on the box lifecycle.

## Doctor and provenance

`doctor` runs only registered noninteractive probes from a private directory. Prefer a scoped probe for the intended operation/workflow and use full-profile doctor for an image audit. It does not load project scripts, install dependencies, download missing browsers, connect an MCP service or qualify a device/host.

Every tool has a granular check and an availability classification. `runnable_operations` distinguishes a package or CLI probe from an implemented registered command. A successful probe with zero runnable operations means the library/CLI is installed; it does not invent a native adapter. Optional, host and external entries can make the overall doctor gate incomplete while individual installed operations remain usable.

Reports include the catalog hash and bounded selected-tool provenance from `/opt/box/agents/installed.json` when present. A probe's output is recorded separately from expected installation metadata. This evidence does not certify application dependencies, credentials, data entitlements, network routes, production readiness or complete end-to-end behavior.

## Builtin `boxoro-analysis` commands

These fixed commands run through `/opt/box/bin/specialist-analysis`; the installer exposes `/opt/box/agents/bin/boxoro-analysis`. They use the Python standard library and never evaluate project code. Their command exit codes are 0 for completion, 1 for findings and 2 for incomplete/invalid inputs, which the specialist runner classifies through the catalog.

All direct input files are bounded at 25 MiB. Directory analyses inspect at most 10,000 entries and 100 MiB of regular files. `.git`, `.specialists`, `.security`, dependency environments and common generated directories are excluded and listed. Unsupported or unreadable files are reported; a partial scan is incomplete.

| Command | Inputs | Output and limits |
| --- | --- | --- |
| `inventory` | File or directory | File paths, byte sizes and SHA-256 hashes, with explicit exclusions/skips. Hashes do not prove provenance or successful restore. |
| `json-check` | JSON file | Strict JSON syntax, duplicate-key and finite-number validation. This does not apply an application JSON Schema. |
| `csv-profile` | UTF-8 CSV file | Row/duplicate counts, missing and distinct counts, inferred types and numeric summaries. Requires comma-separated data, unique headers, at most 100,000 rows and 256 columns. No raw cell samples are returned. |
| `locale-check` | Source and translated JSON objects | Missing/extra leaf keys, nonstring values and common brace/printf placeholder differences. Keys use JSON Pointer notation. This is not a full ICU parser or translation-quality assessment. |
| `rag-evaluate` | JSON relevance judgments | Precision, recall, reciprocal rank, nDCG and average precision at a fixed `k`. The input supplies relevance judgments; the command does not decide whether answers are true. |
| `privacy-scan` | File or directory | Heuristic email, IPv4, phone-like, token-like and private-key-header matches, with redacted evidence and stable fingerprints. Patterns can miss sensitive data or produce false positives; filenames and locations still require review. |
| `knowledge-audit` | Markdown file or directory | Broken local inline links/common heading anchors and duplicate documents. It never fetches external links and does not validate generated routes, embedded HTML or reference-style links. |
| `scaffold` | New destination and Python package name | Creates `pyproject.toml`, a package initializer, `agent.json` and README. Requires mutation declaration through the catalog. Existing destinations are refused. It neither installs nor registers the generated package. |

The retrieval input format is:

```json
{
  "k": 3,
  "queries": [
    {"id": "q1", "relevant": ["doc-a", "doc-b"], "retrieved": ["doc-c", "doc-a", "doc-b"]}
  ]
}
```

IDs in each array must be unique. Binary relevance is used. Precision divides hits by `k`, including when fewer than `k` items were retrieved. Recall divides by the number of relevant items. Reciprocal rank uses the first relevant retrieved position. nDCG uses binary gain and logarithmic discount. Average precision divides by `min(k, relevant_count)`. Queries with no relevant judgments are excluded from recall, reciprocal-rank, nDCG and AP macro averages and counted separately. An execution `pass` means the calculation completed, not that the retrieval system met a quality target.

The scaffold package name must match `[a-z_][a-z0-9_]{0,63}`. Generated files are a starting point for a reviewed package, not a grant of capabilities or an automatically installed specialist.

## Verification

```sh
python3 -m unittest discover -s qa -p 'test_agents_runtime.py' -v
```

The runtime tests exercise request/catalog validation, real child processes, gates, argument boundaries, workspace paths, private artifact handling, process-group timeout/cancellation and deterministic builtin fixtures. Image builds, upstream CLIs, app environments and host/device integrations require the separate build/smoke/qualification steps. A passing Python fixture suite does not certify those external components.
