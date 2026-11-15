# Finance Lab: local analysis and OpenBB research

Finance Lab is the `finance-lab` specialist. It combines **14 fixed financial analyses**, dependency diagnostics, a strict JSON request interface, and a separately authenticated OpenBB V5 service. Local analyses operate on workspace CSV/JSON data. Public/provider research goes through four control-side MCP tools that discover the actual installed OpenBB schemas before executing a command.

The implementation is in [`deploy/finance`](../deploy/finance), the native operation fragment is [`catalog-fragment.json`](../deploy/finance/catalog-fragment.json), and the short agent profile is [Finance Lab](agents/finance-lab.md). [OpenBB providers](OPENBB_PROVIDERS.md) records the provider inventory, credentials, versions, migration constraints, and source terms. [Specialist agents](SPECIALIST_AGENTS.md) covers image selection and the shared runner.

## Start with the installed capabilities

Select `finance-lab` through `agent_config`, or load the `specialist-finance-lab` skill. Selection supplies the workflow; it does not install packages, replace a worker image, configure provider credentials, or grant access to an MCP server.

```json
{"tool":"agent_config","action":"update","config":{"specialist":"finance-lab"},"reason":"Financial and economic research"}
```

```json
{"tool":"specialist","action":"describe","agent":"finance-lab"}
```

```json
{"tool":"specialist","action":"run","agent":"finance-lab","operation":"finance-doctor","path":"."}
```

`finance-doctor` reports installed package metadata. A present package is not evidence that a model fit or a live provider works. Run the relevant analysis and inspect its result. Advanced adapters return `missing_dependency` with an incomplete gate when their backend is unavailable.

For a CSV such as `data/prices.csv`, use the workspace file as the operation path:

```json
{"tool":"specialist","action":"run","agent":"finance-lab","operation":"finance-risk","path":"data/prices.csv","parameters":{"column":"close","frequency":252,"input_kind":"prices"}}
```

The shared runner executes a fixed argument vector and captures its reports. The `finance` executable lives in the immutable installation and starts Python with isolated imports. It never imports a Python module, notebook, pickle, plugin, or configuration file from the input workspace. These calculations require no provider network or project-code effect declaration.

## Local interfaces

The installed CLI supports named analyses, a request file, or JSON on stdin:

```sh
finance risk --input data/prices.csv --column close --frequency 252
finance --request-file research/valuation.json
finance --request-stdin < research/valuation.json
finance doctor
```

For source development, run `python3 -I deploy/finance/cli.py ...` from the repository root. The source launcher treats its working directory as the input root. The installed wrapper fixes that root to `/workspace`; it is not a request parameter. The `BOXORO_FINANCE_WORKSPACE` override exists for a trusted launcher/test harness, not for model-supplied environment values.

The complete JSON request contract is:

```json
{
  "operation": "risk",
  "parameters": {
    "column": "close",
    "input_kind": "prices",
    "frequency": 252,
    "risk_free_rate": 0.04,
    "confidence": 0.95
  },
  "data": [
    {"date":"2026-01-02","close":100},
    {"date":"2026-01-05","close":110},
    {"date":"2026-01-06","close":99},
    {"date":"2026-01-07","close":108.9}
  ]
}
```

Replace `data` with `"input":"data/prices.csv"` to read one workspace file. They cannot be combined. A named CLI operation also accepts a JSON input document containing `parameters` and optional `data`; an optional `operation` must match the named command. Scalar flags override that document's corresponding parameters. Request-file/stdin mode rejects additional parameter flags.

CSV must have a unique, nonempty header and a consistent field count. JSON data may be a numeric array or records. Numeric analyses reject missing/non-finite numbers, booleans passed as numbers, invalid rates, and duplicate timestamps. An explicit `date` column sorts observations chronologically; dates must have consistent timezone treatment. There is no interpolation, forward fill, automatic currency conversion, or silent row deletion. Choose aligned columns deliberately.

Files are restricted to regular CSV/JSON files beneath the workspace. Symlinks, traversal, FIFOs, devices, and out-of-workspace paths are rejected. Inputs are bounded to 4 MiB, 20,000 array elements/rows, 64 table columns, and nesting depth 24. Outputs are bounded to 2 MiB. Operation-specific limits can be smaller.

