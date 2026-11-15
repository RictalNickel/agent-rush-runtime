"""Native specialist tool entrypoint: fixed catalog operations, bounded evidence."""

import hashlib
import os
import secrets
import shutil
import time
from pathlib import Path

from . import __version__
from .catalog import Catalog, strict_json
from .policy import Request, scalar_text, secure_directory, validate_parameters, workspace_path
from .process import run_process
from .qualification import qualify_result
from .reports import artifact_inventory, private_write, report_gate, utc_now, write_reports

LIMITATIONS = [
    "Effect declarations describe the registered command; they are not authorization or an OS sandbox. Project code can execute arbitrary actions within the existing worker boundary.",
    "Network isolation and total CPU, memory and disk quotas depend on the Boxoro worker. CLI options do not make project code offline or read-only.",
    "Missing tools, blocked effects, errors, timeouts, cancellation and output limits are incomplete coverage, never a passing check.",
    "Native logs and artifacts are private local evidence and can contain project data or secrets; review them before publishing.",
    "Only installed dependencies are used. The runner does not install packages, connect external MCP servers, or provision a dedicated host.",
]


def parse_json(raw):
    if not isinstance(raw, (str, bytes)) or len(raw.encode("utf-8") if isinstance(raw, str) else raw) > 32768:
        raise ValueError("specialist request exceeds 32 KiB")
    return strict_json(raw)


def installed_manifest(catalog_path, selected):
    path = Path(catalog_path).parent / "installed.json"
    if not path.is_file():
        return {}
    try:
        data = path.read_bytes()
        if len(data) > 4 * 1024 * 1024:
            return {"status": "invalid"}
        value = strict_json(data)
        if not isinstance(value, dict):
            return {"status": "invalid"}
        # Package locks may contain hundreds of resolved dependencies. Keep the
        # native response small and omit arbitrary installer metadata fields.
        tools = value.get("tools", {})
        if not isinstance(tools, dict):
            tools = {}
        records = {}
        for name in selected:
            record = tools.get(name, {})
            if isinstance(record, dict):
                records[name] = {key: record[key][:512] for key in ("version", "package", "kind", "family", "sha256") if isinstance(record.get(key), str)}
        return {"status": "present", "sha256": hashlib.sha256(data).hexdigest(), "family": str(value.get("family", "unknown"))[:128], "tools": records}
    except (ValueError, OSError):
        return {"status": "invalid"}


def select_operations(request, catalog):
    if request.action == "run":
        return [(request.operation, catalog.operation(request.agent, request.operation))]
    workflows = catalog.agent(request.agent).get("workflows", {})
    if request.workflow not in workflows:
        raise ValueError("unknown workflow for this specialist")
    return [(name, catalog.operation(request.agent, name)) for name in workflows[request.workflow]]


def selected_scope(request, catalog):
    """Resolve a task's exact capabilities without executing project code."""
    agent = catalog.agent(request.agent)
    if request.operation:
        operations = {request.operation: catalog.operation(request.agent, request.operation)}
        kind, identifier = "operation", request.operation
    elif request.workflow:
        operations = dict(select_operations(request, catalog))
        kind, identifier = "workflow", request.workflow
    else:
        operations = {name: op for name, op in catalog.operations.items() if op["tool"] in agent["tools"]}
        kind, identifier = "agent", request.agent
    selected = agent["tools"] if kind == "agent" else list(dict.fromkeys(op["tool"] for op in operations.values()))
    scope = {"kind": kind, "id": identifier, "tools": list(selected), "operations": list(operations),
             "excluded_tools": [name for name in agent["tools"] if name not in selected]}
    return scope, operations


def describe(request, catalog, agent, scope, operations):
    scoped = scope["kind"] != "agent"
    definition = agent
    if scoped:
        definition = {key: agent[key] for key in ("name", "description", "family", "resources", "limitations") if key in agent}
        definition["tools"] = scope["tools"]
        definition["workflows"] = {request.workflow: agent["workflows"][request.workflow]} if request.workflow else {}
    result = {
        "schema_version": 1, "action": "describe", "agent": request.agent, "gate": "pass",
        "scope": scope, "definition": definition,
        "tools": {name: catalog.tools[name] for name in scope["tools"]}, "operations": operations,
        "connectors": {} if scoped else {name: catalog.connectors[name] for name in agent.get("connectors", [])},
        "checks": [], "reports": {},
        "limitations": agent.get("limitations", []) + ["Catalog definitions do not certify installed tools or connected services."],
    }
    if scoped:
        result["required_effects"] = sorted({effect for operation in operations.values() for effect in operation.get("effects", [])})
        result["limitations"].append("This response covers only the selected task; use an unscoped describe for the full profile, skill and connectors.")
    return result


