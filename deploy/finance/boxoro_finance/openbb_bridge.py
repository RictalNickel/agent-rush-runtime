"""Discover and dispatch only the approved installed OpenBB V5 data surface."""
import asyncio
import contextlib
import functools
import hashlib
import importlib.metadata
import inspect
import ipaddress
import json
import os
import re
import socket
from urllib.parse import parse_qsl, quote, urlencode, urlsplit, urlunsplit

from .common import (
    MAX_OUTPUT_BYTES,
    encode,
    error,
    fields,
    integer,
    utc_now,
    validate_structure,
)
from .common import FinanceError as FinanceError  # noqa: PLC0414 - public boundary exception re-export.

PROVIDER_NAMESPACES = ("bls", "cboe", "cftc", "deribit", "ecb", "eia", "famafrench", "federal_reserve", "finra", "fred",
                       "imf", "jodi", "nasdaq", "oecd", "sec", "tmx", "uscongress", "usda", "ustreasury", "news")
ANALYTICAL_NAMESPACES = ("econometrics", "quantitative", "technical")
DEFAULT_NAMESPACES = PROVIDER_NAMESPACES + ANALYTICAL_NAMESPACES
CREDENTIAL_ENV = ("FRED_API_KEY", "EIA_API_KEY", "BLS_API_KEY", "CFTC_APP_TOKEN", "CONGRESS_GOV_API_KEY")
COMMAND_PATTERN = re.compile(r"[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*){1,8}\Z")
SENSITIVE = re.compile(r"(?:api[_-]?key|credential|password|secret|authorization|access[_-]?token|refresh[_-]?token|cookie|^token$)", re.IGNORECASE)


def redact(value, secrets):
    if isinstance(value, str):
        for secret in secrets:
            if secret:
                value = value.replace(secret, "[redacted]")
        # Provider metadata sometimes carries credential-bearing request URLs.
        if value.startswith(("https://", "http://")):
            try:
                parts = urlsplit(value)
                query = [(key, "[redacted]" if SENSITIVE.search(key) else item) for key, item in parse_qsl(parts.query, keep_blank_values=True)]
                if parts.username or parts.password:
                    netloc = parts.hostname or ""
                    if parts.port:
                        netloc += ":" + str(parts.port)
                else:
                    netloc = parts.netloc
                value = urlunsplit((parts.scheme, netloc, parts.path, urlencode(query), parts.fragment))
            except ValueError:
                return "[unparseable provider URL]"
        return value
    if isinstance(value, list):
        return [redact(item, secrets) for item in value]
    if isinstance(value, dict):
        return {redact(str(key), secrets): "[redacted]" if SENSITIVE.search(str(key)) else redact(item, secrets)
                for key, item in value.items()}
    return value


def reject_sensitive_arguments(value):
    if isinstance(value, dict):
        for key, item in value.items():
            if SENSITIVE.search(key):
                error("forbidden_argument", "Credentials cannot be supplied as tool arguments.")
            if key.lower() in {"output", "output_path", "output_dir", "file_path", "filename", "directory", "save", "export", "overwrite", "proxy", "server_url", "base_url"}:
                error("forbidden_argument", "File destinations, proxy settings and alternate servers are not query parameters.")
            reject_sensitive_arguments(item)
    elif isinstance(value, list):
        for item in value:
            reject_sensitive_arguments(item)
    elif isinstance(value, str) and ("://" in value or value.lstrip().lower().startswith(
            ("//", "\\\\", "http:", "https:", "file:", "ftp:", "s3:", "data:", "gopher:", "unix:"))):
        # URL-accepting endpoints remain discoverable, but require an operator-reviewed host list.
        try:
            parts = urlsplit(value)
            allowed = {part.strip().lower() for part in os.environ.get("FINANCE_OPENBB_URL_HOSTS", "").split(",") if part.strip()}
            if (parts.scheme != "https" or not parts.hostname or parts.hostname.lower() not in allowed
                    or parts.username or parts.password or parts.port not in (None, 443)
                    or "\\" in value or any(character.isspace() or ord(character) < 32 for character in value)
                    or any(SENSITIVE.search(key) for key, _ in parse_qsl(parts.query))):
                raise ValueError
        except ValueError:
            error("forbidden_url", "Caller-supplied URLs require an operator-approved HTTPS host.")


