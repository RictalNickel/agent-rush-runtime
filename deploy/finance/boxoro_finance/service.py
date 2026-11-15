"""Authenticated MCP adapter with cached metadata and fresh bounded query children."""
import dataclasses
import hmac
import http.client
import http.server
import os
import re
import selectors
import signal
import socket
import stat
import subprocess
import sys
import threading
import time
from pathlib import Path

from . import __version__
from .common import MAX_OUTPUT_BYTES, FinanceError, encode, error, fields, read_json
from .openbb_bridge import (
    CREDENTIAL_ENV, DEFAULT_NAMESPACES, Catalog, execute_tool, metadata_tool, redact, reject_sensitive_arguments,
    validate_tool_arguments,
)

PROTOCOL_VERSION = "2025-06-18"
MAX_BODY = 512 * 1024
MAX_HEADERS = 16 * 1024
MAX_RPC_RESPONSE = 4 * 1024 * 1024
MAX_CATALOG_BYTES = 16 * 1024 * 1024
MAX_WORKER_BODY = 2 * MAX_BODY + 4096
TOOL_SCHEMAS = {
    "openbb_status": {"type": "object", "properties": {}, "additionalProperties": False},
    "openbb_catalog": {"type": "object", "additionalProperties": False, "properties": {
        "query": {"type": "string", "maxLength": 200}, "namespace": {"type": "string", "enum": list(DEFAULT_NAMESPACES)},
        "offset": {"type": "integer", "minimum": 0, "maximum": 100000}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}}},
    "openbb_describe": {"type": "object", "additionalProperties": False, "required": ["command"], "properties": {"command": {"type": "string", "maxLength": 200}}},
    "openbb_query": {"type": "object", "additionalProperties": False, "required": ["command"], "properties": {
        "command": {"type": "string", "maxLength": 200}, "parameters": {"type": "object"}, "body": {},
        "max_rows": {"type": "integer", "minimum": 1, "maximum": 10000}, "allow_network": {"type": "boolean", "default": False}}},
}
TOOL_DESCRIPTIONS = {
    "openbb_status": "Report installed OpenBB V5 version, permitted command coverage and credential presence without exposing secrets.",
    "openbb_catalog": "Search or page the actual installed, policy-filtered OpenBB command catalog; describe a command before querying it.",
    "openbb_describe": "Get the exact installed query/path parameter and JSON body schemas for one allowed OpenBB command.",
    "openbb_query": "Run one discovered OpenBB data or analytical command. Provider reads require allow_network=true. Preserve provider metadata, timestamps and warnings. No brokerage actions, account settings or installs.",
}


def failure(code, message, *, incomplete=True):
    return {"schema_version": 1, "status": "error", "gate": "incomplete" if incomplete else "fail", "code": code, "message": message}


