"""Opt-in installed OpenBB integration, with no provider network or credentials."""
import importlib.util
import http.client
import json
import os
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/finance"))
from boxoro_finance.service import Settings, make_server, prepare_state  # noqa: E402


@unittest.skipUnless(os.environ.get("BOXORO_TEST_OPENBB") == "1" and importlib.util.find_spec("openbb_core"),
                     "Set BOXORO_TEST_OPENBB=1 in the locked OpenBB environment")
class InstalledOpenBBTests(unittest.TestCase):
    def test_actual_offline_summary_preserves_obbject_metadata(self):
        timings = {}
        with tempfile.TemporaryDirectory() as directory:
            settings = Settings(token="offline-test-" + "x" * 40, state_directory=directory)
            prepare_state(settings)
            server = make_server(settings, port=0)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            def call(label, name, arguments):
                started = time.monotonic()
                connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=95)
                try:
                    connection.request("POST", "/mcp", json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                        "params": {"name": name, "arguments": arguments}}),
                        {"Authorization": "Bearer " + settings.token, "Content-Type": "application/json"})
                    response = connection.getresponse()
                    body = json.loads(response.read())
                    self.assertEqual(response.status, 200, body)
                    result = body["result"]["structuredContent"]
                    self.assertEqual(result.get("gate"), "pass", result)
                    return result
                finally:
                    connection.close()
                    timings[label] = time.monotonic() - started
            try:
                status = call("cold_status", "openbb_status", {})
                repeated = call("warm_status", "openbb_status", {})
                catalog = call("warm_catalog", "openbb_catalog", {"namespace": "quantitative", "query": "summary"})
                described = call("warm_describe", "openbb_describe", {"command": "quantitative.summary"})
                self.assertEqual(status, repeated)
                self.assertGreater(status["command_count"], 0)
                self.assertIn("quantitative.summary", [item["command"] for item in catalog["commands"]])
                self.assertEqual(described["method"], "POST")
                self.assertIn("data", described["body_schema"]["properties"])
                result = call("fresh_offline_query", "openbb_query", {
                    "command": "quantitative.summary", "parameters": {}, "allow_network": False,
                    "body": {"target": "close", "data": [{"close": n} for n in (1, 2, 3, 4)]},
                })
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=2)
        if os.environ.get("BOXORO_BENCH_OPENBB") == "1":
            print(json.dumps({"authenticated_http_seconds": timings}), flush=True)
        self.assertEqual(result.get("gate"), "pass", result)
        self.assertEqual(result["data"]["results"]["count"], 4)
        self.assertAlmostEqual(result["data"]["results"]["mean"], 2.5)
        self.assertAlmostEqual(result["data"]["results"]["var"], 5 / 3)
        self.assertEqual(result["data"]["extra"]["metadata"]["route"], "/quantitative/summary")
        self.assertEqual(result["openbb_core_version"], "2.0.1")
        self.assertEqual(len(result["schema_sha256"]), 64)
        self.assertEqual(result["schema_sha256"], status["schema_sha256"])
        self.assertEqual(result["installation_sha256"], status["installation_sha256"])


if __name__ == "__main__":
    unittest.main()