### Operations and assumptions

The catalog exposes each analysis as `finance-<name>`. `finance-request` accepts a full request document; `finance-doctor` reports readiness. All rates and returns are **decimal fractions**: `0.05` means 5%. Frequencies count observations per year; supplied records are assumed to represent equal periods even if calendar gaps vary.

| Analysis | Parameters and result | Assumptions that affect interpretation |
| --- | --- | --- |
| `returns` | `column`, `input_kind` (`prices`/`returns`); simple returns | Prices must be positive. The caller selects adjusted/total-return data when required. |
| `risk` | Series parameters plus `frequency`, annual `risk_free_rate`, `confidence`; total return, CAGR, volatility, Sharpe, Sortino, drawdown, VaR, expected shortfall | Sample volatility; annual risk-free rate converted to each period; initial wealth included in drawdown. Historical tail losses refer to one observation. |
| `portfolio` | `columns`, `weights`, series risk parameters; portfolio risk and variance-contribution shares | Long-only weights sum to one. Rebalanced to constant weights each observation; taxes and fees excluded. |
| `correlation` | `columns`; sample covariance and Pearson correlation | Computes on supplied numeric values. Supply returns explicitly to analyze return correlation. Zero-variance correlation is null. |
| `valuation` | `cash_flows`, `discount_rate`, optional `terminal_growth`, `net_debt`, `shares`; enterprise/equity/per-share DCF | Annual end-period unlevered cash flows; Gordon terminal value; discount rate must exceed growth. Currency must be consistent. |
| `options` | `spot`, `strike`, `years`, optional `kind`, `rate`, `dividend_yield`, `volatility`, `market_price`; price, Greeks, optional implied volatility | European Black–Scholes–Merton; continuous rates/dividends. Vega/rho are per +0.01 change; theta is per calendar day. |
| `bonds` | `coupon_rate`, `yield_rate`, `years`, optional `face`, `payments_per_year`; price, duration, convexity, DV01 | Valuation on a coupon date, regular coupons, no accrued interest or embedded options. |
| `cashflow` | `cash_flows`, optional `discount_rate`; NPV and bracketed IRR | First flow at time zero; equally spaced periods. IRR omitted unless exactly one sign change and a supported root bracket. |
| `backtest` | `column`, `fast`, `slow`, `cost_bps`, risk parameters; strategy/benchmark risk and trades | Long/cash moving-average rule, prior-bar signal, hypothetical close fill and turnover costs. Final holding marked, not forcibly sold. |
| `monte-carlo` | `spot`, `drift`, `volatility`, optional `years`, `simulations`, `seed`; terminal distribution | Constant-parameter geometric Brownian motion. Seeded scenario generation; no calibration or price-path forecast. |
| `factor` | `target`, `factors`, optional `cov_type`; coefficients, robust errors, confidence intervals | Real statsmodels OLS; HC3 default; aligned units/frequency; rejects rank deficiency. Association does not establish causality. |
| `forecast` | `column`, `p`, `d`, `q`, `horizon`; ARIMA means/95% intervals | Real statsmodels; at least 30 observations, bounded order and horizon. A nonconverged fit is incomplete. Intervals exclude model-selection uncertainty. |
| `optimize` | `columns`, `input_kind`, `frequency`, `risk_free_rate`, `objective`, `max_weight`; weights and in-sample risk | SciPy SLSQP, long-only and fully invested. Objectives: minimum variance, maximum Sharpe, risk parity. Singular covariance and invalid solutions are rejected. |
| `quantlib-bond` | `issue_date`, `maturity_date`, `valuation_date`, `coupon_rate`, `yield_rate`; optional face, frequency, settlement, calendar, day count | Real QuantLib calendar-based fixed-rate bond; clean/dirty/accrued prices per 100, market value, duration, convexity, cash flows. Explicit convention defaults below. |

For `quantlib-bond`, dates are ISO calendar dates in 1901–2199; require issue ≤ valuation < maturity. Frequency is 1, 2, 4, or 12 payments/year, default 2; settlement lag defaults to 2 days. Calendars are `null` (default), `target`, or `us-government-bond`. Day counts are `actual/actual-bond` (default), `actual/365`, or `30/360`. Accrual dates are unadjusted, payments follow to the next business day, schedules generate backward, and redemption is 100. The adapter restores QuantLib's global evaluation date after each calculation.

