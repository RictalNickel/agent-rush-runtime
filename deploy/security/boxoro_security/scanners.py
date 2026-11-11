"""Fixed scanner commands; source content never selects flags or executables."""

import json
import re
import time
from datetime import datetime, timezone
from pathlib import Path

from .process import run_process
from .reports import finding, normalize_findings, text

DATABASE_MAX_AGE_SECONDS = 24 * 60 * 60
PROFILES = {
    "quick": ["gitleaks", "supply-chain"],
    "standard": [
        "gitleaks",
        "trivy-vulnerabilities",
        "trivy-misconfig",
        "semgrep",
        "syft",
        "supply-chain",
        "ruff",
        "shellcheck",
        "hadolint",
        "rustfmt",
    ],
    "secrets": ["gitleaks"],
    "dependencies": ["trivy-vulnerabilities", "syft"],
    "sast": ["semgrep"],
    "lint": ["ruff", "shellcheck", "hadolint", "rustfmt"],
    "supply_chain": ["syft", "supply-chain", "trivy-vulnerabilities"],
    "iac": ["trivy-misconfig", "hadolint"],
    "malware": ["clamscan", "yara", "supply-chain"],
}


def database_state(cache: Path, name: str, now=None):
    now = time.time() if now is None else now
    maximum_age = DATABASE_MAX_AGE_SECONDS
    next_update = None
    if name in ("trivy", "trivy-java"):
        directory = cache / "trivy" / ("db" if name == "trivy" else "java-db")
        metadata = directory / "metadata.json"
        database = directory / ("trivy.db" if name == "trivy" else "trivy-java.db")
        if any(
            p.is_symlink()
            for p in (
                cache.parent,
                cache,
                cache / "trivy",
                directory,
                metadata,
                database,
            )
        ):
            return {
                "status": "error",
                "reason": "Database paths must not contain symlinks.",
            }
        if (
            not metadata.is_file()
            or not database.is_file()
            or database.stat().st_size == 0
        ):
            return {
                "status": "missing",
                "reason": "Vulnerability database missing; run update_databases with offline=false.",
            }
        try:
            if metadata.stat().st_size > 65536:
                raise ValueError("oversized database metadata")
            data = json.loads(metadata.read_text())
            if not isinstance(data, dict) or not isinstance(data.get("UpdatedAt"), str):
                raise ValueError("invalid database build timestamp")
            built = datetime.fromisoformat(data["UpdatedAt"].replace("Z", "+00:00"))
            if built.tzinfo is None:
                raise ValueError("database build timestamp must include timezone")
            updated = built.timestamp()
            if type(data.get("Version")) is not int or data["Version"] != (
                2 if name == "trivy" else 1
            ):
                raise ValueError("unsupported database schema")
            if name == "trivy-java":
                maximum_age = 72 * 60 * 60
                if not isinstance(data.get("NextUpdate"), str):
                    raise ValueError("invalid Java database expiry")
                expiry = datetime.fromisoformat(
                    data["NextUpdate"].replace("Z", "+00:00")
                )
                if expiry.tzinfo is None:
                    raise ValueError("Java database expiry must include timezone")
                next_update = expiry.timestamp()
                if next_update <= updated or next_update > updated + maximum_age + 300:
                    raise ValueError("invalid Java database update window")
        except (KeyError, TypeError, ValueError, OSError):
            return {
                "status": "error",
                "reason": "Vulnerability database metadata is invalid.",
            }
    elif name == "clamav":
        directory = cache / "clamav"
        if any(
            p.is_symlink()
            for p in (
                cache,
                cache.parent,
                directory,
                directory / "daily.cvd",
                directory / "daily.cld",
                directory / "main.cvd",
                directory / "main.cld",
            )
        ):
            return {
                "status": "error",
                "reason": "Signature paths must not contain symlinks.",
            }
        candidates = [
            p for p in (directory / "daily.cvd", directory / "daily.cld") if p.is_file()
        ]
        main = [
            p for p in (directory / "main.cvd", directory / "main.cld") if p.is_file()
        ]
        if not candidates or not main:
            return {
                "status": "missing",
                "reason": "ClamAV main/daily signatures missing; update databases first.",
            }
        try:
            daily = max(candidates, key=lambda p: p.stat().st_mtime)
            with daily.open("rb") as stream:
                fields = stream.read(512).decode("ascii").strip().split(":")
            if fields[0] != "ClamAV-VDB" or len(fields) < 9:
                raise ValueError("invalid CVD/CLD header")
            updated = int(fields[8].strip())
        except (OSError, ValueError, UnicodeError, IndexError):
            return {
                "status": "error",
                "reason": "Signature build time could not be verified from CVD/CLD header.",
            }
    else:
        raise ValueError("unknown database")
    age = now - updated
    fresh = -300 <= age <= maximum_age and (next_update is None or now <= next_update)
    return {
        "status": "complete" if fresh else "stale_database",
        "updated_at": datetime.fromtimestamp(updated, timezone.utc).isoformat(),
        "age_seconds": round(age),
        "max_age_seconds": maximum_age,
        "reason": ""
        if fresh
        else "Database is outside its accepted freshness window or has an invalid future timestamp.",
    }