def execution_plan(request, catalog, workspace, project, target):
    selected = select_operations(request, catalog)
    known = set().union(*(operation.get("parameters", {}).keys() for _name, operation in selected))
    if set(request.parameters) - known:
        raise ValueError("unknown workflow or operation parameter")
    plan = []
    for name, operation in selected:
        target_type = operation.get("target_type", "any")
        if (target_type == "directory" and not target.is_dir()) or (target_type == "file" and not target.is_file()):
            raise ValueError(name + " requires a " + target_type + " target")
        definitions = operation.get("parameters", {})
        supplied = {key: value for key, value in request.parameters.items() if key in definitions}
        values = validate_parameters(supplied, definitions, workspace, project)
        plan.append((name, operation, values))
    return plan


def tool_check(tool_id, tool, operation_id=None):
    check = {
        "tool": tool_id,
        "status": "missing",
        "exit_code": None,
        "duration_seconds": 0,
        "sources": tool.get("sources", []),
        "expected_version": tool.get("install", {}).get("version"),
        "reports": {},
        "artifacts": [],
    }
    if operation_id is not None:
        check["operation"] = operation_id
    return check


def available_executable(tool_id, tool, tool_bin, check):
    if tool.get("availability") in {"host", "external"}:
        check["status"] = "unavailable"
        check["reason"] = "This integration requires a separately configured host or external service."
        return None
    executable = tool_bin / tool_id
    if not executable.is_file() or not os.access(executable, os.X_OK):
        check["reason"] = "The registered executable is not installed in this worker image; choose its documented image family."
        return None
    return executable


def record_process(check, process, output, workspace, success_codes, finding_codes, deadline):
    check.update(exit_code=process.returncode, duration_seconds=process.duration_seconds)
    if process.status != "complete":
        check["status"] = process.status
    elif process.returncode in success_codes:
        check["status"] = "passed"
    elif process.returncode in finding_codes:
        check["status"] = "findings"
    else:
        check["status"] = "error"
    private_write(output / "stdout.txt", process.stdout)
    private_write(output / "stderr.txt", process.stderr)
    check["reports"] = {name: str((output / (name + ".txt")).relative_to(workspace)) for name in ("stdout", "stderr")}
    check["artifacts"], complete = artifact_inventory(output / "artifacts", workspace, deadline)
    if not complete:
        if check["status"] in {"passed", "findings"}:
            check["original_status"] = check["status"]
            check["status"] = "artifact_limit"
        check["reason"] = "Native artifacts were unsafe, changed while reading, or exceeded the entry, size or time limit."
    if check["status"] == "error":
        check["reason"] = "The command failed outside its registered findings exit codes; inspect the private stderr report and project prerequisites."


def run_operation(name, operation, values, request, catalog, workspace, target, tool_bin, output, state, deadline):
    tool_id = operation["tool"]
    tool = catalog.tools[tool_id]
    check = tool_check(tool_id, tool, name)
    check["notes"] = operation.get("notes", [])
    missing_effects = [effect for effect in operation.get("effects", []) if not getattr(request, "allow_" + effect)]
    check["effects"] = operation.get("effects", [])
    if missing_effects:
        check.update(status="blocked", reason="Explicit request declarations are required: " + ", ".join("allow_" + effect + "=true" for effect in missing_effects))
        return check
    if time.monotonic() >= deadline:
        check.update(status="timeout", reason="The shared request deadline was exhausted.")
        return check
    executable = available_executable(tool_id, tool, tool_bin, check)
    if executable is None:
        return check
    secure_directory(output)
    artifacts = secure_directory(output / "artifacts")
    secure_directory(state)
    substitutions = {"path": str(target), "artifacts": str(artifacts), **{key: scalar_text(value) for key, value in values.items()}}
    try:
        argv = [str(executable), *(token.format_map(substitutions) for token in operation["argv"])]
    except KeyError as error:
        raise ValueError("a registered command parameter requires a value") from error
    project = target if target.is_dir() else target.parent
    process = run_process(argv, project, min(operation.get("timeout_seconds", 120), deadline - time.monotonic()), home=state, workspace=workspace)
    record_process(check, process, output, workspace, operation.get("success_codes", [0]), operation.get("finding_codes", []), deadline)
    qualify_result(check, process, artifacts, workspace)
    return check


