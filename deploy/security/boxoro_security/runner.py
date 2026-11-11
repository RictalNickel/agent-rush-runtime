"""Public execution entrypoint used by the Boxoro native tool."""

import hashlib
import json
import os
import resource
import secrets
import shutil
import time
from pathlib import Path

from . import __version__
from .checks import supply_chain, web_headers
from .policy import (
    Request,
    secure_directory,
    snapshot_source,
    validate_cache_tree,
    validate_source,
)
from .reports import finalize_report, text, utc_now, write_reports
from .scanners import PROFILES, Scanners, database_state

TOOL_NAMES = (
    "gitleaks",
    "trivy",
    "syft",
    "semgrep",
    "ruff",
    "shellcheck",
    "hadolint",
    "rustfmt",
    "clamscan",
    "freshclam",
    "yara",
)
LIMITATIONS = [
    "Results apply only to this source snapshot, selected profile, rules and database versions; they do not prove absence of vulnerabilities, malware or compromise.",
    "Secret values and code snippets are omitted. Potential exposures are not verified with providers and do not prove credentials were stolen. Rotate/revoke exposed credentials and investigate independently.",
    "The source snapshot excludes Git history, generated build directories and (except malware profile) installed dependency directories. Audit full Git history and downloaded packages separately.",
    "SAST uses a small original rule pack, not Semgrep's commercial engine or registry rules; cross-file dataflow, authorization and business-logic review remain separate work.",
    "Offline mode disables scanner network enrichment and automatic database updates; it is not a network firewall. The image and Boxoro worker remain the isolation boundary.",
    "Dependency advisory databases and static indicators have coverage gaps. Unknown reachability is not evidence that a finding is unexploitable. License inventory is not legal approval.",
    "Lint coverage is Python, shell, Dockerfiles and Rust formatting. Project ESLint/typecheck, Clippy/build scripts and tests require a reviewed isolated execution step.",
]


def read_manifest(path):
    try:
        if path.stat().st_size > 262144:
            raise ValueError("manifest limit")
        value = json.loads(path.read_text())
        if not isinstance(value, dict):
            raise ValueError("manifest schema")
        return value
    except FileNotFoundError:
        return {"status": "missing"}
    except (OSError, ValueError):
        return {
            "status": "error",
            "reason": "Installation manifest is unreadable or unsupported.",
        }


def observed_resources(workspace):
    """Report visible limits without pretending they are operator configuration."""
    observed = {
        "scope": "Current process and mounted cgroup namespace; confirm worker configuration separately."
    }
    for key, name in (
        ("memory_max", "memory.max"),
        ("cpu_max", "cpu.max"),
        ("pids_max", "pids.max"),
    ):
        try:
            value = (Path("/sys/fs/cgroup") / name).read_text()[:128].strip()
            observed[key] = (
                value if all(c in "0123456789 max" for c in value) else "unavailable"
            )
        except OSError:
            observed[key] = "unavailable"
    try:
        disk = os.statvfs(workspace)
        observed["workspace_available_bytes"] = disk.f_bavail * disk.f_frsize
    except OSError:
        observed["workspace_available_bytes"] = None
    observed["process_open_files_soft_limit"] = resource.getrlimit(
        resource.RLIMIT_NOFILE
    )[0]
    return observed


def doctor(workspace, tool_bin, security_home):
    cache = workspace / ".security/cache"
    tools = {
        name: {
            "installed": (tool_bin / name).is_file(),
            "executable": os.access(tool_bin / name, os.X_OK),
            "path": str(tool_bin / name),
        }
        for name in TOOL_NAMES
    }
    lock = security_home / "tools.lock.json"
    manifest = read_manifest(lock)
    installed = read_manifest(security_home / "versions.json")
    return {
        "runner_version": __version__,
        "tools": tools,
        "profiles": PROFILES,
        "databases": {
            name: database_state(cache, name)
            for name in ("trivy", "trivy-java", "clamav")
        },
        "tool_manifest": manifest,
        "installed_manifest": installed,
        "availability_evidence": "File presence/executable permission and the install-time version manifest; run a scan or image qualification to verify execution.",
        "observed_resources": observed_resources(workspace),
        "credentials_required": False,
        "requirements": {
            "standard": "Security image; fresh Trivy database; sequential scans.",
            "malware": "Malware image, fresh ClamAV signatures, worker memory ceiling at least 8192 MiB and workspace at least 12288 MiB; measure your workload.",
        },
        "limitations": LIMITATIONS,
    }