def resolve_schema(value, document, chain=()):
    if len(chain) > 32:
        error("unsupported_schema", "Recursive or excessively deep OpenAPI schema is unsupported.", incomplete=True)
    if isinstance(value, dict):
        if "$ref" in value:
            reference = value["$ref"]
            if not isinstance(reference, str) or not reference.startswith("#/") or reference in chain:
                error("unsupported_schema", "Only finite local OpenAPI references are supported.", incomplete=True)
            target = document
            try:
                for part in reference[2:].split("/"):
                    target = target[part.replace("~1", "/").replace("~0", "~")]
            except (KeyError, TypeError):
                error("unsupported_schema", "The installed OpenAPI schema contains an unresolved reference.", incomplete=True)
            expanded = resolve_schema(target, document, chain + (reference,))
            return {**expanded, **{key: resolve_schema(item, document, chain) for key, item in value.items() if key != "$ref"}}
        return {key: resolve_schema(item, document, chain) for key, item in value.items()}
    if isinstance(value, list):
        return [resolve_schema(item, document, chain) for item in value]
    return value


class Catalog:
    def __init__(self, schema, *, namespaces=DEFAULT_NAMESPACES):
        if not isinstance(schema, dict) or not isinstance(schema.get("paths"), dict):
            error("unsupported_schema", "OpenBB did not produce an OpenAPI command schema.", incomplete=True)
        self.commands = {}
        self.schema_hash = hashlib.sha256(json.dumps(schema, ensure_ascii=True, allow_nan=False, sort_keys=True, separators=(",", ":")).encode("ascii")).hexdigest()
        for path, item in schema["paths"].items():
            if not isinstance(path, str) or not path.startswith("/api/v1/") or not isinstance(item, dict):
                continue
            pieces = path[len("/api/v1/"):].strip("/").split("/")
            if not pieces or pieces[0] not in namespaces or len(pieces) < 2:
                continue
            if any(not re.fullmatch(r"[a-z][a-z0-9_]*|\{[a-z][a-z0-9_]*\}", part) for part in pieces):
                continue
            command = ".".join(part for part in pieces if not part.startswith("{"))
            if not COMMAND_PATTERN.fullmatch(command):
                continue
            for method in ("get", "post"):
                if method not in item or method == "post" and pieces[0] not in ANALYTICAL_NAMESPACES:
                    continue
                operation = item[method]
                if not isinstance(operation, dict):
                    continue
                # The data catalog never exposes arbitrary installs, account settings, or execution helpers.
                if any(part in {"user", "system", "login", "logout", "install", "execute", "dispatch", "save", "export"} for part in pieces):
                    continue
                parameters = []
                for parameter in [*item.get("parameters", []), *operation.get("parameters", [])]:
                    parameter = resolve_schema(parameter, schema)
                    if parameter.get("in") in ("query", "path"):
                        parameters.append(parameter)
                properties = {parameter["name"]: parameter.get("schema", {}) for parameter in parameters}
                required = list(dict.fromkeys(parameter["name"] for parameter in parameters if parameter.get("required")))
                body = resolve_schema(operation.get("requestBody", {}), schema)
                content = body.get("content", {}).get("application/json") if body else None
                if body and not content:
                    continue  # File/multipart upload routes are not a data query surface.
                name = command if method == "get" or "get" not in item else command + ".post"
                if name in self.commands:
                    error("unsupported_schema", "Command-name collision in the installed schema.", incomplete=True)
                self.commands[name] = {
                    "command": name, "namespace": pieces[0], "path": path, "method": method.upper(),
                    "description": str(operation.get("description", operation.get("summary", "")))[:12000],
                    "parameters_schema": {"type": "object", "properties": properties, "required": required, "additionalProperties": False},
                    "body_schema": content.get("schema", {}) if content else None,
                    "body_required": bool(body.get("required", False)), "parameter_locations": {parameter["name"]: parameter["in"] for parameter in parameters},
                    "response_schema": resolve_schema(operation.get("responses", {}).get("200", {}).get("content", {}).get("application/json", {}).get("schema", {}), schema),
                    "requires_network": pieces[0] not in ANALYTICAL_NAMESPACES or name == "quantitative.capm",
                }

    def describe(self, command):
        if not isinstance(command, str) or not COMMAND_PATTERN.fullmatch(command) or command not in self.commands:
            error("unknown_command", "The command is not available in the allowed installed OpenBB catalog.")
        return self.commands[command]

    def search(self, query="", namespace=None, offset=0, limit=20):
        if not isinstance(query, str) or len(query) > 200:
            error("invalid_parameters", "query must be at most 200 characters.")
        integer(offset, "offset", 0, 100000)
        integer(limit, "limit", 1, 100)
        if namespace is not None and namespace not in DEFAULT_NAMESPACES:
            error("invalid_parameters", "Select an approved namespace.")
        words = query.lower().split()
        results = [item for item in self.commands.values() if (namespace is None or item["namespace"] == namespace)
                   and all(word in (item["command"] + " " + item["description"]).lower() for word in words)]
        results.sort(key=lambda item: item["command"])
        return {"schema_sha256": self.schema_hash, "total": len(results), "offset": offset,
                "commands": [{key: item[key] for key in ("command", "namespace", "method", "requires_network")} |
                             {"description": item["description"][:400]} for item in results[offset:offset + limit]],
                "next_offset": offset + limit if offset + limit < len(results) else None}

    def validate_query(self, request):
        fields(request, {"command", "parameters", "body", "max_rows", "allow_network"}, {"command"})
        validate_structure(request)
        operation = self.describe(request["command"])
        parameters = request.get("parameters", {})
        if not isinstance(parameters, dict):
            error("invalid_parameters", "parameters must be a JSON object.")
        if "max_rows" in request:
            integer(request["max_rows"], "max_rows", 1, 10000)
        if "allow_network" in request and type(request["allow_network"]) is not bool:
            error("invalid_parameters", "allow_network must be a boolean.")
        reject_sensitive_arguments(parameters)
        body = request.get("body")
        reject_sensitive_arguments(body)
        try:
            import jsonschema
        except ImportError:
            error("missing_dependency", "Install the locked Finance OpenBB environment.", incomplete=True)
        try:
            jsonschema.Draft202012Validator(operation["parameters_schema"], format_checker=jsonschema.FormatChecker()).validate(parameters)
            if operation["body_schema"] is None:
                if "body" in request:
                    error("invalid_parameters", "This command does not accept a body.")
            elif "body" in request:
                jsonschema.Draft202012Validator(operation["body_schema"], format_checker=jsonschema.FormatChecker()).validate(body)
            elif operation["body_required"]:
                error("invalid_parameters", "This analytical command requires a JSON body.")
        except (jsonschema.ValidationError, jsonschema.SchemaError):
            error("invalid_parameters", "Arguments do not match the installed command schema; call openbb_describe.")
        return operation, parameters, body


