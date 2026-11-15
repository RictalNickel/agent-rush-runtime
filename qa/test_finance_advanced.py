"""Optional real numerical-backend tests, runnable with the advanced image venv."""
import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/finance"))
from boxoro_finance.cli import run_request  # noqa: E402 - source-checkout test imports
from boxoro_finance.common import FinanceError  # noqa: E402


class AdvancedTests(unittest.TestCase):
    def test_nonconverged_forecast_is_incomplete(self):
        backend = mock.Mock()
        backend.ARIMA.return_value.fit.return_value.mle_retvals = {"converged": False}
        with mock.patch("boxoro_finance.advanced.dependencies", return_value=[backend]), self.assertRaises(FinanceError) as caught:
            run_request({"operation": "forecast", "data": list(range(40))}, workspace=ROOT)
        self.assertEqual(caught.exception.code, "solver_failed")
        self.assertTrue(caught.exception.incomplete)

    @unittest.skipUnless(importlib.util.find_spec("statsmodels"), "statsmodels is optional")
    def test_factor_coefficients_and_intervals_use_real_statsmodels(self):
        result = run_request({"operation": "factor", "data": [
            {"x": 0, "y": 1.1}, {"x": 1, "y": 2.9}, {"x": 2, "y": 4.9}, {"x": 3, "y": 7.1}],
            "parameters": {"target": "y", "factors": ["x"]}}, workspace=ROOT)
        coefficients = result["results"]["coefficients"]
        self.assertAlmostEqual(coefficients[0]["coefficient"], 1)
        self.assertAlmostEqual(coefficients[1]["coefficient"], 2)
        low, high = coefficients[1]["confidence_interval_95"]
        self.assertLess(low, 2)
        self.assertGreater(high, 2)

    @unittest.skipUnless(importlib.util.find_spec("statsmodels"), "statsmodels is optional")
    def test_random_walk_forecast_is_flat_at_last_observation(self):
        result = run_request({"operation": "forecast", "data": list(range(40)),
                              "parameters": {"p": 0, "d": 1, "q": 0, "horizon": 3}}, workspace=ROOT)
        forecasts = result["results"]["forecast"]
        self.assertEqual(len(forecasts), 3)
        for record in forecasts:
            self.assertAlmostEqual(record["mean"], 39)
            self.assertLess(record["lower_95"], record["mean"])
            self.assertGreater(record["upper_95"], record["mean"])

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "SciPy is optional")
    def test_minimum_variance_weights_and_constraints(self):
        data = [{"A": a, "B": b} for _ in range(5) for a, b in [(0.1, 0.01), (-0.1, 0.01), (0.1, -0.01), (-0.1, -0.01)]]
        result = run_request({"operation": "optimize", "data": data,
                              "parameters": {"input_kind": "returns", "objective": "min-variance"}}, workspace=ROOT)
        self.assertAlmostEqual(result["results"]["weights"]["A"], 0.00990099, places=5)
        bounded = run_request({"operation": "optimize", "data": data,
                               "parameters": {"input_kind": "returns", "max_weight": 0.7}}, workspace=ROOT)
        self.assertAlmostEqual(bounded["results"]["weights"]["B"], 0.7)
        self.assertAlmostEqual(sum(bounded["results"]["weights"].values()), 1)

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "SciPy is optional")
    def test_max_sharpe_matches_closed_form_effective_rate_tangency_weights(self):
        data = [{"A": 0.001 + a, "B": 0.0006 + b} for _ in range(5)
                for a, b in [(0.02, 0.01), (-0.02, 0.01), (0.02, -0.01), (-0.02, -0.01)]]
        result = run_request({"operation": "optimize", "data": data, "parameters": {
            "input_kind": "returns", "frequency": 252, "risk_free_rate": 0.1, "objective": "max-sharpe"}}, workspace=ROOT)
        # For uncorrelated assets, tangency weights are proportional to mean
        # excess return divided by variance. The common sample correction cancels.
        per_period_rate = 1.1 ** (1 / 252) - 1
        a = (0.001 - per_period_rate) / 0.02 ** 2
        b = (0.0006 - per_period_rate) / 0.01 ** 2
        self.assertAlmostEqual(result["results"]["weights"]["A"], a / (a + b), places=5)

    @unittest.skipUnless(importlib.util.find_spec("QuantLib"), "QuantLib is optional")
    def test_quantlib_bond_reconciles_par_with_calendar_cashflows(self):
        try:
            result = run_request({"operation": "quantlib-bond", "parameters": {
                "issue_date": "2024-01-01", "maturity_date": "2026-01-01", "valuation_date": "2024-01-01",
                "coupon_rate": 0.05, "yield_rate": 0.05, "payments_per_year": 1, "settlement_days": 0,
                "day_count": "30/360", "calendar": "null"}}, workspace=ROOT)
        except FinanceError as exc:
            self.fail(f"QuantLib adapter did not execute: {type(exc).__name__}: {exc}")
        self.assertAlmostEqual(result["results"]["clean_price_per_100"], 100, places=9)
        self.assertEqual(result["results"]["settlement_date"], "2024-01-01")
        self.assertGreater(len(result["results"]["cash_flows"]), 1)


if __name__ == "__main__":
    unittest.main()
