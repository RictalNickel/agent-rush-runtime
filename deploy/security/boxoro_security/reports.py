"""Allowlisted scanner normalization and credential-minimizing report formats."""

import hashlib
import json
import re
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote, urlsplit

SEVERITIES = {"critical": 0, "high": 1, "medium": 2, "low": 3, "info": 4, "unknown": 5}
SECRET_PATTERNS = [
    re.compile(r"(?:AKIA|ASIA)[A-Z0-9]{16}"),
    re.compile(r"(?:gh[pousr]_|github_pat_|sk-(?:proj-)?)[A-Za-z0-9_-]{16,}"),
    re.compile(r"(?i)(?:password|passwd|secret|token|api[_-]?key)\s*[=:]\s*[^\s;,]+"),
    re.compile(r"https?://[^\s/@]+:[^\s/@]+@"),
]


def text(value, limit=512):
    value = str(value) if isinstance(value, (str, int, float)) else ""
    value = "".join(c if c >= " " and c != "\x7f" else " " for c in value)
    for pattern in SECRET_PATTERNS:
        value = pattern.sub("[REDACTED]", value)
    return value[:limit]


def finding(
    scanner, rule, severity, title, *, path="", line=None, category="security", **extra
):
    severity = str(severity).lower()
    severity = {
        "warning": "medium",
        "warn": "medium",
        "error": "high",
        "note": "info",
        "style": "low",
    }.get(severity, severity)
    if severity not in SEVERITIES:
        severity = "unknown"
    record = {
        "scanner": text(scanner, 80),
        "rule_id": text(rule, 200),
        "severity": severity,
        "category": category,
        "title": text(title, 240),
        "path": text(path, 1024),
        "line": line if type(line) is int and line > 0 else None,
        "remediation": text(
            extra.pop(
                "remediation",
                "Review this finding in context, make a minimal fix, run tests and rescan.",
            ),
            1000,
        ),
    }
    for key in ("package", "installed_version", "fixed_version"):
        if extra.get(key):
            record[key] = text(extra[key], 200)
    refs = []
    for item in extra.get("references", []):
        if not isinstance(item, str) or len(item) > 1024:
            continue
        try:
            parsed = urlsplit(item)
            if (
                parsed.scheme == "https"
                and parsed.hostname
                and not parsed.username
                and not parsed.password
            ):
                refs.append(text(item, 1024))
        except ValueError:
            continue
    record["references"] = refs[:5]
    key = [
        record.get(k)
        for k in ("scanner", "rule_id", "path", "line", "package", "installed_version")
    ]
    record["fingerprint"] = hashlib.sha256(
        json.dumps(key, sort_keys=True).encode()
    ).hexdigest()
    return record


def objects(value):
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise ValueError("scanner output must contain a list of objects")
    return value


def required_string(value, key):
    if (
        not isinstance(value, dict)
        or not isinstance(value.get(key), str)
        or not value[key]
    ):
        raise ValueError("scanner finding is missing a required string")
    return value[key]


def required_line(value, key):
    if not isinstance(value, dict) or type(value.get(key)) is not int or value[key] < 1:
        raise ValueError("scanner finding is missing a valid source line")
    return value[key]


def required_severity(value, key):
    severity = required_string(value, key)
    if severity.lower() not in {
        *SEVERITIES,
        "warning",
        "warn",
        "error",
        "note",
        "style",
    }:
        raise ValueError("scanner finding uses an unsupported severity")
    return severity


