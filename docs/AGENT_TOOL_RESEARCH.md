# Specialist tool research and implementation decisions

Research date: **2026-11-15**. This register describes the committed catalog, not a claim that every image, optional package or external account has been exercised.

## Implemented catalog

`deploy/agents/catalog.json` is the versioned source of truth for **21 profiles, 167 distinct tool entries, 156 fixed operations and 10 connector references**. Profiles reuse tools rather than creating per-agent duplicates. Every tool records its upstream source, license note, install coordinate, availability and probe. Every registered operation records a fixed argv suffix, typed parameters, effect declarations, exit-code interpretation and limits. The [tool-fit review](SPECIALIST_TOOL_REVIEW.md) verifies concrete recipes and records additional uses of existing tools.

The catalog classifies 118 entries as image recipes, 34 as optional packages/libraries, 11 as host integrations and 4 as external-service entries. A recipe is not an installation claim: inspect the resolved lock and run `doctor` on the exact worker. Import-only packages have an empty probe and remain optional until a reviewed adapter is installed. External MCP references are not active connections.

See [operating guides](agents/README.md), [runtime contract](SPECIALIST_RUNTIME.md), [Finance Lab](FINANCE_AGENT.md) and [OpenBB provider routing](OPENBB_PROVIDERS.md).

## Availability and evidence

| Classification | Meaning | Evidence required before use |
| --- | --- | --- |
| `image` | Portable CLI or first-party built-in included by an image recipe | Successful build, matching lock, live executable probe and task-specific smoke |
| `optional` | Supported package/library with non-default dependencies or resource needs | Reviewed optional install or project environment, adapter where needed, then real test |
| `host` | Requires a device, OS, kernel permission, SDK or human assistive-technology setup | Host qualification with platform/toolchain/device identity and actual task evidence |
| `external` | Requires a separately authorized account/service or deployment | Explicit scoped connection and service discovery, not just an environment-variable name |

`pass`, `fail` and `incomplete` describe the registered check. A successful inventory operation captures evidence; it does not certify everything it describes. Operation notes identify where a separate policy threshold or human review is required. Empty test collection, invalid inputs, missing dependencies, truncated output and timeouts cannot be presented as passing coverage.

## Image families and resource choices

Portable families are `core`, `python`, `browser`, `platform`, `data`, `documents` and `mobile`, plus the separately maintained finance family. A family image includes the union of image tools referenced by profiles assigned to that family; cross-family dependencies are explicit. A suite image combines portable image tools. The existing one-image-per-worker model remains: selecting an agent does not dynamically reroute or replace a worker image.