def normalize_result(value, *, command, max_rows, secrets):
    integer(max_rows, "max_rows", 1, 10000)
    value = redact(value, secrets)
    total_rows = None
    truncated = False
    if isinstance(value, dict) and isinstance(value.get("results"), list):
        total_rows = len(value["results"])
        if total_rows > max_rows:
            value = {**value, "results": value["results"][:max_rows]}
            truncated = True
    elif isinstance(value, list):
        total_rows = len(value)
        if total_rows > max_rows:
            value = value[:max_rows]
            truncated = True
    result = {"schema_version": 1, "status": "partial" if truncated else "complete", "gate": "incomplete" if truncated else "pass",
              "command": command, "retrieved_at": utc_now(), "data": value, "total_rows": total_rows,
              "returned_rows": min(total_rows, max_rows) if total_rows is not None else None, "truncated": truncated,
              "limitations": ["Provider timestamps, delay, coverage, entitlements and terms govern this dataset.",
                               "Retrieved time is not the time of the market observation; inspect source metadata."]}
    if len(encode(result)) > MAX_OUTPUT_BYTES:
        error("response_limit", "The result exceeds the response limit; narrow the provider query.", incomplete=True)
    return result


@contextlib.contextmanager
def network_scope(enabled):
    """Defense in depth; operator egress firewall remains the network boundary."""
    original_connect = socket.socket.connect
    original_connect_ex = socket.socket.connect_ex
    original_getaddrinfo = socket.getaddrinfo
    original_sendto = socket.socket.sendto
    def permitted(address):
        if not enabled:
            error("network_required", "This request did not allow provider network access.")
        try:
            value = ipaddress.ip_address(address)
        except ValueError:
            error("network_denied", "The provider connection did not resolve to a public IP address.")
        if not value.is_global or value.is_multicast:
            error("network_denied", "Connections to non-public network addresses are blocked.")
    def resolve(host, *args, **kwargs):
        if not enabled:
            error("network_required", "This request did not allow provider network access.")
        answers = original_getaddrinfo(host, *args, **kwargs)
        for answer in answers:
            permitted(answer[4][0])
        return answers
    def validate_connection(sock, address):
        if sock.family in (socket.AF_INET, socket.AF_INET6):
            if not isinstance(address, tuple) or address[1] not in (80, 443):
                error("network_denied", "Only public HTTP(S) provider ports are enabled.")
            permitted(address[0])
        else:
            error("network_denied", "Local sockets are unavailable to provider queries.")
    def connect(sock, address):
        validate_connection(sock, address)
        return original_connect(sock, address)
    def connect_ex(sock, address):
        validate_connection(sock, address)
        return original_connect_ex(sock, address)
    def sendto(sock, data, *args):
        error("network_denied", "Datagram network access is unavailable to provider queries.")
    socket.getaddrinfo = resolve
    socket.socket.connect = connect
    socket.socket.connect_ex = connect_ex
    socket.socket.sendto = sendto
    try:
        yield
    finally:
        socket.getaddrinfo = original_getaddrinfo
        socket.socket.connect = original_connect
        socket.socket.connect_ex = original_connect_ex
        socket.socket.sendto = original_sendto


