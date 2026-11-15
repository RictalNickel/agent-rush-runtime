"""Deterministic local finance calculations; units are explicit in every result."""
import math
import random
import statistics as st
from itertools import pairwise

from .common import error, fields, integer, number, numeric_table, series

SERIES_PARAMETERS = {"column", "input_kind", "frequency", "risk_free_rate", "confidence"}


def quantile(values, probability):
    ordered = sorted(values)
    index = (len(ordered) - 1) * probability
    lo, hi = math.floor(index), math.ceil(index)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (index - lo)


def returns_from(values, input_kind):
    if input_kind == "returns":
        if any(value < -1 for value in values):
            error("invalid_data", "Simple returns cannot be less than -1.")
        return values
    if input_kind != "prices":
        error("invalid_parameters", "input_kind must be prices or returns.")
    if any(value <= 0 for value in values):
        error("invalid_data", "Price observations must be positive.")
    return [current / previous - 1 for previous, current in pairwise(values)]


def risk_metrics(returns, parameters):
    frequency = integer(parameters.get("frequency", 252), "frequency", 1, 36600)
    annual_rf = number(parameters.get("risk_free_rate", 0), "risk_free_rate", minimum=-0.99, maximum=10)
    confidence = number(parameters.get("confidence", 0.95), "confidence", minimum=0.5, maximum=0.9999)
    if not returns:
        error("invalid_data", "At least one return observation is required.")
    mean = st.fmean(returns)
    volatility = st.stdev(returns) if len(returns) > 1 else None
    rf_period = math.expm1(math.log1p(annual_rf) / frequency)
    excess_mean = mean - rf_period
    downside = math.sqrt(st.fmean(min(value - rf_period, 0) ** 2 for value in returns))
    wealth, peak, maximum_drawdown = 1.0, 1.0, 0.0
    for value in returns:
        wealth *= 1 + value
        peak = max(peak, wealth)
        maximum_drawdown = min(maximum_drawdown, wealth / peak - 1)
    cutoff = quantile(returns, 1 - confidence)
    tail = [value for value in returns if value <= cutoff]
    return {
        "observations": len(returns), "frequency": frequency, "units": "decimal returns",
        "total_return": wealth - 1,
        "cagr": wealth ** (frequency / len(returns)) - 1,
        "annualized_arithmetic_return": mean * frequency,
        "annualized_volatility": volatility * math.sqrt(frequency) if volatility is not None else None,
        "sharpe_ratio": excess_mean / volatility * math.sqrt(frequency) if volatility else None,
        "sortino_ratio": excess_mean / downside * math.sqrt(frequency) if downside else None,
        "max_drawdown": maximum_drawdown,
        "historical_var": max(0, -cutoff),
        "historical_expected_shortfall": max(0, -st.fmean(tail)),
        "confidence": confidence, "tail_observations": len(tail), "tail_horizon": "one observation",
        "risk_free_rate_annual": annual_rf, "risk_free_rate_per_observation": rf_period,
        "annualization": "equal periods; CAGR uses observation count / frequency",
        "undefined_ratios": "null when denominator is zero or sample volatility is unavailable",
    }


def returns_analysis(data, parameters):
    fields(parameters, SERIES_PARAMETERS)
    values, dates = series(data, parameters.get("column", "close"))
    kind = parameters.get("input_kind", "prices")
    returns = returns_from(values, kind)
    dates = dates[1:] if dates and kind == "prices" else dates
    return [{"date": dates[i] if dates else None, "observation": i + 1, "return": value} for i, value in enumerate(returns)]


def risk_analysis(data, parameters):
    fields(parameters, SERIES_PARAMETERS)
    values, _ = series(data, parameters.get("column", "close"))
    return risk_metrics(returns_from(values, parameters.get("input_kind", "prices")), parameters)


def covariance_table(table):
    cols = list(zip(*table))
    means = [st.fmean(values) for values in cols]
    return [[sum((a - means[i]) * (b - means[j]) for a, b in zip(cols[i], cols[j])) / (len(table) - 1)
             for j in range(len(cols))] for i in range(len(cols))]


def correlation_analysis(data, parameters):
    fields(parameters, {"columns"})
    columns, table = numeric_table(data, parameters.get("columns"))
    cov = covariance_table(table)
    return {"observations": len(table), "columns": columns,
            "covariance": {a: {b: cov[i][j] for j, b in enumerate(columns)} for i, a in enumerate(columns)},
            "correlation": {a: {b: cov[i][j] / math.sqrt(cov[i][i] * cov[j][j]) if cov[i][i] > 0 and cov[j][j] > 0 else None
                                 for j, b in enumerate(columns)} for i, a in enumerate(columns)},
            "missing_data": "rejected; no implicit pairwise deletion"}


