"""Optional numerical adapters. Imports are lazy and never execute project code."""
import math

from .analytics import returns_from, risk_metrics
from .common import error, fields, integer, number, numeric_table, series


def dependencies(*names):
    import importlib
    try:
        return [importlib.import_module(name) for name in names]
    except ImportError:
        error("missing_dependency", "Install the optional Finance Lab advanced environment for this analysis.", incomplete=True)


def factor_analysis(data, parameters):
    fields(parameters, {"target", "factors", "cov_type"}, {"target", "factors"})
    target, factors = parameters["target"], parameters["factors"]
    if not isinstance(target, str) or not isinstance(factors, list) or target in factors or not 1 <= len(factors) <= 20:
        error("invalid_parameters", "Supply a target column and one to twenty distinct factor columns.")
    columns, table = numeric_table(data, [target] + factors)
    if len(table) <= len(columns) + 1:
        error("invalid_data", "Factor regression needs more observations than coefficients plus one.")
    cov_type = parameters.get("cov_type", "HC3")
    if cov_type not in ("nonrobust", "HC0", "HC1", "HC2", "HC3"):
        error("invalid_parameters", "Unsupported covariance estimator.")
    np, sm = dependencies("numpy", "statsmodels.api")
    matrix = np.asarray(table, dtype=float)
    design = sm.add_constant(matrix[:, 1:], has_constant="add")
    if np.linalg.matrix_rank(design) < design.shape[1]:
        error("invalid_data", "The factor design is rank deficient.")
    fit = sm.OLS(matrix[:, 0], design).fit(cov_type=cov_type)
    intervals = fit.conf_int()
    return {"observations": int(fit.nobs), "r_squared": float(fit.rsquared), "adjusted_r_squared": float(fit.rsquared_adj),
            "coefficients": [{"factor": name, "coefficient": float(fit.params[i]), "standard_error": float(fit.bse[i]),
                              "p_value": float(fit.pvalues[i]), "confidence_interval_95": intervals[i].tolist()}
                             for i, name in enumerate(["intercept"] + factors)],
            "covariance_estimator": cov_type, "assumptions": "Aligned returns and factors in the same frequency and decimal units; no causal interpretation."}


def forecast_analysis(data, parameters):
    fields(parameters, {"column", "p", "d", "q", "horizon"})
    values, _ = series(data, parameters.get("column", "close"), minimum=30)
    p = integer(parameters.get("p", 1), "p", 0, 5)
    d = integer(parameters.get("d", 1), "d", 0, 2)
    q = integer(parameters.get("q", 0), "q", 0, 5)
    horizon = integer(parameters.get("horizon", 10), "horizon", 1, 252)
    arima, = dependencies("statsmodels.tsa.arima.model")
    fit = arima.ARIMA(values, order=(p, d, q)).fit()
    if not getattr(fit, "mle_retvals", {}).get("converged", True):
        error("solver_failed", "The ARIMA fit did not converge; revise the model or input series.", incomplete=True)
    prediction = fit.get_forecast(steps=horizon)
    intervals = prediction.conf_int(alpha=0.05)
    return {"model": f"ARIMA({p},{d},{q})", "aic": float(fit.aic), "observations": len(values),
            "forecast": [{"step": i + 1, "mean": float(mean), "lower_95": float(intervals[i][0]), "upper_95": float(intervals[i][1])}
                         for i, mean in enumerate(prediction.predicted_mean)],
            "limitations": "In-sample estimated model; intervals are model conditional and exclude model uncertainty. Validate on held-out data."}


def optimize_analysis(data, parameters):
    fields(parameters, {"columns", "input_kind", "frequency", "risk_free_rate", "objective", "max_weight"})
    columns, table = numeric_table(data, parameters.get("columns"))
    if not 2 <= len(columns) <= 32 or len(table) < 10:
        error("invalid_data", "Optimization requires two to 32 assets and at least ten observations.")
    objective = parameters.get("objective", "min-variance")
    if objective not in ("min-variance", "max-sharpe", "risk-parity"):
        error("invalid_parameters", "objective must be min-variance, max-sharpe, or risk-parity.")
    bound = number(parameters.get("max_weight", 1), "max_weight", minimum=0.01, maximum=1)
    if bound * len(columns) < 1 - 1e-9:
        error("invalid_parameters", "max_weight makes the fully invested portfolio infeasible.")
    frequency = integer(parameters.get("frequency", 252), "frequency", 1, 36600)
    risk_free = number(parameters.get("risk_free_rate", 0), "risk_free_rate", minimum=-0.99, maximum=10)
    arithmetic_risk_free = math.expm1(math.log1p(risk_free) / frequency) * frequency
    np, scipy = dependencies("numpy", "scipy.optimize")
    returns = np.array(list(zip(*(returns_from(list(values), parameters.get("input_kind", "prices")) for values in zip(*table)))))
    covariance = np.cov(returns, rowvar=False) * frequency
    mean = returns.mean(axis=0) * frequency
    if float(np.linalg.eigvalsh(covariance).min()) <= 1e-14:
        error("invalid_data", "Covariance is singular or degenerate; clean the data or reduce the asset set.")
    def loss(weights):
        variance = float(weights @ covariance @ weights)
        if objective == "min-variance":
            return variance
        if objective == "max-sharpe":
            return -float(weights @ mean - arithmetic_risk_free) / variance ** 0.5
        shares = weights * (covariance @ weights) / variance
        return float(((shares - 1 / len(columns)) ** 2).sum())
    fit = scipy.minimize(loss, np.ones(len(columns)) / len(columns), method="SLSQP",
                         bounds=[(0, bound)] * len(columns), constraints=[{"type": "eq", "fun": lambda weights: weights.sum() - 1}],
                         options={"maxiter": 1000, "ftol": 1e-12})
    if not fit.success or abs(float(fit.x.sum()) - 1) > 1e-6 or (fit.x < -1e-7).any() or (fit.x > bound + 1e-7).any():
        error("solver_failed", "The constrained optimizer did not produce a validated solution.", incomplete=True)
    result = risk_metrics((returns @ fit.x).tolist(), parameters)
    return {"objective": objective, "weights": dict(zip(columns, fit.x.tolist())), "in_sample_risk": result,
            "solver": "SciPy SLSQP", "iterations": int(fit.nit),
            "limitations": "In-sample, long-only, fully invested allocation; estimates are unstable and not a trade instruction."}