def load_openbb():
    try:
        version = importlib.metadata.version("openbb-core")
        if not version.startswith("2."):
            error("unsupported_version", "The bridge requires OpenBB V5 (openbb-core 2.x).", incomplete=True)
        from openbb_core.api.rest_api import app
    except (ImportError, importlib.metadata.PackageNotFoundError):
        error("missing_dependency", "Install the locked Finance OpenBB environment.", incomplete=True)
    return app, version


def preserve_envelope(app, operation):
    """Keep OBBject metadata mutated after construction in exclude-unset routes.

    OpenBB fills ``extra`` and ``warnings`` in place after constructing the
    result. Pydantic therefore does not mark these fields as explicitly set.
    Preserve only the documented envelope fields before FastAPI serializes;
    retain the endpoint's response model, exclusions and dependency graph.
    """
    from fastapi.routing import APIRoute
    try:
        from fastapi.routing import iter_route_contexts
        routes = iter_route_contexts(app.routes)
    except ImportError:
        # FastAPI before lazy included-router contexts flattened included routes.
        routes = (route for route in app.routes if isinstance(route, APIRoute))

    def mark(value):
        explicit = getattr(value, "model_fields_set", None)
        if isinstance(explicit, set):
            for name in ("id", "provider", "warnings", "extra"):
                if hasattr(value, name):
                    explicit.add(name)
        return value

    for route in routes:
        if route.path != operation["path"] or operation["method"] not in (route.methods or set()):
            continue
        call = route.dependant.call
        if getattr(call, "_boxoro_envelope", False):
            continue
        if inspect.iscoroutinefunction(call) or inspect.iscoroutinefunction(getattr(call, "__call__", None)):  # noqa: B004 - inspect async callable type, not callability.
            @functools.wraps(call)
            async def asynchronous(*args, _call=call, **kwargs):
                return mark(await _call(*args, **kwargs))
            wrapped = asynchronous
        else:
            @functools.wraps(call)
            def synchronous(*args, _call=call, **kwargs):
                return mark(_call(*args, **kwargs))
            wrapped = synchronous
        wrapped._boxoro_envelope = True
        route.dependant.call = wrapped