VaR uses a linearly interpolated historical quantile. Expected shortfall averages observed returns at or below that cutoff; both loss measures have a floor of zero. Sparse tails can be unstable; `tail_observations` reports how many observations support the estimate. Undefined risk ratios are null. Forecasts, optimized allocations, and backtests require out-of-sample evaluation appropriate to the intended use.

### Choosing a practical workflow

Use OpenBB to discover and retrieve a specific dataset, then use the fixed local operations for repeated calculations on that same documented input. Running those operations from a CSV/JSON file records its input hash and avoids paying the all-provider startup cost for each scenario. Keep the provider envelope alongside any exported research data: the local CLI records the supplied file and calculation, and cannot reconstruct omitted source metadata.

| Research question | Useful sequence | Required judgment |
| --- | --- | --- |
| How concentrated is a portfolio's historical risk? | Retrieve a bounded, aligned price/return sample; use `returns`, `risk`, `portfolio`, and `correlation` on explicit columns. | Choose adjusted prices or total returns, align dates/currencies, and state the rebalancing assumption. Correlation takes the supplied values, so supply returns when return correlation is intended. |
| Which market or macro factors explain a return series? | Discover an installed Fama–French, FRED, or other relevant source; align its units and frequency with the target; run local `factor`. | Match excess-return definitions, publication dates, and data vintages. Coefficients describe association and do not establish causality. |
| How would a change in yield affect a bond? | Reuse a `quantlib-bond` request with explicit valuation date, settlement, calendar, and day count; vary `yield_rate` across documented scenarios. | Use `bonds` only when the simpler coupon-date convention is appropriate. Neither adapter prices callable/defaultable features. |
| How sensitive is a valuation to assumptions? | Reuse `valuation`, `cashflow`, or `options` requests while changing the named discount/growth, cash-flow, volatility, or price assumptions. | Preserve the source of each assumption and keep units consistent; these are scenario calculations, not refreshed market valuations. |
| Does an allocation or rule survive outside its fitting sample? | Fit `optimize` on a training file; copy the resulting weights into `portfolio` on a separate holdout file. Run `backtest` on a clearly identified sample and report the benchmark and costs. | The fixed tools do not select train/test splits, rebalance fitted weights out of sample, or remove survivorship bias automatically. |
| Does a forecast add useful information? | Run `forecast` on a training series and compare its horizon with withheld observations; contrast with an explicitly chosen simple baseline. | A completed ARIMA fit does not establish predictive value; model-conditional intervals exclude model-selection uncertainty. |

For example, one documented price file supports both `finance risk --input research/prices.csv --column close` and `finance backtest --input research/prices.csv --column close --fast 20 --slow 50 --cost-bps 10`. Multi-asset weights, factor columns, and bond conventions fit naturally in a reviewed JSON request file. Reuse the input file deliberately for comparisons; retrieve a new source sample when the research question requires newer observations.

### Reproducible examples

Checked-in examples are [risk CSV](../deploy/finance/examples/risk.csv), [DCF request](../deploy/finance/examples/valuation.json), and [QuantLib request](../deploy/finance/examples/quantlib-bond.json). The DCF example has enterprise value approximately 1,340.9091, equity value 1,240.9091, and per-share equity value 124.0909 under its explicit assumptions.

```sh
python3 -I deploy/finance/cli.py risk --input deploy/finance/examples/risk.csv
python3 -I deploy/finance/cli.py --request-file deploy/finance/examples/valuation.json
/opt/finance/venv/bin/python -I /opt/finance/cli.py --request-file /workspace/quantlib-bond.json
```

Success envelopes contain `schema_version`, `operation`, `status`, `gate`, `results`, `provenance`, and `limitations`. File provenance includes its SHA-256; calculation time and engine version are recorded. Source freshness is explicitly unverified. Backend warnings are reported by warning class without dumping provider or filesystem exception details.

| Exit code | Meaning | Gate |
| --- | --- | --- |
| 0 | Calculation or diagnostic completed | `pass` |
| 2 | Invalid input, unknown operation, or rejected path | `fail` |
| 3 | Missing backend, numerical failure, incomplete computation, or output limit | `incomplete` |

