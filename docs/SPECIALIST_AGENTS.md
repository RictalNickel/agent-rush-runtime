# Specialist agents in Boxoro

Boxoro includes 21 specialist profiles alongside the security stack. They use the existing owner-scoped sessions, skills, worker boxes, event stream and report downloads. The specialist catalog is executable configuration: the Rust control plane and Python runner consume the same reviewed file, rather than maintaining separate lists of capabilities.

Start with the [agent directory](agents/README.md), [tool research](AGENT_TOOL_RESEARCH.md), [runtime contract](SPECIALIST_RUNTIME.md), [Finance Lab guide](FINANCE_AGENT.md) and [OpenBB provider map](OPENBB_PROVIDERS.md). The [validation record](SPECIALIST_VALIDATION.md) separates executed checks from deployment qualification that still requires an operator host.

## What is implemented

- A native `specialist` tool with `list`, `describe`, `doctor`, `run` and `workflow` actions.
- 21 built-in `specialist-<id>` skills with responsibilities, procedures, output requirements and boundaries.
- Authenticated discovery through `GET /v1/specialists`, `GET /v1/specialists/{id}`, and public MCP tools `list_specialists` / `describe_specialist`.
- A specialist picker in the chat composer for new sessions. It adds the selected skill and readiness instruction to that session's first task; the original task remains its title. It does not change other sessions or the owner's saved configuration.
- An optional versioned owner preference, `AgentConfig.specialist`, which includes the profile instructions in future model prompts. Set it to a known catalog ID; use JSON `null` in a full config update or an empty string in an agent-config patch to clear it.
- A fixed-command runner with typed parameters, explicit declared effects, deadlines, process cleanup and private evidence artifacts.
- Eight reusable local analyses: file/hash inventory, strict JSON validation, CSV profiling, locale parity, retrieval metrics, redacted sensitive-pattern review, knowledge-corpus checks and Python agent scaffolding.
- Locked image builds, tool provenance, browser configuration, Finance analytics, and an authenticated OpenBB MCP service.

The catalog currently contains **167 tools, 151 registered operations and 10 connector records**. Of the tools, **118 are default image tools, 34 are optional, 11 require a separate host/service, and four represent external integrations**. A catalog record is not a claim that a service is connected. `doctor` distinguishes installed executables from missing optional packages and external prerequisites. The [dependency audit](SPECIALIST_DEPENDENCIES.md) records reviewed updates and retained advisories; successful resolution does not establish that every dependency is vulnerability-free.

## Agent directory

| Profile | Image family | Tool entries | Purpose |
| --- | --- | ---: | --- |
| [Agent Foundry](agents/agent-foundry.md) (`agent-foundry`) | `python` | 15 | Build and validate reviewable agent packages, tool contracts and evaluation scaffolds. |
| [TestForge](agents/testforge.md) (`testforge`) | `python` | 15 | Design and execute functional, regression, contract and browser tests. |
| [API Architect](agents/api-architect.md) (`api-architect`) | `browser` | 14 | Validate API contracts, compatibility, schema quality and generated-interface prerequisites. |
| [Performance Lab](agents/performance-lab.md) (`performance-lab`) | `python` | 13 | Measure latency, throughput, startup, CPU and memory under controlled workloads. |
| [ReleasePilot](agents/releasepilot.md) (`releasepilot`) | `platform` | 15 | Assemble release readiness evidence across source, CI, containers and supply chain. |
| [CloudPilot](agents/cloudpilot.md) (`cloudpilot`) | `platform` | 15 | Review infrastructure plans, Kubernetes configuration and cloud deployment prerequisites. |
| [OpsWatch](agents/opswatch.md) (`opswatch`) | `platform` | 13 | Inspect observability configuration and correlate explicitly connected incident evidence. |
| [Database Pro](agents/database-pro.md) (`database-pro`) | `data` | 14 | Inspect schemas, SQL quality, local analytical databases and backup archive structure. |
| [Accessibility](agents/accessibility.md) (`accessibility`) | `browser` | 12 | Combine automated accessibility and HTML checks with explicit manual-review requirements. |
| [AI Eval Lab](agents/ai-eval-lab.md) (`ai-eval-lab`) | `python` | 14 | Evaluate retrieval and model behavior using versioned datasets and explicit scoring assumptions. |
| [Code Architect](agents/code-architect.md) (`code-architect`) | `python` | 15 | Inspect dependency boundaries, complexity, types, duplication and static architecture risks. |
| [DataWorks](agents/dataworks.md) (`dataworks`) | `data` | 14 | Profile datasets, validate structure, inspect SQL transformations and prepare reproducible data checks. |
| [DocSmith](agents/docsmith.md) (`docsmith`) | `documents` | 15 | Build and validate technical documentation, reference pages, diagrams and document artifacts. |
| [VisualQA](agents/visualqa.md) (`visualqa`) | `browser` | 12 | Capture controlled UI evidence and compare reviewed visual baselines. |
| [PrivacyGuard](agents/privacyguard.md) (`privacyguard`) | `python` | 15 | Inspect sensitive-data exposure, secrets and dependency/configuration risks in supplied assets. |
| [LocaleQA](agents/localeqa.md) (`localeqa`) | `browser` | 14 | Check translation catalogs, placeholders, ICU syntax and localized UI prerequisites. |
| [RestoreOps](agents/restoreops.md) (`restoreops`) | `platform` | 13 | Inspect backup artifacts and compare restored data with explicit recovery evidence. |
| [SiteHealth](agents/sitehealth.md) (`sitehealth`) | `browser` | 14 | Check page quality, links, HTML, accessibility, DNS and certificate evidence. |
| [MobileLab](agents/mobilelab.md) (`mobilelab`) | `mobile` | 15 | Inspect Android packages and prepare device-qualified Android/iOS test workflows. |
| [Knowledge Auditor](agents/knowledge-auditor.md) (`knowledge-auditor`) | `documents` | 15 | Audit local corpus structure, source evidence, duplicates and extraction quality. |
| [Finance Lab](agents/finance-lab.md) (`finance-lab`) | `finance` | 1 | Financial and economic research across all installed permitted OpenBB V5 data commands, portfolio risk, valuation, options, bonds, cash flows and quantitative models. |