def normalize_findings(scanner, payload):
    """Never copy arbitrary scanner messages, snippets, matches, or raw secrets."""
    output = []
    if scanner == "gitleaks":
        for item in objects(payload):
            if not isinstance(item.get("RuleID"), str) or not isinstance(
                item.get("File"), str
            ):
                raise ValueError("invalid secret finding")
            output.append(
                finding(
                    scanner,
                    item["RuleID"],
                    "high",
                    "Potential exposed credential (value redacted)",
                    path=item["File"],
                    line=item.get("StartLine"),
                    category="secret",
                    remediation="Revoke or rotate the credential at its provider; investigate use, remove it from source and history, and use a secret manager. Removal alone is not remediation.",
                )
            )
    elif scanner.startswith("trivy"):
        if (
            not isinstance(payload, dict)
            or payload.get("SchemaVersion") != 2
            or not isinstance(payload.get("Results", []), list)
        ):
            raise ValueError("invalid Trivy result")
        for result in objects(payload.get("Results", [])):
            path = required_string(result, "Target")
            for item in objects(result.get("Vulnerabilities") or []):
                package = required_string(item, "PkgName")
                advisory = required_string(item, "VulnerabilityID")
                severity = required_severity(item, "Severity")
                required_string(item, "InstalledVersion")
                fixed = item.get("FixedVersion", "")
                output.append(
                    finding(
                        scanner,
                        advisory,
                        severity,
                        f"{package}: {advisory}",
                        path=path,
                        category="dependency",
                        package=package,
                        installed_version=item.get("InstalledVersion"),
                        fixed_version=fixed,
                        references=[item.get("PrimaryURL")],
                        remediation=(
                            f"Resolve to a supported fixed version ({fixed}); update and verify the lockfile, run compatibility tests and rescan."
                            if fixed
                            else "No fixed version was reported. Check the upstream advisory, assess exposure and use a reviewed mitigation or replacement."
                        ),
                    )
                )
            for item in objects(result.get("Misconfigurations") or []):
                location = item.get("CauseMetadata") or {}
                if not isinstance(location, dict):
                    raise ValueError("invalid configuration finding location")
                output.append(
                    finding(
                        scanner,
                        required_string(item, "ID"),
                        required_severity(item, "Severity"),
                        "Infrastructure or container configuration issue",
                        path=path,
                        line=location.get("StartLine"),
                        category="configuration",
                        references=[item.get("PrimaryURL")],
                    )
                )
    elif scanner == "semgrep":
        if not isinstance(payload, dict) or not isinstance(
            payload.get("results"), list
        ):
            raise ValueError("invalid Semgrep result")
        for item in objects(payload["results"]):
            output.append(
                finding(
                    scanner,
                    required_string(item, "check_id"),
                    required_severity(item.get("extra"), "severity"),
                    "Static analysis rule matched; review the data flow",
                    path=required_string(item, "path"),
                    line=required_line(item.get("start"), "line"),
                    category="code",
                )
            )
    elif scanner == "ruff":
        for item in objects(payload):
            output.append(
                finding(
                    scanner,
                    required_string(item, "code"),
                    "low",
                    "Python lint or syntax issue",
                    path=required_string(item, "filename"),
                    line=required_line(item.get("location"), "row"),
                    category="lint",
                )
            )
    elif scanner == "shellcheck":
        for item in objects(payload):
            if type(item.get("code")) is not int or item["code"] < 1:
                raise ValueError("invalid ShellCheck rule identifier")
            output.append(
                finding(
                    scanner,
                    "SC" + str(item["code"]),
                    required_severity(item, "level"),
                    "Shell lint issue",
                    path=required_string(item, "file"),
                    line=required_line(item, "line"),
                    category="lint",
                )
            )
    elif scanner == "hadolint":
        for item in objects(payload):
            output.append(
                finding(
                    scanner,
                    required_string(item, "code"),
                    required_severity(item, "level"),
                    "Dockerfile lint issue",
                    path=required_string(item, "file"),
                    line=required_line(item, "line"),
                    category="lint",
                )
            )
    else:
        raise ValueError("unknown scanner parser")
    if len(output) > 10000:
        raise ValueError("finding count exceeds bounded report size")
    return output


def finalize_report(report):
    findings = report.setdefault("findings", [])
    checks = report.setdefault("checks", [])
    if len(findings) > 10000:
        findings = findings[:10000]
        checks.append(
            {
                "scanner": "report-normalizer",
                "status": "output_limit",
                "reason": "The combined finding limit of 10000 was exceeded; split the project and rerun.",
            }
        )
    unique = {}
    for index, item in enumerate(findings):
        unique[item.get("fingerprint", str(index))] = item
    findings = sorted(
        unique.values(),
        key=lambda item: (
            SEVERITIES.get(item.get("severity"), 5),
            item.get("path", ""),
            item.get("rule_id", ""),
        ),
    )
    bounded = []
    serialized_bytes = 0
    for item in findings:
        serialized_bytes += len(json.dumps(item, ensure_ascii=True))
        if serialized_bytes > 8 * 1024 * 1024:
            checks.append(
                {
                    "scanner": "report-normalizer",
                    "status": "output_limit",
                    "reason": "The combined normalized finding byte limit was exceeded; split the project and rerun.",
                }
            )
            break
        bounded.append(item)
    findings = bounded
    report["findings"] = findings
    incomplete = (
        not checks
        or any(c.get("status") not in ("complete", "not_applicable") for c in checks)
        or not report.get("snapshot", {"complete": True}).get("complete", True)
        or any(item.get("severity") == "unknown" for item in findings)
    )
    report["status"] = "partial" if incomplete else "complete"
    severe = any(item.get("severity") in ("critical", "high") for item in findings)
    report["gate"] = "incomplete" if incomplete else "fail" if severe else "pass"
    report["result"] = (
        "findings" if findings else "incomplete" if incomplete else "no_findings"
    )
    report["summary"] = {
        "findings": len(findings),
        "by_severity": dict(Counter(f.get("severity", "unknown") for f in findings)),
        "checks_complete": sum(c.get("status") == "complete" for c in checks),
        "checks_total": len(checks),
    }
    report["gate_policy"] = (
        "A selected profile passes only with complete coverage and no high/critical findings. A pass is not a security guarantee."
    )
    return report


def markdown_text(value):
    return (
        text(value, 2000)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("|", "\\|")
        .replace("[", "\\[")
        .replace("]", "\\]")
        .replace("`", "'")
    )