def update_databases(scanners):
    checks = []
    trivy = scanners.binary("trivy")
    if trivy.is_file():
        for label, flag, database_name in (
            ("trivy-db-update", "--download-db-only", "trivy"),
            ("trivy-java-db-update", "--download-java-db-only", "trivy-java"),
        ):
            command = [
                str(trivy),
                "--config",
                "",
                "--cache-dir",
                str(scanners.cache / "trivy"),
                "image",
                flag,
                "--db-repository",
                "ghcr.io/aquasecurity/trivy-db:2",
                "--java-db-repository",
                "ghcr.io/aquasecurity/trivy-java-db:1",
                "--disable-telemetry",
                "--skip-version-check",
                "--quiet",
            ]
            result = scanners.execute(command, file_limit=1024 * 1024 * 1024)
            database = database_state(scanners.cache, database_name)
            checks.append(
                {
                    "scanner": label,
                    "status": "complete"
                    if result.status == "complete"
                    and result.returncode == 0
                    and database["status"] == "complete"
                    else result.status
                    if result.status != "complete"
                    else "error",
                    "exit_code": result.returncode,
                    "duration_seconds": result.duration_seconds,
                    "database": database,
                    "reason": "Downloads public advisory databases only; no project data is sent.",
                }
            )
    else:
        checks.append(
            {
                "scanner": "trivy-db-update",
                "status": "missing",
                "reason": "Install the security workspace image.",
            }
        )
    freshclam = scanners.binary("freshclam")
    if freshclam.is_file():
        directory = secure_directory(scanners.cache / "clamav")
        config = scanners.state / "freshclam.conf"
        config.write_text(
            "DatabaseDirectory "
            + str(directory)
            + "\nDatabaseMirror database.clamav.net\nDatabaseOwner agent\nChecks 1\nConnectTimeout 15\nReceiveTimeout 60\n"
        )
        result = scanners.execute(
            [str(freshclam), "--config-file=" + str(config), "--stdout"],
            file_limit=1024 * 1024 * 1024,
        )
        state = database_state(scanners.cache, "clamav")
        status = (
            result.status
            if result.status != "complete"
            else "complete"
            if result.returncode == 0 and state["status"] == "complete"
            else "error"
        )
        checks.append(
            {
                "scanner": "clamav-db-update",
                "status": status,
                "exit_code": result.returncode,
                "database": state,
                "reason": "Freshclam downloads and verifies signatures; no files are uploaded.",
            }
        )
    else:
        checks.append(
            {
                "scanner": "clamav-db-update",
                "status": "not_applicable",
                "reason": "Optional malware image is not installed.",
            }
        )
    return checks