def doctor(catalog, selected, workspace, tool_bin, output, state, deadline):
    checks = []
    for index, tool_id in enumerate(selected):
        tool = catalog.tools[tool_id]
        check = tool_check(tool_id, tool)
        check["availability"] = tool["availability"]
        check["runnable_operations"] = sum(operation["tool"] == tool_id for operation in catalog.operations.values())
        check["capability"] = "registered_operations" if check["runnable_operations"] else "library_or_cli"
        checks.append(check)
        if time.monotonic() >= deadline:
            check.update(status="timeout", reason="The shared doctor deadline was exhausted.")
            continue
        executable = available_executable(tool_id, tool, tool_bin, check)
        if executable is None:
            continue
        probe = tool.get("probe", [])
        if not probe:
            check.update(status="unverified", reason="This tool has no noninteractive installed-version probe.")
            continue
        step_output = secure_directory(output / (str(index + 1).zfill(3) + "-" + tool_id))
        secure_directory(step_output / "artifacts")
        home = secure_directory(state / tool_id)
        process = run_process([str(executable), *probe], home, min(5, deadline - time.monotonic()), home=home, workspace=workspace)
        record_process(check, process, step_output, workspace, tool.get("probe_success_codes", [0]), [], deadline)
        if check["status"] == "passed":
            check["version_output"] = (process.stdout or process.stderr)[:1024].decode("utf-8", "replace").strip()
        if check["status"] == "cancelled":
            break
    return checks


def run_request(value, *, workspace=Path("/workspace"), tool_bin=Path("/opt/box/agents/bin"), catalog_path=None):
    request = Request.parse(value)
    catalog_path = Path(catalog_path or Path(__file__).resolve().parents[1] / "catalog.json")
    catalog = Catalog.load(catalog_path)
    if request.action == "list":
        return {"schema_version": 1, "action": "list", "gate": "pass", "agents": [{"id": name, **{key: agent[key] for key in ("name", "description", "family")}, "tools": len(agent["tools"]), "workflows": list(agent.get("workflows", {}))} for name, agent in catalog.agents.items()], "checks": [], "reports": {}, "limitations": ["This is a catalog index; use doctor to verify this worker's installations."]}
    if request.agent:
        agent = catalog.agent(request.agent)
    scope, operations = selected_scope(request, catalog)
    if request.action == "describe":
        return describe(request, catalog, agent, scope, operations)
    workspace = Path(workspace).resolve(strict=True)
    tool_bin = Path(tool_bin).absolute()
    target = workspace_path(workspace, request.path)
    project = target if target.is_dir() else target.parent
    plan = execution_plan(request, catalog, workspace, project, target) if request.action != "doctor" else []
    deadline = time.monotonic() + request.timeout_seconds
    root = secure_directory(workspace / ".specialists")
    reports_root = secure_directory(root / "reports")
    temporary_root = secure_directory(root / "tmp")
    identifier = secrets.token_hex(16)
    output = secure_directory(reports_root / identifier)
    state = secure_directory(temporary_root / identifier)
    result = {
        "schema_version": 1,
        "runner_version": __version__,
        "id": identifier,
        "action": request.action,
        "agent": request.agent,
        "started_at": utc_now(),
        "catalog_sha256": catalog.sha256,
        "installed_manifest": installed_manifest(catalog_path, scope["tools"]),
        "scope": scope,
        "checks": [],
        "reports": {},
        "limitations": list(LIMITATIONS),
        "declared_effects": {name: getattr(request, "allow_" + name) for name in ("project_code", "network", "mutation")},
    }
    if request.agent:
        result["limitations"] += catalog.agent(request.agent).get("limitations", [])
    try:
        if request.action == "doctor":
            result["checks"] = doctor(catalog, scope["tools"], workspace, tool_bin, output, state, deadline)
            result["limitations"].append("Doctor verifies installed command probes; project-specific imports, browsers, databases, device hosts, credentials and service readiness can still require separate qualification.")
            result["limitations"].append("A successful library or CLI probe with zero runnable operations is not a registered command adapter. Missing optional peers do not prevent individual installed operations from running.")
            if scope["kind"] != "agent":
                result["limitations"].append("This readiness result covers only the selected operation/workflow tools; excluded tools and the rest of the profile were not checked.")
        else:
            for index, (name, operation, values) in enumerate(plan):
                folder = str(index + 1).zfill(3) + "-" + name
                check = run_operation(name, operation, values, request, catalog, workspace, target, tool_bin, output / folder, state / folder, deadline)
                result["checks"].append(check)
                if check["status"] != "passed":
                    result["remaining_operations"] = [step[0] for step in plan[index + 1:]]
                    break
        result["completed_at"] = utc_now()
        result["gate"] = report_gate(result["checks"])
        result["coverage"] = {"completed": sum(check["status"] in {"passed", "findings"} for check in result["checks"]), "reported": len(result["checks"]), "findings": sum(check["status"] == "findings" or check.get("original_status") == "findings" for check in result["checks"])}
        result["status_counts"] = {status: sum(check["status"] == status for check in result["checks"]) for status in sorted({check["status"] for check in result["checks"]})}
        write_reports(result, output, workspace)
        return result
    finally:
        shutil.rmtree(state)
