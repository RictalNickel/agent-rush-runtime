# Accessibility

Combine automated accessibility and HTML checks with explicit manual-review requirements.

Profile ID: `accessibility`. Preferred family: `browser`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Accessibility

Find and fix concrete accessibility barriers, preserving both automated evidence and the human tasks that automation cannot assess.

## Working method

Identify important pages, states and user journeys, including validation errors and dialogs. Run semantic HTML and automated rules on a disposable loopback app, then inspect keyboard order, focus visibility/restoration, labels, zoom/reflow, reduced motion and error announcements. Use the target platform screen reader on a qualified host for manual checks. Connect a finding to the affected user task and a specific reproducible state. Re-run the affected interaction after a fix; a clean axe/Pa11y report does not justify a complete WCAG conformance claim.

## Deliverable

Issue list with route/state, impact, reproduction and rule references; remediated code when requested; before/after test evidence; and a manual-review matrix identifying untested assistive technologies.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| axe-core CLI (`axe-cli`) | optional | `npm: @axe-core/cli` | MPL-2.0 | [upstream](https://github.com/dequelabs/axe-core-npm) |
| Pa11y (`pa11y`) | image | `npm: pa11y` | LGPL-3.0-only | [upstream](https://github.com/pa11y/pa11y) |
| Pa11y CI (`pa11y-ci`) | image | `npm: pa11y-ci` | LGPL-3.0-only | [upstream](https://github.com/pa11y/pa11y-ci) |
| Lighthouse (`lighthouse`) | image | `npm: lighthouse` | Apache-2.0 | [upstream](https://github.com/GoogleChrome/lighthouse) |
| HTML Validate (`html-validate`) | image | `npm: html-validate` | MIT | [upstream](https://github.com/html-validate/html-validate) |
| ESLint (`eslint`) | image | `npm: eslint` | MIT | [upstream](https://github.com/eslint/eslint) |
| eslint-plugin-jsx-a11y (`eslint-jsx-a11y`) | optional | `npm: eslint-plugin-jsx-a11y` | MIT | [upstream](https://github.com/jsx-eslint/eslint-plugin-jsx-a11y) |
| HTML Tidy (`html-tidy`) | image | `apt: tidy` | Zlib | [upstream](https://github.com/htacg/tidy-html5) |
| NVDA (`nvda`) | host | `host: NVDA` | GPL-2.0-or-later | [upstream](https://github.com/nvaccess/nvda) |
| Apple VoiceOver (`voiceover`) | host | `host: Apple VoiceOver` | Apple operating system terms | [upstream](https://www.apple.com/accessibility/vision/) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `page-audit` | `pa11y-audit` |
| `markup` | `html-validate-check` → `html-tidy-check` |
| `browser-regressions` | `playwright-test` |
| `axe-qualified` | `axe-audit` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "accessibility",
  "workflow": "page-audit"
}
```

```json
{
  "action": "doctor",
  "agent": "accessibility",
  "workflow": "page-audit"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `axe-audit`

Fail on automated accessibility violations on a loopback page.

Command: `/opt/box/agents/bin/axe-cli '{target}' --exit --stdout`

Effects: network, project_code. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

Requires matching ChromeDriver/browser. Automated rules cover only part of accessibility; keyboard, screen-reader, zoom, language and context review remain required.

### `html-tidy-check`

Check static HTML syntax without rewriting it.

Command: `/opt/box/agents/bin/html-tidy -errors -quiet '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1, 2]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `html-validate-check`

Validate static HTML semantics and structure.

Command: `/opt/box/agents/bin/html-validate --formatter json '{input}'`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `pa11y-audit`

Run accessibility rules on a loopback page with JSON output.

Command: `/opt/box/agents/bin/pa11y --config /opt/box/agents/boxoro_agents/browser-config/pa11y.json --reporter json --timeout 30000 '{target}'`

Effects: network, project_code. Tool timeout: 90 seconds. Success codes: `[0]`; finding codes: `[2]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

Pa11y exit 2 denotes accessibility findings; exit 1 is an execution problem. Chrome must be provisioned and reachable by Puppeteer.

### `playwright-test`

Run the project browser suite with one worker and JSON output.

Command: `/opt/box/agents/bin/playwright test --workers=1 --reporter=json '--output={artifacts}/playwright'`

Effects: project_code, network, mutation. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The runtime qualifies the JSON report: at least one test must execute, and missing evidence or runtime errors are incomplete. Project configuration and tests execute code; passing tests cover only the declared scenarios, not every browser or interaction.

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
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `eslint-check` | Lint project source without fixing it. | project_code |
| `lighthouse-audit` | Capture Lighthouse lab metrics from a loopback page. | project_code, network, mutation |
| `pa11y-suite` | Run a reviewed accessibility suite configuration. | project_code, network, mutation |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Playwright MCP (`playwright-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

No cloud token required for a local disposable browser; authenticated browser sessions remain separately authorized.

Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients.

Primary documentation: [source 1](https://github.com/microsoft/playwright-mcp), [source 2](https://playwright.dev/docs/getting-started-mcp).

### Chrome DevTools MCP (`chrome-devtools-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

Local disposable Chrome/DevTools session; no assumed signed-in profile.

Use the verified chrome-devtools-mcp package for controlled trace, network and rendering inspection. Scope target pages and data before enabling.

Primary documentation: [source 1](https://github.com/ChromeDevTools/chrome-devtools-mcp).

### Tool-specific qualifications

- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.
- **axe-core CLI:** Uses Selenium/ChromeDriver in addition to Chrome. Full image qualification must match browser and driver; no full accessibility conformance claim. Excluded from default browser image until a matching ChromeDriver is explicitly locked and qualified.
- **ESLint:** Config, resolvers and plugins execute project JavaScript; automatic fixes disabled.
- **eslint-plugin-jsx-a11y:** Library integration; no CLI or passing runtime probe is claimed. npm package for a project ESLint configuration, not a standalone CLI.
- **NVDA:** Windows GUI host and human interaction required; automated page checks do not replace this review.
- **Apple VoiceOver:** Apple GUI host and human review required; no Linux container equivalent is asserted.

## Limits and interpretation

- Automated checks cover only a subset of accessibility requirements.
- NVDA needs Windows; VoiceOver needs an Apple host and human review.
- Matching ChromeDriver/browser is required for axe CLI; missing browser support is incomplete.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
