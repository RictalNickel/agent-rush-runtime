#!/usr/bin/env python3
"""Optional, bounded, control-side intelligence MCP for Boxoro (Python 3.10+).

Only fixed public GET APIs are reachable. This is not a scanner, upload endpoint,
general proxy, or sandbox. Install it as a read-only, dedicated-user service.
"""

import dataclasses
import datetime as dt
import hmac
import http.client
import http.server
import json
import math
import os
import re
import selectors
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

PROTOCOL_VERSION = "2025-06-18"
MAX_BODY = 8 * 1024
MAX_HEADERS = 16 * 1024
MAX_RESPONSE = 512 * 1024
MAX_WORKER_RESPONSE = 240 * 1024
HTTP_TIMEOUT = 30
WORKER_TIMEOUT = 25
UPSTREAM_TIMEOUT = 8
CVE_PATTERN = r"CVE-[0-9]{4}-[0-9]{4,12}"
OSV_PATTERN = r"(?:OSV-[0-9]{4}-[0-9]{1,12}|GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4})"
PREFIX_PATTERN = r"[A-Fa-f0-9]{5}"
HASH_PATTERN = r"[A-Fa-f0-9]{64}"
TOOL_FIELDS = {
    "cve_context": ("cve", CVE_PATTERN),
    "osv_vulnerability": ("id", OSV_PATTERN),
    "password_prefix": ("prefix", PREFIX_PATTERN),
    "artifact_reputation": ("sha256", HASH_PATTERN),
}
DESCRIPTIONS = {
    "cve_context": "Public CISA KEV membership and FIRST EPSS for one CVE; not an exploitability verdict.",
    "osv_vulnerability": "Bounded OSV advisory metadata and fixed versions by OSV or GHSA ID; no source upload.",
    "password_prefix": "HIBP SHA-1 range lookup using exactly five hex characters. Never send a password or full hash. Compare suffixes locally.",
    "artifact_reputation": "Optional VirusTotal existing-report lookup using only a SHA-256 hash. No upload or rescan; unknown is not safe.",
}
MESSAGES = {
    "invalid_arguments": "Invalid tool arguments; only the documented identifier is accepted.",
    "disabled": "This intelligence tool is disabled or unavailable in the operator policy.",
    "not_configured": "VirusTotal is not configured; no reputation lookup was performed.",
    "upstream_redirect": "The provider returned a redirect; redirects are not followed.",
    "upstream_rate_limited": "The provider rate limit was reached; coverage is unavailable.",
    "upstream_auth_denied": "The provider denied access; check the operator's service configuration.",
    "upstream_unavailable": "The provider could not be queried; coverage is unavailable.",
    "upstream_too_large": "The provider response exceeded the size limit; coverage is incomplete.",
    "upstream_malformed": "The provider response failed validation; coverage is unavailable.",
    "timeout": "The intelligence request exceeded its time limit; coverage is incomplete.",
    "response_limit": "The result exceeded the response limit; coverage is incomplete.",
    "internal_error": "The intelligence operation failed; coverage is unavailable.",
}


class ToolError(Exception):
    def __init__(self, code):
        self.code = code if code in MESSAGES else "internal_error"
        super().__init__(MESSAGES[self.code])


def failure(code):
    error = ToolError(code)
    return {"status": "error", "code": error.code, "message": str(error)}


def _enabled(value):
    names = tuple(part.strip() for part in value.split(",") if part.strip())
    if len(set(names)) != len(names) or any(name not in TOOL_FIELDS for name in names):
        raise ValueError("invalid enabled tools")
    return names


