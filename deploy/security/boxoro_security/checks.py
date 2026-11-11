"""Small transparent checks that do not execute project code or crawl sites."""

import http.client
import json
import re
import time
from pathlib import Path
from urllib.parse import urlsplit

from .reports import finding


def supply_chain(source: Path, deadline):
    findings = []
    count = 0
    errors = 0
    for path in sorted(source.rglob("*")):
        if time.monotonic() >= deadline:
            return {
                "scanner": "supply-chain",
                "status": "timeout",
                "reason": "Manifest analysis exceeded the scan deadline.",
            }, findings
        if not path.is_file():
            continue
        relative = path.relative_to(source).as_posix()
        if (
            path.name
            not in (
                "package.json",
                "package-lock.json",
                "requirements.txt",
                "pyproject.toml",
                "Cargo.toml",
            )
            and not (
                ".github/workflows/" in relative and path.suffix in (".yml", ".yaml")
            )
            and path.suffix != ".pth"
        ):
            continue
        count += 1
        try:
            if path.stat().st_size > 5 * 1024 * 1024:
                errors += 1
                continue
            content = path.read_text(encoding="utf-8")
            if path.name == "package.json":
                package = json.loads(content)
                if not isinstance(package, dict):
                    raise ValueError("invalid manifest")
                scripts = package.get("scripts", {})
                if not isinstance(scripts, dict):
                    raise ValueError("invalid scripts")
                for lifecycle in ("preinstall", "install", "postinstall", "prepare"):
                    if scripts.get(lifecycle):
                        findings.append(
                            finding(
                                "supply-chain",
                                "npm-lifecycle-" + lifecycle,
                                "medium",
                                "Package lifecycle script requires review before installation",
                                path=relative,
                                category="supply_chain",
                                remediation="Inspect the lifecycle script and provenance before executing it. Resolve dependencies with scripts disabled in a fresh isolated work copy; enable only reviewed scripts when necessary.",
                            )
                        )
                if not any(
                    (path.parent / name).exists()
                    for name in (
                        "package-lock.json",
                        "npm-shrinkwrap.json",
                        "yarn.lock",
                        "pnpm-lock.yaml",
                        "bun.lock",
                        "bun.lockb",
                    )
                ):
                    findings.append(
                        finding(
                            "supply-chain",
                            "missing-node-lockfile",
                            "medium",
                            "JavaScript manifest has no adjacent lockfile",
                            path=relative,
                            category="supply_chain",
                        )
                    )
                for group in (
                    "dependencies",
                    "devDependencies",
                    "optionalDependencies",
                ):
                    dependencies = package.get(group, {})
                    if not isinstance(dependencies, dict):
                        raise ValueError("invalid dependencies")
                    for name, version in dependencies.items():
                        if not isinstance(version, str):
                            raise ValueError("invalid dependency")
                        if version.startswith("http:"):
                            findings.append(
                                finding(
                                    "supply-chain",
                                    "unencrypted-dependency",
                                    "high",
                                    "Dependency uses an unencrypted download URL",
                                    path=relative,
                                    category="supply_chain",
                                    package=name,
                                )
                            )
                        elif version.startswith(
                            ("git+", "git:", "github:", "https://github.com/")
                        ) and not re.search(r"#[0-9a-f]{40}$", version):
                            findings.append(
                                finding(
                                    "supply-chain",
                                    "mutable-git-dependency",
                                    "medium",
                                    "Git dependency is not pinned to a full commit",
                                    path=relative,
                                    category="supply_chain",
                                    package=name,
                                )
                            )
            elif path.name == "package-lock.json":
                lock = json.loads(content)
                if not isinstance(lock, dict) or not isinstance(
                    lock.get("packages", {}), dict
                ):
                    raise ValueError("invalid lockfile")
                for name, entry in lock.get("packages", {}).items():
                    if not isinstance(entry, dict):
                        raise ValueError("invalid lockfile package")
                    resolved = entry.get("resolved", "")
                    if isinstance(resolved, str) and resolved.startswith("http:"):
                        findings.append(
                            finding(
                                "supply-chain",
                                "unencrypted-lockfile-resolution",
                                "high",
                                "Lockfile resolves a package over unencrypted HTTP",
                                path=relative,
                                category="supply_chain",
                                package=name,
                            )
                        )
                    if entry.get("hasInstallScript") is True:
                        findings.append(
                            finding(
                                "supply-chain",
                                "dependency-install-script",
                                "medium",
                                "Locked dependency declares an installation script",
                                path=relative,
                                category="supply_chain",
                                package=name,
                            )
                        )
            elif path.name == "requirements.txt":
                for number, line in enumerate(content.splitlines(), 1):
                    line = line.strip()
                    if (
                        line
                        and not line.startswith(("#", "--", "-r ", "-c "))
                        and ("git+" in line or not re.search(r"==[^*\s]+", line))
                    ):
                        findings.append(
                            finding(
                                "supply-chain",
                                "unpinned-python-requirement",
                                "medium",
                                "Python requirement lacks an exact version pin",
                                path=relative,
                                line=number,
                                category="supply_chain",
                            )
                        )
            elif ".github/workflows/" in relative:
                for number, line in enumerate(content.splitlines(), 1):
                    match = re.search(r"\buses:\s*['\"]?([^\s'\"#]+)", line)
                    if (
                        match
                        and not match[1].startswith("./")
                        and not re.search(r"@[0-9a-f]{40}$", match[1])
                        and not re.search(r"@sha256:[0-9a-f]{64}$", match[1])
                    ):
                        findings.append(
                            finding(
                                "supply-chain",
                                "mutable-ci-action",
                                "medium",
                                "CI action is not pinned to an immutable commit or digest",
                                path=relative,
                                line=number,
                                category="supply_chain",
                            )
                        )
                    if re.search(
                        r"\bpull_request_target\b", line
                    ) and not line.lstrip().startswith("#"):
                        findings.append(
                            finding(
                                "supply-chain",
                                "privileged-pr-trigger",
                                "medium",
                                "Privileged pull request trigger requires trust-boundary review",
                                path=relative,
                                line=number,
                                category="supply_chain",
                                remediation="Verify the workflow never executes untrusted pull-request code with write tokens or secrets. Review checkout refs, permissions and every run step.",
                            )
                        )
            elif path.suffix == ".pth":
                for number, line in enumerate(content.splitlines(), 1):
                    if line.startswith(("import ", "import\t")):
                        findings.append(
                            finding(
                                "supply-chain",
                                "python-startup-code",
                                "medium",
                                "Python startup path file can execute code",
                                path=relative,
                                line=number,
                                category="supply_chain",
                            )
                        )
        except (OSError, ValueError, UnicodeError):
            errors += 1
        if len(findings) > 10000:
            return {
                "scanner": "supply-chain",
                "status": "output_limit",
                "reason": "Manifest finding limit exceeded.",
            }, findings[:10000]
    return {
        "scanner": "supply-chain",
        "status": "error" if errors else "complete",
        "manifests_read": count,
        "findings": len(findings),
        "reason": f"{errors} manifests could not be analyzed."
        if errors
        else "Static manifest/workflow heuristics; presence of a script does not prove malware.",
    }, findings