@dataclasses.dataclass(frozen=True)
class Settings:
    token: str = dataclasses.field(repr=False)
    port: int = 8768
    max_concurrency: int = 2
    deadline_seconds: int = 90
    namespaces: tuple = DEFAULT_NAMESPACES
    provider_credentials: tuple = dataclasses.field(default=(), repr=False)
    url_hosts: str = ""
    state_directory: str = "/var/lib/boxoro-finance"
    bind_address: str = "127.0.0.1"

    @classmethod
    def from_environ(cls, environ):
        token = environ.get("FINANCE_OPENBB_TOKEN", "")
        token_file = environ.get("FINANCE_OPENBB_TOKEN_FILE", "")
        if token_file:
            if token:
                raise ValueError("Configure one bearer-token source")
            try:
                path = Path(token_file)
                if not path.is_absolute() or path.resolve() != path:
                    raise ValueError("token file must be absolute and not symlinked")
                fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(fd, "rb") as source:
                    info = os.fstat(source.fileno())
                    if (not stat.S_ISREG(info.st_mode) or info.st_uid not in (0, os.getuid())
                            or stat.S_IMODE(info.st_mode) & 0o077 or info.st_size > 258):
                        raise ValueError("token file must be a private owned regular file")
                    token = source.read(259).decode("ascii").rstrip("\r\n")
            except (OSError, UnicodeError) as exc:
                raise ValueError("token file is unavailable or unsupported") from exc
        if not re.fullmatch(r"[!-~]{32,256}", token):
            raise ValueError("FINANCE_OPENBB_TOKEN must be 32-256 visible ASCII characters")
        port = int(environ.get("FINANCE_OPENBB_PORT", "8768"))
        concurrency = int(environ.get("FINANCE_OPENBB_MAX_CONCURRENCY", "2"))
        deadline = int(environ.get("FINANCE_OPENBB_DEADLINE_SECONDS", "90"))
        namespaces = tuple(part.strip() for part in environ.get("FINANCE_OPENBB_NAMESPACES", ",".join(DEFAULT_NAMESPACES)).split(",") if part.strip())
        if not 1 <= port <= 65535 or not 1 <= concurrency <= 8 or not 10 <= deadline <= 180:
            raise ValueError("invalid service resource bounds")
        if not namespaces or len(set(namespaces)) != len(namespaces) or any(name not in DEFAULT_NAMESPACES for name in namespaces):
            raise ValueError("invalid allowed namespace")
        credentials = tuple((name, environ.get(name, "")) for name in CREDENTIAL_ENV)
        if any(value and (len(value) > 4096 or any(ord(char) < 32 for char in value)) for _, value in credentials):
            raise ValueError("invalid provider credential")
        state_directory = environ.get("FINANCE_OPENBB_STATE_DIR", "/var/lib/boxoro-finance")
        bind_address = environ.get("FINANCE_OPENBB_BIND", "127.0.0.1")
        if bind_address not in ("127.0.0.1", "0.0.0.0"):
            raise ValueError("bind must be literal loopback or explicit container wildcard")
        if not Path(state_directory).is_absolute() or ".." in Path(state_directory).parts:
            raise ValueError("service state directory must be absolute")
        return cls(token, port, concurrency, deadline, namespaces, credentials, environ.get("FINANCE_OPENBB_URL_HOSTS", ""), state_directory, bind_address)


def stop_process(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass


def bounded_process(argv, payload, environ, *, timeout, max_output=MAX_OUTPUT_BYTES):
    if timeout <= 0:
        return failure("timeout", "The OpenBB operation exceeded its deadline.")
    process = None
    deadline = time.monotonic() + timeout
    try:
        process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env=environ, start_new_session=True, cwd=str(Path(__file__).resolve().parent.parent))
        os.set_blocking(process.stdin.fileno(), False)
        os.set_blocking(process.stdout.fileno(), False)
        chunks, offset, eof = bytearray(), 0, False
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ, "output")
            selector.register(process.stdin, selectors.EVENT_WRITE, "input")
            while not eof:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return failure("timeout", "The OpenBB operation exceeded its deadline.")
                events = selector.select(remaining)
                if not events:
                    return failure("timeout", "The OpenBB operation exceeded its deadline.")
                for key, _ in events:
                    if key.data == "input":
                        try:
                            offset += os.write(process.stdin.fileno(), payload[offset:offset + 16384])
                        except BrokenPipeError:
                            offset = len(payload)
                        if offset == len(payload):
                            selector.unregister(process.stdin)
                            process.stdin.close()
                    else:
                        chunk = os.read(process.stdout.fileno(), 16384)
                        if not chunk:
                            eof = True
                            break
                        chunks.extend(chunk)
                        if len(chunks) > max_output:
                            return failure("response_limit", "The OpenBB result exceeded the bounded output size.")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return failure("timeout", "The OpenBB operation exceeded its deadline.")
        process.wait(timeout=remaining)
        if process.returncode != 0:
            return failure("worker_failed", "The OpenBB worker failed or exceeded its resource budget.")
        result = read_json(chunks)
        if not isinstance(result, dict) or result.get("gate") not in ("pass", "fail", "incomplete"):
            return failure("worker_failed", "The OpenBB worker returned an unsupported result.")
        return result
    except subprocess.TimeoutExpired:
        return failure("timeout", "The OpenBB operation exceeded its deadline.")
    except (OSError, ValueError, FinanceError):
        return failure("worker_failed", "The OpenBB worker could not complete the request.")
    finally:
        if process is not None:
            stop_process(process)
            for stream in (process.stdin, process.stdout):
                if stream and not stream.closed:
                    stream.close()


