"""Finance CLI contract: fixed named analyses over bounded CSV/JSON input."""
import argparse
import importlib.metadata
import os
import sys
import warnings
from pathlib import Path

from . import __version__
from .advanced import OPERATIONS as ADVANCED_OPERATIONS
from .analytics import OPERATIONS as BASE_OPERATIONS
from .common import (
    MAX_INPUT_BYTES,
    MAX_OUTPUT_BYTES,
    FinanceError,
    encode,
    error,
    fields,
    read_file,
    read_json,
    utc_now,
    validate_structure,
)

OPERATIONS = {**BASE_OPERATIONS, **ADVANCED_OPERATIONS}
PACKAGES = ("numpy", "pandas", "scipy", "statsmodels", "arch", "cvxpy", "riskfolio-lib", "QuantLib", "duckdb", "polars", "pyarrow", "openpyxl", "plotly")


def doctor():
    installed = {}
    for name in PACKAGES:
        try:
            installed[name] = {"installed": True, "version": importlib.metadata.version(name)}
        except importlib.metadata.PackageNotFoundError:
            installed[name] = {"installed": False, "version": None}
    return {"baseline": "Python standard library", "operations": sorted(OPERATIONS), "packages": installed,
            "openbb": "Uses the separately authenticated control-side MCP service; no provider keys belong in workspaces.",
            "evidence": "Installed package metadata; an analysis smoke test is required to prove numerical backend execution."}


def run_request(request, *, workspace, provenance=None):
    fields(request, {"operation", "parameters", "data", "input"}, {"operation"}, "request")
    operation = request["operation"]
    if not isinstance(operation, str) or operation not in {*OPERATIONS, "doctor"}:
        error("unknown_operation", "Select a documented Finance Lab operation.")
    if "data" in request and "input" in request:
        error("invalid_parameters", "Provide either inline data or one input file.")
    parameters = request.get("parameters", {})
    if not isinstance(parameters, dict):
        error("invalid_parameters", "parameters must be a JSON object.")
    validate_structure(request)
    data = request.get("data")
    provenance = dict(provenance or {})
    if "input" in request:
        if not isinstance(request["input"], str):
            error("invalid_path", "input must name a workspace CSV or JSON file.")
        data, source = read_file(request["input"], workspace)
        provenance.update(source)
    if operation == "doctor":
        fields(parameters, set())
        result = doctor()
    else:
        with warnings.catch_warnings(record=True) as captured:
            warnings.simplefilter("always")
            result = OPERATIONS[operation](data, parameters)
        if captured:
            provenance["backend_warnings"] = sorted({type(item.message).__name__ for item in captured})
    provenance.update({"calculated_at": utc_now(), "engine": "boxoro-finance", "engine_version": __version__,
                       "source": "user-supplied data and parameters", "data_freshness": "not independently verified"})
    envelope = {"schema_version": 1, "operation": operation, "status": "complete", "gate": "pass", "results": result,
                "provenance": provenance, "limitations": ["Research calculations; assumptions and input quality determine validity.",
                "Rates and returns use decimal fractions. No live orders, brokerage access, or money movement."]}
    try:
        raw = encode(envelope)
    except (ValueError, OverflowError):
        error("numerical_failure", "The calculation produced a non-finite value; inspect the input and model.", incomplete=True)
    if len(raw) > MAX_OUTPUT_BYTES:
        error("response_limit", "The result exceeds the output limit; narrow the analysis.", incomplete=True)
    return envelope


def main(argv=None):
    parser = argparse.ArgumentParser(description="Local Finance Lab research analytics (decimal rates).")
    parser.add_argument("operation", nargs="?", choices=sorted({*OPERATIONS, "doctor"}))
    parser.add_argument("--input")
    parser.add_argument("--request-file")
    parser.add_argument("--request-stdin", action="store_true")
    parser.add_argument("--version", action="version", version=__version__)
    for name in ("column", "input-kind", "objective"):
        parser.add_argument("--" + name)
    for name in ("frequency", "fast", "slow", "horizon", "seed", "simulations"):
        parser.add_argument("--" + name, type=int)
    for name in ("risk-free-rate", "confidence", "cost-bps", "max-weight"):
        parser.add_argument("--" + name, type=float)
    args = parser.parse_args(argv)
    workspace = Path.cwd()
    # The installed wrapper supplies a fixed workspace root, not a model-controlled path.
    if os.environ.get("BOXORO_FINANCE_WORKSPACE"):
        workspace = Path(os.environ["BOXORO_FINANCE_WORKSPACE"])
    try:
        modes = int(bool(args.request_file)) + int(args.request_stdin)
        if modes > 1 or modes and (args.operation or args.input):
            error("invalid_parameters", "Choose one request interface.")
        source = {}
        overrides = {key: value for key, value in vars(args).items()
                     if key not in {"operation", "input", "request_file", "request_stdin"} and value is not None}
        if modes and overrides:
            error("invalid_parameters", "Request-file/stdin inputs cannot be combined with parameter flags.")
        if args.request_file:
            request, source = read_file(args.request_file, workspace)
        elif args.request_stdin:
            raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
            if len(raw) > MAX_INPUT_BYTES:
                error("input_limit", "Request exceeds 4 MiB.")
            request = read_json(raw)
        elif args.operation:
            request = {"operation": args.operation, "parameters": overrides}
            if args.input:
                value, source = read_file(args.input, workspace)
                if isinstance(value, dict):
                    fields(value, {"operation", "parameters", "data"}, (), "input document")
                    if value.get("operation", args.operation) != args.operation:
                        error("invalid_parameters", "Input operation does not match the fixed CLI operation.")
                    if not isinstance(value.get("parameters", {}), dict):
                        error("invalid_parameters", "parameters must be a JSON object.")
                    request["parameters"] = {**value.get("parameters", {}), **overrides}
                    if "data" in value:
                        request["data"] = value["data"]
                else:
                    request["data"] = value
        else:
            error("invalid_parameters", "An operation or request document is required.")
        result = run_request(request, workspace=workspace, provenance=source)
        status = 0
    except FinanceError as exc:
        result = {"schema_version": 1, "status": "error", "gate": "incomplete" if exc.incomplete else "fail", "code": exc.code, "message": str(exc)}
        status = 3 if exc.incomplete else 2
    except (OverflowError, ZeroDivisionError, ArithmeticError):
        result = {"schema_version": 1, "status": "error", "gate": "incomplete", "code": "numerical_failure", "message": "The numerical model could not produce a finite result."}
        status = 3
    except Exception:  # noqa: BLE001 - numerical backends must not disclose arbitrary exception details.
        result = {"schema_version": 1, "status": "error", "gate": "incomplete", "code": "analysis_failed", "message": "The analysis failed; inspect the input schema and installed numerical backend."}
        status = 3
    sys.stdout.buffer.write(encode(result) + b"\n")
    return status
