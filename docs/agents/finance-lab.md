# Finance Lab

Financial and economic research across all installed permitted OpenBB V5 data commands, portfolio risk, valuation, options, bonds, cash flows and quantitative models.

Profile ID: `finance-lab`. Preferred family: `finance`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# Finance Lab

Use `specialist` with agent `finance-lab` for local calculations. Start with describe and doctor scoped to the intended operation or workflow, choosing one selector; use full profile doctor for image inventory. Use finance-doctor for analytical-backend readiness. Use the trusted OpenBB connector for live/public financial data: openbb_status, openbb_catalog, openbb_describe, then openbb_query with the exact discovered schema. Provider queries require allow_network=true; pure analytical commands can remain offline. Never send provider credentials as tool arguments or write them inside a workspace.

Record the provider, retrieval time, observation time, units, frequency, currency and any delay/adjustment. Preserve warnings and results metadata. Missing provider data, rate limits, licensing restrictions, missing packages and truncated output are incomplete coverage. Never invent live quotes or present retrieved time as quote time.

Local files are CSV or JSON. For structured operands use finance-request and a JSON document with operation, parameters and optional data/input. Price data must be positive and dates unique; align series explicitly. Rates/returns are decimal fractions. Distinguish total-return prices from unadjusted prices, arithmetic returns from log returns, annual risk-free rates from per-period rates, and monthly percent Fama-French factors from decimal returns.

Use finance-risk/portfolio/correlation for historical research, finance-valuation for explicit DCF assumptions, finance-options for European Black-Scholes-Merton research, finance-bonds for regular coupon-date valuation, and finance-cashflow for equal-period NPV/IRR. Forecasts and allocations require held-out validation; backtests lag signals and include specified turnover costs but are not execution-quality estimates. No trade/order, brokerage credential, transaction or money-movement tools are available.

See docs/FINANCE_AGENT.md and docs/OPENBB_PROVIDERS.md for exact contracts, source terms and installation. Optional Riskfolio's vectorbt dependency has Commons Clause restrictions and is excluded from default images.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Boxoro Finance Lab (`finance`) | image | `builtin: boxoro-finance 1.0.0` | MIT (Boxoro code); numerical-package licenses are recorded in finance dependency notices. Optional Riskfolio/vectorbt profile is restricted. | [upstream](https://github.com/RictalNickel/agent-rush-runtime) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them. Generated artifacts are not automatically wired into later requests: copy a verified artifact into an ordinary workspace file with the existing authorized file tools before using it as an input; internal report storage is not an input root.

| Workflow | Ordered operations |
| --- | --- |
| `readiness` | `finance-doctor` |

### Discovery and readiness

Discover and probe only the intended workflow:

```json
{
  "action": "describe",
  "agent": "finance-lab",
  "workflow": "readiness"
}
```

```json
{
  "action": "doctor",
  "agent": "finance-lab",
  "workflow": "readiness"
}
```

For one utility, replace `workflow` with `operation` and the registered operation ID; do not supply both selectors. Omit both selectors only when a full profile catalog or availability audit is needed. Missing optional tools outside the selected task do not determine its readiness. A probe checks executable availability; the actual operation and project prerequisites still need validation.

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `finance-doctor`

Report local analytics and optional numerical-package availability.

Command: `/opt/box/agents/bin/finance doctor`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

Research analysis; decimals for rates/returns. Exit 3 means incomplete or missing backend. Input CSV/JSON is data only; no project code, network or brokerage actions.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `finance-request` | Run a fixed named analysis from a strict JSON request document. | none declared |
| `finance-returns` | Compute simple returns from ordered positive prices. | none declared |
| `finance-risk` | Compute compounded return, annualized volatility, Sharpe, Sortino, drawdown, historical VaR and expected shortfall. | none declared |
| `finance-portfolio` | Evaluate aligned long-only constant-weight portfolio risk; weights in input JSON. | none declared |
| `finance-correlation` | Compute aligned numeric-column covariance and Pearson correlation. | none declared |
| `finance-valuation` | Discount annual cash-flow projections and Gordon terminal value. | none declared |
| `finance-options` | Price European options and Greeks; invert a market price to implied volatility. | none declared |
| `finance-bonds` | Price regular fixed-coupon bonds, duration, convexity and DV01. | none declared |
| `finance-cashflow` | Compute equal-period NPV and a bracketed IRR with multiple-root warnings. | none declared |
| `finance-factor` | Fit factor OLS with robust errors and confidence intervals; optional statsmodels required. | none declared |
| `finance-monte-carlo` | Simulate a seeded geometric Brownian terminal-price distribution. | none declared |
| `finance-backtest` | Backtest long/cash moving-average crossover with prior-bar signal and turnover costs. | none declared |
| `finance-forecast` | Fit bounded ARIMA and model-conditional intervals; optional statsmodels required. | none declared |
| `finance-optimize` | Solve constrained long-only min-variance, max-Sharpe or risk-parity allocation; optional SciPy required. | none declared |
| `finance-quantlib-bond` | Calendar-aware fixed-rate bond clean/dirty price, accrual, duration, convexity and scheduled cash flows using QuantLib. | none declared |

Use `describe` with an `operation` selector for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### Boxoro Finance OpenBB (`boxoro-finance-openbb`)

Status: **self-hosted**. Transport: `streamable-http`.

Bearer token from control-side FINANCE_OPENBB_TOKEN_FILE or FINANCE_OPENBB_TOKEN; provider credentials only in trusted service environment.

Four fixed read-only tools: openbb_status, openbb_catalog, openbb_describe, openbb_query. GET commands from approved installed provider namespaces; POST only econometrics, quantitative and technical. No generic CLI, skill installation, user/system routes or trading.

Primary documentation: [source 1](https://docs.openbb.co/odp/python/concepts/architecture), [source 2](https://docs.openbb.co/odp/python/extensions/interface/openbb-mcp), [source 3](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports).

### Tool-specific qualifications


## Limits and interpretation

- OpenBB runs in a separate trusted control-side service; provider entitlements and availability differ.
- Calculations are research outputs; data quality, units, assumptions and model risk require review.
- No brokerage/order execution or money movement.
- Forecast, factor and allocation adapters need the advanced image.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.

For the complete trusted-service architecture and provider setup, see [Finance Lab](../FINANCE_AGENT.md) and [OpenBB providers](../OPENBB_PROVIDERS.md).
