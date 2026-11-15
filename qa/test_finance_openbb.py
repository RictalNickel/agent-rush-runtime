"""OpenBB boundary tests using actual HTTP and fixed schema fixtures."""
import asyncio
import copy
import http.client
import importlib
import json
import socket
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/finance"))
try:
    bridge = importlib.import_module("boxoro_finance.openbb_bridge")
    service = importlib.import_module("boxoro_finance.service")
except ModuleNotFoundError:
    bridge = service = None


SCHEMA = {
    "openapi": "3.1.0", "info": {"title": "OpenBB", "version": "1"},
    "components": {"schemas": {"Rows": {"type": "object", "required": ["data"], "properties": {
        "data": {"type": "array", "items": {"type": "object", "properties": {"close": {"type": "number"}}}}}}}},
    "paths": {
        "/api/v1/cboe/equity/historical": {"get": {"summary": "Historical prices", "parameters": [
            {"name": "symbol", "in": "query", "required": True, "schema": {"type": "string"}},
            {"name": "limit", "in": "query", "schema": {"type": "integer", "minimum": 1, "maximum": 1000}},
            {"name": "provider", "in": "query", "schema": {"enum": ["cboe"], "type": "string"}}]}},
        "/api/v1/quantitative/summary": {"post": {"summary": "Descriptive statistics", "parameters": [
            {"name": "target", "in": "query", "required": True, "schema": {"type": "string"}}],
            "requestBody": {"required": True, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Rows"}}}}}},
        "/api/v1/cboe/order": {"post": {"summary": "Mutation"}},
        "/api/v1/user/me": {"get": {"summary": "Credentials"}},
        "/api/v1/system": {"get": {"summary": "Settings"}},
        "/api/v1/coverage/providers": {"get": {}},
        "/api/v1/custom/read": {"get": {}},
        "/api/v1/cboe/delete": {"delete": {}},
    },
}


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(bridge, "OpenBB schema bridge has not been implemented")
        self.catalog = bridge.Catalog(SCHEMA, namespaces=("cboe", "quantitative"))

    def test_only_approved_read_operations_are_discovered(self):
        self.assertEqual(set(self.catalog.commands), {"cboe.equity.historical", "quantitative.summary"})

    def test_describe_resolves_body_refs_and_keeps_required_parameters(self):
        item = self.catalog.describe("quantitative.summary")
        self.assertEqual(item["method"], "POST")
        self.assertEqual(item["parameters_schema"]["required"], ["target"])
        self.assertEqual(item["body_schema"]["required"], ["data"])
        self.assertEqual(item["body_schema"]["properties"]["data"]["type"], "array")

    def test_schema_fingerprint_is_independent_of_object_key_order(self):
        reordered = dict(reversed(list(SCHEMA.items())))
        self.assertEqual(self.catalog.schema_hash, bridge.Catalog(reordered, namespaces=("cboe", "quantitative")).schema_hash)

    def test_network_scope_denies_private_destinations_and_restores_sockets(self):
        original = socket.socket.connect
        with bridge.network_scope(True), socket.socket() as client, self.assertRaises(bridge.FinanceError):
            client.connect(("127.0.0.1", 443))
        self.assertIs(socket.socket.connect, original)

    def test_unknown_commands_secrets_and_body_on_get_are_refused(self):
        invalid = [
            {"command": "user.credentials.show", "parameters": {}},
            {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY", "api_key": "secret"}},
            {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY"}, "body": {}},
            {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY", "limit": True}},
            {"command": "cboe.equity.historical", "parameters": {"symbol": "SPY", "provider": "yfinance"}},
        ]
        for request in invalid:
            with self.subTest(request=request), self.assertRaises(bridge.FinanceError):
                self.catalog.validate_query(request)

    def test_missing_required_and_nonfinite_values_are_refused(self):
        for request in ({"command": "cboe.equity.historical", "parameters": {}},
                        {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": float("nan")}]}}):
            with self.assertRaises(bridge.FinanceError):
                self.catalog.validate_query(request)

    def test_validated_requests_preserve_query_and_json_body(self):
        request = {"command": "quantitative.summary", "parameters": {"target": "close"}, "body": {"data": [{"close": 1}, {"close": 2}]}}
        operation, parameters, body = self.catalog.validate_query(request)
        self.assertEqual(operation["path"], "/api/v1/quantitative/summary")
        self.assertEqual(parameters, {"target": "close"})
        self.assertEqual(body, {"data": [{"close": 1}, {"close": 2}]})

    def test_remote_schema_refs_cannot_trigger_network_reads(self):
        schema = copy.deepcopy(SCHEMA)
        schema["paths"]["/api/v1/cboe/equity/historical"]["get"]["parameters"][0]["schema"] = {"$ref": "http://169.254.169.254/credentials"}
        with self.assertRaises(bridge.FinanceError):
            bridge.Catalog(schema, namespaces=("cboe",))

    def test_url_guard_rejects_scheme_case_and_whitespace_bypasses(self):
        for value in ("HTTPS://127.0.0.1/private", " https://169.254.169.254/credentials", "FILE:///etc/passwd", "//127.0.0.1/private"):
            with self.subTest(value=value), self.assertRaises(bridge.FinanceError):
                bridge.reject_sensitive_arguments({"url": value})

    def test_secret_redaction_covers_nested_metadata_and_url_query(self):
        result = bridge.redact({"provider": "fred", "extra": {"api_key": "SECRET", "url": "https://fred.example/data?api_key=SECRET&series=GDP"},
                                "warnings": ["Provider echoed SECRET"]}, ["SECRET"])
        encoded = json.dumps(result)
        self.assertNotIn("SECRET", encoded)
        self.assertIn("GDP", encoded)

    def test_result_limit_is_explicit_and_preserves_provenance(self):
        result = bridge.normalize_result({"results": [{"value": i} for i in range(5)], "provider": "fred", "warnings": [], "extra": {"metadata": {"route": "fred.series"}}},
                                          command="fred.series", max_rows=2, secrets=())
        self.assertEqual(len(result["data"]["results"]), 2)
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["total_rows"], 5)
        self.assertEqual(result["data"]["provider"], "fred")

    def test_service_token_is_required_and_not_repr_exposed(self):
        with self.assertRaises(ValueError):
            service.Settings.from_environ({})
        settings = service.Settings.from_environ({"FINANCE_OPENBB_TOKEN": "x" * 40})
        self.assertNotIn("x" * 40, repr(settings))

    def test_owned_private_token_file_is_supported_without_exposing_token(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "token"
            path.write_text("private-token-" + "y" * 40 + "\n")
            path.chmod(0o600)
            settings = service.Settings.from_environ({"FINANCE_OPENBB_TOKEN_FILE": str(path), "FINANCE_OPENBB_BIND": "0.0.0.0"})
            self.assertEqual(settings.token, "private-token-" + "y" * 40)
            self.assertEqual(settings.bind_address, "0.0.0.0")
            self.assertNotIn(settings.token, repr(settings))

    def test_public_or_symlink_token_files_and_nonliteral_bind_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "token"
            path.write_text("x" * 40)
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                service.Settings.from_environ({"FINANCE_OPENBB_TOKEN_FILE": str(path)})
            path.chmod(0o600)
            link = Path(temp) / "link"
            link.symlink_to(path)
            with self.assertRaises(ValueError):
                service.Settings.from_environ({"FINANCE_OPENBB_TOKEN_FILE": str(link)})
        with self.assertRaises(ValueError):
            service.Settings.from_environ({"FINANCE_OPENBB_TOKEN": "x" * 40, "FINANCE_OPENBB_BIND": "service.example"})

    def test_service_state_keeps_cache_private_and_refuses_cache_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            cache = directory / "cache"
            cache.mkdir()
            cache.chmod(0o777)
            settings = service.Settings(token="x" * 40, state_directory=temp)
            service.prepare_state(settings)
            self.assertEqual(cache.stat().st_mode & 0o777, 0o700)
            self.assertEqual((directory / "tmp").stat().st_mode & 0o777, 0o700)
            cache.rmdir()
            cache.symlink_to(directory / "tmp", target_is_directory=True)
            with self.assertRaises(ValueError):
                service.prepare_state(settings)

    def test_process_deadline_and_output_limit_do_not_return_success(self):
        started = time.monotonic()
        result = service.bounded_process([sys.executable, "-I", "-c", "import time; time.sleep(10)"], b"{}", {}, timeout=0.15, max_output=128)
        self.assertEqual(result["code"], "timeout")
        self.assertLess(time.monotonic() - started, 3)
        result = service.bounded_process([sys.executable, "-I", "-c", "print('x' * 10000)"], b"{}", {}, timeout=3, max_output=128)
        self.assertEqual(result["code"], "response_limit")


@unittest.skipUnless(importlib.util.find_spec("fastapi") and importlib.util.find_spec("httpx"), "OpenBB HTTP dependencies are optional")
class EnvelopeTests(unittest.TestCase):
    def test_dispatch_preserves_mutated_default_provenance_on_nested_routes(self):
        from fastapi import APIRouter, FastAPI
        from pydantic import BaseModel, Field

        class Envelope(BaseModel):
            results: list[dict]
            provider: str | None = None
            warnings: list[str] = Field(default_factory=list)
            extra: dict = Field(default_factory=dict)

        router = APIRouter()

        @router.get("/summary", response_model=Envelope, response_model_exclude_unset=True)
        async def summary():
            result = Envelope(results=[{"mean": 2.5}])
            result.extra["metadata"] = {"route": "/quantitative/summary", "timestamp": "2026-11-15T00:00:00Z"}
            result.extra["results_metadata"] = {"source": "fixture"}
            result.warnings.append("fixture warning")
            return result

        parent = APIRouter()
        parent.include_router(router, prefix="/quantitative")
        app = FastAPI()
        app.include_router(parent, prefix="/api/v1")
        operation = {"path": "/api/v1/quantitative/summary", "method": "GET", "parameter_locations": {}, "body_schema": None}
        result = asyncio.run(bridge.dispatch_asgi(app, operation, {}, None))
        self.assertEqual(result["extra"]["metadata"]["route"], "/quantitative/summary")
        self.assertEqual(result["extra"]["results_metadata"]["source"], "fixture")
        self.assertEqual(result["warnings"], ["fixture warning"])
        self.assertIn("provider", result)


class HTTPTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(service, "MCP service has not been implemented")
        self.settings = service.Settings(token="test-token-" + "x" * 40, port=8768)
        self.server = service.make_server(self.settings, port=0, executor=lambda name, args, settings, timeout: {
            "status": "complete", "gate": "pass", "arguments": args, "name": name})
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.close_server)

    def close_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def post(self, method, params=None, *, token=True, headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=4)
        self.addCleanup(connection.close)
        body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}
        request_headers = {"Content-Type": "application/json"}
        if token:
            request_headers["Authorization"] = "Bearer " + self.settings.token
        request_headers.update(headers or {})
        connection.request("POST", "/mcp", json.dumps(body), request_headers)
        response = connection.getresponse()
        raw = response.read()
        return response.status, json.loads(raw) if raw else None

    def test_authentication_and_origin_are_enforced(self):
        self.assertEqual(self.post("tools/list", token=False)[0], 401)
        self.assertEqual(self.post("tools/list", headers={"Origin": "https://evil.example"})[0], 403)
        self.assertEqual(self.post("tools/list", headers={"Host": "attacker.example"})[0], 403)

    def test_mcp_handshake_matches_current_runtime_client(self):
        status, body = self.post("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})
        self.assertEqual(status, 200)
        self.assertEqual(body["result"]["protocolVersion"], "2025-06-18")
        self.assertIn("tools", body["result"]["capabilities"])

    def test_only_fixed_read_only_tools_are_exposed(self):
        status, body = self.post("tools/list")
        self.assertEqual(status, 200)
        tools = body["result"]["tools"]
        self.assertEqual({tool["name"] for tool in tools}, {"openbb_status", "openbb_catalog", "openbb_describe", "openbb_query"})
        self.assertTrue(all(tool["annotations"]["readOnlyHint"] for tool in tools))
        self.assertFalse(any("dispatch" in tool["name"] or "install" in tool["name"] for tool in tools))

    def test_unknown_tools_are_errors_and_do_not_reach_executor(self):
        status, body = self.post("tools/call", {"name": "install_skill", "arguments": {}})
        self.assertEqual(status, 200)
        self.assertTrue(body["result"]["isError"])
        self.assertEqual(body["result"]["structuredContent"]["gate"], "fail")

    def test_rpc_encoding_overhead_cannot_exceed_client_four_mib_limit(self):
        self.server.executor = lambda *args, **kwargs: {"status": "complete", "gate": "pass", "value": '"' * 900000}
        status, body = self.post("tools/call", {"name": "openbb_status", "arguments": {}})
        self.assertEqual(status, 200)
        self.assertTrue(body["result"]["isError"])
        self.assertEqual(body["result"]["structuredContent"]["code"], "response_limit")


if __name__ == "__main__":
    unittest.main()
