"""Strict request, scalar parameter and workspace-path validation."""

import math
import os
import re
import stat
from dataclasses import dataclass, field
from pathlib import Path
from urllib.parse import urlsplit

from .catalog import ID

INTERNAL = {".specialists", ".security"}


def bounded_text(value, label, maximum=4096):
    if not isinstance(value, str) or not value or len(value.encode("utf-8", "surrogatepass")) > maximum:
        raise ValueError(label + " must be a bounded nonempty string")
    if any(ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError(label + " must not contain control characters")
    return value


@dataclass(frozen=True)
class Request:
    action: str
    agent: str | None = None
    operation: str | None = None
    workflow: str | None = None
    path: str = "."
    parameters: dict = field(default_factory=dict)
    allow_project_code: bool = False
    allow_network: bool = False
    allow_mutation: bool = False
    timeout_seconds: int = 540

    @classmethod
    def parse(cls, value):
        if not isinstance(value, dict):
            raise ValueError("request must be an object")
        action = value.get("action")
        common = {"agent", "path", "timeout_seconds"}
        effects = {"allow_project_code", "allow_network", "allow_mutation"}
        allowed = {
            "list": {"timeout_seconds"},
            "describe": {"agent", "operation", "workflow", "timeout_seconds"},
            "doctor": {"agent", "operation", "workflow", "timeout_seconds"},
            "run": common | effects | {"operation", "parameters"},
            "workflow": common | effects | {"workflow", "parameters"},
        }
        if not isinstance(action, str) or action not in allowed:
            raise ValueError("unknown specialist action")
        names = {"action", "agent", "operation", "workflow", "path", "parameters", "timeout_seconds"} | effects
        if set(value) - names:
            raise ValueError("unknown specialist request field")
        arguments = {key: item for key, item in value.items() if key != "action" and item is not None}
        if set(arguments) - allowed[action]:
            raise ValueError("request field is not valid for this action")
        if "operation" in arguments and "workflow" in arguments:
            raise ValueError("operation and workflow selectors are mutually exclusive")
        for name in ("agent", "operation", "workflow"):
            if name in arguments and (not isinstance(arguments[name], str) or not ID.fullmatch(arguments[name]) or len(arguments[name]) > 128):
                raise ValueError("invalid specialist identifier")
        if action in {"describe", "doctor", "run", "workflow"} and "agent" not in arguments:
            raise ValueError("this action requires an agent")
        if action == "run" and "operation" not in arguments:
            raise ValueError("run requires an operation")
        if action == "workflow" and "workflow" not in arguments:
            raise ValueError("workflow requires a workflow name")
        timeout = arguments.get("timeout_seconds", 540)
        if type(timeout) is not int or not 10 <= timeout <= 540:
            raise ValueError("timeout_seconds must be an integer between 10 and 540")
        for name in effects:
            if name in arguments and type(arguments[name]) is not bool:
                raise ValueError("effect declarations must be booleans")
        if "path" in arguments:
            bounded_text(arguments["path"], "path")
        parameters = arguments.get("parameters", {})
        if not isinstance(parameters, dict) or len(parameters) > 32 or any(not isinstance(key, str) for key in parameters):
            raise ValueError("parameters must be an object with at most 32 fields")
        return cls(action=action, **arguments)


def workspace_path(workspace, value, *, base=None, output=False, directory=False):
    """Do not follow any requested symlink, even one pointing inside workspace."""
    bounded_text(value, "path")
    workspace = Path(workspace).resolve(strict=True)
    supplied = Path(value)
    if ".." in supplied.parts:
        raise ValueError("parent traversal is not a workspace path")
    candidate = supplied if supplied.is_absolute() else Path(base or workspace) / supplied
    try:
        relative = candidate.relative_to(workspace)
    except ValueError as error:
        raise ValueError("path must stay inside the workspace") from error
    if any(part in INTERNAL for part in relative.parts):
        raise ValueError("specialist input may not target internal report storage")
    current = workspace
    for part in relative.parts:
        current = current / part
        try:
            info = current.lstat()
        except FileNotFoundError:
            if output:
                continue
            raise ValueError("input path does not exist") from None
        if stat.S_ISLNK(info.st_mode):
            raise ValueError("workspace paths may not traverse symlinks")
        if not stat.S_ISREG(info.st_mode) and not stat.S_ISDIR(info.st_mode):
            raise ValueError("workspace paths must be regular files or directories")
    if not output and not candidate.exists():
        raise ValueError("input path does not exist")
    if directory and not candidate.is_dir():
        raise ValueError("project path must be an existing directory")
    return candidate


def secure_directory(path):
    path = Path(path)
    try:
        path.mkdir(mode=0o700)
    except FileExistsError:
        pass
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid():
        raise ValueError("report storage must be an owned directory without symlinks")
    path.chmod(0o700)
    return path


def parameter_value(name, value, schema, workspace, project):
    kind = schema["type"]
    if kind == "boolean":
        valid = type(value) is bool
    elif kind == "integer":
        valid = type(value) is int
    elif kind == "number":
        valid = type(value) is int or (type(value) is float and math.isfinite(value))
    else:
        valid = isinstance(value, str)
    if not valid:
        raise ValueError("parameter type does not match its registered schema: " + name)
    if "enum" in schema and not any(type(value) is type(item) and value == item for item in schema["enum"]):
        raise ValueError("parameter is not a registered enum value: " + name)
    if kind in {"number", "integer"}:
        if ("minimum" in schema and value < schema["minimum"]) or ("maximum" in schema and value > schema["maximum"]):
            raise ValueError("parameter is outside its registered bounds: " + name)
    if kind == "string":
        maximum = min(schema.get("maxLength", 4096), 8192)
        bounded_text(value, "parameter " + name, maximum)
        if len(value) < schema.get("minLength", 0):
            raise ValueError("parameter is shorter than its registered minimum: " + name)
        if value.startswith("-"):
            raise ValueError("parameter values may not begin with an option prefix")
        fmt = schema.get("format")
        if fmt in {"path", "output_path"}:
            value = str(workspace_path(workspace, value, base=project, output=fmt == "output_path"))
        elif fmt == "identifier" and not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", value):
            raise ValueError("parameter must be a plain identifier: " + name)
        elif fmt in {"https_url", "loopback_url"}:
            try:
                parts = urlsplit(value)
                port = parts.port
            except ValueError as error:
                raise ValueError("invalid URL parameter") from error
            if not parts.hostname or parts.username is not None or parts.password is not None or parts.fragment or "\\" in value or any(char.isspace() for char in value):
                raise ValueError("URL parameters may not contain credentials, fragments or whitespace")
            if fmt == "https_url" and parts.scheme != "https":
                raise ValueError("URL parameter requires HTTPS")
            if fmt == "loopback_url" and (parts.scheme not in {"http", "https"} or parts.hostname not in {"127.0.0.1", "::1"} or not port):
                raise ValueError("URL must use a literal loopback address and explicit port")
    return value


def validate_parameters(supplied, definitions, workspace, project):
    if set(supplied) - set(definitions):
        raise ValueError("unknown operation parameter")
    result = {}
    for name, schema in definitions.items():
        if name in supplied:
            value = supplied[name]
        elif "default" in schema:
            value = schema["default"]
        elif schema.get("required"):
            raise ValueError("missing required operation parameter: " + name)
        else:
            continue
        result[name] = parameter_value(name, value, schema, workspace, project)
    return result


def scalar_text(value):
    if type(value) is bool:
        return "true" if value else "false"
    return str(value)