def run_request(
    value,
    *,
    workspace=Path("/workspace"),
    tool_bin=Path("/opt/security/bin"),
    security_home=None,
):
    request = Request.parse(value)
    workspace = Path(workspace).resolve(strict=True)
    tool_bin = Path(tool_bin).resolve()
    security_home = Path(security_home or Path(__file__).resolve().parents[1])
    deadline = time.monotonic() + request.timeout_seconds
    if request.action == "doctor":
        return doctor(workspace, tool_bin, security_home)
    source = (
        validate_source(workspace, request.path) if request.action == "scan" else None
    )
    root = secure_directory(workspace / ".security")
    cache = secure_directory(root / "cache")
    secure_directory(cache / "trivy")
    validate_cache_tree(cache)
    reports_root = secure_directory(root / "reports")
    temp_root = secure_directory(root / "tmp")
    identifier = secrets.token_hex(16)
    state = temp_root / identifier
    state.mkdir(mode=0o700)
    output = reports_root / identifier
    output.mkdir(mode=0o700)
    started = utc_now()
    report = {
        "schema_version": 1,
        "runner_version": __version__,
        "id": identifier,
        "action": request.action,
        "profile": request.profile if request.action == "scan" else request.action,
        "started_at": started,
        "offline": request.offline,
        "target": text(
            source.relative_to(workspace).as_posix()
            if source
            else request.image or request.target or "public-databases"
        ),
        "checks": [],
        "findings": [],
        "limitations": list(LIMITATIONS),
    }
    try:
        snapshot = state / "source"
        if source:
            report["snapshot"] = snapshot_source(
                source,
                snapshot,
                include_dependencies=request.profile == "malware",
                deadline=deadline,
            )
        else:
            snapshot.mkdir(mode=0o700)
        scanners = Scanners(
            snapshot, state, cache, tool_bin, security_home, deadline, request.offline
        )
        if request.action == "update_databases":
            report["checks"] = update_databases(scanners)
            report["limitations"] = [
                "Database update success is not a scan result. Run the required profile afterward."
            ]
        elif request.action == "web":
            check, findings = web_headers(request.target, deadline)
            report["checks"].append(check)
            report["findings"].extend(findings)
            report["limitations"] = [
                "Only one loopback HTTP response was inspected, without redirect following. This is not a full DAST, TLS, authentication or authorization assessment.",
                "Header findings require application context. A scanner cannot assess business logic from headers.",
            ]
        else:
            selected = (
                ["trivy-vulnerabilities"]
                if request.action == "image"
                else PROFILES[request.profile]
            )
            if request.action == "image" and request.offline:
                report["checks"].append(
                    {
                        "scanner": "trivy-vulnerabilities",
                        "status": "skipped",
                        "reason": "Registry image scans require offline=false to pull the immutable public image; no Docker socket is used.",
                    }
                )
            else:
                for name in selected:
                    if time.monotonic() >= deadline:
                        report["checks"].append(
                            {
                                "scanner": name,
                                "status": "timeout",
                                "reason": "The shared scan deadline was exhausted.",
                            }
                        )
                        continue
                    if name == "supply-chain":
                        check, findings = supply_chain(snapshot, deadline)
                        sbom = None
                    else:
                        check, findings, sbom = scanners.run(name, image=request.image)
                    report["checks"].append(check)
                    report["findings"].extend(findings)
                    if sbom is not None:
                        (output / "sbom.cdx.json").write_text(
                            json.dumps(sbom, indent=2) + "\n"
                        )
                        report["sbom"] = {
                            "path": str(
                                (output / "sbom.cdx.json").relative_to(workspace)
                            ),
                            "components": len(sbom["components"]),
                            "format": "CycloneDX 1.6",
                            "sanitized": True,
                        }
        report["completed_at"] = utc_now()
        manifest = security_home / "tools.lock.json"
        if manifest.is_file():
            report["tool_manifest_sha256"] = hashlib.sha256(
                manifest.read_bytes()
            ).hexdigest()
        installed = read_manifest(security_home / "versions.json")
        report["installed_tool_versions"] = {
            text(name, 80): text(details.get("version", "unknown"), 100)
            for name, details in installed.get("tools", {}).items()
            if isinstance(details, dict)
        }
        rule_hashes = {}
        rules = security_home / "rules"
        if rules.is_dir():
            for rule in sorted(rules.iterdir()):
                if rule.is_file():
                    rule_hashes[rule.name] = hashlib.sha256(
                        rule.read_bytes()
                    ).hexdigest()
        report["rule_sha256"] = rule_hashes
        finalize_report(report)
        files = write_reports(report, output)
        return {
            "id": identifier,
            "status": report["status"],
            "gate": report["gate"],
            "result": report["result"],
            "summary": report["summary"],
            "checks": report["checks"],
            "reports": {
                kind: str((output / filename).relative_to(workspace))
                for kind, filename in files.items()
            },
            **({"sbom": report["sbom"]} if "sbom" in report else {}),
            "note": "Report paths are relative to /workspace. No raw credentials or source snippets are retained. Read coverage and remediation before acting.",
        }
    finally:
        shutil.rmtree(state)


def parse_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON field")
            result[key] = value
        return result

    if len(raw.encode()) > 32768:
        raise ValueError("request exceeds 32 KiB")
    return json.loads(raw, object_pairs_hook=unique)