def web_headers(target, deadline):
    """One bounded GET to a literal loopback origin, no redirects or cookie jar."""
    parsed = urlsplit(target)
    connection = http.client.HTTPConnection(
        parsed.hostname,
        parsed.port,
        timeout=max(0.1, min(10, deadline - time.monotonic())),
    )
    check = {
        "scanner": "web-headers",
        "status": "complete",
        "requests": 1,
        "reason": "One local HTTP response; no crawling, redirect following, auth testing or attack payloads.",
    }
    findings = []
    try:
        connection.request(
            "GET",
            "/",
            headers={
                "User-Agent": "Boxoro-Security/1.0",
                "Accept": "text/html",
                "Connection": "close",
            },
        )
        response = connection.getresponse()
        check["http_status"] = response.status
        headers = response.getheaders()
        if sum(len(k) + len(v) for k, v in headers) > 65536:
            raise ValueError("header limit")
        values = {key.lower(): value for key, value in headers}
        if 300 <= response.status < 400:
            return {
                **check,
                "status": "skipped",
                "reason": "Redirect was deliberately not followed; test the app's direct loopback origin.",
            }, []
        if response.status >= 400:
            check.update(
                status="error",
                reason="The app returned an error response; application-route coverage is incomplete.",
            )
        html = "text/html" in values.get("content-type", "").lower()
        if html and not values.get("content-security-policy"):
            findings.append(
                finding(
                    "web-headers",
                    "missing-csp",
                    "medium",
                    "HTML response has no Content-Security-Policy",
                    category="web",
                )
            )
        if (
            html
            and not values.get("x-frame-options")
            and "frame-ancestors"
            not in values.get("content-security-policy", "").lower()
        ):
            findings.append(
                finding(
                    "web-headers",
                    "missing-frame-policy",
                    "medium",
                    "HTML response has no frame embedding restriction",
                    category="web",
                )
            )
        if values.get("x-content-type-options", "").lower() != "nosniff":
            findings.append(
                finding(
                    "web-headers",
                    "missing-nosniff",
                    "low",
                    "Response lacks X-Content-Type-Options: nosniff",
                    category="web",
                )
            )
        if html and not values.get("referrer-policy"):
            findings.append(
                finding(
                    "web-headers",
                    "missing-referrer-policy",
                    "low",
                    "HTML response has no explicit Referrer-Policy",
                    category="web",
                )
            )
        for key, value in headers:
            if key.lower() != "set-cookie":
                continue
            attributes = {
                part.strip().split("=", 1)[0].lower() for part in value.split(";")[1:]
            }
            for attribute, severity in (("httponly", "medium"), ("samesite", "low")):
                if attribute not in attributes:
                    findings.append(
                        finding(
                            "web-headers",
                            "cookie-missing-" + attribute,
                            severity,
                            "A response cookie lacks "
                            + attribute
                            + "; confirm whether this cookie is security sensitive",
                            category="web",
                        )
                    )
        check["findings"] = len(findings)
    except (OSError, ValueError, http.client.HTTPException):
        check.update(
            status="error",
            reason="Local HTTP response could not be inspected within bounded limits.",
        )
    finally:
        connection.close()
    return check, findings