class Scanners:
    def __init__(
        self, source, state, cache, tool_bin, security_home, deadline, offline=True
    ):
        self.source = Path(source)
        self.state = Path(state)
        self.cache = Path(cache)
        self.tool_bin = Path(tool_bin)
        self.security_home = Path(security_home)
        self.deadline = deadline
        self.offline = offline
        self.rules = self.security_home / "rules"
        self.home = self.state / "home"
        self.home.mkdir(mode=0o700, exist_ok=True)

    def binary(self, scanner):
        return (
            self.tool_bin / scanner.split("-", 1)[0]
            if scanner.startswith("trivy-")
            else self.tool_bin / scanner
        )

    def execute(self, argv, *, file_limit=32 * 1024 * 1024, maximum_seconds=None):
        remaining = self.deadline - time.monotonic()
        return run_process(
            argv,
            self.state,
            min(remaining, maximum_seconds) if maximum_seconds else remaining,
            home=self.home,
            cache=self.cache,
            file_limit=file_limit,
        )

    def trivy_args(self, target, subject, scanners):
        args = [
            str(self.binary("trivy")),
            "--config",
            "",
            "--cache-dir",
            str(self.cache / "trivy"),
            target,
            "--scanners",
            scanners,
            "--format",
            "json",
            "--quiet",
            "--exit-code",
            "0",
            "--ignorefile",
            "",
            "--disable-telemetry",
            "--skip-db-update",
            "--skip-java-db-update",
            "--skip-check-update",
            "--skip-version-check",
        ]
        if self.offline:
            args += ["--offline-scan"]
        if target == "image":
            args += ["--image-src", "remote"]  # Never consult a host Docker socket.
        return args + [subject]

    def run(self, name, image=None):
        record = {"scanner": name, "status": "complete", "findings": 0}
        lint_files = None
        if name == "ruff":
            lint_files = list(self.source.rglob("*.py")) + list(
                self.source.rglob("*.pyi")
            )
        elif name == "shellcheck":
            lint_files = list(self.source.rglob("*.sh")) + list(
                self.source.rglob("*.bash")
            )
        elif name == "hadolint":
            lint_files = sorted(
                p
                for p in self.source.rglob("*")
                if p.is_file()
                and (
                    p.name == "Dockerfile"
                    or p.name.startswith("Dockerfile.")
                    or p.suffix == ".Dockerfile"
                )
            )
        elif name == "rustfmt":
            lint_files = sorted(self.source.rglob("*.rs"))
        if lint_files == []:
            return (
                {
                    **record,
                    "status": "not_applicable",
                    "reason": "No matching files in the bounded source snapshot.",
                },
                [],
                None,
            )
        executable = self.binary(name)
        if not executable.is_file():
            return (
                {
                    **record,
                    "status": "missing",
                    "reason": "Scanner is absent; use the security workspace image and run doctor.",
                },
                [],
                None,
            )
        if name == "trivy-vulnerabilities" or image:
            database = database_state(self.cache, "trivy")
            record["database"] = database
            if database["status"] != "complete":
                return (
                    {
                        **record,
                        "status": database["status"],
                        "reason": database["reason"],
                    },
                    [],
                    None,
                )
            if image or any(
                p.suffix.lower() in (".jar", ".war", ".ear")
                for p in self.source.rglob("*")
            ):
                java = database_state(self.cache, "trivy-java")
                record["java_database"] = java
                if java["status"] != "complete":
                    return (
                        {
                            **record,
                            "status": java["status"],
                            "reason": "Java archive coverage requires a fresh Java database; run update_databases.",
                        },
                        [],
                        None,
                    )
        if name == "clamscan":
            database = database_state(self.cache, "clamav")
            record["database"] = database
            if database["status"] != "complete":
                return (
                    {
                        **record,
                        "status": database["status"],
                        "reason": database["reason"],
                    },
                    [],
                    None,
                )
        raw_path = None
        files = []
        if name == "gitleaks":
            raw_path = self.state / "gitleaks.json"
            args = [
                str(executable),
                "dir",
                str(self.source),
                "--no-banner",
                "--no-color",
                "--redact=100",
                "--ignore-gitleaks-allow",
                "--gitleaks-ignore-path",
                "/dev/null",
                "--config",
                str(self.rules / "gitleaks.toml"),
                "--report-format",
                "json",
                "--report-path",
                str(raw_path),
                "--exit-code",
                "1",
            ]
        elif name.startswith("trivy"):
            args = self.trivy_args(
                "image" if image else "fs",
                image or str(self.source),
                "vuln" if name == "trivy-vulnerabilities" else "misconfig",
            )
        elif name == "semgrep":
            args = [
                str(executable),
                "scan",
                "--config",
                str(self.rules / "semgrep.yml"),
                "--json",
                "--error",
                "--oss-only",
                "--no-secrets-validation",
                "--metrics",
                "off",
                "--disable-version-check",
                "--disable-nosem",
                "--no-git-ignore",
                "--x-ignore-semgrepignore-files",
                "--jobs",
                "2",
                "--max-memory",
                "512",
                "--timeout",
                "10",
                "--max-target-bytes",
                "26214400",
                str(self.source),
            ]
        elif name == "syft":
            args = [
                str(executable),
                "scan",
                "dir:" + str(self.source),
                "--config",
                str(self.rules / "syft.yaml"),
                "--output",
                "cyclonedx-json",
                "--quiet",
            ]
        elif name == "ruff":
            files = lint_files
            args = [
                str(executable),
                "check",
                "--isolated",
                "--ignore-noqa",
                "--no-respect-gitignore",
                "--no-cache",
                "--select",
                "E4,E7,E9,F,I",
                "--output-format",
                "json",
                str(self.source),
            ]
        elif name == "shellcheck":
            files = lint_files
            args = [
                str(executable),
                "--norc",
                "--format=json",
                "--",
                *[str(p) for p in sorted(files)],
            ]
        elif name == "hadolint":
            files = lint_files
            args = [
                str(executable),
                "--config",
                str(self.rules / "hadolint.yaml"),
                "--disable-ignore-pragma",
                "--format",
                "json",
                *[str(p) for p in files],
            ]
        elif name == "rustfmt":
            files = lint_files
            args = [
                str(executable),
                "--check",
                "--edition",
                "2024",
                "--config-path",
                str(self.rules / "rustfmt.toml"),
                *[str(p) for p in files],
            ]
        elif name == "clamscan":
            args = [
                str(executable),
                "--recursive",
                "--infected",
                "--database=" + str(self.cache / "clamav"),
                "--follow-dir-symlinks=0",
                "--follow-file-symlinks=0",
                "--max-filesize=25M",
                "--max-scansize=100M",
                "--max-files=10000",
                "--max-recursion=16",
                "--alert-exceeds-max=yes",
                str(self.source),
            ]
        elif name == "yara":
            args = [
                str(executable),
                "-r",
                "-N",
                "-a",
                "60",
                str(self.rules / "indicators.yar"),
                str(self.source),
            ]
        else:
            raise ValueError("unknown scanner")
        if name in ("ruff", "shellcheck", "hadolint", "rustfmt") and not files:
            return (
                {
                    **record,
                    "status": "not_applicable",
                    "reason": "No matching files in the bounded source snapshot.",
                },
                [],
                None,
            )
        if len(files) > 1000 or sum(len(arg.encode()) for arg in args) > 100000:
            return (
                {
                    **record,
                    "status": "error",
                    "reason": "Too many lint targets for one bounded invocation; split the project.",
                },
                [],
                None,
            )
        suppressions = self.inline_suppressions(name, lint_files)
        result = self.execute(args)
        record.update(
            exit_code=result.returncode, duration_seconds=result.duration_seconds
        )
        if result.status != "complete":
            return (
                {
                    **record,
                    "status": result.status,
                    "reason": "Scanner did not finish within its resource/output budget.",
                },
                [],
                None,
            )
        accepted = (
            (0,) if name in ("syft", "yara") or name.startswith("trivy") else (0, 1)
        )
        if result.returncode not in accepted:
            return (
                {
                    **record,
                    "status": "error",
                    "reason": "Scanner returned an operational error. Raw diagnostics are omitted to protect source and credentials.",
                },
                [],
                None,
            )
        findings, sbom = [], None
        try:
            if name in ("clamscan", "yara", "rustfmt"):
                findings = self.parse_text(name, result.stdout, result.returncode)
                if name == "clamscan" and (
                    "ERROR" in result.stdout
                    or "ERROR" in result.stderr
                    or "WARNING" in result.stderr
                    or "Heuristics.Limits.Exceeded" in result.stdout
                ):
                    record.update(
                        status="error",
                        reason="Malware scan encountered unreadable content, signature or archive/file limits.",
                    )
                if name == "yara" and result.stderr.strip():
                    record.update(
                        status="error",
                        reason="YARA reported warnings or errors; coverage is incomplete.",
                    )
                if name == "rustfmt" and result.stderr.strip():
                    record.update(
                        status="error",
                        reason="Rustfmt reported parsing/configuration errors; coverage is incomplete.",
                    )
            else:
                raw = result.stdout
                if raw_path:
                    if (
                        not raw_path.is_file()
                        or raw_path.is_symlink()
                        or raw_path.stat().st_size > 16 * 1024 * 1024
                    ):
                        raise ValueError("missing or oversized scanner JSON")
                    raw = raw_path.read_text()
                payload = json.loads(raw)
                if name == "syft":
                    sbom = self.sanitize_sbom(payload)
                    record["components"] = len(sbom["components"])
                else:
                    findings = normalize_findings(name, payload)
                if name == "semgrep" and (
                    payload.get("errors") or (payload.get("paths") or {}).get("skipped")
                ):
                    record.update(
                        status="error",
                        reason="Semgrep reported parse/rule/timeout errors or skipped files; inspect supported-language coverage.",
                    )
                if result.returncode == 1 and not findings:
                    record.update(
                        status="error",
                        reason="Nonzero findings exit code without valid findings; do not interpret as clean.",
                    )
        except (ValueError, TypeError, KeyError, AttributeError, OSError):
            record.update(
                status="error",
                reason="Scanner output was missing, malformed, unsupported or exceeded a report limit.",
            )
        finally:
            if raw_path:
                raw_path.unlink(missing_ok=True)
        if suppressions:
            record.update(
                status="suppressed",
                suppression_files=suppressions,
                reason="Repository inline directives can hide scanner findings; review and remove the directives in the audit copy, then rerun. Source was not rewritten.",
            )
        for item in findings:
            raw = item.get("path", "")
            if raw.startswith(str(self.source) + "/"):
                item["path"] = raw[len(str(self.source)) + 1 :]
            elif Path(raw).is_absolute():
                item["path"] = "[scanner path outside snapshot omitted]"
            # Recompute after normalizing the temporary snapshot prefix.
            import hashlib

            identity = [
                item.get(k)
                for k in (
                    "scanner",
                    "rule_id",
                    "path",
                    "line",
                    "package",
                    "installed_version",
                )
            ]
            item["fingerprint"] = hashlib.sha256(
                json.dumps(identity, sort_keys=True).encode()
            ).hexdigest()
        record["findings"] = len(findings)
        return record, findings, sbom

    def inline_suppressions(self, name, lint_files):
        patterns = {
            "shellcheck": rb"(?im)^\s*#\s*shellcheck\b[^\r\n]*\bdisable\s*=",
            "trivy-misconfig": rb"(?i)(?:trivy|tfsec)\s*:\s*ignore\s*:",
            "rustfmt": rb"(?s)#\s*!?\s*\[[^\]]*rustfmt\s*::\s*skip",
        }
        if name not in patterns:
            return []
        expression = re.compile(patterns[name])
        paths = lint_files if lint_files is not None else self.source.rglob("*")
        suppressed = []
        for path in paths:
            if time.monotonic() >= self.deadline:
                break  # execute() observes the exhausted shared deadline.
            if path.is_file() and expression.search(path.read_bytes()):
                suppressed.append(text(path.relative_to(self.source).as_posix(), 1024))
                if len(suppressed) == 200:
                    break
        return suppressed

    def parse_text(self, name, output, returncode):
        findings = []
        if name == "rustfmt":
            if returncode:
                findings.append(
                    finding(
                        name,
                        "rust-format",
                        "low",
                        "Rust formatting differs or could not be checked with edition 2024",
                        category="lint",
                        remediation="Run the project's pinned formatter with its reviewed edition/configuration in the work copy, inspect the diff and re-run checks.",
                    )
                )
            return findings
        for line in output.splitlines():
            if name == "clamscan" and line.endswith(" FOUND"):
                path, separator, signature = line[:-6].rpartition(": ")
                if separator:
                    findings.append(
                        finding(
                            name,
                            signature,
                            "high",
                            "Antivirus signature or heuristic matched; quarantine and investigate",
                            path=path,
                            category="malware",
                            remediation="Do not execute this artifact. Quarantine the copy, validate provenance, investigate the signature and rebuild from trusted inputs; never automatically delete evidence.",
                        )
                    )
            elif name == "yara" and line.strip():
                rule, separator, path = line.partition(" ")
                if not separator or not re.fullmatch(r"[A-Za-z0-9_]+", rule):
                    raise ValueError("invalid YARA match")
                findings.append(
                    finding(
                        name,
                        rule,
                        "medium",
                        "Static indicator matched; this is not proof of malware",
                        path=path,
                        category="malware",
                    )
                )
        if returncode == 1 and not findings:
            raise ValueError("scanner failed without findings")
        return findings

    @staticmethod
    def sanitize_sbom(payload):
        if (
            not isinstance(payload, dict)
            or payload.get("bomFormat") != "CycloneDX"
            or not isinstance(payload.get("components", []), list)
        ):
            raise ValueError("invalid CycloneDX SBOM")
        if len(payload.get("components", [])) > 20000:
            raise ValueError("SBOM component limit exceeded")
        components = []
        serialized_bytes = 0
        for item in payload.get("components", []):
            if not isinstance(item, dict) or not isinstance(item.get("name"), str):
                raise ValueError("invalid component")
            component = {"type": "library", "name": text(item["name"], 300)}
            for key in ("version", "purl", "group", "bom-ref"):
                if item.get(key):
                    component[key] = text(item[key], 1000)
            licenses = []
            for lic in item.get("licenses", []):
                if (
                    isinstance(lic, dict)
                    and isinstance(lic.get("license"), dict)
                    and lic["license"].get("id")
                ):
                    licenses.append(
                        {"license": {"id": text(lic["license"]["id"], 100)}}
                    )
            if licenses:
                component["licenses"] = licenses
            serialized_bytes += len(json.dumps(component, ensure_ascii=True))
            if serialized_bytes > 8 * 1024 * 1024:
                raise ValueError("normalized SBOM byte limit exceeded")
            components.append(component)
        return {
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "version": 1,
            "components": components,
        }
