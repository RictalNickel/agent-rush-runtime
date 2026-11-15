# AI Eval Lab

Evaluate retrieval and model behavior using versioned datasets and explicit scoring assumptions.

Profile ID: `ai-eval-lab`. Preferred family: `python`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# AI Eval Lab

Make model and retrieval changes measurable without confusing lexical metrics, model judgments and human quality labels.

## Working method

Define the user task, dataset split, sampling method, versioned prompts/models, cost limit and acceptance criteria. Start with deterministic retrieval metrics on supplied binary judgments and validate IDs/alignment. For model-based evaluation, review the configuration, prompt/provider endpoints, tools, hooks and sensitive data before enabling external calls. Record judge model/version, uncertainty and disagreement. Use paired cases and inspect failure clusters; guard against leakage and benchmark overfitting. Heavy model harnesses and NLP/model downloads are optional and must be explicitly provisioned.

## Deliverable

A reproducible evaluation dataset/configuration, metric definitions and denominators, case-level failure evidence, cost/provider assumptions and a decision tied to the measured task.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Promptfoo (`promptfoo`) | image | `npm: promptfoo` | MIT | [upstream](https://github.com/promptfoo/promptfoo) |
| DeepEval (`deepeval`) | optional | `python: deepeval` | Apache-2.0 | [upstream](https://github.com/confident-ai/deepeval) |
| Ragas (`ragas`) | optional | `python: ragas` | Apache-2.0 | [upstream](https://github.com/vibrantlabsai/ragas) |
| garak (`garak`) | optional | `python: garak` | Apache-2.0 | [upstream](https://github.com/NVIDIA/garak) |
| lm-evaluation-harness (`lm-eval`) | optional | `python: lm-eval` | MIT | [upstream](https://github.com/EleutherAI/lm-evaluation-harness) |
| SacreBLEU (`sacrebleu`) | image | `python: sacrebleu` | Apache-2.0 | [upstream](https://github.com/mjpost/sacrebleu) |
| ROUGE score (`rouge-score`) | optional | `python: rouge-score` | Apache-2.0 | [upstream](https://github.com/google-research/google-research/tree/master/rouge) |
| pytest (`pytest`) | image | `python: pytest` | MIT | [upstream](https://github.com/pytest-dev/pytest) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| DuckDB CLI (`duckdb`) | image | `python: duckdb-cli` | MIT | [upstream](https://github.com/duckdb/duckdb-python-cli) |
| csvkit (`csvkit`) | image | `python: csvkit` | MIT | [upstream](https://github.com/wireservice/csvkit) |
| coverage.py (`coverage`) | image | `python: coverage` | Apache-2.0 | [upstream](https://github.com/nedbat/coveragepy) |
| Presidio Analyzer (`presidio-analyzer`) | optional | `python: presidio-analyzer` | MIT | [upstream](https://github.com/data-privacy-stack/presidio) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `retrieval-metrics` | `json-check` → `rag-evaluate` |
| `model-eval` | `promptfoo-eval` |
| `eval-tests` | `pytest-test` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "ai-eval-lab",
  "workflow": "retrieval-metrics"
}
```

```json
{
  "action": "doctor",
  "agent": "ai-eval-lab",
  "workflow": "retrieval-metrics"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `json-check`

Validate strict JSON syntax and duplicate-key policy.

Command: `/opt/box/agents/bin/boxoro-analysis json-check '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `promptfoo-eval`

Run a reviewed evaluation configuration with one concurrent request.

Command: `/opt/box/agents/bin/promptfoo eval --config '{config}' --output '{artifacts}/promptfoo.json' --no-cache --no-progress-bar --max-concurrency 1`

Effects: project_code, network, mutation. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[100]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `config` | string; path; required |

Configuration can execute hooks and paid provider requests. Callers must review endpoints, limits and data sensitivity; credentials and provider entitlements are never inferred.

### `pytest-test`

Run Python tests with bounded output and JUnit evidence.

Command: `/opt/box/agents/bin/pytest -q --disable-warnings '--junitxml={artifacts}/pytest.xml' '{path}'`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Project tests/conftest/plugins execute code and can reach network or mutate their environment. Exit 5 (no tests collected) is incomplete. The runtime also checks the JUnit report so all-skipped suites and missing/invalid evidence cannot pass.

### `rag-evaluate`

Compute binary-judgment precision, recall, MRR and nDCG.

Command: `/opt/box/agents/bin/boxoro-analysis rag-evaluate '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Input JSON: {k:10,queries:[{id:"q1",relevant:["d1"],retrieved:["d2","d1"]}]}. IDs must be unique. Precision denominator is k. No judge model, embeddings, factuality or subjective quality score is inferred.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `coverage-run` | Profile branch coverage of a reviewed Python test script. | project_code, network, mutation |
| `coverage-report` | Report coverage from an explicitly supplied coverage database. | project_code |
| `duckdb-schema` | Read local DuckDB table metadata without write access. | none declared |
| `csvkit-profile` | Produce CSV descriptive statistics as JSON. | none declared |
| `sacrebleu-score` | Score paired reference and candidate translation lines. | none declared |
| `duckdb-columns` | Read DuckDB column names, types, nullability and defaults without write access. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Promptfoo:** Requires Node >=22.22; Node24 recommended. Provider calls and hooks can incur cost or execute code. No account is created; no red-team sweep by default. Reviewed exact npm override basic-ftp=6.2.6 fixes GHSA-c475-qrg2-pj4r and subsequent FTP advisories. get-uri 8.0.1 compatibility was exercised with local data/file/HTTP/FTP fixtures; lock regeneration and operation smoke remain required on upgrades.
- **DeepEval:** Some metrics require paid model calls. No login or cloud tracing is performed by default; install in an isolated eval environment.
- **Ragas:** Library integration; no CLI or passing runtime probe is claimed. Judge/embedding providers, model downloads and credentials are separate prerequisites; deterministic retrieval metrics are provided by boxoro-analysis.
- **garak:** Heavy model/detector dependencies and potentially large paid API usage. No default all-probe scan is registered.
- **lm-evaluation-harness:** Model weights, benchmark licenses, accelerators and storage are operator-provided. Optional installation permits hashed source distributions for packages without compatible wheels; build requirements must be locked and built with --no-build-isolation in the dedicated environment.
- **ROUGE score:** Library integration; no CLI or passing runtime probe is claimed. Lexical overlap is not factuality or human preference.
- **check-jsonschema:** External schema references can require network access.
- **DuckDB CLI:** Uses the official standalone CLI wheel, not the Python library pretending to expose a CLI. Extensions and network reads are not enabled by the fixed recipe.
- **coverage.py:** Coverage run requires project test dependencies, including pytest when using the registered recipe.
- **Presidio Analyzer:** Library integration; no CLI or passing runtime probe is claimed. Requires separately downloaded NLP models and language-specific validation; detection cannot prove absence of personal data.

## Limits and interpretation

- Built-in retrieval metrics are binary-judgment precision/recall/MRR/nDCG, not LLM factuality scoring.
- Provider calls may incur fees; no credentials, login, model downloads or paid account are inferred.
- ROUGE/BLEU are lexical signals and cannot establish truthfulness or general usefulness.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