def run_isolated(name, arguments, settings, *, timeout, contract=None):
    request = {"name": name, "arguments": arguments}
    if len(encode(request)) > MAX_BODY:
        return failure("input_limit", "Tool arguments exceed 512 KiB.", incomplete=False)
    if contract is not None:
        if name != "openbb_query" or len(encode(contract)) > MAX_BODY:
            return failure("unsupported_schema", "The internal query contract exceeds its limit.")
        request["contract"] = contract
    payload = encode(request)
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "PYTHONNOUSERSITE": "1",
                   "HOME": settings.state_directory, "PYTHONHASHSEED": "0",
                   "TMPDIR": str(Path(settings.state_directory) / "tmp"), "TZ": "UTC",
                   "XDG_CACHE_HOME": str(Path(settings.state_directory) / "cache"),
                   "OPENBB_DEBUG_MODE": "false", "OPENBB_DEV_MODE": "false", "OPENBB_AUTO_BUILD": "false",
                   "OPENBB_ALLOW_ON_COMMAND_OUTPUT": "false", "OPENBB_ALLOW_MUTABLE_EXTENSIONS": "false",
                   "FINANCE_OPENBB_NAMESPACES": ",".join(settings.namespaces), "FINANCE_OPENBB_URL_HOSTS": settings.url_hosts,
                   "OPENBLAS_NUM_THREADS": "1", "OMP_NUM_THREADS": "1", "MPLBACKEND": "Agg"}
    bootstrap = name == "_catalog_snapshot"
    if not bootstrap:
        environment.update({key: value for key, value in settings.provider_credentials if value})
    script = Path(__file__).resolve().parent.parent / "finance_server.py"
    result = bounded_process([sys.executable, "-I", str(script), "--worker"], payload, environment,
                             timeout=min(timeout, settings.deadline_seconds),
                             max_output=MAX_CATALOG_BYTES if bootstrap else MAX_OUTPUT_BYTES)
    if bootstrap:
        return result  # Schema-only child has neither provider credentials nor the service token.
    return redact(result, [settings.token, *(value for _, value in settings.provider_credentials)])


class CatalogExecutor:
    """One bounded metadata bootstrap per immutable service deployment.

    Only compiled schemas are retained, in memory. Provider results, credentials,
    SDK objects and numerical state never enter this cache. The lock serializes
    cold discovery; waiting and bootstrap both consume the caller's deadline.
    """
    def __init__(self, settings):
        self.settings = settings
        self.lock = threading.Lock()
        self.loaded = None

    def __call__(self, name, arguments, settings, *, timeout):
        if settings is not self.settings:
            error("invalid_configuration", "Catalog executors belong to one service instance.", incomplete=True)
        validate_tool_arguments(name, arguments)
        deadline = time.monotonic() + min(timeout, settings.deadline_seconds)
        if not self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            return failure("timeout", "The OpenBB operation exceeded its deadline waiting for the catalog.")
        try:
            if self.loaded is None:
                snapshot = run_isolated("_catalog_snapshot", {}, settings, timeout=max(0, deadline - time.monotonic()))
                if snapshot.get("gate") != "pass":
                    return snapshot  # A failure is retryable, never cached as valid coverage.
                catalog = Catalog.from_snapshot(snapshot, namespaces=settings.namespaces)
                self.loaded = (catalog, snapshot["openbb_core_version"], snapshot["installation_sha256"])
            loaded = self.loaded
        finally:
            self.lock.release()
        if time.monotonic() >= deadline:
            return failure("timeout", "The OpenBB operation exceeded its deadline.")
        catalog, version, fingerprint = loaded
        if name != "openbb_query":
            return metadata_tool(name, arguments, catalog, version, fingerprint, credentials=dict(settings.provider_credentials))
        operation = catalog.describe(arguments["command"])
        reject_sensitive_arguments(arguments.get("parameters", {}), url_hosts=settings.url_hosts)
        reject_sensitive_arguments(arguments.get("body"), url_hosts=settings.url_hosts)
        if operation["requires_network"] and not arguments.get("allow_network", False):
            error("network_required", "Provider queries require allow_network=true.")
        contract = catalog.query_contract(arguments["command"], version, fingerprint)
        result = run_isolated(name, arguments, settings, timeout=max(0, deadline - time.monotonic()), contract=contract)
        if result.get("code") == "catalog_stale" and self.lock.acquire(timeout=max(0, deadline - time.monotonic())):
            try:
                if self.loaded is loaded:
                    self.loaded = None
            finally:
                self.lock.release()
        return result