def quantlib_bond_analysis(data, parameters):
    fields(parameters, {"issue_date", "maturity_date", "valuation_date", "face", "coupon_rate", "yield_rate",
                        "payments_per_year", "settlement_days", "day_count", "calendar"},
           {"issue_date", "maturity_date", "valuation_date", "coupon_rate", "yield_rate"})
    import datetime as dt
    dates = {}
    for key in ("issue_date", "maturity_date", "valuation_date"):
        try:
            dates[key] = dt.date.fromisoformat(parameters[key])
        except (ValueError, TypeError):
            error("invalid_parameters", "Bond dates must be ISO-8601 calendar dates.")
        if not 1901 <= dates[key].year <= 2199:
            error("invalid_parameters", "QuantLib supports years 1901 through 2199.")
    if not dates["issue_date"] <= dates["valuation_date"] < dates["maturity_date"]:
        error("invalid_parameters", "Require issue_date <= valuation_date < maturity_date.")
    face = number(parameters.get("face", 100), "face", minimum=1e-8, maximum=1e15)
    coupon = number(parameters["coupon_rate"], "coupon_rate", minimum=0, maximum=2)
    yield_rate = number(parameters["yield_rate"], "yield_rate", minimum=-0.5, maximum=10)
    payments = integer(parameters.get("payments_per_year", 2), "payments_per_year", 1, 12)
    settlement_days = integer(parameters.get("settlement_days", 2), "settlement_days", 0, 30)
    if payments not in (1, 2, 4, 12):
        error("invalid_parameters", "payments_per_year must be 1, 2, 4 or 12.")
    calendar_name = parameters.get("calendar", "null")
    day_count_name = parameters.get("day_count", "actual/actual-bond")
    if calendar_name not in ("null", "target", "us-government-bond") or day_count_name not in ("actual/365", "actual/actual-bond", "30/360"):
        error("invalid_parameters", "Unsupported calendar or day-count convention.")
    ql, = dependencies("QuantLib")
    calendar = {"null": ql.NullCalendar, "target": ql.TARGET, "us-government-bond": lambda: ql.UnitedStates(ql.UnitedStates.GovernmentBond)}[calendar_name]()
    day_count = {"actual/365": ql.Actual365Fixed, "actual/actual-bond": lambda: ql.ActualActual(ql.ActualActual.Bond), "30/360": lambda: ql.Thirty360(ql.Thirty360.BondBasis)}[day_count_name]()
    frequency = {1: ql.Annual, 2: ql.Semiannual, 4: ql.Quarterly, 12: ql.Monthly}[payments]
    converted = {key: ql.Date(value.day, value.month, value.year) for key, value in dates.items()}
    previous_date = ql.Settings.instance().evaluationDate
    try:
        ql.Settings.instance().evaluationDate = converted["valuation_date"]
        schedule = ql.Schedule(converted["issue_date"], converted["maturity_date"], ql.Period(frequency), calendar,
                               ql.Unadjusted, ql.Unadjusted, ql.DateGeneration.Backward, False)
        bond = ql.FixedRateBond(settlement_days, face, schedule, [coupon], day_count, ql.Following, 100, converted["issue_date"])
        settlement = bond.settlementDate()
        if settlement >= converted["maturity_date"]:
            error("invalid_parameters", "Settlement falls on or after maturity.")
        clean = bond.cleanPrice(yield_rate, day_count, ql.Compounded, frequency, settlement)
        accrued = bond.accruedAmount(settlement)
        duration = ql.BondFunctions.duration(bond, yield_rate, day_count, ql.Compounded, frequency, ql.Duration.Modified, settlement)
        convexity = ql.BondFunctions.convexity(bond, yield_rate, day_count, ql.Compounded, frequency, settlement)
        return {"engine": "QuantLib", "engine_version": ql.__version__, "clean_price_per_100": clean,
                "dirty_price_per_100": clean + accrued, "accrued_per_100": accrued, "market_value": (clean + accrued) * face / 100,
                "modified_duration_years": duration, "convexity_years_squared": convexity,
                "settlement_date": settlement.ISO(), "calendar": calendar_name, "day_count": day_count_name,
                "cash_flows": [{"date": flow.date().ISO(), "amount": flow.amount()} for flow in bond.cashflows() if flow.date() >= settlement],
                "assumptions": "Fixed-rate noncallable bond; unadjusted accrual dates, following payment dates, backward schedule, redemption at 100."}
    finally:
        ql.Settings.instance().evaluationDate = previous_date


OPERATIONS = {"factor": factor_analysis, "forecast": forecast_analysis, "optimize": optimize_analysis, "quantlib-bond": quantlib_bond_analysis}
