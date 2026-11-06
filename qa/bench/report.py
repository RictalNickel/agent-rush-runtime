#!/usr/bin/env python3
"""Print Markdown tables from bench.py result files.

Usage: report.py infra|agent|matrix|soak A.json [B.json ...]
"""
import json, statistics, sys


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


def matrix(files):
    """Passes per task and model, with median time and mean cost per model."""
    rows = [r for f in files for r in json.load(open(f))["results"] if "task" in r]
    models = sorted({r["model"] for r in rows})
    tasks = sorted({r["task"] for r in rows})
    print("| Task | " + " | ".join(models) + " |\n| --- | " + " | ".join("---" for _ in models) + " |")
    for t in tasks:
        cells = []
        for m in models:
            rs = [r for r in rows if r["task"] == t and r["model"] == m]
            cells.append(f"{sum(r['passed'] for r in rs)}/{len(rs)}")
        print(f"| {t} | " + " | ".join(cells) + " |")
    print("\n| Model | Passed | Median time (s) | Mean cost per task (USD) | Mean tool calls |\n| --- | --- | --- | --- | --- |")
    for m in models:
        rs = [r for r in rows if r["model"] == m]
        print(f"| {m} | {sum(r['passed'] for r in rs)}/{len(rs)} | {statistics.median(r['seconds'] for r in rs):.1f} | "
              f"{sum(r['spend_usd'] or 0 for r in rs) / len(rs):.4f} | {sum(r['tool_calls'] for r in rs) / len(rs):.1f} |")


def soak(files):
    print("| Run | Minute | Cycles | Failed | Median cycle (s) | p95 cycle (s) |\n| --- | --- | --- | --- | --- | --- |")
    for f in files:
        d = json.load(open(f))
        for r in d["results"]:
            print(f"| {d['label']} | {r['minute']} | {r['cycles']} | {r['failed']} | {r['median_s']} | {r['p95_s']} |")


if __name__ == "__main__":
    {"infra": infra, "agent": agent, "matrix": matrix, "soak": soak}[sys.argv[1]](sys.argv[2:])