@dataclasses.dataclass(frozen=True)
class Settings:
    token: str = dataclasses.field(repr=False)
    vt_api_key: str = dataclasses.field(default="", repr=False)
    enabled_tools: tuple = tuple(TOOL_FIELDS)
    port: int = 8767
    max_concurrency: int = 4

    @classmethod
    def from_environ(cls, environ):
        token = environ.get("SECURITY_INTEL_TOKEN", "")
        key = environ.get("VT_API_KEY", "")
        if not re.fullmatch(r"[!-~]{32,256}", token):
            raise ValueError("SECURITY_INTEL_TOKEN must contain 32-256 visible ASCII characters")
        if key and not re.fullmatch(r"[!-~]{8,1024}", key):
            raise ValueError("invalid VT_API_KEY")
        port = int(environ.get("SECURITY_INTEL_PORT", "8767"))
        concurrency = int(environ.get("SECURITY_INTEL_MAX_CONCURRENCY", "4"))
        if not 1 <= port <= 65535 or not 1 <= concurrency <= 8:
            raise ValueError("invalid port or concurrency")
        enabled = _enabled(environ.get("SECURITY_INTEL_TOOLS", ",".join(TOOL_FIELDS)))
        return cls(token, key, enabled, port, concurrency)


def validate_arguments(name, arguments):
    if name not in TOOL_FIELDS:
        raise ToolError("disabled")
    field, pattern = TOOL_FIELDS[name]
    if (not isinstance(arguments, dict) or set(arguments) != {field}
            or not isinstance(arguments[field], str)
            or re.fullmatch(pattern, arguments[field]) is None):
        raise ToolError("invalid_arguments")
    value = arguments[field]
    return value.upper() if name == "password_prefix" else value.lower() if name == "artifact_reputation" else value


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def read_json(raw):
    def invalid_constant(_):
        raise ValueError("non-finite JSON value")
    return json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object,
                      parse_constant=invalid_constant)