def portfolio_analysis(data, parameters):
    fields(parameters, SERIES_PARAMETERS | {"columns", "weights"})
    columns, table = numeric_table(data, parameters.get("columns"))
    weights = parameters.get("weights", [1 / len(columns)] * len(columns))
    if not isinstance(weights, list) or len(weights) != len(columns):
        error("invalid_parameters", "weights must have one entry per selected column.")
    weights = [number(weight, "weight", minimum=0, maximum=1) for weight in weights]
    if not math.isclose(sum(weights), 1, rel_tol=0, abs_tol=1e-8):
        error("invalid_parameters", "Long-only portfolio weights must sum to one.")
    returns = list(zip(*(returns_from(list(values), parameters.get("input_kind", "prices")) for values in zip(*table))))
    combined = [sum(weight * value for weight, value in zip(weights, row)) for row in returns]
    cov = covariance_table(returns) if len(returns) > 1 else None
    variance = sum(weights[i] * weights[j] * cov[i][j] for i in range(len(weights)) for j in range(len(weights))) if cov else None
    contribution = [weights[i] * sum(weights[j] * cov[i][j] for j in range(len(weights))) / variance
                    for i in range(len(weights))] if variance and variance > 0 else [None] * len(weights)
    return {"weights": dict(zip(columns, weights)), "risk": risk_metrics(combined, parameters),
            "variance_contribution_shares": dict(zip(columns, contribution)),
            "rebalancing": "constant weights each observation", "fees_and_taxes": "excluded"}


def valuation_analysis(data, parameters):
    fields(parameters, {"cash_flows", "discount_rate", "terminal_growth", "net_debt", "shares"}, {"cash_flows", "discount_rate"})
    flows, _ = series(parameters["cash_flows"], minimum=1)
    if len(flows) > 100:
        error("invalid_parameters", "At most 100 annual projections are supported.")
    rate = number(parameters["discount_rate"], "discount_rate", minimum=0.000001, maximum=10)
    growth = number(parameters.get("terminal_growth", 0), "terminal_growth", minimum=-0.99, maximum=1)
    if rate <= growth:
        error("invalid_parameters", "discount_rate must exceed terminal_growth.")
    debt = number(parameters.get("net_debt", 0), "net_debt")
    shares = number(parameters.get("shares", 1), "shares", minimum=1e-12)
    discounted = [flow / (1 + rate) ** (i + 1) for i, flow in enumerate(flows)]
    terminal = flows[-1] * (1 + growth) / (rate - growth)
    present_terminal = terminal / (1 + rate) ** len(flows)
    enterprise = sum(discounted) + present_terminal
    return {"enterprise_value": enterprise, "equity_value": enterprise - debt,
            "equity_value_per_share": (enterprise - debt) / shares, "discounted_cash_flows": discounted,
            "terminal_value": terminal, "present_terminal_value": present_terminal,
            "terminal_value_share": present_terminal / enterprise if enterprise else None,
            "assumptions": "annual end-of-year unlevered cash flows, Gordon growth terminal value, consistent currency"}


def _normal_cdf(value):
    return (1 + math.erf(value / math.sqrt(2))) / 2


def _option(spot, strike, rate, dividend, volatility, years, kind):
    sign = 1 if kind == "call" else -1
    root = math.sqrt(years)
    d1 = (math.log(spot / strike) + (rate - dividend + volatility ** 2 / 2) * years) / (volatility * root)
    d2 = d1 - volatility * root
    n1, n2 = _normal_cdf(sign * d1), _normal_cdf(sign * d2)
    density = math.exp(-d1 * d1 / 2) / math.sqrt(2 * math.pi)
    ds, dk = math.exp(-dividend * years), math.exp(-rate * years)
    return {
        "price": sign * (spot * ds * n1 - strike * dk * n2),
        "delta": sign * ds * n1, "gamma": ds * density / (spot * volatility * root),
        "vega_per_volatility_point": spot * ds * density * root / 100,
        "theta_per_calendar_day": (-spot * ds * density * volatility / (2 * root)
                                   - sign * rate * strike * dk * n2 + sign * dividend * spot * ds * n1) / 365,
        "rho_per_rate_point": sign * strike * years * dk * n2 / 100,
    }