def write_reports(report, directory: Path):
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    (directory / "report.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=True) + "\n"
    )
    lines = [
        "# Boxoro security report",
        "",
        f"**Gate: {report['gate']} · Coverage: {report['status']} · Findings: {report['summary']['findings']}**",
        "",
        report["gate_policy"],
        "",
        "## Coverage",
        "",
        "| Check | Status | Details |",
        "| --- | --- | --- |",
    ]
    for check in report["checks"]:
        lines.append(
            f"| {markdown_text(check['scanner'])} | {markdown_text(check['status'])} | {markdown_text(check.get('reason', ''))} |"
        )
    lines += [
        "",
        "## Findings",
        "",
        "| Severity | Rule | Location | Finding |",
        "| --- | --- | --- | --- |",
    ]
    for item in report["findings"]:
        location = item.get("path", "") + (
            f":{item['line']}" if item.get("line") else ""
        )
        lines.append(
            f"| {item['severity']} | {markdown_text(item['rule_id'])} | {markdown_text(location)} | {markdown_text(item['title'])} |"
        )
    if not report["findings"]:
        lines += [
            "",
            "No normalized findings were returned. Check coverage before interpreting this result.",
        ]
    lines += ["", "## Limits and evidence", ""]
    lines.extend("- " + markdown_text(limit) for limit in report.get("limitations", []))
    if report.get("snapshot"):
        lines += [
            "",
            f"Snapshot SHA-256: `{report['snapshot'].get('sha256', 'not applicable')}`",
            f"Regular files copied: {report['snapshot'].get('files', 0)}. Source content is not retained in the report bundle.",
        ]
    lines += [
        "",
        "## Remediation",
        "",
        "See remediation.md for prioritized actions. Re-run the same profile after fixes; use a separate isolated copy for package installs, builds and tests.",
    ]
    (directory / "report.md").write_text("\n".join(lines) + "\n")
    remediation = [
        "# Security remediation plan",
        "",
        "This is a plan, not evidence that a fix was applied. Secret rotation and production changes require the authorized operator.",
        "",
    ]
    for number, item in enumerate(report["findings"], 1):
        remediation += [
            f"## {number}. {item['severity'].upper()}: {markdown_text(item['title'])}",
            "",
            f"Location: {markdown_text(item.get('path', ''))}",
            "",
            markdown_text(item["remediation"]),
            "",
        ]
        if item.get("package"):
            remediation += [
                f"Package: {markdown_text(item['package'])}; installed: {markdown_text(item.get('installed_version', 'unknown'))}; fixed version(s): {markdown_text(item.get('fixed_version', 'not reported'))}.",
                "",
            ]
    remediation += [
        "## Verification checklist",
        "",
        "1. Keep the original source and report; patch an isolated work copy.",
        "2. Inspect dependency scripts/configuration before dependency resolution; do not install with production credentials.",
        "3. Regenerate lockfiles using the project's package manager and review the resolved versions.",
        "4. Run targeted security regression tests and required build, unit, lint and type checks.",
        "5. Rescan with fresh databases and the same profile; compare coverage and findings.",
        "6. Present a reviewable diff and evidence; record unsupported checks and compatibility risks.",
    ]
    (directory / "remediation.md").write_text("\n".join(remediation) + "\n")
    rules = {}
    results = []
    for item in report["findings"]:
        rule_id = item["scanner"] + "/" + item["rule_id"]
        rules.setdefault(
            rule_id, {"id": rule_id, "shortDescription": {"text": item["title"]}}
        )
        result = {
            "ruleId": rule_id,
            "level": "error"
            if item["severity"] in ("high", "critical")
            else "warning"
            if item["severity"] in ("medium", "unknown")
            else "note",
            "message": {"text": item["title"] + ". " + item["remediation"]},
            "partialFingerprints": {"boxoro/v1": item["fingerprint"]},
            "properties": {
                "severity": item["severity"],
                "category": item.get("category", "security"),
            },
        }
        path = item.get("path")
        if path and not Path(path).is_absolute() and ".." not in Path(path).parts:
            physical = {
                "artifactLocation": {
                    "uri": quote(path, safe="/"),
                    "uriBaseId": "%SRCROOT%",
                }
            }
            if item.get("line"):
                physical["region"] = {"startLine": item["line"]}
            result["locations"] = [{"physicalLocation": physical}]
        results.append(result)
    sarif = {
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "Boxoro Security",
                        "version": "1.0.0",
                        "rules": list(rules.values()),
                    }
                },
                "invocations": [
                    {
                        "executionSuccessful": report["status"] == "complete",
                        "toolExecutionNotifications": [
                            {
                                "level": "warning",
                                "message": {"text": c["scanner"] + ": " + c["status"]},
                            }
                            for c in report["checks"]
                            if c["status"] not in ("complete", "not_applicable")
                        ],
                    }
                ],
                "results": results,
                "properties": {"gate": report["gate"], "coverage": report["status"]},
            }
        ],
    }
    (directory / "report.sarif").write_text(
        json.dumps(sarif, indent=2, ensure_ascii=True) + "\n"
    )
    for file in directory.iterdir():
        if file.is_file():
            file.chmod(0o600)
    return {
        "json": "report.json",
        "markdown": "report.md",
        "sarif": "report.sarif",
        "remediation": "remediation.md",
    }


def utc_now():
    return datetime.now(timezone.utc).isoformat()