def prepare_state(settings):
    """An operator-owned service home keeps OpenBB config out of projects/workspaces."""
    import stat
    directory = Path(settings.state_directory)
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if directory.is_symlink() or directory.resolve() != directory:
        raise ValueError("service state directory cannot contain symlinks")
    info = directory.stat()
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077:
        raise ValueError("service state directory must be private and owned by service user")
    config = directory / ".openbb_platform"
    config.mkdir(mode=0o700, exist_ok=True)
    if config.is_symlink() or config.stat().st_uid != os.getuid() or stat.S_IMODE(config.stat().st_mode) & 0o077:
        raise ValueError("OpenBB config directory must be private")
    for name in ("cache", "tmp", "data", "exports"):
        path = directory / name
        path.mkdir(mode=0o700, exist_ok=True)
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
            raise ValueError("service cache/data directories must be owned directories without symlinks")
        path.chmod(0o700)
    # Fixed settings are refreshed only on service startup, never from a query.
    values = {
        "system_settings.json": {"logging_suppress": True, "headless": True, "allow_on_command_output": False, "allow_mutable_extensions": False},
        "user_settings.json": {"credentials": {}, "defaults": {"commands": {}}, "preferences": {
            "output_type": "OBBject", "metadata": True, "show_warnings": False, "request_timeout": min(60, settings.deadline_seconds),
            "data_directory": str(directory / "data"), "cache_directory": str(directory / "cache"), "export_directory": str(directory / "exports")}},
    }
    for name, value in values.items():
        target = config / name
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as output:
            output.write(encode(value))
    # Environment credentials are the only supported secret channel.
    if (config / ".env").exists() or (config / "openbb.toml").exists() or (config / ".openbb.toml").exists():
        raise ValueError("Unexpected OpenBB configuration file; remove it from the isolated service state")


