# Specialist tool fit and workflow review

Review date: **2026-11-15**. This review examined the catalog's tool assignments, fixed command arguments, parameter contracts, declared effects, result interpretation and profile workflows. It used the existing installed CLI environments for representative positive and negative fixtures. It did not install another dependency stack or exercise an external account.

## Assessment

The specialist assignments are broadly appropriate, but **167 tool entries must not be presented as 167 executable capabilities**. The catalog now contains **21 profiles, 167 tool entries, 156 registered operations, 77 named workflows and 10 connector references**. The changes add five useful operations from tools already installed, repair seven sets of existing recipe assumptions, and reduce avoidable discovery and CSV work.

| Catalog population | Count | Practical meaning |
| --- | ---: | --- |
| Image recipes | 118 | A build recipe exists. The exact worker still needs its lock, executable probe and task fixture. |
| Optional tools/libraries | 34 | Separate installation, adapters, browser qualification or resource review may be required. |
| Host prerequisites | 11 | Devices, OS/toolchains, privileges or human accessibility review must be qualified separately. |
| External integrations | 4 | A separately configured account/service is required. |
| Tool IDs with at least one registered operation | 120 | Includes three optional tools: Checkov, axe CLI and BackstopJS. Their operations are not an installation claim. |
| Tool IDs with no fixed operation | 47 | 31 optional entries, all 11 host entries, all four external entries, and the adb client. These remain references/prerequisites rather than callable specialist recipes. |

Keeping an optional library in a profile is useful when it names a relevant integration path. For example, Hypothesis belongs in a testing profile, but installing its package does not create a standalone property-testing recipe. Similarly, an adb client does not supply a device, and a database client does not supply a database connection. The [source register](AGENT_TOOL_RESEARCH.md) and each [operating guide](agents/README.md) retain these distinctions.

## Reproduced defects and corrections

### Source coverage and command scope

**Madge skipped a TypeScript-only directory.** The old `madge-cycles` command returned exit 0 with `[]` for two TypeScript files that imported each other. Madge's documented directory-discovery default is `['js']`. The recipe now explicitly includes `js,jsx,ts,tsx,mjs,cjs`; the same fixture returns exit 1 and the actual two-file cycle. Removing one import returns exit 0. Aliases and unsupported parser constructs still require coverage review; an empty cycle array is not a count of analyzed files. [Madge configuration](https://github.com/pahen/madge/blob/master/README.md).

