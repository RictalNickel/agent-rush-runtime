# OpenBB V5 providers, packages, and source rights

Verified for this implementation on **2026-11-15**. The full service profile installs OpenBB V5 with `openbb-core==2.0.1`, all 17 current provider packages, company-news routing, three analytical extensions, and headless charting. A real clean installation discovered **1,138 approved commands across 23 namespaces**. This is a reproducible observation for the checked-in lock, not a fixed count promised after dependency changes or evidence that every upstream source is reachable.

The exact environment is [`requirements.in`](../deploy/finance/requirements.in) plus the hash-pinned [`requirements.lock`](../deploy/finance/requirements.lock). [Finance Lab](FINANCE_AGENT.md) documents its authenticated service, local calculations, request contract, and verification.

## Installed provider inventory

The official [V5 provider inventory](https://docs.openbb.co/odp/python/extensions/providers) maps packages to their provider-owned namespaces. Version pins below are also verified by the installed package metadata and lock.

| Package | Pin | Namespace | Service credential |
| --- | --- | --- | --- |
| `openbb-bls` | 2.0.0 | `bls` | Optional `BLS_API_KEY` |
| `openbb-cboe` | 2.0.0 | `cboe` | None |
| `openbb-cftc` | 2.0.0 | `cftc` | Optional `CFTC_APP_TOKEN` |
| `openbb-deribit` | 2.0.0 | `deribit` | None |
| `openbb-ecb` | 2.0.0 | `ecb` | None |
| `openbb-us-eia` | 2.0.0 | `eia` | `EIA_API_KEY` for API endpoints |
| `openbb-famafrench` | 2.0.0 | `famafrench` | None |
| `openbb-federal-reserve` | 2.0.0 | `federal_reserve` | None |
| `openbb-finra` | 2.0.0 | `finra` | None |
| `openbb-fred` | 2.0.0 | `fred` | `FRED_API_KEY` for API endpoints |
| `openbb-government-us` | 2.0.0 | `uscongress`, `usda`, `ustreasury` | Optional `CONGRESS_GOV_API_KEY` |
| `openbb-imf` | 3.0.0 | `imf` | None |
| `openbb-jodi` | 2.0.0 | `jodi` | None |
| `openbb-nasdaq` | 2.0.0 | `nasdaq` | None |
| `openbb-oecd` | 2.0.0 | `oecd` | None |
| `openbb-sec` | 2.0.0 | `sec` | None |
| `openbb-tmx` | 2.0.0 | `tmx` | None |

The service accepts only these five provider secret names. Their presence is reported as a boolean; keys are never returned. Required credentials, optional quota improvements, and endpoint-specific access requirements remain distinct. An empty credential does not disable keyless commands in the same package.

## Analytical and presentation extensions

| Package | Pin | Exposed surface |
| --- | --- | --- |
| `openbb-news` | 2.0.0 | `news`; company-news endpoints where a current installed source supports them |
| `openbb-econometrics` | 2.0.0 | `econometrics`; discovered statistical and time-series computations |
| `openbb-quantitative` | 2.0.0 | `quantitative`; descriptive/risk/return computations |
| `openbb-technical` | 2.0.1 | `technical`; typed indicator output |
| `openbb-charting` | 4.0.0 | Headless support where installed endpoint schema requests charting |

The analytical namespaces use JSON POST bodies in the REST contract. They accept data, targets, and bounded numerical parameters through the discovered schemas. Provider GET operations and these analytical POST operations are eligible; other method/namespace combinations are excluded. `quantitative.capm` retrieves external factors and therefore requires explicit network permission. The installed schema is authoritative for each command's exact inputs; do not carry a V4 signature into V5. Upstream documents [econometrics](https://docs.openbb.co/odp/python/extensions/data-processing/econometrics), [quantitative](https://docs.openbb.co/odp/python/extensions/data-processing/quantitative), and [technical](https://docs.openbb.co/odp/python/extensions/data-processing/technical) separately.

The default image contains no stock OpenBB MCP server, generic CLI dispatcher, desktop WebView extra, brokerage execution extension, or dynamically installed community package. The four Boxoro MCP tools dispatch only through the installed, allowlisted OpenAPI catalog. A schema fingerprint lets results identify the catalog used for their execution.

## Credentials and isolated configuration

Only the trusted Finance service receives provider secrets. The agent sends a command name, parameters, and analytical data through authenticated MCP. A provider key in a JSON argument is rejected; it does not override server credentials.

Official [credential documentation](https://docs.openbb.co/odp/python/user-guide/credentials) and [environment-variable documentation](https://docs.openbb.co/odp/python/user-guide/environment-variables) describe several upstream sources of credentials. This deployment deliberately supports the five environment names above. Workers receive a clean environment and an isolated HOME. Service-generated `user_settings.json` contains an empty credential map so stale persisted values cannot override the supplied environment. The `openbb.toml` credential table is not relied on for command execution.

The private state directory is service-owned and mode 0700. Startup refreshes fixed settings for metadata, logging, directories, and callback policy. Unexpected local dotenv/TOML files there cause startup failure. Provider caches can write within that directory; there is no user workspace mount. The OpenBB generated Python client is unnecessary for the in-process REST adapter, so the venv can remain immutable. Every data query gets a fresh worker and a clean credential environment.

Discovery metadata is compiled once per service lifetime in a separate bounded child with provider credentials removed and network disabled. The HTTP service retains only the allowlisted schemas/descriptions in memory, under a 16 MiB transfer limit; concurrent discovery shares the bootstrap, and failures can retry. It does not retain provider results or OpenBB process state. A query receives one internal validated operation contract and checks the installed package/`RECORD` fingerprint before using it, while still loading its own SDK and credentials. The schema fingerprint remains attached to results. Restart the service on package deployment; no catalog is read from a workspace or persisted schema file. [Finance Lab](FINANCE_AGENT.md#deploy-and-authenticate) explains resource costs, invalidation, and the authenticated timing smoke.

For this lock, the actual offline SDK smoke returns count 4, mean 2.5, variance 5/3 from `quantitative.summary`. Provider metadata and arguments are preserved after redaction. No live or paid provider calls were needed to establish that result. Read [Finance Lab deployment](FINANCE_AGENT.md#deploy-and-authenticate) for the matching token, literal MCP headers, and Compose configuration.

## Migrating older OpenBB instructions

V5's provider-owned routes replace many shared V4 asset-class routes. Install into a clean environment: legacy shared router packages can suppress the V5 routes when imported. This service intentionally excludes them. Use `openbb_catalog` followed by `openbb_describe`, then validate with the returned schema. The [official migration guide](https://docs.openbb.co/odp/python/migration-from-v4) documents the breaking changes.

These former packages have no V5 counterpart: `openbb-alpha-vantage`, `openbb-benzinga`, `openbb-biztoc`, `openbb-econdb`, `openbb-finviz`, `openbb-fmp`, `openbb-intrinio`, `openbb-multpl`, `openbb-seeking-alpha`, `openbb-stockgrid`, `openbb-tiingo`, `openbb-tradier`, `openbb-tradingeconomics`, `openbb-wsj`, and `openbb-yfinance`. Their older releases are not compatible additions to this core-2 environment. `openbb-congress-gov` is incorporated into `openbb-government-us`.

There is no promise of a replacement for removed providers or commands. In particular, installing all current provider packages does not make a missing commercial source or former global-news endpoint available. Future extensions require an operator review of source, dependencies, namespace, HTTP methods, side effects, credentials, and data terms, followed by a new lock and tests. Namespace discovery cannot make an arbitrary third-party package trustworthy.

## Data provenance and interpretation

Every research result should retain the original provider, source URL/identifier if present, observation time, publication time, retrieval time, unit, currency, periodicity, and adjustment/vintage. The service preserves upstream OBBject `warnings`, `extra.metadata`, and `extra.results_metadata` where the command provides them, and adds its own UTC retrieval time. A missing upstream field remains missing. The [OBBject contract](https://docs.openbb.co/odp/python/concepts/obbject) distinguishes results from execution and source metadata.

Operational checks depend on the data being used:

- For prices, identify market, timezone, delay, corporate-action adjustment, and whether the data include dividends. A last observed price need not be a live quote.
- For macroeconomic data, distinguish period labels from release dates and revised values from information available at the research date. Avoid backtests that use later revisions without stating that assumption.
- For factors and returns, normalize both frequency and scale before regression. A percent value of `1` and a decimal return of `0.01` have the same economic magnitude but are not interchangeable model inputs.
- For options and bonds, carry expiry/settlement, calendars, day-count and compounding conventions, contract multiplier, and quote units into valuation.
- For joins, specify alignment and missing-data policy; do not quietly forward-fill a low-frequency series into an apparently independent high-frequency sample.

These are research requirements, not transformations performed silently by the service. Output limits, provider throttling, access errors, schema failures, and timeouts remain visible as failed/incomplete requests. Truncated data must never support a claim of complete coverage.

## Licenses and commercial use

**OpenBB V5 is Apache-2.0.** V4 and earlier AGPL releases retain their original terms. The V5 license allows commercial/proprietary software and hosted use subject to its license/notice obligations; it does not grant OpenBB trademark rights. The [OpenBB license FAQ](https://docs.openbb.co/odp/python/faqs/license) and [repository license](https://github.com/openbq-org/OpenBB/blob/develop/LICENSE) are the governing primary references. Preserve licenses/notices when distributing the image.

**Software rights and data rights are separate.** Installing an Apache-licensed connector does not grant permission to redistribute source datasets or bypass a provider's quotas/entitlements. Storage, display, redistribution, and commercial-use terms come from each source. No-key access is not a license grant. The service records source context but cannot grant those rights.

The numerical environment has its own dependency notices; see the locks and [third-party notices](../THIRD_PARTY_NOTICES.md). QuantLib 1.44 is a BSD-licensed native dependency; consult its [official package](https://pypi.org/project/QuantLib/) and [project](https://www.quantlib.org/) for conventions and upstream licensing.

### Optional Riskfolio restriction

[`requirements-riskfolio.in`](../deploy/finance/requirements-riskfolio.in) and its separate lock add `riskfolio-lib==7.4.1`. The Riskfolio package itself is BSD-3-Clause, but its **actual wheel metadata requires `vectorbt>=0.28.0`**. The resolved `vectorbt==1.2.1` distribution includes **Apache-2.0 with Commons Clause**, which adds restrictions on selling covered software or services. This profile is excluded from the default Finance image and the all-provider OpenBB image.

Verify the wheel metadata and bundled license when updating; do not infer runtime dependencies only from an examples list in documentation. Primary package references: [Riskfolio-Lib](https://pypi.org/project/Riskfolio-Lib/7.4.1/), [vectorbt](https://pypi.org/project/vectorbt/1.2.1/), and [Riskfolio documentation](https://riskfolio-lib.readthedocs.io/en/latest/). Review the exact terms for the intended distribution before opting into that separate environment. The fixed portfolio optimizer uses SciPy and does not require this profile.

## Retained DiskCache advisory

The locked service still contains `diskcache==5.6.3`, affected by **CVE-2025-69872** (aliases **GHSA-w8v5-vhqr-4h9v**, **PYSEC-2026-2447**). The [reviewed advisory](https://github.com/advisories/GHSA-w8v5-vhqr-4h9v) describes arbitrary code execution if an attacker can write cache contents that an application later unpickles. As of 2026-11-15, [official PyPI](https://pypi.org/project/diskcache/) still publishes 5.6.3 as the latest release, and [upstream issue 357](https://github.com/grantjenks/python-diskcache/issues/357) has no released fix. These two advisory IDs identify the same underlying issue.

Inspection of the installed provider sources confirms DiskCache use in ECB, CFTC, Federal Reserve, FRED, SEC, TMX, and USDA helpers. It is not merely an unused package in the environment. The cache lives under the isolated service's fixed `cache_directory`, with no workspace mount or caller-selected cache path. Startup checks ownership and mode of the private state root, rejects symlinked managed directories, and tightens cache/temp/data/export directories to mode 0700. Workers set umask 0077 and use service-private HOME, temporary storage, and cache locations. A regression test verifies cache permissions and symlink refusal.

This narrows the required attacker access; it **does not repair DiskCache's deserialization behavior**. A process or user that already has the service's filesystem identity, a compromised host, or an imported malicious cache can still cross that trust boundary. Use a dedicated service identity/volume, exclude it from agent mounts, discard state of uncertain origin, and apply storage quotas. Recheck the upstream release and advisory when changing the lock. Do not suppress the finding globally or describe this dependency profile as vulnerability-free. Machine-readable applicability is recorded in [`dependency-advisories.json`](../deploy/finance/dependency-advisories.json).

## Maintainer checklist

1. Resolve providers and analytical packages together in a clean Python 3.12 environment; regenerate hashes and inspect the dependency/license diff.
2. Run the stdlib boundary/cache suites, real advanced-backend tests, and opt-in installed OpenBB HTTP smoke from [Finance Lab verification](FINANCE_AGENT.md#dependency-profiles-and-verification). Recheck cold/warm metadata and fresh-query timing when upgrading; cached discovery does not remove SDK import cost from each data query.
3. Compare `openbb_status` namespaces/count and catalog schema fingerprint. Review any newly exposed namespace, method, URL/file parameter, or execution capability before expanding policy.
4. Test authenticated HTTP behavior with the current runtime client and verify the image remains nonroot/read-only with only private state writable.
5. Test explicitly authorized sources within their entitlements and limits, recording actual retrieval outcomes. Installed coverage and offline smoke are reported separately from live availability.

The supplied lock and tests establish a reproducible software environment. They do not establish a subscription, a live account connection, a trade capability, or an ongoing data freshness guarantee.
