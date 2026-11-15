# SiteHealth

Check page quality, links, HTML, accessibility, DNS and certificate evidence.

Profile ID: `sitehealth`. Preferred family: `browser`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# SiteHealth

Assess a defined site or local build through reproducible page and network checks, keeping crawl scope and measurement limits explicit.

## Working method

Identify important routes, environments and the allowed crawl domain. Prefer a local build for browser audits; record viewport and browser revision. Capture Lighthouse metrics and use separate reviewed budget assertions for pass/fail. Validate markup/accessibility and links, interpreting authentication, redirects, anti-bot and transient failures carefully. Read DNS or certificate metadata only for explicitly named inputs; neither alone establishes end-to-end availability or trust. Distinguish current lab evidence from real-user performance and historical uptime.

## Deliverable

Route-level findings, raw audit/link evidence, reproducible browser settings, prioritized fixes and a scope statement covering crawl exclusions and untested authenticated/user journeys.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Lighthouse (`lighthouse`) | image | `npm: lighthouse` | Apache-2.0 | [upstream](https://github.com/GoogleChrome/lighthouse) |
| Lighthouse CI (`lighthouse-ci`) | image | `npm: @lhci/cli` | Apache-2.0 | [upstream](https://github.com/GoogleChrome/lighthouse-ci) |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| axe-core CLI (`axe-cli`) | optional | `npm: @axe-core/cli` | MPL-2.0 | [upstream](https://github.com/dequelabs/axe-core-npm) |
| HTML Validate (`html-validate`) | image | `npm: html-validate` | MIT | [upstream](https://github.com/html-validate/html-validate) |
| Linkinator (`linkinator`) | image | `npm: linkinator` | MIT | [upstream](https://github.com/JustinBeckwith/linkinator) |
| lychee (`lychee`) | image | `binary: lycheeverse/lychee` | Apache-2.0 OR MIT | [upstream](https://github.com/lycheeverse/lychee) |
| curl (`curl`) | image | `apt: curl` | curl | [upstream](https://curl.se/) |
| OpenSSL (`openssl`) | image | `apt: openssl` | Apache-2.0 | [upstream](https://openssl-library.org/) |
| BIND dig (`dig`) | image | `apt: dnsutils` | MPL-2.0 | [upstream](https://www.isc.org/bind/) |
| check-jsonschema (`check-jsonschema`) | image | `python: check-jsonschema` | Apache-2.0 | [upstream](https://github.com/python-jsonschema/check-jsonschema) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Pa11y (`pa11y`) | image | `npm: pa11y` | LGPL-3.0-only | [upstream](https://github.com/pa11y/pa11y) |
| HTML Tidy (`html-tidy`) | image | `apt: tidy` | Zlib | [upstream](https://github.com/htacg/tidy-html5) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `page-health` | `lighthouse-audit` → `pa11y-audit` |
| `links` | `linkinator-check` |
| `documentation-links` | `lychee-links` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "sitehealth"
}
```

```json
{
  "action": "doctor",
  "agent": "sitehealth"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `lighthouse-audit`

Capture Lighthouse lab metrics from a loopback page.

Command: `/opt/box/agents/bin/lighthouse {target} --output=json --output-path={artifacts}/lighthouse.json --quiet --chrome-flags=--headless --disable-gpu --no-sandbox --disable-setuid-sandbox`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

This operation captures a report; it does not enforce performance/WCAG budgets. Use a reviewed Lighthouse CI assertion config for numerical gates. Lab scores vary with worker load. Browser sandbox flags match the isolated worker container; the outer worker sandbox remains required.

### `linkinator-check`

Check a bounded loopback page crawl.

Command: `/opt/box/agents/bin/linkinator {target} --recurse --concurrency 2 --timeout 5000 --format json`

Effects: network. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

Linked pages may reference external resources; caller must authorize the crawl scope. Runtime timeout/output caps bound execution, not all remote side effects.

### `lychee-links`

Check links in a local document while excluding private addresses.

Command: `/opt/box/agents/bin/lychee --format json --max-concurrency 4 --timeout 10 --no-progress --exclude-private --exclude-mail {input}`

Effects: network. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[2]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Private-address and mail links are intentionally excluded. Network/server/anti-bot failures require interpretation; a successful request does not verify source credibility.

### `pa11y-audit`

Run accessibility rules on a loopback page with JSON output.

Command: `/opt/box/agents/bin/pa11y --config /opt/box/agents/boxoro_agents/browser-config/pa11y.json --reporter json --timeout 30000 {target}`

Effects: network, project_code. Tool timeout: 90 seconds. Success codes: `[0]`; finding codes: `[2]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

Pa11y exit 2 denotes accessibility findings; exit 1 is an execution problem. Chrome must be provisioned and reachable by Puppeteer.

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
| `playwright-test` | Run the project browser suite with one worker and JSON output. | project_code, network, mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `curl-head` | Read HTTPS response headers with redirects disabled. | network |
| `lighthouse-assert` | Assert reviewed budgets against existing local Lighthouse reports. | project_code |
| `axe-audit` | Fail on automated accessibility violations on a loopback page. | network, project_code |
| `html-validate-check` | Validate static HTML semantics and structure. | project_code |
| `html-tidy-check` | Check static HTML syntax without rewriting it. | none declared |
| `certificate-inspect` | Inspect a local X.509 certificate. | none declared |
| `dns-query` | Read a DNS record for an explicit host. | network |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

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

### Sentry MCP (`sentry-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

OAuth through the existing account connector.

Prefer a project-scoped /mcp/{organization}/{project} endpoint. Read/search issue and performance tools only by default; project management and incident writes need explicit authorization.

Primary documentation: [source 1](https://mcp.sentry.dev/), [source 2](https://github.com/getsentry/toolkit).

### Tool-specific qualifications

- **Lighthouse CI:** Registered recipe asserts local results; upload, GitHub status and remote server commands are not registered. Reviewed exact npm override basic-ftp=6.2.6 fixes GHSA-c475-qrg2-pj4r and subsequent FTP advisories. get-uri 8.0.1 compatibility was exercised with local data/file/HTTP/FTP fixtures; lock regeneration and operation smoke remain required on upgrades.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.
- **axe-core CLI:** Uses Selenium/ChromeDriver in addition to Chrome. Full image qualification must match browser and driver; no full accessibility conformance claim. Excluded from default browser image until a matching ChromeDriver is explicitly locked and qualified.
- **check-jsonschema:** External schema references can require network access.

## Limits and interpretation

- Lab metrics are not real-user field data or uptime guarantees.
- Certificate inspection is not chain/hostname/revocation verification.
- Authenticated journeys and production monitoring need explicit account/integration setup.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
