# API Architect

Validate API contracts, compatibility, schema quality and generated-interface prerequisites.

Profile ID: `api-architect`. Preferred family: `browser`. Planning allowance: **4096 MiB RAM / 6144 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# API Architect

Keep consumer-facing contracts explicit and testable across OpenAPI, GraphQL and Protobuf. Trace proposed changes to actual client compatibility and observable server behavior.

## Working method

Locate the authoritative specification and exact released consumer baseline. Resolve local schema references and identify remote references before network use. Lint structure and organization rules, then compare the intended baseline and candidate for breaking changes using the matching OpenAPI, GraphQL or Protobuf operation. Use a Redocly bundle when a multi-file specification needs one reviewable artifact. Review Buf check plugins as executable project code. For a live contract test use a disposable loopback API and the registered GET-only recipe. Separate specification issues, implementation mismatches and policy decisions. A mock server, JVM generator or remote registry needs its separately documented prerequisites; do not treat a successful lint as proof the API implementation is correct.

## Deliverable

Versioned specification changes, compatibility findings with affected operations/fields, JSON lint/diff evidence, client migration notes and an explicit list of unresolved server checks.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Spectral (`spectral`) | image | `npm: @stoplight/spectral-cli` | Apache-2.0 | [upstream](https://github.com/stoplightio/spectral) |
| Redocly CLI (`redocly`) | image | `npm: @redocly/cli` | MIT | [upstream](https://github.com/Redocly/redocly-cli) |
| Prism (`prism`) | optional | `npm: @stoplight/prism-cli` | Apache-2.0 | [upstream](https://github.com/stoplightio/prism) |
| oasdiff (`oasdiff`) | image | `binary: oasdiff/oasdiff` | Apache-2.0 | [upstream](https://github.com/oasdiff/oasdiff) |
| Buf (`buf`) | image | `npm: @bufbuild/buf` | Apache-2.0 | [upstream](https://github.com/bufbuild/buf) |
| grpcurl (`grpcurl`) | image | `binary: fullstorydev/grpcurl` | MIT | [upstream](https://github.com/fullstorydev/grpcurl) |
| HTTPie (`httpie`) | image | `python: httpie` | BSD-3-Clause | [upstream](https://github.com/httpie/cli) |
| Schemathesis (`schemathesis`) | image | `python: schemathesis >=4,<5` | MIT | [upstream](https://github.com/schemathesis/schemathesis) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| GraphQL Inspector (`graphql-inspector`) | image | `npm: @graphql-inspector/cli` | MIT | [upstream](https://github.com/graphql-hive/graphql-inspector) |
| OpenAPI Generator (`openapi-generator`) | optional | `npm: @openapitools/openapi-generator-cli` | Apache-2.0 | [upstream](https://github.com/OpenAPITools/openapi-generator-cli) |
| curl (`curl`) | image | `apt: curl` | curl | [upstream](https://curl.se/) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `lint-contract` | `spectral-lint` → `redocly-lint` |
| `breaking-change` | `oasdiff-breaking` |
| `protobuf` | `buf-lint` |
| `bundle-contract` | `redocly-bundle` |
| `protobuf-compatibility` | `buf-breaking` |
| `graphql-compatibility` | `graphql-diff` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "api-architect",
  "workflow": "lint-contract"
}
```

```json
{
  "action": "doctor",
  "agent": "api-architect",
  "workflow": "lint-contract"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `buf-breaking`

Check Protobuf compatibility against an explicitly supplied local baseline.

Command: `/opt/box/agents/bin/buf breaking '{candidate}' --against '{baseline}' --error-format json --disable-symlinks`

Effects: project_code, network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[100]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `baseline` | string; path; required |
| `candidate` | string; path; required |

Both inputs must be reviewed local paths. Compare the released baseline with the candidate in that order. Buf configuration determines breaking rules; FILE is the upstream default. Check plugins may execute project-selected programs and module/policy references may fetch data. Rule findings exit 100; COMPILE diagnostics mean compatibility was not evaluated and are incomplete, as are configuration/plugin failures. No registry upload or code generation is invoked.

### `buf-lint`

Lint local Protobuf definitions.

Command: `/opt/box/agents/bin/buf lint '{path}' --error-format json --disable-symlinks`

Effects: project_code, network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[100]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Buf 1.75.0 emits lint and source-syntax diagnostics with exit 100; configuration and plugin execution errors exit 1 and remain incomplete. Local buf.yaml check plugins can execute project-selected programs even for lint; review plugins and policies before allowing project code. Remote module/policy references may use network/cache. Symlink traversal is disabled; no generation or publication command is registered.

### `graphql-diff`

Compare two supplied GraphQL schemas.

Command: `/opt/box/agents/bin/graphql-inspector diff '{baseline}' '{candidate}'`

Effects: project_code, network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `baseline` | string; path; required |
| `candidate` | string; path; required |

Prefer plain SDL inputs. Custom loaders can execute code or fetch resources; breaking-change policy is the tool default.

### `oasdiff-breaking`

Find breaking changes between two supplied API specifications.

Command: `/opt/box/agents/bin/oasdiff breaking '{baseline}' '{candidate}' --format json --fail-on ERR`

Effects: network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `baseline` | string; path; required |
| `candidate` | string; path; required |

External $ref values may fetch resources. Compare the exact consumer baseline to the candidate; normalization cannot infer intended compatibility policy.

### `redocly-bundle`

Bundle an API description and its references into one reviewable JSON artifact.

Command: `/opt/box/agents/bin/redocly bundle '{input}' --output '{artifacts}/openapi.bundle.json' --ext json`

Effects: project_code, network, mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Local references are resolved into the artifact; external references may make network requests. Redocly configuration, preprocessors and decorators can execute project code. No --force or removal of unused components is accepted. Unresolved references fail without producing a forced partial bundle. A bundle is a stable review input, not implementation validation or publication.

### `redocly-lint`

Validate and lint a local OpenAPI definition.

Command: `/opt/box/agents/bin/redocly lint '{input}' --format=json`

Effects: project_code, network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `spectral-lint`

Lint a local API specification with an explicit rule set.

Command: `/opt/box/agents/bin/spectral lint '{input}' --ruleset '{rules}' --format json --fail-severity warn`

Effects: project_code, network. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |
| `rules` | string; path; required |

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
| `jsonschema-validate` | Validate a supplied data file against an explicit schema. | network |
| `schemathesis-get` | Generate bounded GET-only checks against a loopback API. | project_code, network, mutation |
| `playwright-test` | Run the project browser suite with one worker and JSON output. | project_code, network, mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `grpc-descriptor-list` | List services from a local protobuf descriptor set. | none declared |
| `http-head-local` | Read headers from an explicitly named local HTTP service. | network |
| `curl-head` | Read HTTPS response headers with redirects disabled. | network |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Playwright MCP (`playwright-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

No cloud token required for a local disposable browser; authenticated browser sessions remain separately authorized.

Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients.

Primary documentation: [source 1](https://github.com/microsoft/playwright-mcp), [source 2](https://playwright.dev/docs/getting-started-mcp).

### Tool-specific qualifications

- **Prism:** Long-running service requires lifecycle orchestration outside one-shot checks. Current package metadata requires Node 24.22 or newer.
- **Buf:** Remote modules/plugins require network; breaking checks require a reviewed baseline.
- **HTTPie:** PYSEC-2023-242 reports an open affected range while the corresponding GHSA record lists a 3.2.3 fix. Upstream distinguishes the internal update-check TLS bypass from verified user requests. The fixed operation explicitly enables TLS verification and the runtime disables updates via image-owned configuration.
- **Schemathesis:** Generated requests may create, alter or delete records. Registered operation is constrained to GET requests on an explicitly supplied loopback API.
- **check-jsonschema:** External schema references can require network access.
- **GraphQL Inspector:** Schema loaders may execute project modules; supplied local SDL files are preferred.
- **OpenAPI Generator:** The npm wrapper downloads a separately versioned JVM generator. JRE, generator JAR version and hash must be locked before use; not treated as fully bundled by an npm lock alone.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.

## Limits and interpretation

- External schema references can fetch network data.
- Prism is a separately managed service; OpenAPI Generator additionally needs a locked JVM generator JAR.
- No default endpoint mutation, production fuzzing or remote API publication is performed.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
