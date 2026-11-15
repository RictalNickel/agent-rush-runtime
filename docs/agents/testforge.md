# TestForge

Design and execute functional, regression, contract and browser tests.

Profile ID: `testforge`. Preferred family: `python`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# TestForge

Exercise observable behavior and important failure paths with the smallest useful test suite. Make failures reproducible and distinguish regressions, infrastructure failures and missing coverage.

## Working method

Identify the changed behavior, test entrypoint, disposable dependencies and required state. Use project tests and fixtures before inventing another harness. Test normal, boundary and negative cases. Keep generated API traffic on the explicit loopback target, GET-only in the default recipe. Run browser tests with the pinned browser version and stable fixtures. Report flakes honestly; never erase a failure through silent retry, skipped test collection or a weaker assertion. Verify that isolated wrappers can import the project dependencies before interpreting the result.

## Deliverable

Test changes, commands/versions, JUnit or JSON evidence, reproducible failures and a coverage statement naming untested platforms, services and paths.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| pytest (`pytest`) | image | `python: pytest` | MIT | [upstream](https://github.com/pytest-dev/pytest) |
| coverage.py (`coverage`) | image | `python: coverage` | Apache-2.0 | [upstream](https://github.com/nedbat/coveragepy) |
| tox (`tox`) | image | `python: tox` | MIT | [upstream](https://github.com/tox-dev/tox) |
| Nox (`nox`) | image | `python: nox` | Apache-2.0 | [upstream](https://github.com/wntrblm/nox) |
| Robot Framework (`robotframework`) | image | `python: robotframework` | Apache-2.0 | [upstream](https://github.com/robotframework/robotframework) |
| Schemathesis (`schemathesis`) | image | `python: schemathesis >=4,<5` | MIT | [upstream](https://github.com/schemathesis/schemathesis) |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| Vitest (`vitest`) | image | `npm: vitest` | MIT | [upstream](https://github.com/vitest-dev/vitest) |
| Newman (`newman`) | optional | `npm: newman` | Apache-2.0 | [upstream](https://github.com/postmanlabs/newman) |
| Locust (`locust`) | image | `python: locust` | MIT | [upstream](https://github.com/locustio/locust) |
| Hyperfine (`hyperfine`) | image | `apt: hyperfine` | Apache-2.0 OR MIT | [upstream](https://github.com/sharkdp/hyperfine) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| Hypothesis (`hypothesis`) | optional | `python: hypothesis` | MPL-2.0 | [upstream](https://github.com/HypothesisWorks/hypothesis) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Ruff (`ruff`) | image | `python: ruff` | MIT | [upstream](https://github.com/astral-sh/ruff) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `python-gate` | `ruff-check` → `pytest-test` |
| `browser-gate` | `playwright-test` |
| `api-contract` | `schemathesis-get` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "testforge",
  "workflow": "python-gate"
}
```

```json
{
  "action": "doctor",
  "agent": "testforge",
  "workflow": "python-gate"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `playwright-test`

Run the project browser suite with one worker and JSON output.

Command: `/opt/box/agents/bin/playwright test --workers=1 --reporter=json '--output={artifacts}/playwright'`

Effects: project_code, network, mutation. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The runtime qualifies the JSON report: at least one test must execute, and missing evidence or runtime errors are incomplete. Project configuration and tests execute code; passing tests cover only the declared scenarios, not every browser or interaction.

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

### `schemathesis-get`

Generate bounded GET-only checks against a loopback API.

Command: `/opt/box/agents/bin/schemathesis run '{schema}' --url '{target}' --include-method GET --max-examples '{examples}' --max-failures 5`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `schema` | string; path; required |
| `target` | string; loopback_url; required |
| `examples` | integer; default=10; min=1; max=25 |

GET-only method selection reduces state-change risk but endpoints must honor safe-method semantics. Local schema references and server URLs must be reviewed; scope is the explicit loopback target. Project schemathesis.toml may load Python hooks and write cache/reports. max-examples bounds fuzz/stateful generation; example/coverage phases can add cases, so the runtime timeout remains the overall bound.

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
| `coverage-run` | Profile branch coverage of a reviewed Python test script. | project_code, network, mutation |
| `coverage-report` | Report coverage from an explicitly supplied coverage database. | project_code |
| `tox-test` | Run one named test environment from the project tox configuration. | project_code, network, mutation |
| `nox-session` | Run one reviewed Nox session without automatic package installation. | project_code, network, mutation |
| `robot-test` | Run a reviewed Robot Framework test file or directory. | project_code, network, mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `vitest-test` | Run the project JavaScript/TypeScript suite once. | project_code, network, mutation |
| `locust-load` | Run a short, small-user load test against a loopback service. | project_code, network, mutation |
| `hyperfine-startup` | Measure the fixed built-in analysis CLI startup cost. | mutation |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Playwright MCP (`playwright-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

No cloud token required for a local disposable browser; authenticated browser sessions remain separately authorized.

Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients.

Primary documentation: [source 1](https://github.com/microsoft/playwright-mcp), [source 2](https://playwright.dev/docs/getting-started-mcp).

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **coverage.py:** Coverage run requires project test dependencies, including pytest when using the registered recipe.
- **Schemathesis:** Generated requests may create, alter or delete records. Registered operation is constrained to GET requests on an explicitly supplied loopback API.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.
- **Newman:** Collection scripts execute JavaScript and collection URLs can target remote services. No default collection runner is registered until target/method scope is reviewed.
- **Hyperfine:** Registered recipe uses --shell=none and a fixed executable; user-supplied benchmark commands are not accepted.
- **check-jsonschema:** External schema references can require network access.
- **Hypothesis:** Library integration; no CLI or passing runtime probe is claimed. Install in the project test environment and run through pytest; this package has no general standalone test command.

## Limits and interpretation

- Newman collections require reviewed target/method scope and remain optional.
- Project code can make network requests and change test state.
- Coverage percentages do not establish correctness; no-tests-collected is incomplete.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