Python CLIs use isolated environments on Python 3.12; npm CLIs use isolated prefixes on Node 24. Playwright Test 1.64.1 is pinned to the matching `1.64.1-noble` browser image. Exact versions, transitive artifacts and checksums belong in generated locks. Runtime `npx @latest`, `uvx @latest`, remote install-script piping and implicit upgrades are not specialist operations. [Playwright Docker documentation](https://playwright.dev/docs/docker) explains package/browser coupling; [the repository](https://github.com/microsoft/playwright) is the framework source.

The image provides a stable `/opt/box/chromium` link and trusted Pa11y/Puppeteer configuration for the isolated worker. Lighthouse uses the matching container launch flags; the outer worker sandbox remains required. Project-driven BackstopJS and Pa11y CI configs must explicitly use the qualified browser/launch setup. Axe CLI remains optional until ChromeDriver is locked and tested against the browser. A version probe alone does not qualify browser execution.

Profile memory/disk allowances are planning estimates. Static checks commonly start at 2–4 GiB RAM; browser/data jobs at 4 GiB plus artifact space. Limit concurrency on shared workers. Model weights, GPU evaluation, SDK images and profiles may be far larger and are not silently added to every worker.

Android emulators require qualified host acceleration, system images and accepted SDK terms. Installed adb proves only that the client exists. Native iOS needs macOS/Xcode; screen-reader review needs the matching GUI platform and human interaction. See [Android Emulator requirements](https://developer.android.com/studio/run/emulator) and [Xcode](https://developer.apple.com/xcode/).

## Decisions requiring current source checks

| Area | Decision | Primary evidence |
| --- | --- | --- |
| Node engines | Use Node 24; stale README minimums cannot override current package engines | [Promptfoo installation](https://github.com/promptfoo/promptfoo/blob/main/site/docs/installation.md), [Prism metadata](https://github.com/stoplightio/prism/blob/main/packages/http/package.json) |
| DuckDB | Use official `duckdb-cli` exposing a real `duckdb` executable | [DuckDB CLI project](https://github.com/duckdb/duckdb-python-cli) |
| dbt | Constrain optional Python dbt-core to >=1.11,<2; do not silently substitute v2 product licensing | [dbt source](https://github.com/dbt-labs/dbt), [1.x source](https://github.com/dbt-labs/dbt/tree/1.latest) |
| dlt | Install OSS `dlt[cli]`; dltHub commercial extras are separate | [OSS vs dltHub](https://github.com/dlt-hub/dlt/blob/devel/docs/website/docs/hub/getting-started/oss-and-dlthub.md) |
| Androguard | Pin published 4.1.4; upstream main documents a different unpublished 5.x design | [current status](https://github.com/androguard/androguard), [4.1.4 source](https://github.com/androguard/androguard/tree/v4.1.4) |
| Gitleaks | Modern dir command, redacted reports and explicit feature-complete maintenance note | [Gitleaks](https://github.com/gitleaks/gitleaks) |
| Pixelmatch | 65/66 are dimension/pixel findings; 64 is accepted only for help/usage probe | [Pixelmatch CLI](https://github.com/mapbox/pixelmatch) |
| Accessibility | Chrome/ChromeDriver/Puppeteer qualification is separate; no complete conformance claim | [axe packages](https://github.com/dequelabs/axe-core-npm), [Pa11y](https://github.com/pa11y/pa11y) |
| Model harness | Optional lm-eval may need hashed source distributions and locked build tools | [lm-evaluation-harness](https://github.com/EleutherAI/lm-evaluation-harness) |
| Test imports | Isolated global wrappers may not see the project environment; import/setup failure is not pass | [pytest import behavior](https://docs.pytest.org/en/stable/explanation/pythonpath.html), [Vitest](https://vitest.dev/guide/) |
| Schemathesis effects | Project configuration can import hooks; max-examples alone does not cap every phase | [configuration](https://schemathesis.readthedocs.io/en/stable/reference/configuration/), [CLI tutorial](https://schemathesis.readthedocs.io/en/stable/tutorials/cli/) |
| Coverage effects | Report plugins/file reporters can execute code even without running tests | [coverage plugins](https://coverage.readthedocs.io/en/latest/api_plugin.html) |

## Licenses and distribution

The complete source register below records upstream project license information. Distribution must also preserve transitive/native dependency notices. A permissive CLI license does not erase bundled-library obligations. GPL/AGPL/LGPL tools are explicitly labeled: examples include k6, Semgrep CE, Pa11y, Hadolint and GNU tools. This is packaging metadata, not a legal compatibility opinion. Review the resolved SBOM and actual source/license files before distribution.

The resolved package metadata was checked for changes that older summaries miss. REUSE includes GPL-3.0-or-later tool code plus Apache-2.0, CC0-1.0 and CC-BY-SA-4.0 components; current Translate Toolkit declares GPL-3.0-or-later; Semgrep CE 1.185.0 declares LGPL-2.1-or-later. MCP Inspector documents an Apache-2.0 transition while retaining MIT for contributions not relicensed, with CC-BY-4.0 documentation. Sources: [REUSE metadata](https://github.com/fsfe/reuse-tool/blob/main/pyproject.toml), [Translate Toolkit metadata](https://pypi.org/project/translate-toolkit/), [Semgrep metadata](https://pypi.org/project/semgrep/1.185.0/), [Inspector license](https://github.com/modelcontextprotocol/inspector/blob/main/LICENSE).

Model weights, benchmark datasets, SDK images, commercial extensions and provider data have separate terms. Finance maintains its own provider/entitlement list: OpenBB does not make paid/restricted data universally available. No accounts or purchases are created by these recipes. Optional lm-eval source builds use locked build requirements and hashed source artifacts in an isolated environment; build isolation is disabled so it cannot fetch an untracked backend.

The [dependency audit and remediation record](SPECIALIST_DEPENDENCIES.md) documents the 2026-11-15 npm and Python/finance lock audits. Reviewed exact overrides use basic-ftp 6.2.6 for Lighthouse CI/Promptfoo and sharp 0.35.5 for Sharp CLI. BackstopJS and Checkov remain optional because their current dependency trees include unresolved findings. Moderate advisories in Semgrep, finance caching and other packages are recorded explicitly; HTTPie's conflicting database records and updater mitigation are explained. A clean high/critical count is not a claim of no vulnerabilities. Re-audit the exact lock after every change.

## Execution and reporting

- Recipes accept no shell, command string, executable path, arbitrary argv, provider credential or unrestricted SQL. Paths remain within the owner workspace, typed selectors have bounds and time/output are capped.
- Static checks disable automatic fixes. Test/build/plugin operations declare expected code execution, mutation and network use. These are request checks, not a replacement for the worker sandbox.
- Default production deployment, database migrations, registry writes, release publishing, money movement and trades are absent. Tests/plugins can still perform additional behavior and require review.
- Newman collection execution remains optional until endpoint/method scope is reviewed. Prism/Appium need separate lifecycle and host qualification. An executable version is not a live API/device.
- Backup recipes inspect or compare; they do not restore over live data, prune, repair or delete repositories. An archive listing is not a demonstrated recovery.
- Redacted scans still reveal private paths and diagnostics. Third-party logs, screenshots, prompts and extracted documents can contain sensitive content. Review reports before sharing.
- Ignored/large files, absent parsers, missing CRD schemas, unavailable services and empty test suites are coverage gaps, not silent successes.

## Verified MCP integration register

Only official project/vendor implementations are listed. GitHub and Sentry have documented live HTTP MCP endpoints. Playwright, Chrome DevTools, AWS, Grafana, Brave and database Toolbox references identify real software requiring explicit installation/deployment and credentials where applicable. The catalog does not auto-launch or authenticate them.

Historical GitHub/Postgres/SQLite reference servers are archived. Do not present their old package names as maintained integrations. Use the official GitHub server and current vendor projects. [Maintained reference repository](https://github.com/modelcontextprotocol/servers); [archive](https://github.com/modelcontextprotocol/servers-archived).

| Connector | Vendor / transport | Reference or endpoint | Scope decision |
| --- | --- | --- | --- |
| `github-mcp` | GitHub / streamable-http | [GitHub official MCP](https://api.githubcopilot.com/mcp/) | Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization. |
| `playwright-mcp` | Microsoft / stdio | [Playwright MCP](https://github.com/microsoft/playwright-mcp) | Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients. |
| `chrome-devtools-mcp` | Google / stdio | [Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp) | Use the verified chrome-devtools-mcp package for controlled trace, network and rendering inspection. Scope target pages and data before enabling. |
| `grafana-mcp` | Grafana / operator-configured-http-or-stdio | [Grafana official MCP](https://github.com/grafana/mcp-grafana) | Run the vendor server with --disable-write and selected tool categories. Datasource SQL/query credentials must independently be read-only; do not enable raw query tools casually. |
| `sentry-mcp` | Sentry / streamable-http | [Sentry MCP](https://mcp.sentry.dev/mcp) | Prefer a project-scoped /mcp/{organization}/{project} endpoint. Read/search issue and performance tools only by default; project management and incident writes need explicit authorization. |
| `aws-mcp` | AWS / operator-configured-stdio-or-http | [AWS official MCP toolkits](https://github.com/awslabs/mcp) | Select a named official service server or the current Agent Toolkit for AWS successor. Documentation/read-only diagnostics first; no automatic cloud resource creation. |
| `database-toolbox` | Google / operator-configured-stdio-or-http | [MCP Toolbox for Databases](https://github.com/googleapis/mcp-toolbox) | Use reviewed predefined parameterized queries/toolsets. Avoid default arbitrary execute_sql against production and do not assume a server-side read-only flag is an authorization boundary. |
| `brave-search-mcp` | Brave / stdio-or-operator-configured-http | [Brave Search MCP](https://github.com/brave/brave-search-mcp-server) | Use the official @brave/brave-search-mcp-server package for scoped research. Preserve source URLs and verify important claims on primary pages. |
| `mcp-reference` | Model Context Protocol project / stdio | [MCP reference SDK/server implementations](https://github.com/modelcontextprotocol/servers) | Use current maintained reference packages only after selecting a concrete server. This is a development reference, not a blanket entitlement to filesystem, Git or network tools. |
| `boxoro-finance-openbb` | Boxoro / OpenBB / streamable-http | [Boxoro Finance OpenBB](http://127.0.0.1:8768/mcp) | Four fixed read-only tools: openbb_status, openbb_catalog, openbb_describe, openbb_query. GET commands from approved installed provider namespaces; POST only econometrics, quantitative and technical. No generic CLI, skill installation, user/system routes or trading. |

GitHub documents `X-MCP-Readonly: true` and toolset scoping. Grafana documents `--disable-write` and separately requires truly read-only datasource credentials for raw SQL/query capabilities. Sentry documents OAuth at `https://mcp.sentry.dev/mcp`, optionally scoped to an organization/project path. Google's database server is now `googleapis/mcp-toolbox`, formerly `genai-toolbox`. These names/boundaries were checked against their primary sources.

### Reference HTTP configuration

This is a reviewable example, not an applied account connection. The existing MCP client performs authentication and enforces account permissions.

```json
{
  "mcpServers": {
    "github-readonly": {
      "type": "http",
      "url": "https://api.githubcopilot.com/mcp/",
      "headers": {
        "X-MCP-Readonly": "true",
        "X-MCP-Toolsets": "repos,pull_requests"
      }
    },
    "sentry": {
      "type": "http",
      "url": "https://mcp.sentry.dev/mcp"
    }
  }
}
```

Sentry includes state-changing tools, so select read/search capabilities and a project-scoped endpoint before use. MCP write approval and account scopes remain necessary; this example authorizes no write or message. Finance's localhost reference points to the included authenticated service only after operator deployment.

## Complete tool/source register

Install version fields are input constraints; exact resolved versions are in the lock. Empty constraints mean resolve and lock at build time, not install latest during a task. First-party built-in source URIs point to this repository.

| Tool ID | Package / distribution | Availability | License note | Primary source |
| --- | --- | --- | --- | --- |
| `actionlint` | `binary: rhysd/actionlint` | image | MIT | [actionlint](https://github.com/rhysd/actionlint) |
| `adb` | `apt: adb` | image | Apache-2.0 | [Android Debug Bridge](https://android.googlesource.com/platform/packages/modules/adb/) |
| `age` | `apt: age` | image | BSD-3-Clause | [age](https://github.com/FiloSottile/age) |
| `androguard` | `python: androguard ==4.1.4` | image | Apache-2.0 | [Androguard](https://github.com/androguard/androguard/tree/v4.1.4) |
| `android-emulator` | `host: Android Emulator` | host | Android SDK terms and bundled component licenses | [Android Emulator](https://developer.android.com/studio/run/emulator) |
| `apkanalyzer` | `host: Android APK Analyzer` | host | Android SDK terms and Apache-2.0 source components | [Android APK Analyzer](https://developer.android.com/tools/apkanalyzer) |
| `apktool` | `apt: apktool` | image | Apache-2.0 | [Apktool](https://github.com/iBotPeaches/Apktool) |
| `appium` | `npm: appium` | optional | Apache-2.0 | [Appium](https://github.com/appium/appium) |
| `autocannon` | `npm: autocannon` | image | MIT | [Autocannon](https://github.com/mcollina/autocannon) |
| `aws-cli` | `external: awscli` | external | Apache-2.0 | [AWS CLI](https://github.com/aws/aws-cli) |
| `axe-cli` | `npm: @axe-core/cli` | optional | MPL-2.0 | [axe-core CLI](https://github.com/dequelabs/axe-core-npm) |
| `babel` | `python: Babel` | image | BSD-3-Clause | [Babel](https://github.com/python-babel/babel) |
| `backstopjs` | `npm: backstopjs` | optional | MIT | [BackstopJS](https://github.com/garris/BackstopJS) |
| `bandit` | `python: bandit` | image | Apache-2.0 | [Bandit](https://github.com/PyCQA/bandit) |
| `borg` | `apt: borgbackup` | image | BSD-3-Clause | [BorgBackup](https://github.com/borgbackup/borg) |
| `boxoro-analysis` | `builtin: boxoro 1` | image | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| `buf` | `npm: @bufbuild/buf` | image | Apache-2.0 | [Buf](https://github.com/bufbuild/buf) |
| `check-jsonschema` | `python: check-jsonschema` | image | Apache-2.0 | [check-jsonschema](https://github.com/python-jsonschema/check-jsonschema) |
| `checkov` | `python: checkov` | optional | Apache-2.0 | [Checkov](https://github.com/bridgecrewio/checkov) |
| `cloc` | `apt: cloc` | image | GPL-2.0-or-later | [cloc](https://github.com/AlDanial/cloc) |
| `codespell` | `python: codespell` | image | GPL-2.0-only | [codespell](https://github.com/codespell-project/codespell) |
| `conftest` | `binary: open-policy-agent/conftest` | image | Apache-2.0 | [Conftest](https://github.com/open-policy-agent/conftest) |
| `cookiecutter` | `python: cookiecutter` | image | BSD-3-Clause | [Cookiecutter](https://github.com/cookiecutter/cookiecutter) |
| `copier` | `python: copier` | image | MIT | [Copier](https://github.com/copier-org/copier) |
| `cosign` | `binary: sigstore/cosign` | optional | Apache-2.0 | [Cosign](https://github.com/sigstore/cosign) |
| `coverage` | `python: coverage` | image | Apache-2.0 | [coverage.py](https://github.com/nedbat/coveragepy) |
| `csvkit` | `python: csvkit` | image | MIT | [csvkit](https://github.com/wireservice/csvkit) |
| `curl` | `apt: curl` | image | curl | [curl](https://curl.se/) |
| `dbmate` | `binary: amacneil/dbmate` | optional | MIT | [dbmate](https://github.com/amacneil/dbmate) |
| `dbt-core` | `python: dbt-core >=1.11,<2` | optional | Apache-2.0 | [dbt Core 1.x](https://github.com/dbt-labs/dbt/tree/1.latest) |
| `deepeval` | `python: deepeval` | optional | Apache-2.0 | [DeepEval](https://github.com/confident-ai/deepeval) |
| `dependency-cruiser` | `npm: dependency-cruiser` | image | MIT | [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) |
| `detect-secrets` | `python: detect-secrets` | image | Apache-2.0 | [detect-secrets](https://github.com/Yelp/detect-secrets) |
| `detekt` | `host: detekt` | host | Apache-2.0 | [detekt](https://github.com/detekt/detekt) |
| `dig` | `apt: dnsutils` | image | MPL-2.0 | [BIND dig](https://www.isc.org/bind/) |
| `dlt` | `python: dlt[cli]` | image | Apache-2.0 | [dlt](https://github.com/dlt-hub/dlt) |
| `doc8` | `python: doc8` | image | Apache-2.0 | [doc8](https://github.com/PyCQA/doc8) |
| `duckdb` | `python: duckdb-cli` | image | MIT | [DuckDB CLI](https://github.com/duckdb/duckdb-python-cli) |
| `eslint` | `npm: eslint` | image | MIT | [ESLint](https://github.com/eslint/eslint) |
| `eslint-jsx-a11y` | `npm: eslint-plugin-jsx-a11y` | optional | MIT | [eslint-plugin-jsx-a11y](https://github.com/jsx-eslint/eslint-plugin-jsx-a11y) |
| `finance` | `builtin: boxoro-finance 1.0.0` | image | MIT (Boxoro code); numerical-package licenses are recorded in finance dependency notices. Optional Riskfolio/vectorbt profile is restricted. | [Boxoro Finance Lab](https://github.com/RictalNickel/agent-rush-runtime) |
| `formatjs` | `npm: @formatjs/cli` | image | MIT | [FormatJS CLI](https://github.com/formatjs/formatjs) |
| `frictionless` | `python: frictionless` | image | MIT | [Frictionless](https://github.com/frictionlessdata/frictionless-py) |
| `garak` | `python: garak` | optional | Apache-2.0 | [garak](https://github.com/NVIDIA/garak) |
| `gettext` | `apt: gettext` | image | GPL-3.0-or-later | [GNU gettext](https://www.gnu.org/software/gettext/) |
| `git` | `apt: git` | image | GPL-2.0-only | [Git](https://git-scm.com/) |
| `github-cli` | `external: gh` | external | MIT | [GitHub CLI](https://github.com/cli/cli) |
| `gitleaks` | `binary: gitleaks/gitleaks` | image | MIT | [Gitleaks](https://github.com/gitleaks/gitleaks) |
| `gpg` | `apt: gnupg` | image | GPL-3.0-or-later | [GnuPG](https://gnupg.org/) |
| `grafana` | `external: grafana/mcp-grafana` | external | Apache-2.0 | [Grafana MCP service](https://github.com/grafana/mcp-grafana) |
| `graphql-inspector` | `npm: @graphql-inspector/cli` | image | MIT | [GraphQL Inspector](https://github.com/graphql-hive/graphql-inspector) |
| `graphviz` | `apt: graphviz` | image | EPL-1.0 | [Graphviz](https://graphviz.org/) |
| `grpcurl` | `binary: fullstorydev/grpcurl` | image | MIT | [grpcurl](https://github.com/fullstorydev/grpcurl) |
| `grype` | `binary: anchore/grype` | image | Apache-2.0 | [Grype](https://github.com/anchore/grype) |
| `hadolint` | `binary: hadolint/hadolint` | image | GPL-3.0-only | [Hadolint](https://github.com/hadolint/hadolint) |
| `helm` | `binary: helm/helm` | image | Apache-2.0 | [Helm](https://github.com/helm/helm) |
| `html-tidy` | `apt: tidy` | image | Zlib | [HTML Tidy](https://github.com/htacg/tidy-html5) |
| `html-validate` | `npm: html-validate` | image | MIT | [HTML Validate](https://github.com/html-validate/html-validate) |
| `httpie` | `python: httpie` | image | BSD-3-Clause | [HTTPie](https://github.com/httpie/cli) |
| `hyperfine` | `apt: hyperfine` | image | Apache-2.0 OR MIT | [Hyperfine](https://github.com/sharkdp/hyperfine) |
| `hypothesis` | `python: hypothesis` | optional | MPL-2.0 | [Hypothesis](https://github.com/HypothesisWorks/hypothesis) |
| `i18next` | `npm: i18next-cli` | image | MIT | [i18next CLI](https://github.com/i18next/i18next-cli) |
| `imagemagick` | `apt: imagemagick` | image | ImageMagick | [ImageMagick compare](https://imagemagick.org/) |
| `jadx` | `host: JADX` | host | Apache-2.0 | [JADX](https://github.com/skylot/jadx) |
| `jq` | `apt: jq` | image | MIT | [jq](https://jqlang.org/) |
| `jscpd` | `npm: jscpd` | image | MIT | [jscpd](https://github.com/kucherenko/jscpd) |
| `k6` | `binary: grafana/k6` | image | AGPL-3.0-only | [Grafana k6](https://github.com/grafana/k6) |
| `ktlint` | `host: ktlint` | host | MIT | [ktlint](https://github.com/pinterest/ktlint) |
| `kubeconform` | `binary: yannh/kubeconform` | image | Apache-2.0 | [Kubeconform](https://github.com/yannh/kubeconform) |
| `kubectl` | `external: kubectl` | external | Apache-2.0 | [kubectl](https://kubernetes.io/docs/reference/kubectl/) |
| `kustomize` | `binary: kubernetes-sigs/kustomize` | image | Apache-2.0 | [Kustomize](https://github.com/kubernetes-sigs/kustomize) |
| `lighthouse` | `npm: lighthouse` | image | Apache-2.0 | [Lighthouse](https://github.com/GoogleChrome/lighthouse) |
| `lighthouse-ci` | `npm: @lhci/cli` | image | Apache-2.0 | [Lighthouse CI](https://github.com/GoogleChrome/lighthouse-ci) |
| `lingui` | `npm: @lingui/cli` | image | MIT | [Lingui CLI](https://github.com/lingui/js-lingui) |
| `linkinator` | `npm: linkinator` | image | MIT | [Linkinator](https://github.com/JustinBeckwith/linkinator) |
| `lizard` | `python: lizard` | image | MIT | [Lizard](https://github.com/terryyin/lizard) |
| `lm-eval` | `python: lm-eval` | optional | MIT | [lm-evaluation-harness](https://github.com/EleutherAI/lm-evaluation-harness) |
| `locust` | `python: locust` | image | MIT | [Locust](https://github.com/locustio/locust) |
| `lychee` | `binary: lycheeverse/lychee` | image | Apache-2.0 OR MIT | [lychee](https://github.com/lycheeverse/lychee) |
| `madge` | `npm: madge` | image | MIT | [Madge](https://github.com/pahen/madge) |
| `maestro` | `host: Maestro` | host | Apache-2.0 | [Maestro](https://github.com/mobile-dev-inc/Maestro) |
| `markdownlint` | `npm: markdownlint-cli2` | image | MIT | [markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2) |
| `mcp-inspector` | `npm: @modelcontextprotocol/inspector` | optional | Apache-2.0 AND MIT AND CC-BY-4.0 | [MCP Inspector](https://github.com/modelcontextprotocol/inspector) |
| `memray` | `python: memray` | image | Apache-2.0 | [Memray](https://github.com/bloomberg/memray) |
| `mermaid-cli` | `npm: @mermaid-js/mermaid-cli` | image | MIT | [Mermaid CLI](https://github.com/mermaid-js/mermaid-cli) |
| `miller` | `apt: miller` | image | BSD-2-Clause | [Miller](https://github.com/johnkerl/miller) |
| `mkdocs` | `python: mkdocs` | image | BSD-2-Clause | [MkDocs](https://github.com/mkdocs/mkdocs) |
| `mycli` | `python: mycli` | optional | BSD-3-Clause | [mycli](https://github.com/dbcli/mycli) |
| `mypy` | `python: mypy` | image | MIT | [mypy](https://github.com/python/mypy) |
| `newman` | `npm: newman` | optional | Apache-2.0 | [Newman](https://github.com/postmanlabs/newman) |
| `nox` | `python: nox` | image | Apache-2.0 | [Nox](https://github.com/wntrblm/nox) |
| `nvda` | `host: NVDA` | host | GPL-2.0-or-later | [NVDA](https://github.com/nvaccess/nvda) |
| `oasdiff` | `binary: oasdiff/oasdiff` | image | Apache-2.0 | [oasdiff](https://github.com/oasdiff/oasdiff) |
| `odiff` | `npm: odiff-bin` | image | MIT | [ODiff](https://github.com/dmtrKovalenko/odiff) |
| `openapi-generator` | `npm: @openapitools/openapi-generator-cli` | optional | Apache-2.0 | [OpenAPI Generator](https://github.com/OpenAPITools/openapi-generator-cli) |
| `openssl` | `apt: openssl` | image | Apache-2.0 | [OpenSSL](https://openssl-library.org/) |
| `opentofu` | `binary: opentofu/opentofu` | image | MPL-2.0 | [OpenTofu](https://github.com/opentofu/opentofu) |
| `pa11y` | `npm: pa11y` | image | LGPL-3.0-only | [Pa11y](https://github.com/pa11y/pa11y) |
| `pa11y-ci` | `npm: pa11y-ci` | image | LGPL-3.0-only | [Pa11y CI](https://github.com/pa11y/pa11y-ci) |
| `pandas` | `python: pandas` | optional | BSD-3-Clause | [pandas](https://github.com/pandas-dev/pandas) |
| `pandera` | `python: pandera` | optional | MIT | [Pandera](https://github.com/unionai-oss/pandera) |
| `pandoc` | `apt: pandoc` | image | GPL-2.0-or-later | [Pandoc](https://pandoc.org/) |
| `pdfinfo` | `apt: poppler-utils` | image | GPL-2.0-or-later | [Poppler pdfinfo](https://poppler.freedesktop.org/) |
| `pg-dump` | `apt: postgresql-client` | optional | PostgreSQL | [PostgreSQL pg_dump](https://www.postgresql.org/docs/current/app-pgdump.html) |
| `pg-restore` | `apt: postgresql-client` | image | PostgreSQL | [PostgreSQL pg_restore](https://www.postgresql.org/docs/current/app-pgrestore.html) |
| `pgcli` | `python: pgcli` | optional | BSD-3-Clause | [pgcli](https://github.com/dbcli/pgcli) |
| `pillow` | `python: Pillow` | optional | MIT-CMU | [Pillow](https://github.com/python-pillow/Pillow) |
| `pip-audit` | `python: pip-audit` | image | Apache-2.0 | [pip-audit](https://github.com/pypa/pip-audit) |
| `pixelmatch` | `npm: pixelmatch >=7 <9` | image | ISC | [Pixelmatch](https://github.com/mapbox/pixelmatch) |
| `playwright` | `npm: @playwright/test 1.64.1` | image | Apache-2.0 | [Playwright Test](https://github.com/microsoft/playwright) |
| `polars` | `python: polars` | optional | MIT | [Polars](https://github.com/pola-rs/polars) |
| `polib` | `python: polib` | optional | MIT | [polib](https://github.com/izimobil/polib) |
| `presidio-analyzer` | `python: presidio-analyzer` | optional | MIT | [Presidio Analyzer](https://github.com/data-privacy-stack/presidio) |
| `presidio-anonymizer` | `python: presidio-anonymizer` | optional | MIT | [Presidio Anonymizer](https://github.com/data-privacy-stack/presidio) |
| `prettier` | `npm: prettier` | image | MIT | [Prettier](https://github.com/prettier/prettier) |
| `prism` | `npm: @stoplight/prism-cli` | optional | Apache-2.0 | [Prism](https://github.com/stoplightio/prism) |
| `promptfoo` | `npm: promptfoo` | image | MIT | [Promptfoo](https://github.com/promptfoo/promptfoo) |
| `promtool` | `apt: prometheus` | image | Apache-2.0 | [Prometheus promtool](https://github.com/prometheus/prometheus) |
| `psql` | `apt: postgresql-client` | optional | PostgreSQL | [PostgreSQL psql](https://www.postgresql.org/docs/current/app-psql.html) |
| `py-spy` | `host: py-spy` | host | MIT | [py-spy](https://github.com/benfred/py-spy) |
| `pyinstrument` | `python: pyinstrument` | image | BSD-3-Clause | [Pyinstrument](https://github.com/joerick/pyinstrument) |
| `pypdf` | `python: pypdf` | optional | BSD-3-Clause | [pypdf](https://github.com/py-pdf/pypdf) |
| `pytest` | `python: pytest` | image | MIT | [pytest](https://github.com/pytest-dev/pytest) |
| `python-docx` | `python: python-docx` | optional | MIT | [python-docx](https://github.com/python-openxml/python-docx) |
| `qpdf` | `apt: qpdf` | image | Apache-2.0 | [qpdf](https://github.com/qpdf/qpdf) |
| `radon` | `python: radon` | image | MIT | [Radon](https://github.com/rubik/radon) |
| `ragas` | `python: ragas` | optional | Apache-2.0 | [Ragas](https://github.com/vibrantlabsai/ragas) |
| `rclone` | `apt: rclone` | image | MIT | [rclone](https://github.com/rclone/rclone) |
| `redocly` | `npm: @redocly/cli` | image | MIT | [Redocly CLI](https://github.com/Redocly/redocly-cli) |
| `restic` | `apt: restic` | image | BSD-2-Clause | [Restic](https://github.com/restic/restic) |
| `reuse` | `python: reuse` | image | Apache-2.0 AND CC0-1.0 AND CC-BY-SA-4.0 AND GPL-3.0-or-later | [REUSE](https://codeberg.org/fsfe/reuse-tool) |
| `ripgrep` | `apt: ripgrep` | image | Unlicense OR MIT | [ripgrep](https://github.com/BurntSushi/ripgrep) |
| `robotframework` | `python: robotframework` | image | Apache-2.0 | [Robot Framework](https://github.com/robotframework/robotframework) |
| `rouge-score` | `python: rouge-score` | optional | Apache-2.0 | [ROUGE score](https://github.com/google-research/google-research/tree/master/rouge) |
| `rsync` | `apt: rsync` | image | GPL-3.0-or-later | [rsync](https://rsync.samba.org/) |
| `ruff` | `python: ruff` | image | MIT | [Ruff](https://github.com/astral-sh/ruff) |
| `sacrebleu` | `python: sacrebleu` | image | Apache-2.0 | [SacreBLEU](https://github.com/mjpost/sacrebleu) |
| `scalene` | `python: scalene` | optional | Apache-2.0 | [Scalene](https://github.com/plasma-umass/scalene) |
| `schemathesis` | `python: schemathesis >=4,<5` | image | MIT | [Schemathesis](https://github.com/schemathesis/schemathesis) |
| `semgrep` | `python: semgrep` | image | LGPL-2.1-or-later | [Semgrep CE](https://github.com/semgrep/semgrep) |
| `sharp-cli` | `npm: sharp-cli` | image | MIT | [sharp-cli](https://github.com/vseventer/sharp-cli) |
| `shellcheck` | `apt: shellcheck` | image | GPL-3.0-or-later | [ShellCheck](https://github.com/koalaman/shellcheck) |
| `spectral` | `npm: @stoplight/spectral-cli` | image | Apache-2.0 | [Spectral](https://github.com/stoplightio/spectral) |
| `sphinx` | `python: sphinx` | image | BSD-2-Clause | [Sphinx](https://github.com/sphinx-doc/sphinx) |
| `sqlfluff` | `python: sqlfluff` | image | MIT | [SQLFluff](https://github.com/sqlfluff/sqlfluff) |
| `sqlglot` | `python: sqlglot` | optional | MIT | [SQLGlot](https://github.com/tobymao/sqlglot) |
| `sqlite` | `apt: sqlite3` | image | Public-Domain | [SQLite CLI](https://www.sqlite.org/) |
| `swiftlint` | `host: SwiftLint` | host | MIT | [SwiftLint](https://github.com/realm/SwiftLint) |
| `syft` | `binary: anchore/syft` | image | Apache-2.0 | [Syft](https://github.com/anchore/syft) |
| `tar` | `apt: tar` | image | GPL-3.0-or-later | [GNU tar](https://www.gnu.org/software/tar/) |
| `tesseract` | `apt: tesseract-ocr` | image | Apache-2.0 | [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) |
| `tflint` | `binary: terraform-linters/tflint` | image | MPL-2.0 | [TFLint](https://github.com/terraform-linters/tflint) |
| `tox` | `python: tox` | image | MIT | [tox](https://github.com/tox-dev/tox) |
| `trafilatura` | `python: trafilatura` | image | Apache-2.0 | [Trafilatura](https://github.com/adbar/trafilatura) |
| `translate-toolkit` | `python: translate-toolkit` | image | GPL-3.0-or-later | [Translate Toolkit](https://github.com/translate/translate) |
| `trivy` | `binary: aquasecurity/trivy` | image | Apache-2.0 | [Trivy](https://github.com/aquasecurity/trivy) |
| `trufflehog` | `binary: trufflesecurity/trufflehog` | optional | AGPL-3.0-only | [TruffleHog](https://github.com/trufflesecurity/trufflehog) |
| `typescript` | `npm: typescript` | image | Apache-2.0 | [TypeScript](https://github.com/microsoft/TypeScript) |
| `uv` | `python: uv` | image | Apache-2.0 OR MIT | [uv](https://github.com/astral-sh/uv) |
| `vale` | `binary: errata-ai/vale` | image | MIT | [Vale](https://github.com/errata-ai/vale) |
| `vegeta` | `binary: tsenart/vegeta` | image | MIT | [Vegeta](https://github.com/tsenart/vegeta) |
| `vitest` | `npm: vitest` | image | MIT | [Vitest](https://github.com/vitest-dev/vitest) |
| `voiceover` | `host: Apple VoiceOver` | host | Apple operating system terms | [Apple VoiceOver](https://www.apple.com/accessibility/vision/) |
| `xcodebuild` | `host: Xcode build/test tools` | host | Apple Xcode and Apple SDK agreements | [Xcode build/test tools](https://developer.apple.com/xcode/) |
| `yamllint` | `python: yamllint` | image | GPL-3.0-or-later | [yamllint](https://github.com/adrienverge/yamllint) |
| `yq` | `python: yq` | image | Apache-2.0 | [yq (kislyuk)](https://github.com/kislyuk/yq) |
| `zstd` | `apt: zstd` | image | BSD-3-Clause OR GPL-2.0-only | [Zstandard](https://github.com/facebook/zstd) |

## Verification status

Runtime tests and image/package qualification results belong in the execution report for the exact checkout and worker. This register does not claim that Docker builds, every optional dependency, every CLI recipe, paid provider calls or device qualification completed merely because entries exist. Run the committed tests and real-tool fixtures, then state the remaining gaps explicitly.