Shared tools appear in more than one profile. The count of tool entries per profile must not be added to estimate distinct installations. Operations are allowed through the profile's tool membership; shared utilities remain usable across the specialists that declare them.

## Start in the app

1. Deploy the updated Boxoro control plane and an appropriate worker image as described below.
2. Sign in and choose a specialist in the new-session composer, or send a task such as: “Use the specialist-testforge skill, inspect the project, run doctor, then test the app and report remaining coverage gaps.”
3. The agent loads the skill, calls `describe` for exact operations and parameters, and calls `doctor` against its actual worker.
4. Authorize the task's intended actions in the conversation. The model supplies the operation's effect declarations when applicable. These declarations describe the command; they do not replace user authorization or enforce a separate sandbox.
5. Review the summary and retrieve the private reports using the existing session files API or chat attachments.

A failed optional check does not prevent a separately installed operation from running. `doctor` is comprehensive and may report `incomplete` when host or optional integrations are absent. Check its per-tool statuses and runnable operations before selecting a workflow.

### Native calls

```json
{"tool":"specialist","action":"list"}
```

```json
{"tool":"specialist","action":"describe","agent":"testforge"}
```

```json
{"tool":"specialist","action":"doctor","agent":"finance-lab"}
```

A local CSV assessment requires no project code execution or outbound data access:

```json
{"tool":"specialist","action":"run","agent":"dataworks","operation":"csv-profile","path":"app","parameters":{"input":"data.csv"},"timeout_seconds":120}
```

An authorized project test executes project code and may write artifacts or make network requests:

```json
{"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","path":"app","allow_project_code":true,"allow_mutation":true,"allow_network":true,"timeout_seconds":300}
```

A finance operation can take a file as its target:

```json
{"tool":"specialist","action":"run","agent":"finance-lab","operation":"finance-risk","path":"research/prices.csv","parameters":{"column":"close","frequency":252,"input_kind":"prices"},"timeout_seconds":120}
```

Always read `describe` before choosing a workflow: some workflows require additional file parameters or effects. A workflow runs its registered operations in order under one shared deadline and stops on failure or incomplete coverage. Do not interpret an interrupted workflow as full coverage.

### Evidence and gates

Every run writes a JSON summary, a Markdown report, and bounded stdout/stderr or native artifacts below `.specialists/reports/<run-id>/`. Paths in results are relative to `/workspace`, so existing session downloads work unchanged. The report includes source links, declared effects, command exit status, durations, installed metadata and limitations.

- `pass`: the selected check completed according to its registered success codes. A metrics-only command does not thereby certify the application's quality.
- `fail`: at least one completed check returned registered findings.
- `incomplete`: missing prerequisites, blocked declarations, tool/configuration errors, timeout, cancellation or artifact limits prevented full coverage.

Raw native logs can contain project data or secrets. The summary avoids dumping them into chat, but they remain private evidence that must be reviewed before external publication. The existing security scanner has stronger finding-level secret redaction and remains the appropriate default for credential review.

## Images and dependencies

`deploy/specialists.Dockerfile` has ten targets. All workspace targets extend a reviewed Boxoro workspace base and default to UID/GID1000. The separately deployed `finance-service` target also runs without root privileges.

