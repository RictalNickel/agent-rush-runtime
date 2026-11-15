# VisualQA

Capture controlled UI evidence and compare reviewed visual baselines.

Profile ID: `visualqa`. Preferred family: `browser`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# VisualQA

Find visual regressions while preserving the difference between an image change and a user-visible defect.

## Working method

Record browser revision, viewport, device scale, fonts, locale, theme, reduced motion and test data. Wait for deterministic rendering and review authorized masks for dynamic or sensitive regions. Capture the exact UI states relevant to the change, compare against a reviewed baseline and inspect the diff in context. Pixelmatch threshold is color sensitivity, not a permitted percentage of changed pixels. A dimension mismatch is a finding. Never approve or overwrite baselines solely to silence a failing comparison; explain why an intended change merits a new baseline.

## Deliverable

Baseline/candidate/diff artifacts, reproducible capture settings, issue locations and human interpretation, with a separate recommendation for intentional baseline changes.

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
| Pixelmatch (`pixelmatch`) | image | `npm: pixelmatch >=7 <9` | ISC | [upstream](https://github.com/mapbox/pixelmatch) |
| ODiff (`odiff`) | image | `npm: odiff-bin` | MIT | [upstream](https://github.com/dmtrKovalenko/odiff) |
| BackstopJS (`backstopjs`) | optional | `npm: backstopjs` | MIT | [upstream](https://github.com/garris/BackstopJS) |
| sharp-cli (`sharp-cli`) | image | `npm: sharp-cli` | MIT | [upstream](https://github.com/vseventer/sharp-cli) |
| ImageMagick compare (`imagemagick`) | image | `apt: imagemagick` | ImageMagick | [upstream](https://imagemagick.org/) |
| HTML Validate (`html-validate`) | image | `npm: html-validate` | MIT | [upstream](https://github.com/html-validate/html-validate) |
| Lighthouse (`lighthouse`) | image | `npm: lighthouse` | Apache-2.0 | [upstream](https://github.com/GoogleChrome/lighthouse) |
| axe-core CLI (`axe-cli`) | optional | `npm: @axe-core/cli` | MPL-2.0 | [upstream](https://github.com/dequelabs/axe-core-npm) |
| Pillow (`pillow`) | optional | `python: Pillow` | MIT-CMU | [upstream](https://github.com/python-pillow/Pillow) |
| Tesseract OCR (`tesseract`) | image | `apt: tesseract-ocr` | Apache-2.0 | [upstream](https://github.com/tesseract-ocr/tesseract) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `capture` | `playwright-screenshot` |
| `compare` | `pixelmatch-compare` |
| `regression-suite` | `backstop-test` |
| `odiff-baseline` | `odiff-compare` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "visualqa",
  "workflow": "capture"
}
```

```json
{
  "action": "doctor",
  "agent": "visualqa",
  "workflow": "capture"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `backstop-test`

Compare a reviewed browser scenario suite to existing baselines.

Command: `/opt/box/agents/bin/backstopjs test '--config={config}'`

Effects: project_code, network, mutation. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `config` | string; path; required |

No baseline approval/overwrite command is registered. Scenario URLs, browser scripts and artifact paths must be reviewed as project code. The project browser config must select /opt/box/chromium and the qualified container launch options; no automatic project-config rewrite is performed.

### `odiff-compare`

Compare two raster images with explicit pixel and layout findings.

Command: `/opt/box/agents/bin/odiff '{baseline}' '{candidate}' '{artifacts}/odiff.png' --threshold '{threshold}' --fail-on-layout --parsable-stdout`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[21, 22]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `baseline` | string; path; required |
| `candidate` | string; path; required |
| `threshold` | number; default=0.1; min=0; max=1 |

Qualified with odiff-bin 4.5.0: matching images exit 0, different dimensions exit 21, changed pixels exit 22 and invalid decode exits 1. Control viewport, fonts and data; threshold controls color sensitivity, not allowed changed-pixel percentage. Visual intent requires human review.

### `pixelmatch-compare`

Compare two PNG images and emit a pixel diff.

Command: `/opt/box/agents/bin/pixelmatch '{baseline}' '{candidate}' '{artifacts}/pixel-diff.png' '{threshold}'`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[65, 66]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `baseline` | string; path; required |
| `candidate` | string; path; required |
| `threshold` | number; default=0.1; min=0; max=1 |

Baseline and candidate must have controlled viewport, fonts, browser and data. Dimension mismatch is a finding. Tool threshold is color sensitivity, not allowed changed-pixel percentage.

### `playwright-screenshot`

Capture a full-page Chromium screenshot of a loopback app.

Command: `/opt/box/agents/bin/playwright screenshot --browser chromium --full-page --wait-for-timeout 1000 '{target}' '{artifacts}/screenshot.png'`

Effects: project_code, network, mutation. Tool timeout: 90 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `target` | string; loopback_url; required |

Screenshot is evidence, not an automatic visual pass. Page JavaScript may make additional network requests; review the local app and mask authorized dynamic/sensitive content in the project suite.

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
| `ocr-image` | Extract English OCR text from a local image. | none declared |
| `lighthouse-audit` | Capture Lighthouse lab metrics from a loopback page. | project_code, network, mutation |
| `axe-audit` | Fail on automated accessibility violations on a loopback page. | network, project_code |
| `html-validate-check` | Validate static HTML semantics and structure. | project_code |
| `imagemagick-compare` | Compute RMSE and a difference image for two PNG inputs. | mutation |
| `sharp-inspect` | Decode a local image and print bounded metadata without saving changes. | none declared |

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
- **Pixelmatch:** CLI returns 66 for pixel differences, 65 for dimension mismatch, 64 for usage. Doctor accepts the documented help/usage exit; resolved version is recorded in the install lock.
- **ODiff:** Package name odiff-bin exposes executable odiff; platform native binary must match the worker architecture.
- **BackstopJS:** Config/scripts execute JavaScript. Baseline approval is a separate human review step and is not registered as a default operation. Excluded from default images after the 2026-11-15 npm audit found unpatched extract-zip symlink traversal (GHSA-jmr9-qjv8-65gv, GHSA-7pqw-9j4j-h8q3), vulnerable zod 3.x (GHSA-mrhx-c7xc-m3w5) and basic-ftp dependencies. A supported patched dependency tree and browser qualification are required before use.
- **sharp-cli:** CLI is MIT; underlying sharp is Apache-2.0 and libvips is LGPL-2.1-or-later. Retain native dependency notices. Reviewed exact npm override sharp=0.35.5 fixes the bundled librsvg issue GHSA-wq5f-xc86-pv6w; the registered PNG metadata operation was qualified against this patch.
- **ImageMagick compare:** Ubuntu ImageMagick 6 command compare; installed security policy and resource limits restrict coders/delegates. Use raster inputs; no shell or external delegate options are accepted.
- **axe-core CLI:** Uses Selenium/ChromeDriver in addition to Chrome. Full image qualification must match browser and driver; no full accessibility conformance claim. Excluded from default browser image until a matching ChromeDriver is explicitly locked and qualified.
- **Pillow:** Library integration; no CLI or passing runtime probe is claimed.
- **Tesseract OCR:** Language data has separate packages, versions and licenses; English base model only unless operator adds more.

## Limits and interpretation

- Image differences are not automatically defects or accessibility failures.
- Font, GPU, browser and platform variations can create noise.
- Default recipes do not approve or overwrite stored baselines.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
