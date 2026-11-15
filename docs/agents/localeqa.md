# LocaleQA

Check translation catalogs, placeholders, ICU syntax and localized UI prerequisites.

Profile ID: `localeqa`. Preferred family: `browser`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# LocaleQA

Protect meaning and application behavior across locales. Combine machine-checkable catalog contracts with language and UI review.

## Working method

Identify source locale, target locales, catalog format, plural rules, variables and fallback behavior. Compare keys and common placeholders, validate gettext/ICU syntax with the matching ecosystem and inspect untranslated/fuzzy coverage. Check number/date/currency presentation, text expansion, bidi/RTL, font coverage and accessibility in controlled browser states. Use a language reviewer for meaning, tone and culturally appropriate copy. i18next status checks missing source/translation keys; the separate unused-key report leaves catalogs unchanged and needs review for dynamically constructed keys. Select a suitable built-in SacreBLEU tokenizer (zh for Chinese) and retain the metric signature. Cloud sync, AI translation and migration commands are outside the default workflow.

## Deliverable

Catalog issues by locale/key, reproducible placeholder/plural failures, UI screenshots for relevant layouts, corrected strings when requested and a clear human-language-review checklist.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Babel (`babel`) | image | `python: Babel` | BSD-3-Clause | [upstream](https://github.com/python-babel/babel) |
| Translate Toolkit (`translate-toolkit`) | image | `python: translate-toolkit` | GPL-3.0-or-later | [upstream](https://github.com/translate/translate) |
| GNU gettext (`gettext`) | image | `apt: gettext` | GPL-3.0-or-later | [upstream](https://www.gnu.org/software/gettext/) |
| FormatJS CLI (`formatjs`) | image | `npm: @formatjs/cli` | MIT | [upstream](https://github.com/formatjs/formatjs) |
| Lingui CLI (`lingui`) | image | `npm: @lingui/cli` | MIT | [upstream](https://github.com/lingui/js-lingui) |
| i18next CLI (`i18next`) | image | `npm: i18next-cli` | MIT | [upstream](https://github.com/i18next/i18next-cli) |
| codespell (`codespell`) | image | `python: codespell` | GPL-2.0-only | [upstream](https://github.com/codespell-project/codespell) |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| HTML Validate (`html-validate`) | image | `npm: html-validate` | MIT | [upstream](https://github.com/html-validate/html-validate) |
| polib (`polib`) | optional | `python: polib` | MIT | [upstream](https://github.com/izimobil/polib) |
| Prettier (`prettier`) | image | `npm: prettier` | MIT | [upstream](https://github.com/prettier/prettier) |
| SacreBLEU (`sacrebleu`) | image | `python: sacrebleu` | Apache-2.0 | [upstream](https://github.com/mjpost/sacrebleu) |
| axe-core CLI (`axe-cli`) | optional | `npm: @axe-core/cli` | MPL-2.0 | [upstream](https://github.com/dequelabs/axe-core-npm) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `json-parity` | `locale-check` |
| `gettext-quality` | `gettext-check` → `translation-count` |
| `icu-catalog` | `formatjs-compile` |
| `translation-metrics` | `sacrebleu-score` |
| `i18next-coverage` | `i18next-status` |
| `i18next-unused` | `i18next-unused` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "localeqa",
  "workflow": "json-parity"
}
```

```json
{
  "action": "doctor",
  "agent": "localeqa",
  "workflow": "json-parity"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `formatjs-compile`

Validate ICU messages and emit compiled catalog JSON.

Command: `/opt/box/agents/bin/formatjs compile '{input}' --out-file '{artifacts}/messages.compiled.json'`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Assumes the standard FormatJS catalog shape. No --skip-errors is accepted, so parse failures remain visible.

### `gettext-check`

Check PO syntax/format strings and emit an isolated MO file.

Command: `/opt/box/agents/bin/gettext --check --check-format '--output-file={artifacts}/messages.mo' '{input}'`

Effects: mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `i18next-status`

Check missing translation keys using the existing i18next project configuration.

Command: `/opt/box/agents/bin/i18next status`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Qualified i18next-cli 1.88.0 exits 1 for missing translations or missing source-language keys. A completed report with a positive extracted-key count is required; configuration, parser and plugin failures and zero-key scans are incomplete. Review status.ignoreKeys/checkIdentical policy and static extraction coverage. Local configuration/plugins execute project code. No extraction, catalog rewrite, AI translation, upload or cloud sync is registered.

### `i18next-unused`

Report statically unused translation keys without changing catalogs.

Command: `/opt/box/agents/bin/i18next status --unused`

Effects: project_code. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Qualified i18next-cli 1.88.0 exits 1 when unused keys are reported and leaves translation files unchanged. A completed recognized summary is required; source, configuration and plugin failures remains incomplete even if a partial unused list was emitted. Dynamic key construction, ignored files and extraction plugins can limit static reachability, so review before removing a key. This operation does not extract, delete, translate or synchronize messages.

### `locale-check`

Check locale key and common placeholder parity.

Command: `/opt/box/agents/bin/boxoro-analysis locale-check '{source}' '{translation}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `source` | string; path; required |
| `translation` | string; path; required |

JSON string dictionaries; common brace and printf placeholders only. This is not a full ICU/plural, linguistic or visual RTL assessment.

### `sacrebleu-score`

Score paired reference and candidate translation lines.

Command: `/opt/box/agents/bin/sacrebleu '{reference}' --input '{candidate}' --metrics bleu chrf --format json --tokenize '{tokenizer}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `reference` | string; path; required |
| `candidate` | string; path; required |
| `tokenizer` | string; default="13a"; choices=13a, intl, zh, char, none |

Line alignment and target-language/tokenizer choice must be reviewed. The 13a default is unsuitable for unsegmented Chinese; select zh. char and none are explicit analysis choices, not substitutes for a validated Japanese/Korean tokenizer. These built-in tokenizers require no model downloads. Preserve the emitted metric signatures; lexical similarity and short-corpus BLEU do not establish factuality or overall translation quality.

### `translation-count`

Count translated/untranslated/fuzzy catalog units.

Command: `/opt/box/agents/bin/translate-toolkit --csv '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Coverage count only; does not determine linguistic correctness or successful rendering.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `json-check` | Validate strict JSON syntax and duplicate-key policy. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `playwright-test` | Run the project browser suite with one worker and JSON output. | project_code, network, mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |
| `codespell-check` | Find common spelling errors in supplied files. | none declared |
| `prettier-check` | Check formatting of one supplied file or directory. | project_code |
| `axe-audit` | Fail on automated accessibility violations on a loopback page. | network, project_code |
| `html-validate-check` | Validate static HTML semantics and structure. | project_code |
| `babel-compile` | Compile a supplied PO catalog using Python Babel checks. | mutation |
| `lingui-compile` | Compile reviewed Lingui catalogs with strict missing-message checks. | project_code, mutation |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Playwright MCP (`playwright-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

No cloud token required for a local disposable browser; authenticated browser sessions remain separately authorized.

Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients.

Primary documentation: [source 1](https://github.com/microsoft/playwright-mcp), [source 2](https://playwright.dev/docs/getting-started-mcp).

### Tool-specific qualifications

- **Translate Toolkit:** Current package metadata specifies GPL-3.0-or-later; redistribution must retain the package license files. The CLI lacks a version command, so --help is the readiness probe and installed-manifest metadata supplies the package version.
- **i18next CLI:** Official replacement path for older i18next-parser workflows. Localize, cloud sync and AI translation commands are not registered.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.
- **polib:** Library integration; no CLI or passing runtime probe is claimed.
- **axe-core CLI:** Uses Selenium/ChromeDriver in addition to Chrome. Full image qualification must match browser and driver; no full accessibility conformance claim. Excluded from default browser image until a matching ChromeDriver is explicitly locked and qualified.

## Limits and interpretation

- Built-in locale parity handles common brace/printf placeholders, not full ICU or linguistic meaning.
- BLEU/chrF and spelling suggestions are limited lexical signals.
- No translation-service account, automatic cloud sync or paid translation is enabled.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