| Target | Contents and use |
| --- | --- |
| `core` | Local built-in analyses, finance baseline helper and common Git/curl/jq/ripgrep utilities. |
| `python` | Agent engineering, QA, profiling, architecture, privacy and AI evaluation CLI packages used by profiles in this family. |
| `browser` | API contracts, browser tests, accessibility, visual checks, localization and site diagnostics. |
| `platform` | Infrastructure, release, observability and restore tools. Cloud accounts, clusters and remote storage remain operator-configured. |
| `data` | SQL/CSV/data-quality tools and local database clients. Database servers are separate. |
| `documents` | Document conversion, Markdown/prose quality, OCR and local knowledge evidence tools. |
| `mobile` | Android static tooling and adb client. Emulators/devices require a separate host; iOS tooling requires macOS. |
| `finance` | Baseline finance helper plus the locked advanced numerical/QuantLib environment. |
| `suite` | Union of all default image tools and the advanced finance environment. This is the broadest worker image. |
| `finance-service` | All maintained OpenBB V5 provider packages, analytics extensions and the authenticated control-side adapter. Never use it as a session workspace image. |

Choose `suite` when one worker must serve all specialists. Boxoro currently configures `AR_BOX_IMAGE` per worker; selecting a profile does not provision or switch images. Family targets reduce image size and help dedicated deployments. The catalog's resource recommendations are planning metadata, not automatic worker quota changes. Check actual limits in [CONFIGURATION.md](CONFIGURATION.md) and size the host for concurrent tools. Browser, model evaluation and numerical jobs can exceed small default session limits.

### Build, qualify and select an image

From a Docker host with sufficient disk space and the repository checked out:

```sh
# Complete security/malware scanners and all default specialists in one image.
bash scripts/build-full-agent-image.sh --tag boxoro-agents:local

# Builds the existing workspace base, then the selected family and its smoke checks.
bash scripts/build-specialist-image.sh --target suite --tag boxoro-specialists:local

# Or use a reviewed immutable workspace base.
bash scripts/build-specialist-image.sh --base registry.example/boxoro-workspace@sha256:YOUR_VERIFIED_DIGEST --target browser --tag boxoro-browser:local
```

The full-image wrapper builds the existing security `malware` target, uses its observed immutable local image ID as the specialist base, and runs both smoke suites against the final result. Use `--security-target core` if optional YARA/ClamAV installation is not wanted. `full-agent-image-build.json` retains both receipts and the final identity. The specialist-only script does not populate the original `/opt/security` scanner installation; use the full wrapper or an explicitly supplied qualified security base when the native `security_scan` tool must share the worker. Malware/vulnerability databases still need their explicit, authorized update workflow.

The placeholder digest must be replaced with a real 64-character SHA-256 value. The script validates it and never invents an image identity. `specialist-image-build.json` records observed local image IDs, any actual registry RepoDigests, source revision/dirty state and lock hashes. Local image IDs work only on the Docker host that has them. Distribute a published image using an observed registry digest.

Set the worker's `AR_BOX_IMAGE` to the chosen immutable identity, restart the worker according to the existing installation procedure, and create a new session. Existing boxes keep their image. Run the existing worker/gVisor/egress qualification as well as the image smoke check. `BOXORO_SMOKE_RUNTIME=runsc` can request gVisor for the image smoke on a properly configured host; this does not replace the host qualification suite.

The manual GitHub workflow **Qualify specialist images** builds and runs checks for a selected family. It does not publish an image, create cloud resources or silently incur CI on every push. The largest suite may need a runner with more storage than a default hosted machine.

### Dependency layout and update process

The build uses Python3.12 and a separately checksum-pinned Node24 distribution. Python CLI tools have independent virtual environments under `/opt/box/agents/python/<id>`; npm tools have independent locked prefixes under `/opt/box/agents/node/<id>`. Fixed executable links live under `/opt/box/agents/bin`. This prevents unrelated tools from forcing incompatible library versions into one environment.

- `deploy/agents/catalog.json`: reviewed capabilities, install coordinates, typed commands, profiles, skills and connector prerequisites.
- `deploy/agents/install.lock.json`: exact package/release versions, source artifacts and hashes of associated locks.
- `deploy/agents/locks/python/*.lock`: pinned dependency versions and package hashes.
- `deploy/agents/locks/npm/*/package-lock.json`: npm dependency and integrity graph.
- `deploy/agents/node.lock.json`: actual Node distribution URL, size and SHA-256, checked against the upstream release checksums.
- `deploy/finance/requirements*.lock`: OpenBB and numerical environments, separate from generic CLI tools.
- `/opt/box/agents/installed.json`: build-generated installed-version record.
- `/opt/box/agents/os-packages.tsv`: actual Ubuntu package inventory produced by the build.

