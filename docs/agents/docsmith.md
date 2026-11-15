# DocSmith

Build and validate technical documentation, reference pages, diagrams and document artifacts.

Profile ID: `docsmith`. Preferred family: `documents`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# DocSmith

Produce documentation that a reader can use to complete the intended task, with accurate examples and traceable source material.

## Working method

Identify the audience, desired action and authoritative code/specification. Reuse the project structure, then write examples that match real interfaces and failure modes. Validate Markdown/reStructuredText, links and style; build the actual site with strict warnings when dependencies are available. Render diagrams/documents and inspect the resulting artifact rather than assuming syntax success guarantees layout. Separate factual review from spelling/style checks. Project documentation hooks/plugins and remote image references need declared effects.

Use markdown-file for a single changed Markdown file and markdown-quality for a documentation directory. The file recipe treats its input literally; the recursive recipe requires a directory and runs from that directory. Keep generated/vendor trees outside the selected scope; zero matched files are incomplete coverage. Review markdownlint-cli2 configuration before allowing mutation: fix:true can rewrite the selected files even without a --fix argument.

## Deliverable

Edited source documentation, working examples, built/rendered artifacts where requested, link/style/build results and the remaining factual or visual-review limitations.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor` scoped to the intended `workflow` or `operation`; choose one selector, never both. This probes only the tools needed for that task. Use an unscoped profile `doctor` for image audits or a full availability inventory. Select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Pandoc (`pandoc`) | image | `apt: pandoc` | GPL-2.0-or-later | [upstream](https://pandoc.org/) |
| MkDocs (`mkdocs`) | image | `python: mkdocs` | BSD-2-Clause | [upstream](https://github.com/mkdocs/mkdocs) |
| Sphinx (`sphinx`) | image | `python: sphinx` | BSD-2-Clause | [upstream](https://github.com/sphinx-doc/sphinx) |
| doc8 (`doc8`) | image | `python: doc8` | Apache-2.0 | [upstream](https://github.com/PyCQA/doc8) |
| Vale (`vale`) | image | `binary: errata-ai/vale` | MIT | [upstream](https://github.com/errata-ai/vale) |
| markdownlint-cli2 (`markdownlint`) | image | `npm: markdownlint-cli2` | MIT | [upstream](https://github.com/DavidAnson/markdownlint-cli2) |
| Mermaid CLI (`mermaid-cli`) | image | `npm: @mermaid-js/mermaid-cli` | MIT | [upstream](https://github.com/mermaid-js/mermaid-cli) |
| lychee (`lychee`) | image | `binary: lycheeverse/lychee` | Apache-2.0 OR MIT | [upstream](https://github.com/lycheeverse/lychee) |
| codespell (`codespell`) | image | `python: codespell` | GPL-2.0-only | [upstream](https://github.com/codespell-project/codespell) |
| qpdf (`qpdf`) | image | `apt: qpdf` | Apache-2.0 | [upstream](https://github.com/qpdf/qpdf) |
| Poppler pdfinfo (`pdfinfo`) | image | `apt: poppler-utils` | GPL-2.0-or-later | [upstream](https://poppler.freedesktop.org/) |
| Prettier (`prettier`) | image | `npm: prettier` | MIT | [upstream](https://github.com/prettier/prettier) |
| Graphviz (`graphviz`) | image | `apt: graphviz` | EPL-1.0 | [upstream](https://graphviz.org/) |
| python-docx (`python-docx`) | optional | `python: python-docx` | MIT | [upstream](https://github.com/python-openxml/python-docx) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `markdown-quality` | `markdownlint-check` → `codespell-check` |
| `build-mkdocs` | `mkdocs-build` |
| `pdf-check` | `qpdf-check` → `pdf-info` |
| `prose-style` | `vale-check` |
| `markdown-file` | `markdownlint-file` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "docsmith",
  "workflow": "markdown-quality"
}
```

