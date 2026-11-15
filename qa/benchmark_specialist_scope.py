#!/usr/bin/env python3
"""Compare full-profile discovery/readiness with an explicitly selected task.

The scopes intentionally differ. This measures avoided unrelated work, not a
claim that a narrow check establishes full-profile readiness.
"""

import argparse
import hashlib
import json
import statistics
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/agents"))

from boxoro_agents.runner import run_request  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", default="testforge")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--operation")
    selection.add_argument("--workflow")
    parser.add_argument("--tool-bin", type=Path, default=Path("/opt/box/agents/bin"))
    parser.add_argument("--repetitions", type=int, choices=range(1, 11), default=3)
    parser.add_argument("--timeout", type=int, choices=range(10, 541), default=60)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    catalog = ROOT / "deploy/agents/catalog.json"
    catalog_hash = hashlib.sha256(catalog.read_bytes()).hexdigest()
    selector = {"operation": args.operation} if args.operation else {"workflow": args.workflow}
    descriptions = {}
    observations = {"profile": [], "selected_task": []}
    scopes = {"profile": {}, "selected_task": selector}
    for label, scope in scopes.items():
        result = run_request({"action": "describe", "agent": args.agent, **scope}, catalog_path=catalog)
        descriptions[label] = {"response_bytes": len(json.dumps(result, ensure_ascii=True).encode()),
                               "operations": len(result["operations"]), "tools": len(result["tools"])}
    with tempfile.TemporaryDirectory(prefix="boxoro-scope-benchmark-") as directory:
        for repetition in range(args.repetitions):
            # Alternate ordering to reduce systematic first-run/cache bias.
            labels = list(scopes) if repetition % 2 == 0 else list(reversed(scopes))
            for label in labels:
                started = time.perf_counter()
                result = run_request({"action": "doctor", "agent": args.agent,
                                      "timeout_seconds": args.timeout, **scopes[label]},
                                     workspace=Path(directory), tool_bin=args.tool_bin, catalog_path=catalog)
                observations[label].append({"seconds": round(time.perf_counter() - started, 6),
                                            "checks": len(result["checks"]), "gate": result["gate"],
                                            "status_counts": result["status_counts"]})
    medians = {label: statistics.median(item["seconds"] for item in rows) for label, rows in observations.items()}
    if hashlib.sha256(catalog.read_bytes()).hexdigest() != catalog_hash:
        raise RuntimeError("The catalog changed during the measurement; rerun against a stable checkout.")
    output = {"schema_version": 1, "agent": args.agent, "selection": selector,
              "catalog_sha256": catalog_hash,
              "repetitions": args.repetitions, "descriptions": descriptions,
              "doctor_observations": observations, "median_seconds": medians,
              "limitations": ["Different scopes: selected-task readiness does not establish full-profile readiness.",
                              "Host elapsed measurements are sensitive to caches and concurrent workload; this is not a Docker or production benchmark.",
                              "Missing/unavailable tools remain visible in each observation; only present tools actually execute probes."]}
    encoded = json.dumps(output, indent=2, allow_nan=False) + "\n"
    if args.output:
        args.output.write_text(encoded)
    print(encoded, end="")


if __name__ == "__main__":
    main()