**Markdown file requests could lint nothing.** Passing a file as the path to `markdownlint-check` constructed `README.md/**/*.md`. The installed CLI exited 0 with `Linting: 0 files`. The new `markdownlint-file` operation passes one explicit input, while the recursive operation declares `target_type: directory`; runtime preflight rejects a file target before launching the tool. Runtime qualification requires a positive `Linting:` count. It deliberately does not use the second count in `Summary: 0 issues in 0 files`, because that is also the valid summary for one clean file. [markdownlint-cli2 command and glob behavior](https://github.com/DavidAnson/markdownlint-cli2).

**Markdown configuration could rewrite files without a mutation declaration.** A local `.markdownlint-cli2.jsonc` containing `fix:true` rewrote a malformed heading even though the command had no `--fix` flag. CLI2 has no `--no-fix` switch; the command-line switch only supplies a default that configuration can override. Both Markdown recipes now declare `project_code` and `mutation`, and use `--no-globs` to prevent configured extra top-level globs from broadening the selection. A real fixture proves that a missing mutation declaration blocks execution, an authorized run honors `fix:true`, and an additional configured file remains unchanged. Review the configuration before authorizing mutation. [CLI2 configuration documentation](https://github.com/DavidAnson/markdownlint-cli2).

**Glob characters in Markdown input paths could broaden mutation scope.** `--no-globs` suppresses configured globs but does not make command arguments literal. With `fix:true`, requesting `README[1].md` also rewrote `README1.md`; embedding `docs[1]` in the recursive glob also rewrote a file under `docs1`. The file recipe now passes CLI2's documented `:{input}` literal-file form. The directory recipe runs the fixed `**/*.md` glob from the already selected directory, without interpolating that directory into the glob. Three actual-CLI regressions first reproduced both sibling mutations and the missing directory preflight. They now verify one-file coverage, unchanged unselected siblings, and rejection of a file target before execution. Custom rules still execute project code under the declared effects; these argument fixes constrain built-in file selection. [CLI2 literal paths and glob arguments](https://github.com/DavidAnson/markdownlint-cli2).

### Findings, errors and execution effects

**Buf's exit codes and plugin behavior were misrepresented.** Installed Buf 1.75.0 returns exit **100** for lint or compatibility diagnostics. The prior `buf-lint` entry classified exit 1 as findings and did not declare project-code execution. A harmless fixture plugin named in `buf.yaml` was actually launched, created its marker and exited 1. Both lint and the new compatibility recipe now require `project_code`, disable symlink traversal and classify exit 100 as findings. Configuration/plugin execution failures remain incomplete. Source-syntax diagnostics are lint findings; for `buf-breaking`, `COMPILE` diagnostics mean compatibility was not evaluated and are incomplete. [Buf check-plugin configuration](https://buf.build/docs/configuration/), [lint usage](https://buf.build/docs/lint/usage/), [breaking CLI](https://buf.build/docs/reference/cli/buf/breaking/).

**i18next status is a check, not merely inventory.** Installed i18next CLI 1.88.0 returned 0 for a complete English/German fixture and 1 after the German key was removed. The catalog formerly treated that exit as an operational failure. The status recipe now records missing translations as findings, while the runtime requires a completed report with a positive extracted-key count. Invalid configuration and partial parsing remain incomplete. This is especially important for the new unused-key report: malformed source still produced a plausible unused list and `Analysis complete`, but also reported that some source files could not be parsed. That warning now prevents a completed quality claim. A second fixture with one valid source file and one malformed source file returned exit 0 and 100% translation coverage while printing a skipped-file parser warning. Runtime qualification also rejects those partial scans. A reviewed fixture plugin that threw in onVisitNode likewise left a 100% status report while logging its failure; plugin failure diagnostics also make the result incomplete. [Official status and unused-key documentation](https://github.com/i18next/i18next-cli#status-locale).

**Profiler output did not imply successful workload completion.** A script that raised an exception still produced `memray.bin` and a success message about capture generation, then exited 1. The recipe had classified that exit as a finding despite having no memory-budget threshold. Nonzero workload exits are now incomplete, while the capture is retained for diagnosis. The real fixture checks both the incomplete status and preserved capture. Pyinstrument had the same exit-1 classification issue: a missing project import was labeled as a performance finding. Its nonzero workload exits are now also incomplete, with a real missing-dependency fixture. [Memray run behavior](https://bloomberg.github.io/memray/run.html).

### Correct interpretation of data and state

**SacreBLEU lacked a way to select the appropriate tokenizer.** One identical Chinese reference/candidate sentence scored BLEU 0 with the fixed default `13a` tokenizer and BLEU 100 with `zh`; chrF was 100 in both cases. The recipe now exposes a bounded enum of built-in tokenizers: `13a`, `intl`, `zh`, `char`, and `none`. This avoids model downloads while making the choice explicit. It does not claim that character tokenization replaces validated Japanese/Korean tokenizers, or that a lexical score establishes translation quality. Preserve the emitted metric signature. [SacreBLEU tokenization and signatures](https://github.com/mjpost/sacrebleu/blob/master/README.md).

**dlt listed the fresh runner home instead of the requested pipelines.** The runtime creates an isolated home for each operation. The former `dlt pipeline --list` therefore reported no pipelines even when the requested workspace directory contained pipeline folders. The recipe now uses the documented `--list-pipelines --pipelines-dir {path}` form. The fixture lists the supplied `fixture_pipeline` folder. This is directory/last-run-time inventory, not proof that a pipeline can load data successfully. [dlt CLI reference](https://dlthub.com/docs/reference/command-line-interface), [pipeline working directories](https://dlthub.com/docs/general-usage/pipeline#pipeline-working-directory).

**Generated report paths are not automatically reusable as inputs.** A Memray capture succeeded, but passing its returned `.specialists/.../artifacts/memray.bin` path to the summary operation was rejected by the existing internal-storage policy. The documented procedure now copies the verified artifact into an ordinary project file through the existing authorized file tools before running `memory-report`. The same handoff rule applies to a bundled API specification or other generated artifact used by a later request. The input boundary has not been weakened, and workflows do not imply automatic output-to-input wiring.

## More useful work from existing tools

| Operation or workflow | Added practical use | Validation and limits |
| --- | --- | --- |
| `buf-breaking` / `protobuf-compatibility` | Compare a supplied candidate with an exact local consumer baseline. | Identical schemas pass; changing a field from string to int32 fails with `FIELD_SAME_TYPE`; an invalid candidate is incomplete. Configuration/plugins remain reviewed inputs. |
| `redocly-bundle` / `bundle-contract` | Turn a specification with multiple files into one JSON review artifact. | A local referenced schema appears in the bundle; a missing reference fails without a forced partial artifact. Remote references and project preprocessors can fetch data/execute code. |
| `duckdb-columns` / expanded `duckdb-schema` | Review names, order, types, nullability and defaults alongside table inventory. | A three-column database returns the expected BIGINT, DECIMAL and VARCHAR metadata; its file hash is unchanged. This does not query application rows or validate integrity. |
| `i18next-unused` | Find candidate stale translation keys without deleting them. | A complete fixture passes; a retired key produces a finding while catalog bytes remain unchanged. Dynamic keys and parse gaps require review. |
| `markdownlint-file` / `markdown-file` | Check one changed Markdown document instead of recursively scanning its project. | Invalid/valid fixtures fail/pass; zero matches are incomplete; configured fixes require explicit mutation authorization. |
| `graphql-compatibility` | Make the existing GraphQL Inspector comparison discoverable as a workflow. | Identical SDL passes; removing a query field fails. The registered input remains local, with custom-loader effects declared. |
| `memory-profile` followed by `memory-report` | Capture a Python allocation workload before interpreting a Memray summary. | A small real allocation script produces `memray.bin`; summary succeeds after the verified artifact is copied to a normal project path. No memory budget is silently inferred. |

The new operations reuse existing install coordinates and locks. No libraries, model weights, browser drivers, cloud services or broad command-execution adapters were added. The official documentation supports [Redocly bundling](https://redocly.com/docs/cli/commands/bundle), [DuckDB information-schema queries](https://duckdb.org/docs/current/sql/meta/information_schema), [GraphQL schema comparison](https://the-guild.dev/graphql/inspector/docs/commands/diff), and [Memray capture](https://bloomberg.github.io/memray/run.html).

An attempted Redocly fixture also showed why an offline claim would be inappropriate: `--keep-url-references` still attempted to resolve a remote reference in the installed CLI. The new operation declares network access and does not rely on that flag as an egress control.

## Efficiency changes

**Discover the task, then probe its tools.** All 21 profile guides now show `describe` and `doctor` scoped to one named operation or workflow. A full-profile doctor remains useful for an image audit. Routine work should not probe unrelated optional packages, host-only integrations and external clients before checking a local file. The scoped metadata lists the required parameters/effects so callers can prepare one complete run request.

```json
{
  "action": "doctor",
  "agent": "api-architect",
  "operation": "buf-breaking"
}
```

Use `workflow` instead of `operation` to scope a workflow; the two selectors are mutually exclusive. A version probe still does not prove project-specific imports, configuration or browser support.

**The routine CSV workflow has two full-file consumers instead of three.** On the same 2,000-row CSV fixture, the old workflow completed `csv-profile`, `csvkit-profile`, and `frictionless-validate` with exit 0. The revised workflow completed `csv-profile` and `frictionless-validate` with exit 0. `csv-statistics` retains the richer csvkit operation as an explicit option. This is one fewer CLI launch and one fewer whole-input statistical analysis, not a measured universal latency or disk-I/O percentage improvement. csvkit can infer dates/types and emit frequent/minimum/maximum values, so it should run when those outputs answer an actual question. [csvstat behavior](https://csvkit.readthedocs.io/en/latest/scripts/csvstat.html).

**Choose the narrowest useful operation.** A source change does not always need both a general API linter and an organization-specific ruleset; `redocly-lint` is available independently when no Spectral policy is involved. Similarly, a quick cycle question can use `madge-cycles`, while the broader dependency workflow applies a separate dependency-cruiser policy. These are selection decisions, not weaker replacements for required project gates.

## Profile-by-profile fit

The matrix records how each profile should use its existing tools. It does not claim a live test of every listed tool or host.

| Profile | Appropriate starting work | Useful follow-up and boundary |
| --- | --- | --- |
| Agent Foundry | Inventory a small agent package, lint it, then run its real tests. | Local reviewed scaffolds and explicit schema checks are useful. Hypothesis and MCP Inspector remain project integrations; neither is a default agent framework or arbitrary server launcher. |
| TestForge | Select the actual Python, browser or loopback GET contract suite. | Use coverage reports for an explicit threshold and reviewed project dependencies. Empty suites and configuration failures remain incomplete. Newman still needs collection/target review. |
| API Architect | Choose OpenAPI, GraphQL or Protobuf checks for the actual contract. | New local-baseline Protobuf comparison and API bundling fill real gaps. Prism lifecycle, generator JARs and server implementation testing remain separate. |
| Performance Lab | Record a workload and environment, then run bounded loopback load or Python profiling. | Capture and summarize allocations with Memray. The fixed Hyperfine recipe measures helper startup only; host attachment and production load are not implied. |
| ReleasePilot | Tie secret/license/CI/container checks and SBOM evidence to the release candidate. | Reuse the SBOM for a separately scoped vulnerability audit. Inventory success is not a release approval, and signing/publishing/deploying remain separate actions. |
| CloudPilot | Inspect formatting/policy and validate or render local manifests/charts. | Keep schema, provider and plugin versions explicit. Checkov is optional; no cloud account, cluster connection or apply operation is inferred. |
| OpsWatch | Validate the supplied Prometheus rules/configuration and inspect incident evidence. | The current promtool operation is syntax validation, not an alert-behavior test or telemetry proof. Grafana/kubectl access remains a separately scoped integration. |
| Database Pro | Lint SQL with its actual dialect; inspect read-only database metadata or an archive index. | DuckDB now exposes column-level evidence. A schema or pg_restore listing does not prove integrity or a successful restore; remote clients and migrations remain separate. |
| Accessibility | Audit a qualified local page and validate relevant markup, then run browser regressions. | Pa11y and browser evidence support review; axe needs its separately qualified driver/browser. Screen-reader and human interaction review remain necessary. |
| AI Eval Lab | Validate judgments and compute retrieval metrics, or run a reviewed evaluation/test configuration. | SacreBLEU's explicit tokenizer and DuckDB metadata are available for appropriate evaluation datasets. Optional model/NER packages are not executable evaluation adapters or factuality guarantees. |
| Code Architect | Scope language/type/architecture checks to the actual source and reviewed policies. | Madge now sees TypeScript/JSX directories. Complexity and duplication are diagnostic signals; dynamic imports and unresolved aliases still limit static coverage. |
| DataWorks | Use the two-step CSV workflow or an explicit JSON/SQL contract. | Ask for rich csvkit statistics when needed; use dlt's supplied pipeline directory for inventory. Dataframes, dbt adapters and destinations remain project/environment requirements. |
| DocSmith | Check the changed Markdown file or documentation subtree; use strict project builds when relevant. | Use fixed Pandoc/Mermaid/Graphviz conversion recipes for appropriate artifacts. Review Markdown fix configuration; PDF syntax and prose lint do not verify facts or layout. |
| VisualQA | Capture a controlled browser state and compare an explicit baseline/candidate. | Use Pixelmatch or ODiff according to the needed evidence, not both by default. Pixel thresholds describe sensitivity; BackstopJS remains optional and no baseline is auto-approved. |
| PrivacyGuard | Choose redacted local pattern/secret checks, reviewed static rules or pinned-dependency audit. | Preserve scanner limitations and skipped coverage. Presidio needs a reviewed model/adapter; no legal compliance, live secret verification or data deletion is implied. |
| LocaleQA | Check keys/placeholders and the catalog's actual gettext/ICU/i18next ecosystem. | Missing/unused i18next checks and explicit BLEU tokenization are now useful gates/signals. Static unused keys need review; linguistic meaning, RTL and rendered expansion still need humans/browser evidence. |
| RestoreOps | Inspect supplied archives/repositories or compare a restored local tree with its source. | rclone's registered one-way check ignores extra destination files; rsync dry-run lists changes without enforcing equality. No pruning, destructive restore or recoverability certification is provided. |
| SiteHealth | Audit a qualified loopback page or check the explicitly selected links. | Apply Lighthouse CI budgets to reviewed reports instead of treating report capture as a score gate. DNS/certificate metadata do not establish uptime or complete TLS trust. |
| MobileLab | Start with authorized APK/source inspection or the existing mobile-web suite. | Appium, emulators, devices and native iOS tooling retain their real prerequisites. Installed adb and browser emulation are not native device testing. |
| Knowledge Auditor | Inventory the local corpus, check local links, then extract relevant PDF/HTML/OCR evidence. | The same read-only DuckDB metadata recipes can inspect a local corpus index. Extraction and lexical auditing do not establish semantic truth or external-source authority. |
| Finance Lab | Select a local analytical operation and inspect its backend prerequisites. | Preserve the separately maintained OpenBB discovery/provider routing and financial assumptions. This review changes discovery guidance only; no finance calculation, provider entitlement or trading behavior was changed. |

Potential further tools such as SQLite integrity checking or Prometheus rule-unit testing need a proper result adapter and real qualification before being promoted to fixed gates. A raw SQLite integrity-check command can return rows describing problems without a failing process status, so simply adding another argv would not be sufficient. [SQLite integrity and quick checks](https://www.sqlite.org/pragma.html#pragma_integrity_check). This review intentionally stops at the additions actually verified here.

## Executed validation and reproduction

The committed opt-in suite runs **19 tests against the actual installed CLIs through the real specialist request runner**. It first reproduced the old recipes' failures, then passed after the catalog and output-qualification corrections. The tests cover valid and invalid inputs, findings versus configuration failures, Buf plugin execution declarations, Markdown mutation/literal-file/directory scope, source-parse gaps, database immutability and read-only translation reporting.

```sh
BOXORO_TEST_SPECIALIST_TOOLS=1 python -m unittest qa.test_agents_tool_recipes -v
```

By default the test suite uses adjacent `specialist-tools/bin`, `specialist-js-tools/bin`, and `specialist-node/bin/node` installations. Set `BOXORO_TEST_SPECIALIST_PYTHON_BIN`, `BOXORO_TEST_SPECIALIST_JS_BIN`, and `BOXORO_TEST_SPECIALIST_NODE` for another qualified installation. A missing dependency is reported as a skip; the suite does not install it. For these fixtures, Node launchers point at the qualified Node binary because this host is not the `/opt/box` container filesystem.

| Installed component | Version used | Representative observed result |
| --- | --- | --- |
| Madge | 8.0.0 | TypeScript cycle fails; corrected acyclic input passes. |
| Buf | 1.75.0 | Lint/compatibility diagnostics exit 100; plugin/config errors exit 1; compatibility compile errors are incomplete. |
| markdownlint-cli2 / markdownlint | 0.24.1 / 0.42.1 | Correct literal-file and bracketed-directory scope, file-target rejection for recursion, zero-input rejection, declared fixes and no configured extra glob. |
| i18next CLI | 1.88.0 | Complete/missing/unused cases separated from malformed config/source. |
| SacreBLEU | 2.6.0 | Explicit Chinese tokenizer produces the expected signature and identical-corpus score. |
| Redocly CLI | 2.78.0 | Local reference bundle succeeds; missing reference produces no forced bundle. |
| DuckDB CLI package | 2.0.0 | Column metadata matches the fixture; database file hash unchanged. |
| dlt | 1.32.0 | Lists supplied pipeline folder rather than isolated runner-home state. |
| GraphQL Inspector | 7.0.4 | Additional workflow fixture: identical schema passes, removed field fails. |
| Memray | 1.20.0 | Capture and copied-input summary exit 0; a failing workload remains incomplete while retaining its capture. |
| csvkit / Frictionless | 2.2.0 / 5.21.0 | Additional 2,000-row workflow fixture: old three-consumer and new two-consumer checks both pass. |
| Pyinstrument | 5.1.3 | Missing project import is incomplete, with no performance-budget finding inferred. |
| Node | 24.22.3 | Used for the installed JavaScript CLI fixtures. |

This evidence is deliberately narrower than an all-tools certification. Docker images, apt packages, browser/device hosts, paid/provider calls, external MCP accounts, arbitrary project dependencies and every possible CLI input were not qualified by this review. The broader execution record is maintained in [SPECIALIST_VALIDATION.md](SPECIALIST_VALIDATION.md).