def options_analysis(data, parameters):
    fields(parameters, {"spot", "strike", "rate", "dividend_yield", "volatility", "years", "kind", "market_price"}, {"spot", "strike", "years"})
    spot = number(parameters["spot"], "spot", minimum=1e-12, maximum=1e12)
    strike = number(parameters["strike"], "strike", minimum=1e-12, maximum=1e12)
    years = number(parameters["years"], "years", minimum=1e-8, maximum=100)
    rate = number(parameters.get("rate", 0), "rate", minimum=-0.5, maximum=2)
    dividend = number(parameters.get("dividend_yield", 0), "dividend_yield", minimum=-0.5, maximum=2)
    kind = parameters.get("kind", "call")
    if kind not in ("call", "put"):
        error("invalid_parameters", "kind must be call or put.")
    volatility = number(parameters.get("volatility", 0.2), "volatility", minimum=1e-8, maximum=10)
    implied = None
    if "market_price" in parameters:
        market = number(parameters["market_price"], "market_price", minimum=0)
        lower, upper = 1e-8, 10.0
        low_price = _option(spot, strike, rate, dividend, lower, years, kind)["price"]
        high_price = _option(spot, strike, rate, dividend, upper, years, kind)["price"]
        if not low_price < market < high_price:
            error("invalid_parameters", "market_price has no implied volatility within the supported positive range.")
        for _ in range(100):
            middle = (lower + upper) / 2
            price = _option(spot, strike, rate, dividend, middle, years, kind)["price"]
            if price < market:
                lower = middle
            else:
                upper = middle
        implied = volatility = (lower + upper) / 2
    result = _option(spot, strike, rate, dividend, volatility, years, kind)
    result.update({"kind": kind, "volatility": volatility, "implied_volatility": implied,
                   "model": "Black-Scholes-Merton European option with continuous dividend yield",
                   "units": "rates/volatility are annual decimal fractions; vega/rho are for +0.01 changes",
                   "limitations": "No American exercise, discrete dividends, transaction costs or volatility smile."})
    return result


def bonds_analysis(data, parameters):
    fields(parameters, {"face", "coupon_rate", "yield_rate", "years", "payments_per_year"}, {"coupon_rate", "yield_rate", "years"})
    face = number(parameters.get("face", 100), "face", minimum=1e-12, maximum=1e15)
    coupon = number(parameters["coupon_rate"], "coupon_rate", minimum=0, maximum=2)
    yield_rate = number(parameters["yield_rate"], "yield_rate", minimum=-0.99, maximum=10)
    years = number(parameters["years"], "years", minimum=1 / 12, maximum=100)
    frequency = integer(parameters.get("payments_per_year", 2), "payments_per_year", 1, 12)
    if not math.isclose(years * frequency, round(years * frequency), abs_tol=1e-8):
        error("invalid_parameters", "years must contain a whole number of coupon periods.")
    count = round(years * frequency)
    base = 1 + yield_rate / frequency
    flows = [face * coupon / frequency + (face if i == count else 0) for i in range(1, count + 1)]
    pv = [flow / base ** i for i, flow in enumerate(flows, 1)]
    price = sum(pv)
    duration = sum(i / frequency * value for i, value in enumerate(pv, 1)) / price
    convexity = sum(value * i * (i + 1) for i, value in enumerate(pv, 1)) / (price * frequency ** 2 * base ** 2)
    return {"price": price, "macaulay_duration_years": duration, "modified_duration_years": duration / base,
            "convexity_years_squared": convexity, "dv01": price * duration / base / 10000,
            "coupon_periods": count, "assumptions": "valuation on coupon date; regular coupons; no default or embedded options; no accrued interest"}


def cashflow_analysis(data, parameters):
    fields(parameters, {"cash_flows", "discount_rate"}, {"cash_flows"})
    flows, _ = series(parameters["cash_flows"], minimum=2)
    if len(flows) > 1000:
        error("invalid_parameters", "At most 1000 cash-flow periods are supported.")
    rate = number(parameters.get("discount_rate", 0.1), "discount_rate", minimum=-0.9, maximum=10)
    def present_value(value):
        # Backward discounting avoids overflowing a large denominator for long
        # amortization schedules even when their present value is ordinary.
        total = 0.0
        for flow in reversed(flows):
            total = flow + total / (1 + value)
        return total
    npv = present_value(rate)
    nonzero = [flow for flow in flows if flow != 0]
    sign_changes = sum((a < 0) != (b < 0) for a, b in pairwise(nonzero))
    irr = None
    reason = None
    if sign_changes != 1:
        reason = "IRR omitted: cash flows do not have exactly one sign change; roots may be absent or multiple."
    else:
        scale = max(abs(flow) for flow in flows)
        normalized = [flow / scale for flow in flows]
        # Discount backwards, avoiding overflow near the -1 lower rate boundary.
        def scaled_npv(value):
            total = 0.0
            if value < 0:
                for flow in normalized:
                    total = total * (1 + value) + flow
                return total
            for flow in reversed(normalized):
                total = flow + total / (1 + value)
            return total
        low, high = -0.999999, 1000.0
        lo_value, hi_value = scaled_npv(low), scaled_npv(high)
        if lo_value == 0:
            irr = low
        elif hi_value == 0:
            irr = high
        elif (lo_value > 0) == (hi_value > 0):
            reason = "No IRR root bracketed between -0.999999 and 1000 per period."
        else:
            for _ in range(150):
                middle = (low + high) / 2
                if (scaled_npv(middle) > 0) == (lo_value > 0):
                    low = middle
                else:
                    high = middle
            irr = (low + high) / 2
    return {"npv": npv, "irr": irr, "irr_note": reason, "cash_flow_sign_changes": sign_changes,
            "units": "rate per equal cash-flow period; first flow occurs at time zero"}


