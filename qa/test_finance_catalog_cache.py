"""Metadata reuse must not reuse credentials, provider results, or query workers."""
import copy
import concurrent.futures
import importlib
import json
import sys
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/finance"))
bridge = importlib.import_module("boxoro_finance.openbb_bridge")
service = importlib.import_module("boxoro_finance.service")
from qa.test_finance_openbb import SCHEMA  # noqa: E402


class CatalogReuseTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(hasattr(service, "CatalogExecutor"), "Service still rebuilds OpenBB metadata for every call")
        self.settings = service.Settings(token="cache-test-" + "x" * 40, namespaces=("cboe", "quantitative"),
                                         provider_credentials=(("FRED_API_KEY", "private-provider-key"),))
        self.catalog = bridge.Catalog(SCHEMA, namespaces=self.settings.namespaces)
        self.snapshot = self.catalog.snapshot("2.0.1", "a" * 64)
        self.executor = service.CatalogExecutor(self.settings)

    def test_status_search_and_describe_share_metadata_without_importing_sdk_in_server(self):
        with mock.patch.object(service, "run_isolated", return_value=self.snapshot) as child:
            status = self.executor("openbb_status", {}, self.settings, timeout=5)
            catalog = self.executor("openbb_catalog", {"query": "historical"}, self.settings, timeout=5)
            description = self.executor("openbb_describe", {"command": "quantitative.summary"}, self.settings, timeout=5)
        self.assertEqual(status["command_count"], 2)
        self.assertEqual(status["provider_setup"]["fred"], True)
        self.assertEqual(catalog["total"], 1)
        self.assertEqual(description["body_schema"]["required"], ["data"])
        self.assertEqual(child.call_count, 1)
        self.assertEqual(child.call_args.args[0], "_catalog_snapshot")
        self.assertNotIn("private-provider-key", json.dumps(self.snapshot))
        self.assertNotIn("openbb_core.api.rest_api", sys.modules)

    def test_concurrent_cold_discovery_starts_one_bootstrap(self):
        entered, release = threading.Event(), threading.Event()
        def bootstrap(*args, **kwargs):
            entered.set()
            self.assertTrue(release.wait(3))
            return self.snapshot
        with mock.patch.object(service, "run_isolated", side_effect=bootstrap) as child:
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                first = pool.submit(self.executor, "openbb_status", {}, self.settings, timeout=5)
                self.assertTrue(entered.wait(2))
                second = pool.submit(self.executor, "openbb_catalog", {}, self.settings, timeout=5)
                release.set()
                self.assertEqual(first.result()["command_count"], 2)
                self.assertEqual(second.result()["total"], 2)
        self.assertEqual(child.call_count, 1)

    def test_failed_bootstrap_is_not_cached_and_later_request_retries(self):
        failed = service.failure("timeout", "fixture timeout")
        with mock.patch.object(service, "run_isolated", side_effect=[failed, self.snapshot]) as child:
            self.assertEqual(self.executor("openbb_status", {}, self.settings, timeout=5), failed)
            self.assertEqual(self.executor("openbb_status", {}, self.settings, timeout=5)["gate"], "pass")
        self.assertEqual(child.call_count, 2)

    def test_waiting_for_cold_catalog_respects_request_deadline(self):
        entered, release = threading.Event(), threading.Event()
        def bootstrap(*args, **kwargs):
            entered.set()
            self.assertTrue(release.wait(3))
            return self.snapshot
        with mock.patch.object(service, "run_isolated", side_effect=bootstrap):
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                first = pool.submit(self.executor, "openbb_status", {}, self.settings, timeout=5)
                self.assertTrue(entered.wait(2))
                started = time.monotonic()
                result = self.executor("openbb_status", {}, self.settings, timeout=0.03)
                self.assertEqual(result["code"], "timeout")
                self.assertLess(time.monotonic() - started, 0.5)
                release.set()
                self.assertEqual(first.result()["gate"], "pass")

    def test_queries_start_fresh_workers_and_carry_only_the_selected_contract(self):
        result = {"gate": "pass", "status": "complete", "data": {"results": [1]}}
        request = {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": 1}]}}
        with mock.patch.object(service, "run_isolated", side_effect=[self.snapshot, result, result]) as child:
            first = self.executor("openbb_query", request, self.settings, timeout=5)
            second = self.executor("openbb_query", request, self.settings, timeout=5)
        self.assertEqual(first, second)
        self.assertEqual(child.call_count, 3)
        for call in child.call_args_list[1:]:
            self.assertEqual(call.args[0], "openbb_query")
            contract = call.kwargs["contract"]
            self.assertEqual(contract["operation"]["command"], "quantitative.summary")
            self.assertNotIn("response_schema", contract["operation"])
            self.assertEqual(contract["schema_sha256"], self.catalog.schema_hash)
            self.assertNotIn("private-provider-key", json.dumps(contract))

    def test_invalid_fields_are_rejected_before_expensive_bootstrap(self):
        with mock.patch.object(service, "run_isolated") as child:
            for name, args in (("openbb_status", {"surprise": 1}),
                               ("openbb_query", {"command": "quantitative.summary", "contract": {}}),
                               ("openbb_catalog", {"limit": True})):
                with self.subTest(name=name), self.assertRaises(bridge.FinanceError):
                    self.executor(name, args, self.settings, timeout=5)
            child.assert_not_called()

    def test_network_permission_and_credentials_still_precede_query_workers(self):
        with mock.patch.object(service, "run_isolated", return_value=self.snapshot) as child:
            for request in ({"command": "cboe.equity.historical", "parameters": {"symbol": "SPY"}},
                            {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY", "api_key": "secret"}, "allow_network": True}):
                with self.subTest(request=request), self.assertRaises(bridge.FinanceError):
                    self.executor("openbb_query", request, self.settings, timeout=5)
        self.assertEqual(child.call_count, 1)

    def test_stale_query_fingerprint_invalidates_catalog_for_next_request(self):
        request = {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": 1}]}}
        failed = service.failure("catalog_stale", "Installed packages changed")
        with mock.patch.object(service, "run_isolated", side_effect=[self.snapshot, failed, self.snapshot]) as child:
            self.assertEqual(self.executor("openbb_query", request, self.settings, timeout=5), failed)
            self.assertEqual(self.executor("openbb_status", {}, self.settings, timeout=5)["gate"], "pass")
        self.assertEqual(child.call_count, 3)

    def test_stale_invalidation_does_not_wait_past_the_query_deadline(self):
        request = {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": 1}]}}
        entered = threading.Event()
        def child(name, *args, **kwargs):
            if name == "_catalog_snapshot":
                return self.snapshot
            self.executor.lock.acquire()  # Another request has begun refreshing the catalog.
            entered.set()
            return service.failure("catalog_stale", "Installed packages changed")
        with mock.patch.object(service, "run_isolated", side_effect=child):
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                result = pool.submit(self.executor, "openbb_query", request, self.settings, timeout=0.03)
                self.assertTrue(entered.wait(2))
                try:
                    _, pending = concurrent.futures.wait([result], timeout=0.5)
                    self.assertFalse(pending, "Invalidation waited beyond the request deadline")
                    self.assertEqual(result.result()["code"], "catalog_stale")
                finally:
                    self.executor.lock.release()

    def test_bootstrap_environment_excludes_secrets_and_has_finite_output_bound(self):
        with mock.patch.object(service, "bounded_process", return_value=self.snapshot) as child:
            service.run_isolated("_catalog_snapshot", {}, self.settings, timeout=5)
        environment = child.call_args.args[2]
        self.assertTrue(all(name not in environment for name in bridge.CREDENTIAL_ENV))
        self.assertNotIn(self.settings.token, environment.values())
        self.assertLessEqual(child.call_args.kwargs["max_output"], 16 * 1024 * 1024)

    def test_snapshot_cannot_change_allowed_namespace_or_invalid_fingerprint(self):
        changed = copy.deepcopy(self.snapshot)
        changed["namespaces"].append("custom")
        malformed = copy.deepcopy(self.snapshot)
        malformed["schema_sha256"] = "unverified"
        forbidden = copy.deepcopy(self.snapshot)
        forbidden["commands"][0]["path"] = "/api/v1/user/credentials"
        for snapshot in (changed, malformed, forbidden):
            with self.subTest(snapshot=snapshot), mock.patch.object(service, "run_isolated", return_value=snapshot), self.assertRaises(bridge.FinanceError):
                self.executor("openbb_status", {}, self.settings, timeout=5)

    def test_worker_refuses_stale_installation_before_loading_sdk(self):
        request = {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": 1}]}}
        contract = self.catalog.query_contract(request["command"], "2.0.1", "a" * 64)
        with mock.patch.object(bridge, "installation_fingerprint", return_value="b" * 64), mock.patch.object(bridge, "load_openbb") as sdk:
            with self.assertRaises(bridge.FinanceError) as raised:
                bridge.execute_tool("openbb_query", request, self.settings.namespaces, contract=contract)
        self.assertEqual(raised.exception.code, "catalog_stale")
        sdk.assert_not_called()

    def test_worker_validates_exact_schema_before_loading_sdk(self):
        request = {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY", "limit": True}, "allow_network": True}
        contract = self.catalog.query_contract(request["command"], "2.0.1", "a" * 64)
        with mock.patch.object(bridge, "installation_fingerprint", return_value="a" * 64), \
                mock.patch.object(bridge, "load_openbb", side_effect=AssertionError("Invalid input loaded the SDK")):
            with self.assertRaises(bridge.FinanceError) as raised:
                bridge.execute_tool("openbb_query", request, self.settings.namespaces, contract=contract)
        self.assertEqual(raised.exception.code, "invalid_parameters")

    def test_bootstrap_cannot_make_network_connections_during_sdk_load(self):
        import os
        import socket
        def connect_during_import():
            with socket.socket() as connection:
                connection.connect(("8.8.8.8", 443))
        with mock.patch.dict(os.environ, {key: "" for key in bridge.CREDENTIAL_ENV}), \
                mock.patch.object(socket.socket, "connect", side_effect=AssertionError("Bootstrap reached the network")), \
                mock.patch.object(bridge, "load_openbb", side_effect=connect_during_import):
            with self.assertRaises(bridge.FinanceError) as raised:
                bridge.execute_tool("_catalog_snapshot", {}, self.settings.namespaces)
        self.assertEqual(raised.exception.code, "network_required")


if __name__ == "__main__":
    unittest.main()