Ubuntu apt dependencies follow the reviewed Noble repositories; they are not individually frozen to a snapshot. The recorded inventory makes that variability explicit. The build is hash-locked for Python/npm/downloaded binaries, but is not a bit-for-bit reproducible apt snapshot build.

Refresh dependencies only in a maintainer checkout, review the resulting diff and rerun qualification:

```sh
python3 deploy/agents/resolve-locks.py --refresh --include-optional --jobs 4 --cache /path/to/release-cache
```

The resolver requires `uv`, npm and network access. Normal image builds consume committed locks and do not resolve “latest.” Python installs require hashed wheels by default. The explicitly optional `lm-eval` package needs reviewed source-only transitive dependencies; its separate hashed build requirements are installed first and build isolation is disabled to avoid unpinned build downloads. npm lifecycle scripts are enabled only for the reviewed packages that require them. These build steps execute upstream package code inside the build environment, never as a model-directed workspace operation.

For npm environments that need lifecycle hooks, exact-version `allowScripts` entries and `--strict-allow-scripts` enforce the reviewed dependency-level policy. A new unreviewed hook requires a maintainer decision during lock refresh. Other npm environments use `--ignore-scripts`. Do not replace this policy with blanket script approval.

Optional packages are also locked. To make a custom reviewed image, extend the `base` stage and call `build-family.sh FAMILY true`; repeat image qualification for those extras. API keys, paid resources, GPU model weights, language models, browser drivers and datasets do not become available merely because their client library is installed. Riskfolio/vectorbt's additional restrictions are documented separately and excluded from the default finance environment.

### Project environments

A bundled pytest, Vitest, mypy or build CLI cannot automatically supply the application's own dependencies. Prepare the project's reviewed environment using the existing session workflow; inspect install hooks before running them. The generic runner exposes fixed commands and a clean environment, so it may intentionally report missing project imports/plugins. Use the existing authorized project shell workflow when a project-specific environment is necessary, and retain its test evidence. Do not silently expand the runner to accept arbitrary executable paths or shell flags.

## MCPs, APIs and account integrations

The native helpers cover the default local toolset. No third-party MCP is required to lint a file, analyze a local dataset or run an installed test CLI. The operator's existing `AR_MCP_SERVERS_FILE` remains the integration path for authenticated HTTP MCPs; credentials remain on the control side, and owner configuration enables only registered server names.

The catalog documents official GitHub, Playwright, Chrome DevTools, Grafana, Sentry, AWS, Database Toolbox, Brave and MCP reference implementations, plus the included OpenBB adapter. These are explicit reference/deployment records, not fabricated live connections. See [CONNECTORS.md](CONNECTORS.md) for exact integration requirements and examples. Boxoro currently supports Streamable HTTP clients; stdio-only references need a separately reviewed supported HTTP deployment. It does not run arbitrary stdio server commands on the control host.

For finance, build `finance-service`, deploy `deploy/finance/compose.yaml` on the control host, provision a private random bearer token and provider credentials, and register the four fixed tools from the included MCP configuration example. The service discovers actual installed OpenBB schemas rather than hard-coding an obsolete provider map. Its status, pagination, schema descriptions, row limits, metadata, URL restrictions and analytics assumptions are documented in [FINANCE_AGENT.md](FINANCE_AGENT.md).

## Security and operational limits

The existing gVisor workspace, read-only root, container quotas and host egress policy remain the main boundary for untrusted project code. `allow_project_code`, `allow_network` and `allow_mutation` are required declarations for certain recipes, not a firewall or an approval system. A test, plugin or configuration file can execute arbitrary code within its worker. Catalog changes and optional package builds require review.

External target URLs in ordinary web checks are constrained to loopback to keep default tests focused on the app running in the box. This is not a complete network isolation mechanism: browsers can load subresources and redirects, and project code may open connections. Enforce host egress policy for the worker. Load testing must target systems the user controls and has authorized, with conservative request ceilings.

Malware absence, complete privacy compliance, financial forecast accuracy and application security cannot be certified by a successful CLI. Preserve coverage gaps, sources, data timestamps, configuration, uncertainty and missing credentials in reports. The specialized skills describe each domain's specific limits.

## Extend a specialist

1. Add a maintained tool with primary sources, licensing, install coordinates, availability level and an honest readiness probe.
2. Define fixed argv, typed parameters, expected exits, output limits and every effect. Review config-loading behavior as well as the obvious command.
3. Assign the tool and workflow to profiles; update their skill instructions and documentation.
4. Resolve and review locks. Verify an actual installed CLI, meaningful fixture behavior, and incomplete/error handling.
5. Run native/configuration tests, Python helper tests and the image smoke on the target runtime.
6. Publish the code for review and deploy only a qualified image identity.

Never interpret a source URL, README example or a command existing on `PATH` as proof that an account, API entitlement, device, database or application dependency has been configured.
