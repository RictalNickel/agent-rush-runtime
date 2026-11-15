# Specialist dependency audit and remediation

Audit date: **2026-11-15**. Scope: the 29 npm tool lockfiles initially classified as default image dependencies. The read-only command was `npm audit --json --package-lock-only --omit=dev --ignore-scripts`, using Node 24.22.3 and the committed isolated package lockfiles. All 29 audits returned valid registry results. No critical finding was reported. These results cover npm's advisory database at that time, not every native library, Python package or possible vulnerability.

## Initial findings and decisions

The numbers below count affected dependency entries in each isolated tree. Several entries describe the same underlying advisory as it propagates through parent dependencies; they must not be described as distinct vulnerabilities.

| Tool / resolved package | Initial high | Initial moderate | Initial low | Decision |
| --- | ---: | ---: | ---: | --- |
| Lighthouse CI / `@lhci/cli@0.16.1` | 9 | 0 | 0 | Reviewed override `basic-ftp=6.2.6` |
| Promptfoo / `promptfoo@0.125.1` | 5 | 4 | 0 | Reviewed override `basic-ftp=6.2.6`; remaining moderate advisories listed below |
| BackstopJS / `backstopjs@6.3.25` | 11 | 0 | 0 | Moved to optional; no unqualified dependency replacement |
| Sharp CLI / `sharp-cli@6.1.0` | 2 | 2 | 0 | Reviewed override `sharp=0.35.5`; remaining moderate advisories listed below |
| Madge / `madge@8.0.0` | 0 | 2 | 0 | Record minimatch advisory and require reviewed inputs |
| Linkinator / `linkinator@8.1.0` | 0 | 3 | 0 | Record glob/minimatch advisories and retain process limits |
| Markdownlint / `markdownlint-cli2@0.24.1` | 0 | 0 | 4 | Record KaTeX advisory; no rendering service is registered |
| Other 22 initial npm tools | 0 | 0 | 0 | Retain resolved locks and task-specific qualification |

The other 22 tools were Playwright, Vitest, Spectral, Redocly, Buf, GraphQL Inspector, Autocannon, Pa11y, Pa11y CI, Lighthouse, HTML Validate, TypeScript, ESLint, dependency-cruiser, jscpd, Mermaid CLI, Prettier, Pixelmatch, ODiff, FormatJS, Lingui and i18next CLI.

The catalog records overrides in `tool.install.overrides`; the resolver puts them in the isolated root `package.json` before resolving a fresh lock. Adding a second top-level dependency would not reliably replace a vulnerable nested package. `npm audit fix --force` was not used: its suggestions included downgrading tool packages across incompatible releases and did not establish API compatibility.

## Reviewed updates

### Lighthouse CI and Promptfoo: basic-ftp 6.2.6

The resolved chain used `proxy-agent@8.0.2` → `pac-proxy-agent@9.1.0` → `get-uri@8.0.1` → `basic-ftp@5.3.1`. The current get-uri package still declares `basic-ftp:^5.3.1`, so the security update crosses that declared major range and requires explicit compatibility evidence.

