#!/usr/bin/env python3
"""Print Markdown tables from bench.py result files.

Usage: report.py infra A.json [B.json ...]   |   report.py agent A.json [B.json ...]
"""
import json, sys


def infra(files):
    docs = [json.load(open(f)) for f in files]
    names = list(docs[0]["results"][0]["workloads"])
    print("| Host | Sessions at once | Wall (s) | " + " | ".join(f"{n} median (s)" for n in names) + " | Refused |")
    print("| --- | --- | --- | " + " | ".join("---" for _ in names) + " | --- |")
    for d in docs:
        for row in d["results"]:
            w = row["workloads"]
            refused = max(v["failed"] for v in w.values())
            cells = [str(w[n]["median_s"]) if w[n]["median_s"] is not None else "-" for n in names]
            print(f"| {d['label']} | {row['concurrency']} | {row['wall_seconds']} | " + " | ".join(cells) + f" | {refused} |")


def agent(files):
    print("| Run | Task | Passed | Time (s) | Tool calls | Cost (USD) | Ended |")
    print("| --- | --- | --- | --- | --- | --- | --- |")
    totals = []
    for f in files:
        d = json.load(open(f))
        rows = sorted((r for r in d["results"] if "task" in r), key=lambda r: (r["task"], r["rep"]))
        for r in rows:
            print(f"| {d['label']} | {r['task']} | {'yes' if r['passed'] else '**no**'} | {r['seconds']} | "
                  f"{r['tool_calls']} | {r['spend_usd'] or 0:.4f} | {r['end_reason']} |")
        if rows:
            totals.append((d["label"], sum(r["passed"] for r in rows), len(rows),
                           sum(r["seconds"] for r in rows) / len(rows), sum(r["spend_usd"] or 0 for r in rows)))
    print("\n| Run | Passed | Mean time (s) | Total cost (USD) |\n| --- | --- | --- | --- |")
    for label, ok, n, mean, cost in totals:
        print(f"| {label} | {ok}/{n} | {mean:.1f} | {cost:.4f} |")


if __name__ == "__main__":
    {"infra": infra, "agent": agent}[sys.argv[1]](sys.argv[2:])
