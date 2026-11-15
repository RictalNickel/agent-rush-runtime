"""Opt-in installed OpenBB integration, with no provider network or credentials."""
import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/finance"))
from boxoro_finance.service import Settings, prepare_state, run_isolated  # noqa: E402


@unittest.skipUnless(os.environ.get("BOXORO_TEST_OPENBB") == "1" and importlib.util.find_spec("openbb_core"),
                     "Set BOXORO_TEST_OPENBB=1 in the locked OpenBB environment")
class InstalledOpenBBTests(unittest.TestCase):
    def test_actual_offline_summary_preserves_obbject_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            settings = Settings(token="offline-test-" + "x" * 40, state_directory=directory)
            prepare_state(settings)
            result = run_isolated("openbb_query", {
                "command": "quantitative.summary", "parameters": {}, "allow_network": False,
                "body": {"target": "close", "data": [{"close": n} for n in (1, 2, 3, 4)]},
            }, settings, timeout=90)
        self.assertEqual(result.get("gate"), "pass", result)
        self.assertEqual(result["data"]["results"]["count"], 4)
        self.assertAlmostEqual(result["data"]["results"]["mean"], 2.5)
        self.assertAlmostEqual(result["data"]["results"]["var"], 5 / 3)
        self.assertEqual(result["data"]["extra"]["metadata"]["route"], "/quantitative/summary")
        self.assertEqual(result["openbb_core_version"], "2.0.1")
        self.assertEqual(len(result["schema_sha256"]), 64)


if __name__ == "__main__":
    unittest.main()
