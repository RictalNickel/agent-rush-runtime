"""Independent numerical fixtures and CLI boundary tests for Finance Lab."""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "deploy/finance/cli.py"


class FinanceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.workspace = Path(self.temp.name)

    def call(self, operation, data=None, parameters=None, okay=True):
        request = {"operation": operation, "parameters": parameters or {}}
        if data is not None:
            request["data"] = data
        source = self.workspace / "request.json"
        source.write_text(json.dumps(request))
        process = subprocess.run([sys.executable, "-I", "-S", str(CLI), "--request-file", str(source)],
                                 cwd=self.workspace, capture_output=True, text=True, timeout=20, check=False)
        if okay:
            self.assertEqual(process.returncode, 0, process.stderr + process.stdout)
        else:
            self.assertNotEqual(process.returncode, 0, process.stdout)
        self.assertTrue(process.stdout.strip(), process.stderr)
        return json.loads(process.stdout)

    def test_risk_compounds_returns_and_includes_initial_wealth_drawdown(self):
        result = self.call("risk", [-0.2, 0.25, -0.1, 0.1], {"input_kind": "returns", "frequency": 4})
        self.assertEqual(result["gate"], "pass")
        metrics = result["results"]
        self.assertAlmostEqual(metrics["total_return"], -0.01, places=12)
        self.assertAlmostEqual(metrics["max_drawdown"], -0.2, places=12)
        self.assertAlmostEqual(metrics["cagr"], -0.01, places=12)

    def test_returns_do_not_sort_undated_values_or_drop_loss(self):
        result = self.call("returns", [100, 110, 99])
        self.assertAlmostEqual(result["results"][0]["return"], 0.1)
        self.assertAlmostEqual(result["results"][1]["return"], -0.1)

    def test_portfolio_aligns_columns_and_compounds_weighted_returns(self):
        data = [{"A": 0.1, "B": 0.0}, {"A": -0.1, "B": 0.2}]
        result = self.call("portfolio", data, {"columns": ["A", "B"], "weights": [0.5, 0.5], "input_kind": "returns"})
        self.assertAlmostEqual(result["results"]["risk"]["total_return"], 0.1025)
        self.assertEqual(result["results"]["rebalancing"], "constant weights each observation")

    def test_dcf_discounts_terminal_value_at_final_projection_date(self):
        result = self.call("valuation", parameters={"cash_flows": [100, 110], "discount_rate": 0.1,
                                                     "terminal_growth": 0.02, "net_debt": 50, "shares": 10})
        self.assertAlmostEqual(result["results"]["enterprise_value"], 1340.9090909090908)
        self.assertAlmostEqual(result["results"]["equity_value_per_share"], 129.0909090909091)

    def test_dcf_refuses_nonconvergent_terminal_growth(self):
        result = self.call("valuation", parameters={"cash_flows": [100], "discount_rate": 0.02,
                                                     "terminal_growth": 0.02}, okay=False)
        self.assertEqual(result["gate"], "fail")

    def test_european_option_price_and_put_call_parity(self):
        params = {"spot": 100, "strike": 100, "rate": 0.05, "volatility": 0.2, "years": 1}
        call = self.call("options", parameters=params)["results"]
        put = self.call("options", parameters={**params, "kind": "put"})["results"]
        self.assertAlmostEqual(call["price"], 10.450583572185565)
        self.assertAlmostEqual(call["price"] - put["price"], 4.877057549928594)
        self.assertAlmostEqual(call["delta"], 0.6368306511756191)

    def test_implied_volatility_recovers_known_option_price(self):
        result = self.call("options", parameters={"spot": 100, "strike": 100, "rate": 0.05,
                                                   "years": 1, "market_price": 10.450583572185565})
        self.assertAlmostEqual(result["results"]["implied_volatility"], 0.2, places=7)

    def test_par_bond_price_and_duration(self):
        result = self.call("bonds", parameters={"face": 1000, "coupon_rate": 0.05, "yield_rate": 0.05,
                                                 "years": 2, "payments_per_year": 1})["results"]
        self.assertAlmostEqual(result["price"], 1000)
        self.assertAlmostEqual(result["macaulay_duration_years"], 1.9523809523809523)

    def test_cashflow_npv_and_irr(self):
        result = self.call("cashflow", parameters={"cash_flows": [-100, 60, 60], "discount_rate": 0.1})["results"]
        self.assertAlmostEqual(result["npv"], 4.132231404958667)
        self.assertAlmostEqual(result["irr"], 0.1306623862918075, places=8)

    def test_long_amortizing_cashflows_do_not_overflow_irr_bracket(self):
        result = self.call("cashflow", parameters={"cash_flows": [-100] + [0.5995505251527569] * 360,
                                                    "discount_rate": 0.005})["results"]
        self.assertAlmostEqual(result["npv"], 0, places=8)
        self.assertAlmostEqual(result["irr"], 0.005, places=10)

    def test_cashflow_irr_is_invariant_to_small_currency_scale(self):
        result = self.call("cashflow", parameters={"cash_flows": [-1e-200, 1.1e-200]})["results"]
        self.assertEqual(result["cash_flow_sign_changes"], 1)
        self.assertAlmostEqual(result["irr"], 0.1, places=10)

    def test_backtest_lags_signals_and_charges_turnover(self):
        result = self.call("backtest", [100, 100, 100, 200, 100], {"fast": 1, "slow": 3, "cost_bps": 0})
        self.assertAlmostEqual(result["results"]["strategy"]["total_return"], -0.5)
        self.assertAlmostEqual(result["results"]["benchmark"]["total_return"], 0.0)

    def test_backtest_does_not_silently_treat_return_input_as_prices(self):
        result = self.call("backtest", [0.01, 0.02, 0.03, 0.04, 0.05],
                           {"input_kind": "returns", "fast": 1, "slow": 3}, okay=False)
        self.assertEqual(result["code"], "invalid_parameters")

    def test_correlation_reports_perfect_positive_and_negative_relations(self):
        result = self.call("correlation", [{"A": 1, "B": 2, "C": 3}, {"A": 2, "B": 4, "C": 2}, {"A": 3, "B": 6, "C": 1}])
        self.assertAlmostEqual(result["results"]["correlation"]["A"]["B"], 1.0)
        self.assertAlmostEqual(result["results"]["correlation"]["A"]["C"], -1.0)

    def test_monte_carlo_seed_reproduces_and_zero_volatility_is_deterministic(self):
        params = {"spot": 100, "drift": 0, "volatility": 0, "years": 1, "simulations": 200, "seed": 12}
        first = self.call("monte-carlo", parameters=params)["results"]
        second = self.call("monte-carlo", parameters=params)["results"]
        self.assertEqual(first, second)
        self.assertEqual(first["terminal_percentiles"]["50"], 100)

    def test_missing_advanced_dependencies_are_incomplete_not_pass(self):
        result = self.call("factor", [{"y": 1, "x": 1}, {"y": 2, "x": 2}, {"y": 4, "x": 3}, {"y": 4, "x": 4}],
                           {"target": "y", "factors": ["x"]}, okay=False)
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["code"], "missing_dependency")

    def test_malformed_or_unsafe_numeric_inputs_fail_closed(self):
        cases = [("risk", [1, 2, 0], {}), ("risk", [True, 1, 2], {}),
                 ("risk", [0.1, -1.1], {"input_kind": "returns"}),
                 ("portfolio", [{"A": 1, "B": 2}, {"A": 2, "B": 3}], {"weights": [1, 1]}),
                 ("options", None, {"spot": 0, "strike": 100, "volatility": 0.2, "years": 1}),
                 ("risk", [1, 10 ** 500], {}),
                 ("correlation", [{"A": 1}, {"A": 2}], {"columns": [["A"]]}),
                 ("risk", [1, 2, 3], {"frequency": 0}), ("risk", [1, 2, 3], {"surprise": 1})]
        for operation, data, params in cases:
            with self.subTest(operation=operation, params=params):
                self.assertEqual(self.call(operation, data, params, okay=False)["gate"], "fail")

    def test_dates_are_sorted_but_duplicate_timestamps_rejected(self):
        result = self.call("returns", [{"date": "2024-01-03", "close": 121}, {"date": "2024-01-01", "close": 100},
                                       {"date": "2024-01-02", "close": 110}])
        self.assertAlmostEqual(result["results"][0]["return"], 0.1)
        self.assertEqual(result["results"][0]["date"], "2024-01-02")
        result = self.call("risk", [{"date": "2024-01-01", "close": 100}, {"date": "2024-01-01", "close": 101}], okay=False)
        self.assertEqual(result["code"], "invalid_data")

    def test_mixed_naive_and_timezone_aware_dates_are_rejected(self):
        result = self.call("risk", [{"date": "2024-01-01", "close": 100},
                                    {"date": "2024-01-02T00:00:00Z", "close": 101}], okay=False)
        self.assertEqual(result["code"], "invalid_data")

    def test_workspace_file_reader_rejects_symlinks_traversal_and_empty_header(self):
        source = self.workspace / "prices.csv"
        source.write_text(",close\nfirst,100\nsecond,110\n")
        link = self.workspace / "link.csv"
        link.symlink_to(source)
        for path in (str(source), str(link), "../outside.csv"):
            with self.subTest(path=path):
                process = subprocess.run([sys.executable, "-I", "-S", str(CLI), "risk", "--input", path],
                                         cwd=self.workspace, capture_output=True, text=True, timeout=20, check=False)
                self.assertEqual(process.returncode, 2, process.stdout + process.stderr)
                self.assertEqual(json.loads(process.stdout)["gate"], "fail")

    def test_csv_cli_does_not_import_workspace_python(self):
        (self.workspace / "json.py").write_text("raise RuntimeError('untrusted import')")
        source = self.workspace / "prices.csv"
        source.write_text("date,close\n2024-01-01,100\n2024-01-02,110\n2024-01-03,99\n")
        process = subprocess.run([sys.executable, "-I", "-S", str(CLI), "risk", "--input", str(source), "--frequency", "252"],
                                 cwd=self.workspace, capture_output=True, text=True, timeout=20, check=False)
        self.assertEqual(process.returncode, 0, process.stdout + process.stderr)
        result = json.loads(process.stdout)
        self.assertAlmostEqual(result["results"]["total_return"], -0.01)
        self.assertEqual(result["provenance"]["input_sha256"].__len__(), 64)


if __name__ == "__main__":
    unittest.main()