```json
{
  "action": "doctor",
  "agent": "docsmith",
  "workflow": "markdown-quality"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `codespell-check`

Find common spelling errors in supplied files.

Command: `/opt/box/agents/bin/codespell --quiet-level=2 '{path}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[65]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Dictionary-based suggestions can be false positives for names, source identifiers and locale-specific terms; reviewed allowlists remain necessary.

### `markdownlint-check`

Lint Markdown recursively beneath the supplied project directory.

Command: `/opt/box/agents/bin/markdownlint --no-globs '**/*.md'`

Target type: directory. The runtime rejects a file target before execution and runs this fixed glob from the selected directory.

Effects: project_code, mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

A directory path is required and checked before execution; for one changed Markdown file use markdownlint-file. The fixed **/*.md glob runs from the selected directory, so brackets and other glob characters in that directory's name are not reinterpreted. --no-globs ignores configured extra top-level globs. Project configuration/custom rules can execute JavaScript, and fix:true can rewrite selected files; CLI2 has no --no-fix switch, so mutation authorization and config review are required. Zero linted files are incomplete. Scope a documentation subtree to avoid generated or vendored content.

### `markdownlint-file`

Lint one explicitly supplied Markdown file.

Command: `/opt/box/agents/bin/markdownlint --no-globs ':{input}'`

Effects: project_code, mutation. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Use this targeted operation for a changed Markdown file; markdownlint-check requires a directory. CLI2's : prefix selects the input as a literal filename, including names containing glob characters. --no-globs prevents configured extra top-level globs from broadening this selection. Project custom rules can execute JavaScript and fix:true can rewrite the selected file; mutation authorization is required because CLI2 has no --no-fix switch. Zero linted files are incomplete. Formatting/factual accuracy remain separate concerns.

### `mkdocs-build`

Build documentation with strict warnings.

Command: `/opt/box/agents/bin/mkdocs build --strict --config-file '{config}' --site-dir '{artifacts}/site'`

Effects: project_code, network, mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `config` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `pdf-info`

Inspect page count, dimensions and PDF metadata.

Command: `/opt/box/agents/bin/pdfinfo '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

The result covers only the supplied inputs and this registered check. Missing dependencies or unsupported inputs are incomplete, not a passing quality claim.

### `qpdf-check`

Check PDF structure and stream integrity.

Command: `/opt/box/agents/bin/qpdf --check '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[2, 3]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

qpdf structural checks do not assess accessible reading order, visual layout or OCR accuracy. Warnings and structural errors fail this check.

### `vale-check`

Check a supplied prose file using installed style rules.

Command: `/opt/box/agents/bin/vale --output=JSON --minAlertLevel=warning '{input}'`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Installed rules and project style config determine coverage. The tool does not download styles automatically or certify factual accuracy.

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
| `graphviz-render` | Render a supplied DOT graph as SVG. | mutation |
| `sphinx-build` | Build HTML documentation and fail on warnings. | project_code, network, mutation |
| `doc8-check` | Lint reStructuredText/plaintext documentation. | none declared |
| `prettier-check` | Check formatting of one supplied file or directory. | project_code |
| `mermaid-render` | Render a supplied Mermaid diagram to SVG. | network, mutation |
| `pandoc-html` | Convert a local Markdown file to standalone HTML. | mutation |
| `pandoc-docx` | Convert supplied Markdown to Word for review. | network, mutation |
| `lychee-links` | Check links in a local document while excluding private addresses. | network |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Tool-specific qualifications

- **Vale:** Style packages must be installed separately; the default recipe does not run vale sync.
- **Mermaid CLI:** Chromium required. Render only local reviewed diagram content; output is a workspace artifact.
- **python-docx:** Library integration; no CLI or passing runtime probe is claimed.

## Limits and interpretation

- Spelling/style/PDF-structure tools do not establish factual accuracy or visual quality.
- MkDocs/Sphinx plugins execute project code.
- Remote publishing, external uploads and licensing decisions are not implicit.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