The original high finding is a denial of service in FTP directory-list parsing, [GHSA-c475-qrg2-pj4r](https://github.com/advisories/GHSA-c475-qrg2-pj4r). Later fixes address [MLSD fact handling](https://github.com/advisories/GHSA-5pm8-86m8-9m6r) and [control-character error handling](https://github.com/advisories/GHSA-r4jh-29pc-gfqr). The [6.2.6 release](https://github.com/patrickjuchli/basic-ftp/releases/tag/v6.2.6) also fixes path traversal in `downloadToDir`.

An isolated `get-uri@8.0.1` installation with the exact override `basic-ftp:6.2.6` passed actual data-URI, local-file, loopback HTTP and loopback FTP retrieval. The FTP fixture exercised both the MDTM metadata path and the MLSD listing fallback, followed by file transfer and connection closure. This covers the Client, access, lastMod, list, downloadTo and close interfaces consumed by get-uri. The isolated compatibility tree's npm audit returned zero advisories. This is focused compatibility evidence, not a claim that every FTP server or every Promptfoo/Lighthouse feature was tested.

After full lock regeneration, run the complete affected tool's probe and fixed operation, and re-audit that exact lock. The audit of the narrow compatibility tree does not substitute for the full dependency tree.

### Sharp CLI: sharp 0.35.5

Sharp CLI 6.1.0 pins sharp 0.35.4. [GHSA-wq5f-xc86-pv6w](https://github.com/advisories/GHSA-wq5f-xc86-pv6w) identifies a bundled librsvg issue fixed in sharp 0.35.5. This is an exact patch update within the same sharp minor release. The [0.35.5 release notes](https://github.com/lovell/sharp/releases/tag/v0.35.5) describe the corresponding binary updates and fixes.

An isolated Sharp CLI 6.1.0 installation with override `sharp:0.35.5` successfully ran the registered bounded PNG metadata recipe with `--dry --print --limitInputPixels 16000000 --timeout 30`. It returned width, height, format and channel metadata without overwriting the source. Its audit reported no high/critical entries; three moderate dependency entries remained in the glob/minimatch chain. Test the actual image's native binary on every supported architecture.

## BackstopJS remains optional

The current BackstopJS package depends on older Puppeteer and related dependencies. Its resolved tree included `extract-zip@2.0.1`, `zod@3.23.8` and `basic-ftp@5.3.1`. The extract-zip [symlink traversal advisory](https://github.com/advisories/GHSA-jmr9-qjv8-65gv) lists no patched version; a [second archive write advisory](https://github.com/advisories/GHSA-7pqw-9j4j-h8q3) affects the same package. The [Zod resource-consumption advisory](https://github.com/advisories/GHSA-mrhx-c7xc-m3w5) requires a newer incompatible dependency family in this tree.

The default image therefore excludes BackstopJS. Its optional catalog entry and fixed operation preserve the integration path but do not claim it is ready or safe to install as-is. Enabling it requires a supported patched dependency tree, a fresh audit and a browser scenario smoke. Playwright screenshots and the qualified Pixelmatch/ODiff comparison operations provide the default visual-regression path.

## Remaining moderate and low advisories

These entries remain explicit rather than being hidden behind a high/critical-only summary. Re-resolution may remove an entry as upstream constraints change; confirm the exact new lock before claiming it resolved.

| Dependency / advisory | Affected tools in the initial audit | Fixed version / qualification issue |
| --- | --- | --- |
| `@hono/node-server` [WebSocket connection reset](https://github.com/advisories/GHSA-qv5m-9pm6-hqp3) and [backslash path parsing](https://github.com/advisories/GHSA-jpvv-r467-5p4w) | Promptfoo, direct pin 2.1.5 | 2.1.6; no HTTP serving operation is registered, but that does not erase the dependency finding |
| `glob` [cancellation bypass](https://github.com/advisories/GHSA-3g3m-2fjr-6pw4) | Promptfoo, Sharp CLI, Linkinator | Fixed after 14.0.1; current parents use older major/minor ranges, so an override needs separate compatibility review |
| `minimatch` [pattern expansion resource use](https://github.com/advisories/GHSA-g3hv-75wg-m249) | Promptfoo, Sharp CLI, Linkinator, Madge | 11.0.1; some parents declare an older major range |
| `katex` [prototype-pollution trust bypass](https://github.com/advisories/GHSA-238p-pmpm-9mq7) | Markdownlint dependency chain | 0.18.2; current parent constraints retain an older family |

Fixed argv, typed parameters, bounded subprocess time/output and the worker sandbox reduce exposure for the registered operations. They do not establish that a vulnerable library is harmless. Do not expose these CLIs as arbitrary command or public rendering services.

## Python and finance dependency audit

A separate read-only OSV batch audit queried **459 distinct resolved Python package/version coordinates**, including the default Python tools and finance environments. It returned six affected coordinates and 14 records before triage. Several GHSA/PYSEC records are aliases for the same issue; 14 is a record count, not 14 independent vulnerabilities. Current catalog defaults exclude Checkov after this review.

| Resolved dependency / owning tool | Evidence and status | Decision and remaining scope |
| --- | --- | --- |
| `asteval@1.0.6` / Checkov 3.4.5 | [BaseException escape](https://github.com/advisories/GHSA-89v8-rhwq-hf77), [NumPy ctypes escape](https://github.com/advisories/GHSA-9w56-46f6-3qhx); OSV records a 1.0.9 fix while Checkov pins 1.0.6 | Checkov is optional until a supported patched tree is qualified; no silent override of its exact requirement |
| `ecdsa@0.19.2` / Checkov | [Minerva timing leakage](https://github.com/tlsfuzzer/python-ecdsa/security/advisories/GHSA-wj6h-64fc-37mp), high, no planned upstream fix | Checkov is excluded from default images. The issue concerns signing/key generation/ECDH; the advisory excludes signature verification, but no blanket clean-dependency claim is made |
| `mcp@1.29.0` / Semgrep 1.185.0 | Moderate [cross-origin redirects](https://github.com/modelcontextprotocol/python-sdk/security/advisories/GHSA-5h93-6whr-6q8j), [HTTP session retention](https://github.com/modelcontextprotocol/python-sdk/security/advisories/GHSA-84m7-p3x7-pcfv), [schema reference fetching](https://github.com/modelcontextprotocol/python-sdk/security/advisories/GHSA-rwrf-2pqf-9j8j), [token audience handling](https://github.com/modelcontextprotocol/python-sdk/security/advisories/GHSA-w4fh-qvv9-3v23); fixed in 1.30.0 for the 1.x series | Semgrep's upstream pin is retained. Only the local static scan is registered, with metrics/version checks disabled; its MCP client/server modes are not exposed |
| `opentelemetry-api@1.37.0` / Semgrep | Moderate [tracestate parsing](https://github.com/open-telemetry/opentelemetry-python/security/advisories/GHSA-xcq7-p3pf-p88h) and [traceparent parsing](https://github.com/open-telemetry/opentelemetry-python/security/advisories/GHSA-mf5v-w3w3-f6cg); fixes through 1.45.2 | Semgrep requires the 1.37 family. The fixed static operation exposes no inbound telemetry endpoint. Retain the finding and update with a compatible upstream release |
| `httpie@3.2.4` | [PYSEC-2023-242](https://osv.dev/vulnerability/PYSEC-2023-242) has an open affected range, whereas [GHSA-8r96-8889-qg2x](https://osv.dev/vulnerability/GHSA-8r96-8889-qg2x) records fixed version 3.2.3 | Database conflict is recorded. The [maintainer explanation](https://github.com/httpie/cli/issues/1549) distinguishes the internal updater from verified user requests. The operation forces `--verify=yes`, and image-owned config disables update checks |
| `diskcache@5.6.3` / OpenBB finance service | Moderate [unsafe pickle deserialization](https://github.com/advisories/GHSA-w8v5-vhqr-4h9v), also PYSEC-2026-2447; no fixed release in the audit | Retained with private service state/cache, no workspace mount, owner-only directories and restrictive umask. This limits who can tamper with cache entries; same-UID/host compromise and untrusted restored caches remain risks |

HTTPie's installed updater source still contains a request with certificate verification disabled, so the mitigation does not depend solely on the narrower GHSA version range. A real UID 1000 HEAD request passed with a root-owned read-only configuration containing `disable_update_warnings:true`; no updater/version state was written. No metadata migration was needed for this configuration.

Finance documents the retained cache finding, exact filesystem checks and residual risks in [OpenBB provider routing](OPENBB_PROVIDERS.md#retained-diskcache-advisory) and `deploy/finance/dependency-advisories.json`. The service's state and top-level cache/temp/data/export directories must be owned, non-symlink private directories; the worker uses umask 0077 and private HOME/TMPDIR/XDG cache paths. Lifetime cache growth still needs an operator filesystem quota. These are mitigations, not a patched DiskCache claim.

The reviewed Semgrep applicability statement is limited to the registered local scan. Installing the same environment as a general MCP service, telemetry receiver or arbitrary plugin host changes the exposure and requires another review.

## Operation qualification findings

Real negative and empty-input fixtures were used in addition to version probes:

- ODiff 4.5.0 returns 0 for matching images, 21 for layout differences with `--fail-on-layout`, 22 for pixel differences, and 1 for invalid image decoding. The catalog preserves those distinct outcomes.
- Vitest can return success for an empty suite when project configuration sets `passWithNoTests:true`. The fixed recipe now forces `--passWithNoTests=false`. Runtime evidence guards for Vitest JSON, Playwright JSON and pytest JUnit treat empty/all-skipped suites and missing/malformed reports as incomplete; actual passing and skipped fixtures verified this behavior.
- Bandit returns exit 0 for Python syntax errors while reporting them in JSON `errors`. The runtime qualifies this JSON and treats scanner errors and zero scanned source as incomplete; both real negative fixtures verified that behavior.
- Frictionless's default path policy rejects runtime-normalized absolute file paths. The recipe now forces the local file/CSV table parser and uses `--trusted` only for the path already checked by the workspace guard. A valid two-row CSV passed; a malformed extra-cell row failed with the corresponding data finding.
- Translate Toolkit's `pocount` has no version command. Its `--help` probe succeeds; the installed package manifest supplies the version. A real translated PO fixture produced the expected CSV count, while malformed PO input failed.
- Trivy was exercised with a fresh empty check cache and all HTTP(S) proxy traffic directed to an unavailable loopback port. Its embedded checks detected unrestricted SSH in Terraform (AWS-0107) and two low findings, with exit 1. A corrected fixture exited 0 with 71 successful checks; an empty directory exited 0 with no results. Runtime qualification uses JSON `MisconfSummary` counts, including `--include-non-failures`, so empty scope and missing/fatal reports are incomplete. Project Trivy configuration/ignore files, telemetry and version/check updates are disabled by fixed flags; external Terraform module references remain a separate network prerequisite.
- Madge reported an actual two-file cycle with exit 1. Madge, Markdownlint, Codespell and Lizard can exit 0 on empty input; their results require checking the selected file scope and counts. Their exit status is not evidence of coverage over absent or ignored source.
- A successful empty Trafilatura extraction produces no content. Extraction is an evidence collection operation; verify that the expected output files exist before treating it as corpus coverage.

## Reproduction and evidence boundaries

The initial audit and compatibility fixtures were run outside the repository's dependency directories; shared locks were not mutated by the audit. The runtime tests exercise policy, timeout, reporting and first-party analysis behavior separately. Full image builds and browser/device/provider qualification have their own evidence and must not be inferred from this document.

For a fresh audit, run the read-only npm command above from each `deploy/agents/locks/npm/<tool>` directory. Record the exact lock digest, tool version, Node version, advisory response and any install overrides. A scheduled or release-time re-audit is needed because advisory databases and upstream releases change after this dated record.

## Final patched npm lock verification

The committed isolated locks were regenerated from fresh dependency resolution, reinstalled with Node24.22.3/npm11.19.1, and re-audited after the reviewed overrides on 2026-11-15:

| Tool | Critical | High | Moderate | Low |
| --- | ---: | ---: | ---: | ---: |
| Lighthouse CI | 0 | 0 | 0 | 0 |
| Promptfoo | 0 | 0 | 5 | 0 |
| Sharp CLI | 0 | 0 | 3 | 0 |

These are affected dependency entries in each tree, not distinct CVEs. Moderate advisories remain as documented above; the extra Promptfoo entry reflects the refreshed transitive graph. All three installed CLI probes passed. Exact-version npm `allowScripts` entries and `--strict-allow-scripts` now cover the explicitly enabled build hooks; unknown new scripts fail maintainer resolution before reviewed locks are replaced. All other npm prefixes disable lifecycle scripts.