def encode(value):
    return json.dumps(value, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("ascii")


def redact(value, settings):
    if isinstance(value, str):
        for secret in (settings.token, settings.vt_api_key):
            if secret:
                value = value.replace(secret, "[redacted]")
        return value
    if isinstance(value, list):
        return [redact(item, settings) for item in value]
    if isinstance(value, dict):
        return {redact(key, settings): redact(item, settings) for key, item in value.items()}
    return value


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def fetch_upstream(service, identifier, *, api_key=None):
    """No caller URL, proxy environment, redirects, compression or error-body forwarding."""
    headers = {"User-Agent": "Boxoro-Security-Intelligence/1.0", "Accept": "application/json",
               "Accept-Encoding": "identity"}
    if service == "kev" and identifier == "":
        url = "https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json"
        limit = 8 * 1024 * 1024
    elif service == "epss" and re.fullmatch(CVE_PATTERN, identifier):
        url = "https://api.first.org/data/v1/epss?cve=" + identifier
        limit = 64 * 1024
    elif service == "osv" and re.fullmatch(OSV_PATTERN, identifier):
        url = "https://api.osv.dev/v1/vulns/" + identifier
        limit = 512 * 1024
    elif service == "passwords" and re.fullmatch(r"[A-F0-9]{5}", identifier):
        url = "https://api.pwnedpasswords.com/range/" + identifier
        limit = 128 * 1024
        headers.update({"Accept": "text/plain", "Add-Padding": "true"})
    elif service == "virustotal" and re.fullmatch(r"[a-f0-9]{64}", identifier):
        if not api_key:
            raise ToolError("not_configured")
        url = "https://www.virustotal.com/api/v3/files/" + identifier
        limit = 512 * 1024
        headers["x-apikey"] = api_key
    else:
        raise ToolError("invalid_arguments")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    request = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with opener.open(request, timeout=UPSTREAM_TIMEOUT) as response:
            if response.status != 200:
                raise ToolError("upstream_unavailable")
            if response.headers.get("Content-Encoding", "identity").lower() != "identity":
                raise ToolError("upstream_malformed")
            lengths = response.headers.get_all("Content-Length", [])
            if len(lengths) > 1 or (lengths and not re.fullmatch(r"[0-9]{1,12}", lengths[0])):
                raise ToolError("upstream_malformed")
            if lengths and int(lengths[0]) > limit:
                raise ToolError("upstream_too_large")
            body = response.read(limit + 1)
            if len(body) > limit:
                raise ToolError("upstream_too_large")
            if lengths and len(body) != int(lengths[0]):
                raise ToolError("upstream_malformed")
            return 200, body
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()  # Never read, log or return the provider's error body or Location.
        if status == 404 and service in ("osv", "virustotal"):
            return 404, b""
        code = ("upstream_redirect" if 300 <= status < 400 else
                "upstream_rate_limited" if status == 429 else
                "upstream_auth_denied" if status in (401, 403) else "upstream_unavailable")
        raise ToolError(code) from None
    except (OSError, urllib.error.URLError, http.client.HTTPException, ValueError):
        raise ToolError("upstream_unavailable") from None


def _object(value):
    if not isinstance(value, dict):
        raise ToolError("upstream_malformed")
    return value


def _items(value, maximum):
    if not isinstance(value, list) or len(value) > maximum:
        raise ToolError("upstream_malformed")
    return value


def _integer(value, maximum=10**12):
    if type(value) is not int or not 0 <= value <= maximum:
        raise ToolError("upstream_malformed")
    return value


def _date(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", value):
        raise ToolError("upstream_malformed")
    try:
        return dt.date.fromisoformat(value)
    except ValueError:
        raise ToolError("upstream_malformed") from None


def _timestamp(value):
    if not isinstance(value, str) or len(value) > 40:
        raise ToolError("upstream_malformed")
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError("missing timezone")
        return parsed.astimezone(dt.timezone.utc).isoformat().replace("+00:00", "Z")
    except ValueError:
        raise ToolError("upstream_malformed") from None


def _freshness(day, max_age):
    age = (dt.datetime.now(dt.timezone.utc).date() - day).days
    if age < -1:
        raise ToolError("upstream_malformed")
    return "stale" if age > max_age else "ok"


def _atom(value, pattern=r"[A-Za-z0-9@._+:/!~*-]{1,200}"):
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise ToolError("upstream_malformed")
    return value


class Intelligence:
    def __init__(self, settings):
        self.settings = settings

    def _json(self, service, identifier, **kwargs):
        status, raw = fetch_upstream(service, identifier, **kwargs)
        if status == 404:
            return None
        try:
            return _object(read_json(raw))
        except (ValueError, UnicodeError, RecursionError):
            raise ToolError("upstream_malformed") from None

    def call(self, name, arguments):
        if name not in self.settings.enabled_tools:
            raise ToolError("disabled")
        value = validate_arguments(name, arguments)
        result = getattr(self, name)(value)
        result["retrieved_at"] = dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")
        return result

    def cve_context(self, cve):
        sources = {}
        for name, operation in (("kev", self._kev), ("epss", self._epss)):
            try:
                sources[name] = operation(cve)
            except ToolError as error:
                sources[name] = failure(error.code)
        return {"status": "ok" if all(v["status"] == "ok" for v in sources.values()) else "partial",
                "cve": cve, "sources": sources,
                "limitation": "Public threat context does not establish affected-version presence, reachability or safety."}

    def _kev(self, cve):
        data = self._json("kev", "")
        entries = _items(data.get("vulnerabilities"), 20000)
        if _integer(data.get("count"), 20000) != len(entries) or not entries:
            raise ToolError("upstream_malformed")
        released = _timestamp(data.get("dateReleased"))
        result = {"status": _freshness(_date(released[:10]), 30), "listed": False,
                  "catalog_released": released,
                  "source_url": "https://www.cisa.gov/known-exploited-vulnerabilities-catalog"}
        seen = set()
        for item in entries:
            item = _object(item)
            identifier = _atom(item.get("cveID"), CVE_PATTERN)
            if identifier in seen:
                raise ToolError("upstream_malformed")
            seen.add(identifier)
            if identifier != cve:
                continue
            result.update({"listed": True, "date_added": _date(item.get("dateAdded")).isoformat(),
                           "due_date": _date(item.get("dueDate")).isoformat()})
            ransomware = item.get("knownRansomwareCampaignUse")
            if ransomware in ("Known", "Unknown"):
                result["ransomware_campaign_use"] = ransomware
        return result

    def _epss(self, cve):
        data = self._json("epss", cve)
        if data.get("status") != "OK":
            raise ToolError("upstream_malformed")
        entries = _items(data.get("data"), 1)
        if not entries:
            return {"status": "not_found", "source_url": "https://www.first.org/epss/"}
        record = _object(entries[0])
        if record.get("cve") != cve:
            raise ToolError("upstream_malformed")
        scores = []
        for field in ("epss", "percentile"):
            value = record.get(field)
            if type(value) not in (str, int, float) or len(str(value)) > 32:
                raise ToolError("upstream_malformed")
            try:
                score = float(value)
            except ValueError:
                raise ToolError("upstream_malformed") from None
            if not math.isfinite(score) or not 0 <= score <= 1:
                raise ToolError("upstream_malformed")
            scores.append(score)
        day = _date(record.get("date"))
        return {"status": _freshness(day, 3), "probability": scores[0], "percentile": scores[1],
                "score_date": day.isoformat(), "horizon_days": 30, "source_url": "https://www.first.org/epss/"}

    def osv_vulnerability(self, identifier):
        data = self._json("osv", identifier)
        if data is None:
            return {"status": "not_found", "id": identifier, "assessment": "unknown"}
        if data.get("id") != identifier:
            raise ToolError("upstream_malformed")
        affected = _items(data.get("affected", []), 2000)
        truncated = len(affected) > 20
        packages = []
        for record in affected[:20]:
            record = _object(record)
            package = _object(record.get("package"))
            fixes = set()
            ranges = _items(record.get("ranges", []), 1000)
            truncated = truncated or len(ranges) > 20
            for version_range in ranges[:20]:
                events = _items(_object(version_range).get("events", []), 10000)
                truncated = truncated or len(events) > 100
                for event in events[:100]:
                    if "fixed" in _object(event):
                        fixes.add(_atom(event["fixed"]))
            truncated = truncated or len(fixes) > 50
            packages.append({"ecosystem": _atom(package.get("ecosystem"), r"[A-Za-z0-9 ._-]{1,80}"),
                             "name": _atom(package.get("name")), "fixed_versions": sorted(fixes)[:50]})
        aliases = _items(data.get("aliases", []), 500)
        retained_aliases = [value for value in aliases if isinstance(value, str)
                            and re.fullmatch("(?:" + CVE_PATTERN + "|" + OSV_PATTERN + ")", value)]
        truncated = truncated or len(retained_aliases) > 32
        withdrawn = data.get("withdrawn")
        result = {"status": "partial" if truncated else "ok", "id": identifier,
                  "modified": _timestamp(data.get("modified")), "withdrawn": bool(withdrawn),
                  "aliases": retained_aliases[:32], "affected": packages, "truncated": truncated,
                  "source_url": "https://osv.dev/vulnerability/" + identifier,
                  "limitation": "Summary only; inspect ecosystem ranges and compatibility before selecting a fix."}
        if withdrawn:
            result["withdrawn_at"] = _timestamp(withdrawn)
        return result

    def password_prefix(self, prefix):
        _, raw = fetch_upstream("passwords", prefix)
        try:
            lines = raw.decode("ascii").splitlines()
        except UnicodeError:
            raise ToolError("upstream_malformed") from None
        if not lines or len(lines) > 2048:
            raise ToolError("upstream_malformed")
        suffixes, seen = [], set()
        for line in lines:
            match = re.fullmatch(r"([A-F0-9]{35}):([0-9]{1,12})", line)
            if not match or match[1] in seen:
                raise ToolError("upstream_malformed")
            seen.add(match[1])
            count = int(match[2])
            if count:  # HIBP's Add-Padding entries have count zero.
                suffixes.append({"suffix": match[1], "count": count})
        return {"status": "ok", "provider": "Have I Been Pwned Pwned Passwords", "prefix": prefix,
                "suffixes": suffixes,
                "limitation": "Compare the remaining 35 SHA-1 characters locally. This prefix lookup cannot determine a password match."}

    def artifact_reputation(self, sha256):
        if not self.settings.vt_api_key:
            raise ToolError("not_configured")
        data = self._json("virustotal", sha256, api_key=self.settings.vt_api_key)
        if data is None:
            return {"status": "not_found", "sha256": sha256, "assessment": "unknown", "provider": "VirusTotal"}
        record = _object(data.get("data"))
        if record.get("id") != sha256 or record.get("type") != "file":
            raise ToolError("upstream_malformed")
        attributes = _object(record.get("attributes"))
        if "sha256" in attributes and attributes["sha256"] != sha256:
            raise ToolError("upstream_malformed")
        stats = _object(attributes.get("last_analysis_stats"))
        if not {"malicious", "suspicious", "undetected"}.issubset(stats):
            raise ToolError("upstream_malformed")
        counts = {key: _integer(stats[key], 100000) for key in (
            "malicious", "suspicious", "harmless", "undetected", "timeout", "confirmed-timeout",
            "failure", "type-unsupported") if key in stats}
        analysis_date = _integer(attributes.get("last_analysis_date"), int(time.time()) + 86400)
        age = max(0, (int(time.time()) - analysis_date) // 86400)
        result = {"status": "stale" if age > 30 else "ok", "sha256": sha256, "provider": "VirusTotal", "counts": counts,
                  "last_analysis_date": analysis_date, "analysis_age_days": age,
                  "limitation": "Existing vendor detections are historical signals. Zero detections or an unknown hash do not establish safety."}
        for key in ("first_submission_date", "last_submission_date"):
            if key in attributes:
                result[key] = _integer(attributes[key], 32503680000)
        return result


def _stop_worker(process):
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=2)


def run_isolated(name, arguments, settings, *, timeout=WORKER_TIMEOUT):
    """Fixed child process provides a total deadline, including slow DNS/TLS/body reads."""
    if timeout <= 0:
        return failure("timeout")
    env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8",
           "SECURITY_INTEL_TOOLS": ",".join(settings.enabled_tools)}
    if name == "artifact_reputation" and settings.vt_api_key:
        env["VT_API_KEY"] = settings.vt_api_key
    process = None
    deadline = time.monotonic() + min(timeout, WORKER_TIMEOUT)
    try:
        payload = encode({"name": name, "arguments": arguments})
        if len(payload) > MAX_BODY:
            return failure("invalid_arguments")
        process = subprocess.Popen([sys.executable, "-I", os.path.abspath(__file__), "--worker"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env=env, start_new_session=True)
        process.stdin.write(payload)
        process.stdin.close()
        chunks = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return failure("timeout")
                events = selector.select(remaining)
                if not events:
                    return failure("timeout")
                chunk = os.read(process.stdout.fileno(), 16384)
                if not chunk:
                    break
                chunks.extend(chunk)
                if len(chunks) > MAX_WORKER_RESPONSE:
                    return failure("response_limit")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return failure("timeout")
        process.wait(timeout=remaining)
        if process.returncode != 0:
            return failure("internal_error")
        result = read_json(chunks)
        return result if isinstance(result, dict) and isinstance(result.get("status"), str) else failure("internal_error")
    except subprocess.TimeoutExpired:
        return failure("timeout")
    except (OSError, ValueError, UnicodeError, RecursionError):
        return failure("internal_error")
    finally:
        if process is not None:
            _stop_worker(process)
            for stream in (process.stdin, process.stdout):
                if stream and not stream.closed:
                    stream.close()


def _worker():
    try:
        raw = sys.stdin.buffer.read(MAX_BODY + 1)
        if len(raw) > MAX_BODY:
            raise ToolError("invalid_arguments")
        request = read_json(raw)
        if not isinstance(request, dict) or set(request) != {"name", "arguments"}:
            raise ToolError("invalid_arguments")
        settings = Settings(token="", vt_api_key=os.environ.get("VT_API_KEY", ""),
                            enabled_tools=_enabled(os.environ.get("SECURITY_INTEL_TOOLS", "")))
        result = Intelligence(settings).call(request["name"], request["arguments"])
    except ToolError as error:
        result = failure(error.code)
    except Exception:
        result = failure("internal_error")
    raw = encode(result)
    sys.stdout.buffer.write(raw if len(raw) <= MAX_WORKER_RESPONSE else encode(failure("response_limit")))


class HeaderReader:
    def __init__(self, source):
        self.source, self.remaining = source, MAX_HEADERS

    def readline(self, limit=-1):
        line = self.source.readline(min(limit if limit > 0 else MAX_HEADERS + 1, self.remaining + 1))
        self.remaining -= len(line)
        if self.remaining < 0:
            raise http.client.LineTooLong("header block")
        return line


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "BoxoroSecurityIntel/1"
    sys_version = ""

    def setup(self):
        self.request.settimeout(HTTP_TIMEOUT)
        super().setup()
        self.deadline = time.monotonic() + HTTP_TIMEOUT
        self.expiry = threading.Timer(HTTP_TIMEOUT, self._expire)
        self.expiry.daemon = True
        self.expiry.start()

    def _expire(self):
        try:
            self.request.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

    def finish(self):
        self.expiry.cancel()
        super().finish()

    def log_message(self, format, *args):
        pass  # Do not log identifiers, prefixes, credentials, request paths or provider errors.

    def parse_request(self):
        if len(self.raw_requestline) > 2048:
            self.requestline, self.request_version, self.command = "", "HTTP/1.1", None
            self.send_error(414)
            return False
        source = self.rfile
        self.rfile = HeaderReader(source)
        try:
            return super().parse_request()
        finally:
            self.rfile = source

    def send_error(self, code, message=None, explain=None):
        self._respond(code, {"error": "request_rejected"})

    def _respond(self, status, value=None):
        self.close_connection = True
        raw = b"" if value is None else encode(redact(value, self.server.settings))
        if len(raw) > MAX_RESPONSE:
            status, raw = 500, b'{"error":"response_limit"}'
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("Connection", "close")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        if raw:
            self.wfile.write(raw)

    def _authorized(self):
        if self.path != "/mcp":
            self._respond(404, {"error": "not_found"})
            return False
        if self.headers.get_all("Origin"):
            self._respond(403, {"error": "origin_rejected"})
            return False
        hosts = self.headers.get_all("Host", [])
        port = self.server.server_port
        allowed_hosts = {"127.0.0.1:" + str(port), "localhost:" + str(port)}
        if port == 80:
            allowed_hosts.update(("127.0.0.1", "localhost"))
        if len(hosts) != 1 or hosts[0].lower() not in allowed_hosts:
            self._respond(403, {"error": "host_rejected"})
            return False
        auth = self.headers.get_all("Authorization", [])
        expected = ("Bearer " + self.server.settings.token).encode("ascii")
        if len(auth) != 1 or not hmac.compare_digest(auth[0].encode("utf-8"), expected):
            self._respond(401, {"error": "unauthorized"})
            return False
        versions = self.headers.get_all("MCP-Protocol-Version", [])
        if len(versions) > 1 or (versions and versions[0] != PROTOCOL_VERSION):
            self._respond(400, {"error": "unsupported_protocol"})
            return False
        return True

    def do_GET(self):
        if self._authorized():
            self._respond(405, {"error": "method_not_allowed"})

    do_DELETE = do_GET
    do_OPTIONS = do_GET

    def do_POST(self):
        if not self._authorized():
            return
        if self.headers.get_all("Transfer-Encoding") or self.headers.get_all("Content-Encoding"):
            self._respond(400, {"error": "unsupported_encoding"})
            return
        lengths = self.headers.get_all("Content-Length", [])
        if not lengths:
            self._respond(411, {"error": "length_required"})
            return
        if len(lengths) != 1 or not re.fullmatch(r"[0-9]{1,8}", lengths[0]):
            self._respond(400, {"error": "invalid_length"})
            return
        size = int(lengths[0])
        if size > MAX_BODY:
            self._respond(413, {"error": "body_too_large"})
            return
        types = self.headers.get_all("Content-Type", [])
        if len(types) != 1 or types[0].split(";", 1)[0].strip().lower() != "application/json":
            self._respond(415, {"error": "content_type_required"})
            return
        try:
            raw = self.rfile.read(size)
            if len(raw) != size:
                raise ValueError("short body")
            message = read_json(raw)
        except (ValueError, UnicodeError, RecursionError):
            self._respond(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Invalid JSON"}})
            return
        self._rpc(message)

    def _rpc(self, message):
        invalid = {"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "Invalid request"}}
        if (not isinstance(message, dict) or set(message) - {"jsonrpc", "id", "method", "params"}
                or message.get("jsonrpc") != "2.0" or not isinstance(message.get("method"), str)):
            self._respond(400, invalid)
            return
        method, request_id, params = message["method"], message.get("id"), message.get("params", {})
        if not isinstance(params, dict):
            self._respond(400, invalid)
            return
        if method == "notifications/initialized" and "id" not in message and not params:
            self._respond(202)
            return
        if not ((type(request_id) is int and 0 <= request_id <= 2**53)
                or (isinstance(request_id, str) and re.fullmatch(r"[A-Za-z0-9_.:-]{1,64}", request_id))):
            self._respond(400, invalid)
            return
        response = {"jsonrpc": "2.0", "id": request_id}
        if method == "initialize":
            if (set(params) != {"protocolVersion", "capabilities", "clientInfo"}
                    or not isinstance(params["protocolVersion"], str)
                    or not isinstance(params["capabilities"], dict)
                    or not isinstance(params["clientInfo"], dict)):
                response["error"] = {"code": -32602, "message": "Invalid initialization parameters"}
            else:
                response["result"] = {"protocolVersion": PROTOCOL_VERSION, "capabilities": {"tools": {}},
                                      "serverInfo": {"name": "boxoro-security-intel", "version": "1.0.0"}}
        elif method == "ping" and not params:
            response["result"] = {}
        elif method == "tools/list" and not params:
            response["result"] = {"tools": [
                {"name": name, "description": DESCRIPTIONS[name],
                 "inputSchema": {"type": "object", "additionalProperties": False,
                                 "required": [TOOL_FIELDS[name][0]],
                                 "properties": {TOOL_FIELDS[name][0]: {"type": "string", "pattern": "^" + TOOL_FIELDS[name][1] + "$"}}},
                 "annotations": {"readOnlyHint": True, "idempotentHint": True, "openWorldHint": True}}
                for name in self.server.settings.enabled_tools]}
        elif method == "tools/call":
            try:
                if set(params) != {"name", "arguments"} or not isinstance(params.get("name"), str):
                    raise ToolError("invalid_arguments")
                name, arguments = params["name"], params["arguments"]
                if name not in self.server.settings.enabled_tools:
                    raise ToolError("disabled")
                validate_arguments(name, arguments)
                result = self.server.executor(name, arguments, self.server.settings,
                                              timeout=min(WORKER_TIMEOUT, self.deadline - time.monotonic()))
            except ToolError as error:
                result = failure(error.code)
            except Exception:
                result = failure("internal_error")
            if not isinstance(result, dict) or len(encode(result)) > MAX_WORKER_RESPONSE:
                result = failure("response_limit")
            result = redact(result, self.server.settings)
            response["result"] = {"content": [{"type": "text", "text": encode(result).decode("ascii")}],
                                  "structuredContent": result,
                                  "isError": result.get("status") not in ("ok", "not_found")}
        else:
            response["error"] = {"code": -32601, "message": "Method or parameters not supported"}
        if len(encode(response)) > MAX_RESPONSE:
            response = {"jsonrpc": "2.0", "id": request_id,
                        "error": {"code": -32603, "message": "Response limit exceeded"}}
        self._respond(200, response)


class IntelServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 8

    def __init__(self, settings, port, executor):
        self.settings, self.executor = settings, executor
        self.slots = threading.BoundedSemaphore(settings.max_concurrency)
        super().__init__(("127.0.0.1", port), Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            try:
                request.settimeout(0.2)
                body = b'{"error":"server_busy"}'
                request.sendall(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n"
                                b"Content-Type: application/json\r\nContent-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
            except OSError:
                pass
            finally:
                self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except Exception:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        pass  # No untrusted exception details or credential-bearing headers in logs.


def make_server(settings, *, port=None, executor=None):
    return IntelServer(settings, settings.port if port is None else port, executor or run_isolated)


def main():
    if sys.argv[1:] == ["--worker"]:
        _worker()
        return 0
    if sys.argv[1:]:
        print("usage: intel_server.py", file=sys.stderr)
        return 2
    try:
        settings = Settings.from_environ(os.environ)
        server = make_server(settings)
    except (ValueError, OSError):
        print("security-intel startup failed: check token, service settings and loopback port", file=sys.stderr)
        return 2
    print("security-intel listening on 127.0.0.1:" + str(settings.port), file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