def worker():
    bootstrap = False
    try:
        os.umask(0o077)
        import resource
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_NOFILE, (128, 128))
        resource.setrlimit(resource.RLIMIT_CPU, (180, 180))
        # Native numerical libraries reserve virtual memory; bound actual memory at the service/container level.
        payload = sys.stdin.buffer.read(MAX_WORKER_BODY + 1)
        if len(payload) > MAX_WORKER_BODY:
            error("input_limit", "Tool arguments exceed 512 KiB.")
        request = read_json(payload)
        fields(request, {"name", "arguments", "contract"}, {"name", "arguments"})
        bootstrap = request["name"] == "_catalog_snapshot"
        if request["name"] not in {*TOOL_SCHEMAS, "_catalog_snapshot"} or not isinstance(request["arguments"], dict):
            error("unknown_tool", "Select a documented OpenBB data tool.")
        if len(encode({key: request[key] for key in ("name", "arguments")})) > MAX_BODY:
            error("input_limit", "Tool arguments exceed 512 KiB.")
        if "contract" in request and (request["name"] != "openbb_query" or len(encode(request["contract"])) > MAX_BODY):
            error("unsupported_schema", "The internal query contract exceeds its limit.", incomplete=True)
        namespaces = tuple(os.environ["FINANCE_OPENBB_NAMESPACES"].split(","))
        if not namespaces or any(name not in DEFAULT_NAMESPACES for name in namespaces):
            error("invalid_configuration", "The configured namespace allowlist is invalid.")
        saved_stdout = os.dup(1)
        with open(os.devnull, "w") as sink:
            try:
                os.dup2(sink.fileno(), 1)
                result = execute_tool(request["name"], request["arguments"], namespaces, contract=request.get("contract"))
            finally:
                sys.stdout.flush()
                os.dup2(saved_stdout, 1)
                os.close(saved_stdout)
    except FinanceError as exc:
        result = failure(exc.code, str(exc), incomplete=exc.incomplete)
    except Exception:  # noqa: BLE001 - redact all third-party backend exceptions at the process boundary.
        result = failure("provider_unavailable", "OpenBB could not complete the request; check installed packages, credentials and provider availability.")
    if not bootstrap:
        result = redact(result, [os.environ.get(name, "") for name in CREDENTIAL_ENV])
    try:
        raw = encode(result)
    except (ValueError, OverflowError):
        raw = encode(failure("unsupported_response", "OpenBB returned unsupported or non-finite data."))
    if len(raw) > (MAX_CATALOG_BYTES if bootstrap else MAX_OUTPUT_BYTES):
        raw = encode(failure("response_limit", "OpenBB returned too much data; narrow the query."))
    sys.stdout.buffer.write(raw)


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
    server_version = "BoxoroFinance/1"
    sys_version = ""

    def setup(self):
        self.request.settimeout(self.server.settings.deadline_seconds + 5)
        super().setup()
        self.deadline = time.monotonic() + self.server.settings.deadline_seconds
        self.expiry = threading.Timer(self.server.settings.deadline_seconds + 5, self.expire)
        self.expiry.daemon = True
        self.expiry.start()

    def expire(self):
        try:
            self.request.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

    def finish(self):
        self.expiry.cancel()
        super().finish()

    def log_message(self, format, *args):
        pass

    def parse_request(self):
        if len(self.raw_requestline) > 2048:
            self.requestline, self.request_version, self.command = "", "HTTP/1.1", None
            self.send_error(414)
            return False
        source, self.rfile = self.rfile, HeaderReader(self.rfile)
        try:
            return super().parse_request()
        finally:
            self.rfile = source

    def send_error(self, code, message=None, explain=None):
        self.respond(code, {"error": "request_rejected"})

    def respond(self, status, value=None):
        self.close_connection = True
        value = redact(value, [self.server.settings.token, *(value for _, value in self.server.settings.provider_credentials)])
        raw = b"" if value is None else encode(value)
        if len(raw) > MAX_RPC_RESPONSE:
            status, raw = 500, b'{"error":"response_limit"}'
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        if raw:
            self.wfile.write(raw)

    def authorized(self):
        if self.path != "/mcp":
            self.respond(404, {"error": "not_found"})
            return False
        if self.headers.get_all("Origin"):
            self.respond(403, {"error": "origin_rejected"})
            return False
        hosts = self.headers.get_all("Host", [])
        valid_hosts = {"127.0.0.1:" + str(self.server.server_port), "localhost:" + str(self.server.server_port)}
        if len(hosts) != 1 or hosts[0].lower() not in valid_hosts:
            self.respond(403, {"error": "host_rejected"})
            return False
        auth = self.headers.get_all("Authorization", [])
        if len(auth) != 1 or not hmac.compare_digest(auth[0].encode("utf-8"), ("Bearer " + self.server.settings.token).encode("ascii")):
            self.respond(401, {"error": "unauthorized"})
            return False
        versions = self.headers.get_all("MCP-Protocol-Version", [])
        if len(versions) > 1 or versions and versions[0] != PROTOCOL_VERSION:
            self.respond(400, {"error": "unsupported_protocol"})
            return False
        return True

    def do_GET(self):
        if self.authorized():
            self.respond(405, {"error": "method_not_allowed"})

    do_DELETE = do_GET
    do_OPTIONS = do_GET

    def do_POST(self):
        if not self.authorized():
            return
        if self.headers.get_all("Transfer-Encoding") or self.headers.get_all("Content-Encoding"):
            self.respond(400, {"error": "unsupported_encoding"})
            return
        sizes = self.headers.get_all("Content-Length", [])
        if len(sizes) != 1 or not re.fullmatch(r"[0-9]{1,9}", sizes[0]):
            self.respond(411 if not sizes else 400, {"error": "length_required"})
            return
        size = int(sizes[0])
        if size > MAX_BODY:
            self.respond(413, {"error": "body_too_large"})
            return
        types = self.headers.get_all("Content-Type", [])
        if len(types) != 1 or types[0].split(";", 1)[0].strip().lower() != "application/json":
            self.respond(415, {"error": "content_type_required"})
            return
        try:
            raw = self.rfile.read(size)
            if len(raw) != size:
                raise ValueError
            message = read_json(raw)
        except (FinanceError, ValueError):
            self.respond(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Invalid JSON"}})
            return
        self.rpc(message)

    def rpc(self, message):
        invalid = {"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "Invalid request"}}
        if (not isinstance(message, dict) or set(message) - {"jsonrpc", "id", "method", "params"}
                or message.get("jsonrpc") != "2.0" or not isinstance(message.get("method"), str) or not isinstance(message.get("params", {}), dict)):
            self.respond(400, invalid)
            return
        method, params, request_id = message["method"], message.get("params", {}), message.get("id")
        if method == "notifications/initialized" and "id" not in message and not params:
            self.respond(202)
            return
        if not (type(request_id) is int and 0 <= request_id <= 2 ** 53 or isinstance(request_id, str) and re.fullmatch(r"[A-Za-z0-9_.:-]{1,64}", request_id)):
            self.respond(400, invalid)
            return
        response = {"jsonrpc": "2.0", "id": request_id}
        if method == "initialize":
            if set(params) != {"protocolVersion", "capabilities", "clientInfo"} or not isinstance(params["capabilities"], dict) or not isinstance(params["clientInfo"], dict):
                response["error"] = {"code": -32602, "message": "Invalid initialization parameters"}
            else:
                response["result"] = {"protocolVersion": PROTOCOL_VERSION, "capabilities": {"tools": {}}, "serverInfo": {"name": "boxoro-finance-openbb", "version": __version__}}
        elif method == "ping" and not params:
            response["result"] = {}
        elif method == "tools/list" and not params:
            response["result"] = {"tools": [{"name": name, "description": TOOL_DESCRIPTIONS[name], "inputSchema": schema,
                                               "annotations": {"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": name == "openbb_query"}}
                                              for name, schema in TOOL_SCHEMAS.items()]}
        elif method == "tools/call":
            try:
                if set(params) != {"name", "arguments"} or params.get("name") not in TOOL_SCHEMAS or not isinstance(params.get("arguments"), dict):
                    error("unknown_tool", "Select a documented OpenBB data tool.")
                result = self.server.executor(params["name"], params["arguments"], self.server.settings,
                                              timeout=max(0, self.deadline - time.monotonic()))
            except FinanceError as exc:
                result = failure(exc.code, str(exc), incomplete=exc.incomplete)
            except Exception:  # noqa: BLE001 - HTTP errors must never expose provider secrets or tracebacks.
                result = failure("worker_failed", "The OpenBB operation could not complete.")
            if not isinstance(result, dict) or len(encode(result)) > MAX_OUTPUT_BYTES:
                result = failure("response_limit", "The OpenBB result exceeds the output limit.")
            result = redact(result, [self.server.settings.token, *(value for _, value in self.server.settings.provider_credentials)])
            response["result"] = {"content": [{"type": "text", "text": encode(result).decode("ascii")}],
                                  "structuredContent": result, "isError": result.get("gate") != "pass"}
            if len(encode(response)) > MAX_RPC_RESPONSE:
                result = failure("response_limit", "The encoded result exceeds the MCP client limit; narrow the query.")
                response["result"] = {"content": [{"type": "text", "text": encode(result).decode("ascii")}],
                                      "structuredContent": result, "isError": True}
        else:
            response["error"] = {"code": -32601, "message": "Method not found"}
        self.respond(200, response)


class FinanceServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, settings, port, executor):
        self.settings, self.executor = settings, executor
        self.slots = threading.BoundedSemaphore(settings.max_concurrency)
        super().__init__((settings.bind_address, port), Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            try:
                request.sendall(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
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
        pass


def make_server(settings, *, port=None, executor=None):
    return FinanceServer(settings, settings.port if port is None else port, executor or CatalogExecutor(settings))


def main(argv=None):
    os.umask(0o077)
    argv = sys.argv[1:] if argv is None else argv
    if argv == ["--worker"]:
        worker()
        return 0
    if argv:
        print("usage: finance_server.py", file=sys.stderr)
        return 2
    try:
        settings = Settings.from_environ(os.environ)
        prepare_state(settings)
        server = make_server(settings)
    except (OSError, ValueError):
        print("Finance OpenBB startup failed: check token, settings and loopback port.", file=sys.stderr)
        return 2
    print("Finance OpenBB listening on " + settings.bind_address + ":" + str(settings.port), file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0