async def dispatch_asgi(app, operation, parameters, body):
    try:
        import httpx
    except ImportError:
        error("missing_dependency", "The installed OpenBB service is missing httpx.", incomplete=True)
    preserve_envelope(app, operation)
    path = operation["path"]
    query = []
    for name, value in parameters.items():
        if operation["parameter_locations"][name] == "path":
            text = str(value)
            if "/" in text or ".." in text or "\\" in text:
                error("invalid_parameters", "Path parameter contains an unsupported separator.")
            path = path.replace("{" + name + "}", quote(text, safe=""))
        elif isinstance(value, list):
            query.extend((name, item) for item in value)
        elif value is not None:
            query.append((name, value))
    transport = httpx.ASGITransport(app=app, raise_app_exceptions=False)
    async with httpx.AsyncClient(transport=transport, base_url="http://openbb-in-process", trust_env=False) as client:
        kwargs = {"params": query}
        if operation["body_schema"] is not None:
            kwargs["json"] = body
        response = await client.request(operation["method"], path, **kwargs)
    if response.status_code != 200:
        code = "provider_rate_limited" if response.status_code == 429 else "provider_auth_or_parameters" if response.status_code in (400, 401, 403, 422) else "provider_unavailable"
        error(code, "OpenBB could not complete the command; check provider setup and discovered parameters.", incomplete=True)
    if len(response.content) > MAX_OUTPUT_BYTES * 8:
        error("response_limit", "The provider response is too large; narrow the query.", incomplete=True)
    try:
        return response.json()
    except ValueError:
        error("unsupported_response", "The command did not return a finite JSON result; streaming responses are not supported.", incomplete=True)


def execute_tool(name, arguments, namespaces=DEFAULT_NAMESPACES):
    app, version = load_openbb()
    catalog = Catalog(app.openapi(), namespaces=namespaces)
    if name == "openbb_status":
        fields(arguments, set())
        return {"status": "complete", "gate": "pass", "openbb_core_version": version, "openbb_generation": 5,
                "command_count": len(catalog.commands), "schema_sha256": catalog.schema_hash,
                "namespaces": sorted({item["namespace"] for item in catalog.commands.values()}),
                "provider_setup": {name.removesuffix("_API_KEY").removesuffix("_APP_TOKEN").lower(): bool(os.environ.get(name)) for name in CREDENTIAL_ENV},
                "license": "Apache-2.0 software; provider data terms apply separately"}
    if name == "openbb_catalog":
        fields(arguments, {"query", "namespace", "offset", "limit"})
        return {"status": "complete", "gate": "pass", **catalog.search(**arguments)}
    if name == "openbb_describe":
        fields(arguments, {"command"}, {"command"})
        return {"status": "complete", "gate": "pass", "schema_sha256": catalog.schema_hash, **catalog.describe(arguments["command"])}
    if name != "openbb_query":
        error("unknown_tool", "Select a documented OpenBB data tool.")
    operation, parameters, body = catalog.validate_query(arguments)
    allow_network = arguments.get("allow_network", False)
    if operation["requires_network"] and not allow_network:
        error("network_required", "Provider queries require allow_network=true.")
    with network_scope(allow_network):
        value = asyncio.run(dispatch_asgi(app, operation, parameters, body))
    result = normalize_result(value, command=arguments["command"], max_rows=arguments.get("max_rows", 1000),
                              secrets=[os.environ.get(name, "") for name in CREDENTIAL_ENV])
    result["openbb_core_version"] = version
    result["schema_sha256"] = catalog.schema_hash
    return result