def backtest_analysis(data, parameters):
    fields(parameters, SERIES_PARAMETERS | {"fast", "slow", "cost_bps"})
    if parameters.get("input_kind", "prices") != "prices":
        error("invalid_parameters", "The moving-average backtest requires prices, not return observations.")
    prices, _ = series(data, parameters.get("column", "close"), minimum=3)
    returns = returns_from(prices, "prices")
    fast = integer(parameters.get("fast", 20), "fast", 1, 1000)
    slow = integer(parameters.get("slow", 50), "slow", 2, 2000)
    if fast >= slow or slow >= len(prices):
        error("invalid_parameters", "fast < slow < observation count is required.")
    cost = number(parameters.get("cost_bps", 10), "cost_bps", minimum=0, maximum=1000) / 10000
    prefix = [0.0]
    for price in prices:
        prefix.append(prefix[-1] + price)
    signals = [0.0] * len(prices)
    for i in range(slow - 1, len(prices)):
        signals[i] = float((prefix[i + 1] - prefix[i + 1 - fast]) / fast > (prefix[i + 1] - prefix[i + 1 - slow]) / slow)
    strategy, previous, trades = [], 0.0, 0
    for index, value in enumerate(returns):
        holding = signals[index]  # End-of-bar signal can affect only the next return.
        turnover = abs(holding - previous)
        trades += int(turnover > 0)
        strategy.append((1 - cost * turnover) * (1 + holding * value) - 1)
        previous = holding
    return {"strategy": risk_metrics(strategy, parameters), "benchmark": risk_metrics(returns, parameters),
            "trades": trades, "ending_position": previous, "cost_bps_per_unit_turnover": cost * 10000,
            "execution": "signal from prior closing bar; hypothetical fill at that close; final position marked, not liquidated",
            "limitations": "Illustrative long/cash backtest; excludes slippage beyond fixed costs, dividends unless prices adjusted, taxes and survivorship bias."}


def monte_carlo_analysis(data, parameters):
    fields(parameters, {"spot", "drift", "volatility", "years", "simulations", "seed"}, {"spot", "drift", "volatility"})
    spot = number(parameters["spot"], "spot", minimum=1e-12, maximum=1e12)
    drift = number(parameters["drift"], "drift", minimum=-2, maximum=2)
    volatility = number(parameters["volatility"], "volatility", minimum=0, maximum=3)
    years = number(parameters.get("years", 1), "years", minimum=1e-6, maximum=100)
    simulations = integer(parameters.get("simulations", 10000), "simulations", 100, 100000)
    seed = integer(parameters.get("seed", 42), "seed", 0, 2 ** 32 - 1)
    generator = random.Random(seed)
    terminal = [spot * math.exp((drift - volatility ** 2 / 2) * years + volatility * math.sqrt(years) * generator.gauss(0, 1)) for _ in range(simulations)]
    return {"model": "geometric Brownian motion terminal distribution", "simulations": simulations, "seed": seed,
            "terminal_mean": st.fmean(terminal), "terminal_percentiles": {str(q): quantile(terminal, q / 100) for q in (1, 5, 25, 50, 75, 95, 99)},
            "probability_of_loss": sum(value < spot for value in terminal) / simulations,
            "assumptions": "constant annual drift and volatility; independent Gaussian shocks; a scenario model, not a calibrated forecast"}


OPERATIONS = {
    "returns": returns_analysis, "risk": risk_analysis, "portfolio": portfolio_analysis,
    "correlation": correlation_analysis, "valuation": valuation_analysis, "options": options_analysis,
    "bonds": bonds_analysis, "cashflow": cashflow_analysis, "backtest": backtest_analysis,
    "monte-carlo": monte_carlo_analysis,
}
