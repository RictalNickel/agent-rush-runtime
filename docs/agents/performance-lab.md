# Performance Lab

Measure latency, throughput, startup, CPU and memory under controlled workloads.

Profile ID: `performance-lab`. Preferred family: `python`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Performance Lab

Produce defensible measurements and isolate bottlenecks. Compare like-for-like workload, hardware, dependency state and data rather than presenting an unqualified speedup.

## Working method

Record the target, version, workload, warmup, concurrency, duration and acceptance threshold before measuring. Default load traffic must use an isolated authorized service and conservative limits. Review load scripts because they can override endpoints and methods. Use repeat runs and separate warmup from samples; report sample count and variance. Start with in-process profiling; py-spy attachment and hardware counters need a qualified host. Use Lighthouse as a lab report and a separate budget assertion for pass/fail. Tie any optimization to fresh equivalent before/after evidence.

## Deliverable

A benchmark recipe, raw JSON/profile artifacts, workload and environment record, measured bottleneck explanation, comparison with uncertainty and a practical regression budget.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Locust (`locust`) | image | `python: locust` | MIT | [upstream](https://github.com/locustio/locust) |
| Grafana k6 (`k6`) | image | `binary: grafana/k6` | AGPL-3.0-only | [upstream](https://github.com/grafana/k6) |
| Vegeta (`vegeta`) | image | `binary: tsenart/vegeta` | MIT | [upstream](https://github.com/tsenart/vegeta) |
| Hyperfine (`hyperfine`) | image | `apt: hyperfine` | Apache-2.0 OR MIT | [upstream](https://github.com/sharkdp/hyperfine) |
| Autocannon (`autocannon`) | image | `npm: autocannon` | MIT | [upstream](https://github.com/mcollina/autocannon) |
| Pyinstrument (`pyinstrument`) | image | `python: pyinstrument` | BSD-3-Clause | [upstream](https://github.com/joerick/pyinstrument) |
| Memray (`memray`) | image | `python: memray` | Apache-2.0 | [upstream](https://github.com/bloomberg/memray) |
| py-spy (`py-spy`) | host | `host: py-spy` | MIT | [upstream](https://github.com/benfred/py-spy) |
| Scalene (`scalene`) | optional | `python: scalene` | Apache-2.0 | [upstream](https://github.com/plasma-umass/scalene) |
| Lighthouse (`lighthouse`) | image | `npm: lighthouse` | Apache-2.0 | [upstream](https://github.com/GoogleChrome/lighthouse) |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| curl (`curl`) | image | `apt: curl` | curl | [upstream](https://curl.se/) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `http-baseline` | `autocannon-get` |
| `python-profile` | `pyinstrument-profile` |
| `memory-report` | `memray-summary` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "performance-lab"
}
```

```json
{
  "action": "doctor",
  "agent": "performance-lab"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `autocannon-get`

Measure a short GET load against a loopback service.

Command: `/opt/box/agents/bin/autocannon --connections {connections} --duration {seconds} --json --method GET {target}`

Effects: network. Tool timeout: 60 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |
| `connections` | integer; default=2; min=1; max=10 |
| `seconds` | integer; default=10; min=1; max=30 |

Latency/throughput capture only; no application-specific SLO is automatically enforced. GET endpoints must be safe and the target isolated.

### `memray-summary`

Summarize a supplied Memray allocation profile.

Command: `/opt/box/agents/bin/memray summary {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `pyinstrument-profile`

Capture a sampling profile of a reviewed Python script.

Command: `/opt/box/agents/bin/pyinstrument --renderer html --outfile {artifacts}/profile.html {script}`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `script` | string; path; required |

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
| `playwright-test` | Run the project browser suite with one worker and JSON output. | project_code, network, mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `curl-head` | Read HTTPS response headers with redirects disabled. | network |
| `locust-load` | Run a short, small-user load test against a loopback service. | project_code, network, mutation |
| `k6-load` | Execute a reviewed small bounded k6 script. | project_code, network, mutation |
| `hyperfine-startup` | Measure the fixed built-in analysis CLI startup cost. | mutation |
| `vegeta-report` | Summarize an existing Vegeta result file as JSON. | none declared |
| `memray-profile` | Capture allocations while executing a reviewed Python script. | project_code, network, mutation |
| `lighthouse-audit` | Capture Lighthouse lab metrics from a loopback page. | project_code, network, mutation |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Chrome DevTools MCP (`chrome-devtools-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

Local disposable Chrome/DevTools session; no assumed signed-in profile.

Use the verified chrome-devtools-mcp package for controlled trace, network and rendering inspection. Scope target pages and data before enabling.

Primary documentation: [source 1](https://github.com/ChromeDevTools/chrome-devtools-mcp).

### Grafana official MCP (`grafana-mcp`)

Status: **reference-not-connected**. Transport: `operator-configured-http-or-stdio`.

Explicit Grafana URL and narrowly scoped service-account token.

Run the vendor server with --disable-write and selected tool categories. Datasource SQL/query credentials must independently be read-only; do not enable raw query tools casually.

Primary documentation: [source 1](https://github.com/grafana/mcp-grafana), [source 2](https://github.com/grafana/mcp-grafana/blob/main/docs/sources/configure/enable-and-disable-tools.md).

### Tool-specific qualifications

- **Grafana k6:** AGPL licensed executable. Runs scripts against only authorized targets; script content can choose other URLs, so review remains necessary.
- **Vegeta:** The default report recipe performs no network load generation.
- **Hyperfine:** Registered recipe uses --shell=none and a fixed executable; user-supplied benchmark commands are not accepted.
- **Memray:** Linux/macOS wheels and native extensions; attached-process profiling is not enabled by default.
- **py-spy:** A host administrator must qualify ptrace/process permissions. No privileged container or arbitrary PID attachment is registered.
- **Scalene:** Optional native profiler with platform and memory overhead; use a dedicated profiling environment.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.

## Limits and interpretation

- Worker contention and virtualization affect timing.
- py-spy, hardware profiling and GPU measurements require host qualification.
- Capturing metrics alone does not enforce an SLO; no production load is generated by default.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
