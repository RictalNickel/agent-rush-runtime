"""Exercise security web checks against ephemeral loopback HTTP servers only."""

import json
import sys
import tempfile
import threading
import time
import unittest
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "deploy/security"))

from boxoro_security.checks import web_headers
from boxoro_security.runner import run_request

SAFE_HTML_HEADERS = (
    ("Content-Type", "text/html; charset=utf-8"),
    ("Content-Security-Policy", "default-src 'self'; frame-ancestors 'none'"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
)


@contextmanager
def loopback_response(status=200, headers=(), body=b""):
    """Record real requests and make redirects terminate locally if followed."""
    requests = []
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            with lock:
                requests.append((self.command, self.path, dict(self.headers)))
            self.send_response(status if self.path == "/" else 200)
            for name, value in headers:
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                # The header-only client may close without consuming the body.
                pass

        def log_message(self, _format, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(
        target=server.serve_forever,
        kwargs={"poll_interval": 0.01},
        daemon=True,
    )
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", requests
    finally:
        server.shutdown()
        thread.join(timeout=2)
        server.server_close()


class SecurityWebTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()

    def run_web(self, target):
        return run_request(
            {"action": "web", "target": target, "timeout_seconds": 10},
            workspace=self.workspace,
            tool_bin=self.root / "no-scanners-installed",
        )

    def test_checks_one_root_response_without_crawling_or_sending_credentials(self):
        headers = SAFE_HTML_HEADERS + (
            ("Set-Cookie", "session=opaque; HttpOnly; SameSite=Lax"),
        )
        with loopback_response(
            headers=headers,
            body=b'<a href="/must-not-be-crawled">Another page</a>',
        ) as (target, requests):
            check, findings = web_headers(target, time.monotonic() + 5)

        self.assertEqual(
            [(method, path) for method, path, _ in requests], [("GET", "/")]
        )
        sent_headers = {name.lower(): value for name, value in requests[0][2].items()}
        self.assertNotIn("authorization", sent_headers)
        self.assertNotIn("cookie", sent_headers)
        self.assertEqual(check["http_status"], 200)
        self.assertEqual(check["status"], "complete")
        self.assertEqual(findings, [])

    def test_missing_headers_and_unsafe_cookie_have_expected_findings(self):
        headers = (
            ("Content-Type", "text/html"),
            ("Set-Cookie", "unsafe=opaque; Path=/"),
            ("Set-Cookie", "safe=opaque; HttpOnly; SameSite=Strict"),
        )
        with loopback_response(headers=headers) as (target, _requests):
            check, findings = web_headers(target, time.monotonic() + 5)

        self.assertEqual(check["status"], "complete")
        self.assertEqual(
            {item["rule_id"]: item["severity"] for item in findings},
            {
                "missing-csp": "medium",
                "missing-frame-policy": "medium",
                "missing-nosniff": "low",
                "missing-referrer-policy": "low",
                "cookie-missing-httponly": "medium",
                "cookie-missing-samesite": "low",
            },
        )
        self.assertEqual(len(findings), 6)

    def test_redirect_is_not_followed_and_cannot_produce_a_passing_gate(self):
        with loopback_response(
            status=302,
            headers=(("Location", "/must-not-be-followed"),),
        ) as (target, requests):
            result = self.run_web(target)

        self.assertEqual(
            [(method, path) for method, path, _ in requests], [("GET", "/")]
        )
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["checks"][0]["status"], "skipped")
        self.assertEqual(result["checks"][0]["http_status"], 302)
        report = json.loads((self.workspace / result["reports"]["json"]).read_text())
        self.assertEqual(report["findings"], [])

    def test_header_and_cookie_values_never_reach_summary_or_report_artifacts(self):
        header_token = "OPAQUE_HEADER_SENTINEL_94f2c8a1"
        cookie_token = "OPAQUE_COOKIE_SENTINEL_5e97b631"
        csp_nonce = "OPAQUE_NONCE_SENTINEL_6a84c913"
        headers = (
            ("Content-Type", "text/html"),
            ("X-Trace-Note", header_token),
            (
                "Content-Security-Policy",
                "default-src 'self'; frame-ancestors 'none'; script-src 'nonce-"
                + csp_nonce
                + "'",
            ),
            ("X-Content-Type-Options", "nosniff"),
            ("Referrer-Policy", "no-referrer"),
            ("Set-Cookie", "session=" + cookie_token + "; Path=/"),
        )
        with loopback_response(headers=headers) as (target, requests):
            result = self.run_web(target)

        self.assertEqual(len(requests), 1)
        self.assertEqual(result["checks"][0]["status"], "complete")
        self.assertEqual(
            set(result["reports"]), {"json", "markdown", "sarif", "remediation"}
        )
        artifacts = {"summary": json.dumps(result)}
        artifacts.update(
            (kind, (self.workspace / path).read_text())
            for kind, path in result["reports"].items()
        )
        for kind, content in artifacts.items():
            with self.subTest(artifact=kind):
                for value in (header_token, cookie_token, csp_nonce):
                    self.assertNotIn(value, content)
        report = json.loads(artifacts["json"])
        self.assertEqual(
            {item["rule_id"] for item in report["findings"]},
            {"cookie-missing-httponly", "cookie-missing-samesite"},
        )

    def test_client_error_is_incomplete_even_when_security_headers_are_present(self):
        with loopback_response(status=404, headers=SAFE_HTML_HEADERS) as (
            target,
            requests,
        ):
            result = self.run_web(target)

        self.assertEqual(len(requests), 1)
        self.assertEqual(result["checks"][0]["http_status"], 404)
        self.assertEqual(result["checks"][0]["status"], "error")
        self.assertEqual(result["gate"], "incomplete")
        report = json.loads((self.workspace / result["reports"]["json"]).read_text())
        self.assertEqual(report["findings"], [])
        sarif = json.loads((self.workspace / result["reports"]["sarif"]).read_text())
        self.assertFalse(sarif["runs"][0]["invocations"][0]["executionSuccessful"])


if __name__ == "__main__":
    unittest.main()
