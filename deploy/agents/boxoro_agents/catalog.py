"""Load and validate the image-owned specialist catalog, never project config."""

import hashlib
import json
import math
import re
from pathlib import Path
from string import Formatter

ID = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
PARAMETER = re.compile(r"[a-z][a-z0-9_]*\Z")
EFFECTS = {"project_code", "network", "mutation"}
FORMATS = {"path", "output_path", "loopback_url", "https_url", "identifier"}
TYPES = {"string", "integer", "number", "boolean"}


def strict_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON field")
            result[key] = value
        return result

    def constant(_value):
        raise ValueError("JSON numbers must be finite")

    def number(value):
        parsed = float(value)
        if not math.isfinite(parsed):
            raise ValueError("JSON numbers must be finite")
        return parsed

    try:
        return json.loads(raw, object_pairs_hook=unique, parse_constant=constant, parse_float=number)
    except (json.JSONDecodeError, UnicodeError, RecursionError) as error:
        raise ValueError("invalid JSON document") from error


def placeholders(tokens):
    result = set()
    if not isinstance(tokens, list) or len(tokens) > 128:
        raise ValueError("catalog argv must be a bounded token list")
    for token in tokens:
        if not isinstance(token, str) or len(token) > 8192 or "\0" in token:
            raise ValueError("catalog argv tokens must be bounded strings")
        try:
            parsed = list(Formatter().parse(token))
        except ValueError as error:
            raise ValueError("invalid catalog argv template") from error
        for _literal, name, format_spec, conversion in parsed:
            if name is not None:
                if not PARAMETER.fullmatch(name) or format_spec or conversion:
                    raise ValueError("catalog templates accept simple scalar names only")
                result.add(name)
    return result


class Catalog:
    def __init__(self, value, digest=""):
        if not isinstance(value, dict) or value.get("schema_version") != 1:
            raise ValueError("unsupported specialist catalog schema")
        self.value = value
        self.sha256 = digest
        for name in ("agents", "tools", "operations", "connectors"):
            section = value.get(name)
            if not isinstance(section, dict) or len(section) > 2000:
                raise ValueError("invalid specialist catalog section")
            if any(not isinstance(key, str) or not ID.fullmatch(key) for key in section):
                raise ValueError("catalog identifiers must be lowercase hyphenated IDs")
            if any(not isinstance(item, dict) for item in section.values()):
                raise ValueError("catalog records must be objects")
            setattr(self, name, section)
        self.validate()

    @classmethod
    def load(cls, path):
        data = Path(path).read_bytes()
        if len(data) > 8 * 1024 * 1024:
            raise ValueError("specialist catalog exceeds 8 MiB")
        return cls(strict_json(data), hashlib.sha256(data).hexdigest())

    def validate(self):
        for tool in self.tools.values():
            if tool.get("availability") not in {"image", "optional", "host", "external"}:
                raise ValueError("invalid tool availability")
            if not isinstance(tool.get("install"), dict) or tool["install"].get("kind") not in {
                "python", "npm", "binary", "apt", "builtin", "host", "external"
            }:
                raise ValueError("invalid tool installation record")
            if placeholders(tool.get("probe", [])):
                raise ValueError("version probes may not contain parameter templates")
            probe_codes = tool.get("probe_success_codes", [0])
            if not isinstance(probe_codes, list) or not probe_codes or any(type(code) is not int or not 0 <= code <= 255 for code in probe_codes):
                raise ValueError("invalid registered version-probe exit codes")
            if not isinstance(tool.get("sources"), list) or not tool["sources"]:
                raise ValueError("tools require primary-source references")
        for operation in self.operations.values():
            if operation.get("tool") not in self.tools:
                raise ValueError("operation references an unknown tool")
            parameters = operation.get("parameters", {})
            if not isinstance(parameters, dict) or len(parameters) > 32:
                raise ValueError("operation parameters must be a bounded object")
            for name, schema in parameters.items():
                if not PARAMETER.fullmatch(name) or name in {"path", "artifacts"}:
                    raise ValueError("invalid or reserved parameter name")
                if not isinstance(schema, dict) or schema.get("type") not in TYPES:
                    raise ValueError("parameter requires a supported scalar type")
                if set(schema) - {"type", "enum", "minimum", "maximum", "maxLength", "minLength", "default", "required", "format", "description"}:
                    raise ValueError("unsupported parameter-schema constraint")
                if schema.get("format") is not None and schema["format"] not in FORMATS:
                    raise ValueError("unknown parameter format")
                if schema.get("format") is not None and schema["type"] != "string":
                    raise ValueError("parameter formats require strings")
                for name in ("maxLength", "minLength"):
                    if name in schema and (type(schema[name]) is not int or not 0 <= schema[name] <= 8192 or schema["type"] != "string"):
                        raise ValueError("invalid string-length constraint")
                for name in ("minimum", "maximum"):
                    if name in schema and (type(schema[name]) not in {int, float} or (type(schema[name]) is float and not math.isfinite(schema[name])) or schema["type"] not in {"integer", "number"}):
                        raise ValueError("invalid numeric parameter constraint")
                if schema.get("minLength", 0) > schema.get("maxLength", 8192) or schema.get("minimum", -math.inf) > schema.get("maximum", math.inf):
                    raise ValueError("inverted parameter bounds")
                if "enum" in schema and (not isinstance(schema["enum"], list) or not schema["enum"]):
                    raise ValueError("parameter enum must be a nonempty array")
                if "required" in schema and type(schema["required"]) is not bool:
                    raise ValueError("parameter required must be boolean")
            used = placeholders(operation.get("argv", []))
            if used - {"path", "artifacts"} - set(parameters):
                raise ValueError("operation argv contains an unknown placeholder")
            effects = operation.get("effects", [])
            if not isinstance(effects, list) or set(effects) - EFFECTS:
                raise ValueError("unknown operation effect")
            successful = operation.get("success_codes", [0])
            findings = operation.get("finding_codes", [])
            for codes in (successful, findings):
                if not isinstance(codes, list) or any(type(code) is not int or not 0 <= code <= 255 for code in codes):
                    raise ValueError("invalid operation exit-code classification")
            if not successful or set(successful) & set(findings):
                raise ValueError("operation exit-code classes must be disjoint")
            timeout = operation.get("timeout_seconds", 120)
            if type(timeout) is not int or not 1 <= timeout <= 540:
                raise ValueError("invalid operation timeout")
        for agent in self.agents.values():
            tools = agent.get("tools")
            if not isinstance(tools, list) or set(tools) - set(self.tools):
                raise ValueError("agent references unknown tools")
            connectors = agent.get("connectors", [])
            if not isinstance(connectors, list) or set(connectors) - set(self.connectors):
                raise ValueError("agent references unknown connectors")
            workflows = agent.get("workflows", {})
            if not isinstance(workflows, dict):
                raise ValueError("agent workflows must be an object")
            for name, steps in workflows.items():
                if not ID.fullmatch(name) or not isinstance(steps, list) or not 1 <= len(steps) <= 64:
                    raise ValueError("invalid bounded workflow")
                for operation_id in steps:
                    operation = self.operations.get(operation_id)
                    if not operation or operation["tool"] not in tools:
                        raise ValueError("workflow operation is not owned by this agent")

    def agent(self, identifier):
        if identifier not in self.agents:
            raise ValueError("unknown specialist agent")
        return self.agents[identifier]

    def operation(self, agent_id, identifier):
        agent = self.agent(agent_id)
        if identifier not in self.operations or self.operations[identifier]["tool"] not in agent["tools"]:
            raise ValueError("operation is not registered for this specialist")
        return self.operations[identifier]