The generic runner treats invalid/error exits as unsuccessful execution, not as a financial finding. A numerical `pass` means the defined calculation completed; it is not an investment recommendation or proof of economic validity.

## Trusted OpenBB service

The service runs outside agent boxes. Its immutable environment contains current provider packages, and its private service home contains only service-managed configuration/cache. It receives a JSON command and data, never a workspace path, arbitrary Python expression, CLI command, package name, or alternate server address. Provider secrets remain in this trusted process environment.

The installed OpenBB FastAPI application supplies the OpenAPI contract. A bounded, credential-free, network-disabled bootstrap child resolves finite local schema references and selects approved provider `GET` routes and analytical `POST` routes. The service retains that compiled metadata in memory, checks request shape and policy, and passes one selected internal operation contract to each fresh query worker. The worker validates exact parameter/body schemas before importing the SDK and dispatching through an in-process ASGI transport, keeping third-party schema validation within the process resource/deadline boundary. It does not guess Python attributes, generate source code, or run the stock OpenBB MCP command-dispatch/install tools. Current OpenBB defines data processing with POST; rejecting every POST would remove valid analytics. The upstream architecture and extension model are documented in [OpenBB architecture](https://docs.openbb.co/odp/python/concepts/architecture).

### Four MCP tools

| Tool | Purpose |
| --- | --- |
| `openbb_status` | Installed core version, schema/package fingerprints, command count, namespaces, and credential-presence booleans |
| `openbb_catalog` | Search descriptions or page commands using `query`, `namespace`, `offset`, `limit` |
| `openbb_describe` | Exact method, query/path schema, body schema, response schema, and network requirement for one dotted command |
| `openbb_query` | Execute one discovered command with `parameters`, optional `body`, optional `max_rows`, and explicit `allow_network` |

Use the configured server name `finance-openbb`:

```json
{"tool":"mcp","action":"call","server":"finance-openbb","name":"openbb_status","arguments":{}}
```

```json
{"tool":"mcp","action":"call","server":"finance-openbb","name":"openbb_catalog","arguments":{"namespace":"cboe","query":"historical","limit":10}}
```

```json
{"tool":"mcp","action":"call","server":"finance-openbb","name":"openbb_describe","arguments":{"command":"quantitative.summary"}}
```

The installed V5 summary schema puts **both `target` and `data` inside `body`**:

```json
{"tool":"mcp","action":"call","server":"finance-openbb","name":"openbb_query","arguments":{"command":"quantitative.summary","parameters":{},"body":{"target":"close","data":[{"close":1},{"close":2},{"close":3},{"close":4}]},"allow_network":false}}
```

This offline example returns count 4, mean 2.5, and sample variance 5/3 in `data.results`. Provider reads require `allow_network:true`; so does `quantitative.capm`, which obtains external factor data. A discovered command establishes installed coverage, not an active source connection or entitlement. `openbb_status` never tests paid providers.

OpenBB query results preserve the OBBject envelope, including id, provider, warnings, `extra.metadata`, and source-specific `extra.results_metadata` when supplied. The adapter marks those documented fields before REST serialization so OpenBB's `exclude_unset` setting cannot discard metadata populated after construction. It adds a UTC retrieval timestamp, core version, schema SHA-256, returned/total row counts, and explicit truncation state. See [OBBject](https://docs.openbb.co/odp/python/concepts/obbject) for upstream field semantics.

Do not relabel retrieval time as quote time. Upstream metadata can contain a naive timestamp without a timezone; preserve it without inventing one. Check source observation date, publication date, lag, currency, units, adjustment, and revisions before comparing datasets. Secrets are redacted recursively from results, metadata, URLs, and error paths.

### Deploy and authenticate

Build the `finance-service` image using the specialist image workflow. The image launches:

```sh
/opt/openbb/venv/bin/python -I /opt/finance/finance_server.py
```

OpenBB's generated Python client is not used; no `openbb-build` at runtime is required. The bridge imports `openbb_core.api.rest_api:app` with automatic client builds disabled. The venv may remain read-only. The service home must be writable for package settings/cache, private mode 0700, owned by the running service uid. The Compose image uses uid/gid 1000.

Provision a random bearer token of 32–256 visible ASCII characters in a regular file outside all workspaces. Use mode 0600 and uid1000 ownership for the container. Symlinked token paths, group/world-readable files, and invalid tokens are rejected. Configure its host path and a private persistent state directory in a protected copy of [service.env.example](../deploy/finance/service.env.example). The service accepts either `FINANCE_OPENBB_TOKEN_FILE` or `FINANCE_OPENBB_TOKEN`, never both. The token file avoids putting the service bearer secret into its container environment.

Set the actual built image identity in that private environment file, and provision any provider keys there. Start the included [Compose definition](../deploy/finance/compose.yaml):

```sh
docker compose --env-file /etc/boxoro-finance/service.env -f deploy/finance/compose.yaml up -d
```

Compose publishes only `127.0.0.1:8768:8768`, drops capabilities, uses a read-only root filesystem, and bounds memory, CPUs, process count, and `/tmp`. It explicitly forwards only the five supported provider credentials. Do not mount an agent workspace into this service. Use container/host egress policy appropriate to the allowed data providers; the Python socket guard is defense in depth, not a replacement for an operating-system network boundary.

Copy [mcp-servers.example.json](../deploy/finance/mcp-servers.example.json) into a protected control-plane file, replace the placeholder with the **same literal token**, and set `AR_MCP_SERVERS_FILE` to that file. It accepts this exact server object:

```json
{"servers":[{"name":"finance-openbb","url":"http://127.0.0.1:8768/mcp","headers":{"Authorization":"Bearer REPLACE_WITH_FINANCE_OPENBB_TOKEN"},"allowed_tools":["openbb_status","openbb_catalog","openbb_describe","openbb_query"]}]}
```

Header values are literal; the MCP configuration does not expand `${ENV_VAR}` placeholders. Keep this file readable only by the control-plane service user. Add `finance-openbb` to the agent's allowed `mcp_servers` configuration after the operator registry loads it. These are control-side secrets/settings, never chat parameters or workspace files.

The service implements MCP protocol `2025-06-18`, matching the runtime client. `tools/list`, initialization, and ping test the transport; an offline `openbb_query` additionally tests installed execution. Auth is checked before parsing tool arguments. HTTP requests require the literal loopback Host and port, reject Origin headers, and support JSON POST on `/mcp`. Container wildcard binding is explicit and does not relax Host validation.

### Service configuration and bounds

| Variable | Default / accepted values |
| --- | --- |
| `FINANCE_OPENBB_TOKEN_FILE` / `FINANCE_OPENBB_TOKEN` | Exactly one required bearer source |
| `FINANCE_OPENBB_BIND` | `127.0.0.1`; `0.0.0.0` only for an explicitly isolated container |
| `FINANCE_OPENBB_PORT` | 8768; 1–65535 |
| `FINANCE_OPENBB_STATE_DIR` | `/var/lib/boxoro-finance`; absolute, private, service-owned directory without symlink components |
| `FINANCE_OPENBB_MAX_CONCURRENCY` | 2; range 1–8 |
| `FINANCE_OPENBB_DEADLINE_SECONDS` | 90; range 10–180. Keep below the runtime MCP client's 120-second request timeout. |
| `FINANCE_OPENBB_NAMESPACES` | All approved V5 namespaces; comma-separated subset may narrow access |
| `FINANCE_OPENBB_URL_HOSTS` | Empty. Exact HTTPS hostnames approved for endpoint parameters that accept URLs. No wildcard matching. |

Provider credential variables are documented in [OpenBB providers](OPENBB_PROVIDERS.md). They are read at service start. Worker environments omit unrelated cloud keys, model-provider keys, proxy settings, and the MCP bearer token. Service-managed OpenBB user settings have empty credential fields, metadata enabled, fixed private directories, suppressed logging, and no output callbacks or mutable extensions. Local `.env` and TOML files in that configuration directory are rejected. Do not put keys in its JSON settings: startup deliberately refreshes those files.

Each **data query** starts a fresh installed worker, with a process-group deadline, CPU/open-file limits, bounded input/output, and discarded stderr. This avoids cross-request numerical global state and limits a stalled provider. The HTTP service never imports the OpenBB SDK. Instead, its first request builds the catalog in one isolated child; concurrent cold discovery shares that work within each request's deadline. Failed bootstrap attempts are not cached. Later status/search/describe requests use metadata alone, and query workers skip generation/expansion of the full OpenAPI schema.

The cache contains only installed schemas and command descriptions, with no provider response, credential, or SDK object. It is bounded to a 16 MiB bootstrap transfer, lives only in service memory, and is discarded on restart. No saved catalog or caller-supplied operation contract is accepted through MCP. The snapshot carries the schema SHA-256 and an installation fingerprint over package names, versions, and wheel `RECORD` manifests. Every fresh query worker checks that installation fingerprint before loading the SDK; a mismatch returns `catalog_stale` and invalidates the service cache for a later retry. Deploy updated immutable packages by replacing/restarting the service; edits to installed package source in place are unsupported.

The all-provider environment still has a substantial cold cost. `FINANCE_OPENBB_NAMESPACES` narrows permission and catalog coverage; it does not selectively unload installed provider packages. For an empty catalog, `openbb_query` shares one total request deadline across catalog bootstrap and fresh-worker query execution, including any wait for concurrent bootstrap. Plan for that first request's combined work and use the local CLI for repeated small calculations. The [verification command](#dependency-profiles-and-verification) can emit actual authenticated HTTP timings with `BOXORO_BENCH_OPENBB=1`; measurements depend on the host and are not a deployment performance guarantee.

Measured on 2026-11-15 using the checked-in lock, all 1,138 commands, no provider credentials, and the offline summary fixture above:

| Measurement | Before metadata reuse | With metadata reuse |
| --- | --- | --- |
| Cold `openbb_status`, authenticated HTTP | 39.863 s | 37.528 s |
| Warm status / catalog / describe, authenticated HTTP | Each call started another full SDK/schema worker | 1.61 / 2.05 / 1.65 ms |
| Fresh-worker offline summary, authenticated HTTP | 38.483 s | 27.802 s |
| Query-worker peak RSS, initial cache measurement | 1,085 MiB | 867 MiB |
| HTTP parent peak RSS, initial cache measurement | 19 MiB | 63 MiB |

The fresh query was about 28% faster in the final HTTP comparison (an earlier cache run measured about 30%). The tradeoff is additional metadata memory in the parent; the one-time bootstrap still reaches roughly 1,085 MiB in its child. RSS was sampled on the initial cache implementation, before moving exact schema validation exclusively into the worker; the final HTTP smoke was rerun after that refinement. These are individual development-host measurements, not distribution-wide latency or capacity guarantees. The [machine-readable receipt](validation/finance-efficiency-2026-11-15.json) separates HTTP timings, executor timings, memory methods, source revisions, and test outcomes. Profiling attributed about 9.9 seconds to full OpenAPI generation and 1.4 seconds to catalog expansion before the change. Run `openbb_status` once after deployment as an installed-readiness check so catalog bootstrap does not consume the first provider query's deadline. `tools/list` alone checks the MCP transport and does not warm the installed catalog.

The service retains the unpatched `diskcache==5.6.3` transitive dependency, affected by CVE-2025-69872. Exploitation requires write access to a cache that is later deserialized. Startup requires a private service-owned state root, rejects cache-directory symlinks, and sets cache/temp/data/export directories to mode 0700; workers use umask 0077. Keep that state unavailable to agent boxes and other service identities, and never restore an untrusted cache. This is a deployment mitigation, not an upstream fix. See the [exact retained advisory and source review](OPENBB_PROVIDERS.md#retained-diskcache-advisory). Apply a filesystem/volume quota to persistent service state; per-request limits do not bound its lifetime disk growth.

Tool requests are limited to 512 KiB; query-worker output is limited to 2 MiB; the final duplicated text/structured MCP envelope is limited to the runtime's 4 MiB ceiling. The private schema bootstrap has its own 16 MiB output limit and is not an exposed tool. Internal per-query operation contracts are bounded to 512 KiB and never count as caller parameters. `max_rows` defaults to 1,000, maximum 10,000. Truncation sets `status:partial`, `gate:incomplete`, and `truncated:true`; it never appears as complete coverage. Larger upstream responses can fail before truncation, so constrain dates/symbols using the provider parameters shown by `openbb_describe`. `max_rows` limits the returned result and does not narrow the upstream fetch. Streaming responses and file uploads are excluded.

The data policy excludes user/system settings, installation/execution helpers, unapproved namespaces, all mutation HTTP methods, and provider POST routes. Caller credentials, destination paths, proxies, and alternate servers are rejected. The socket guard denies non-public addresses, local sockets, datagrams, and non-HTTP(S) ports during provider dispatch. Native libraries can bypass Python monkeypatches; enforce required egress restrictions outside Python.

## Dependency profiles and verification

The standard-library baseline works without pandas/OpenBB. `requirements-advanced.lock` supplies the separate local numerical environment: NumPy, pandas, SciPy, statsmodels, arch, CVXPY, QuantLib, DuckDB, Polars, Arrow, openpyxl, matplotlib, and Plotly. Dedicated fixed adapters currently use SciPy, statsmodels, and QuantLib; other installed libraries are available for separately authorized research code, not arbitrary expressions accepted by the fixed Finance CLI.

`requirements.lock` supplies the separate all-provider OpenBB service. It includes headless charting support without desktop browser/WebView extras. Chart creation remains subject to each discovered command schema and response limit; the service provides no GUI, chart-file export, or arbitrary output path. Dependency versions and hashes are recorded in the locks, not downloaded opportunistically at execution time.

Riskfolio is deliberately a separate `requirements-riskfolio.lock` profile. Although Riskfolio itself is BSD licensed, the current wheel requires `vectorbt`, whose license includes the Commons Clause. It is excluded from default images. Review that profile's license restrictions before commercial packaging or hosted use; see [OpenBB providers](OPENBB_PROVIDERS.md#licenses-and-commercial-use).

Generate updated locks and test in clean Python 3.12 environments:

```sh
uv pip compile --python-version 3.12 --generate-hashes deploy/finance/requirements.in -o deploy/finance/requirements.lock
uv pip compile --python-version 3.12 --generate-hashes deploy/finance/requirements-advanced.in -o deploy/finance/requirements-advanced.lock
uv venv --python 3.12 .finance-openbb
uv pip sync --python .finance-openbb/bin/python --require-hashes deploy/finance/requirements.lock
uv venv --python 3.12 .finance-advanced
uv pip sync --python .finance-advanced/bin/python --require-hashes deploy/finance/requirements-advanced.lock
python3 -m unittest qa.test_finance qa.test_finance_openbb qa.test_finance_catalog_cache -v
.finance-advanced/bin/python -m unittest qa.test_finance_advanced -v
BOXORO_TEST_OPENBB=1 .finance-openbb/bin/python -m unittest qa.test_finance_sdk -v
# Add BOXORO_BENCH_OPENBB=1 to print cold/warm authenticated HTTP request timings.
```

The actual installed SDK smoke uses the authenticated HTTP service with no provider credentials or provider network. It verifies cold/warm metadata discovery, shared schema/package fingerprints, and the numerical result plus real OBBject metadata from a fresh query worker. The regression suites cover known analytical results, one-bar backtest lag, initial drawdown, invalid values, constrained optimization, missing dependencies, path containment, MCP authentication/Host/Origin checks, schema validation, metadata preservation, redaction, deadlines, response-size limits, concurrent catalog bootstrap, failure retry, credential exclusion, and stale-installation rejection. Live provider entitlements, rate limits, source coverage, freshness, and container deployment must be verified in the intended operating environment. Package availability is never reported as successful live-market access.

## Primary references

- [OpenBB V5 provider packages](https://docs.openbb.co/odp/python/extensions/providers), [migration guide](https://docs.openbb.co/odp/python/migration-from-v4), and [settings](https://docs.openbb.co/odp/python/settings)
- [OpenBB credentials](https://docs.openbb.co/odp/python/user-guide/credentials), [environment variables](https://docs.openbb.co/odp/python/user-guide/environment-variables), and [MCP extension](https://docs.openbb.co/odp/python/extensions/interface/openbb-mcp)
- [SciPy optimization reference](https://docs.scipy.org/doc/scipy/reference/optimize.html), [statsmodels ARIMA](https://www.statsmodels.org/stable/generated/statsmodels.tsa.arima.model.ARIMA.html), and [QuantLib](https://www.quantlib.org/)
- [MCP 2025-06-18 transport specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
