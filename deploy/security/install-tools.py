#!/usr/bin/env python3
"""Build-time installer. Only locked, verified upstream artifacts are executed.

Normal Boxoro scans never run this installer. Archive extraction copies selected
regular files rather than using extractall. --offline requires a preseeded cache
whose file names are the expected SHA-256 digest (without an extension).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path, PurePosixPath

MAX_BYTES = 512 * 1024 * 1024
MAX_MEMBERS = 10_000
DOWNLOAD_HOSTS = {"github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com", "raw.githubusercontent.com"}


class InstallError(Exception):
    pass


def validate_url(value: str) -> None:
    parsed = urllib.parse.urlsplit(value)
    if (parsed.scheme != "https" or parsed.hostname not in DOWNLOAD_HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)
            or parsed.fragment):
        raise InstallError("artifact URL must use an approved upstream HTTPS host")


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        validate_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def digest_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    count = 0
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            count += len(chunk)
            if count > MAX_BYTES:
                raise InstallError("artifact exceeds the installation size limit")
            digest.update(chunk)
    return digest.hexdigest(), count


def verified_artifact(artifact: dict, cache: Path, offline: bool) -> Path:
    expected = artifact["sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise InstallError("invalid locked SHA-256")
    validate_url(artifact["url"])
    path = cache / expected
    if path.is_symlink():
        raise InstallError("artifact cache entries must not be symlinks")
    if not path.exists():
        if offline:
            raise InstallError("offline artifact is missing: " + expected)
        cache.mkdir(parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(prefix="download-", dir=cache)
        try:
            request = urllib.request.Request(artifact["url"], headers={"User-Agent": "Boxoro-security-image/1"})
            opener = urllib.request.build_opener(SafeRedirect())
            with os.fdopen(fd, "wb") as output, opener.open(request, timeout=60) as response:
                validate_url(response.geturl())
                count = 0
                while chunk := response.read(1024 * 1024):
                    count += len(chunk)
                    if count > MAX_BYTES:
                        raise InstallError("download exceeds the installation size limit")
                    output.write(chunk)
            actual, count = digest_file(Path(temporary))
            if actual != expected:
                raise InstallError("SHA-256 mismatch for downloaded artifact")
            if artifact.get("size") is not None and count != artifact["size"]:
                raise InstallError("locked artifact size mismatch")
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)
    if not path.is_file():
        raise InstallError("artifact cache entry is not a regular file")
    actual, count = digest_file(path)
    if actual != expected:
        raise InstallError("SHA-256 mismatch for cached artifact")
    if artifact.get("size") is not None and count != artifact["size"]:
        raise InstallError("locked artifact size mismatch")
    return path


def copy_bounded(source, destination: Path, mode: int = 0o755) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix="install-", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            count = 0
            while chunk := source.read(1024 * 1024):
                count += len(chunk)
                if count > MAX_BYTES:
                    raise InstallError("extracted file exceeds the installation size limit")
                output.write(chunk)
        os.chmod(temporary, mode)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def install_tar(archive_path: Path, tool: dict, prefix: Path) -> None:
    with tarfile.open(archive_path, "r:*") as archive:
        executable = []
        notices = []
        members = archive.getmembers()
        if len(members) > MAX_MEMBERS:
            raise InstallError("archive has too many members")
        for member in members:
            path = PurePosixPath(member.name)
            if (path.is_absolute() or ".." in path.parts or "\\" in member.name
                    or not (member.isfile() or member.isdir())):
                raise InstallError("unsafe archive member: " + repr(member.name))
            if member.size < 0 or member.size > MAX_BYTES:
                raise InstallError("archive member exceeds the installation size limit")
            if member.isfile() and path.name == tool["member"]:
                executable.append(member)
            if member.isfile() and re.match(r"(?i)^(license|copying|notice)([._-].*)?$", path.name):
                notices.append(member)
        if len(executable) != 1:
            raise InstallError("archive must contain exactly one expected executable")
        with archive.extractfile(executable[0]) as source:
            copy_bounded(source, prefix / "bin" / tool["executable"])
        for number, member in enumerate(notices):
            with archive.extractfile(member) as source:
                copy_bounded(source, prefix / "licenses" / tool["name"] / f"{number}-{PurePosixPath(member.name).name}", 0o644)


def run(command: list[str], *, cwd: Path | None = None, timeout: int = 1200) -> None:
    subprocess.run(command, cwd=cwd, check=True, timeout=timeout)


def install_deb(path: Path, tool: dict, prefix: Path) -> None:
    if os.geteuid() != 0 or str(prefix) != "/opt/security":
        raise InstallError("official Debian packages may only be installed into the image as root")
    # Upstream package maintainer scripts, if present, are part of the verified
    # artifact. This runs only in Docker build, never against a repository.
    run(["dpkg", "--install", str(path)])
    for name in tool["executables"]:
        candidates = [Path("/usr/local/bin") / name, Path("/usr/bin") / name]
        executable = next((p for p in candidates if p.is_file() and os.access(p, os.X_OK)), None)
        if executable is None:
            raise InstallError("verified package did not install expected executable: " + name)
        destination = prefix / "bin" / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.unlink(missing_ok=True)
        destination.symlink_to(executable)
    run(["ldconfig"])


def install_source(tool: dict, prefix: Path, offline: bool) -> None:
    if offline:
        raise InstallError("offline source build requires a separately reviewed source mirror; unsupported")
    commit = tool["commit"]
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise InstallError("source build requires an exact Git commit")
    validate_url(tool["repository"])
    # Only the fixed YARA source build is supported. There is no user-supplied
    # build command or package install script from the workspace.
    if tool["name"] != "yara":
        raise InstallError("unsupported source build")
    with tempfile.TemporaryDirectory(prefix="boxoro-yara-") as directory:
        work = Path(directory)
        run(["git", "init", "--quiet", str(work)])
        run(["git", "-c", "core.hooksPath=/dev/null", "-c", "protocol.file.allow=never",
             "fetch", "--quiet", "--depth=1", "--no-tags", tool["repository"], commit], cwd=work)
        fetched = subprocess.check_output(["git", "rev-parse", "FETCH_HEAD"], cwd=work, text=True).strip()
        if fetched != commit:
            raise InstallError("source Git commit mismatch")
        run(["git", "-c", "core.hooksPath=/dev/null", "checkout", "--quiet", "--detach", commit], cwd=work)
        run(["./bootstrap.sh"], cwd=work)
        run(["./configure", "--prefix=" + str(prefix), *tool["configure"]], cwd=work)
        run(["make", "-j2"], cwd=work)
        run(["make", "install"], cwd=work)
        notices = prefix / "licenses" / "yara"
        notices.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(work / "COPYING", notices / "COPYING")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("tools.lock.json"))
    parser.add_argument("--prefix", type=Path, default=Path("/opt/security"))
    parser.add_argument("--artifact-dir", type=Path, default=Path("/tmp/boxoro-security-artifacts"))
    parser.add_argument("--profile", choices=("core", "malware"), default="core")
    parser.add_argument("--only", help="one tool in the selected profile, for a separate builder stage")
    parser.add_argument("--platform", default="linux/amd64" if platform.system() == "Linux" and platform.machine() == "x86_64" else "unsupported")
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    lock = json.loads(args.lock.read_text())
    if lock.get("schema_version") != 1:
        raise InstallError("unsupported lock schema")
    if args.platform not in lock.get("supported_platforms", []):
        raise InstallError("unsupported platform: " + args.platform)
    selected = [t for t in lock["tools"] if t["profile"] == args.profile and (not args.only or t["name"] == args.only)]
    if not selected:
        raise InstallError("no tools match the requested profile")
    args.prefix.mkdir(parents=True, exist_ok=True)
    manifest_path = args.prefix / "versions.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {"schema_version": 1, "platform": args.platform, "tools": {}}
    for tool in selected:
        name = tool["name"]
        if not re.fullmatch(r"[a-z][a-z0-9_-]*", name):
            raise InstallError("invalid tool name")
        record = {"version": tool["version"], "kind": tool["kind"], "license": tool.get("license", "see upstream")}
        if tool["kind"] == "git-source":
            install_source(tool, args.prefix, args.offline)
            record.update({"repository": tool["repository"], "commit": tool["commit"]})
        else:
            if not re.fullmatch(r"[a-z][a-z0-9_-]*", tool["executable"]):
                raise InstallError("invalid executable name")
            artifact = tool["artifacts"][args.platform]
            path = verified_artifact(artifact, args.artifact_dir, args.offline)
            if tool["kind"] == "tar":
                install_tar(path, tool, args.prefix)
            elif tool["kind"] == "raw":
                with path.open("rb") as source:
                    copy_bounded(source, args.prefix / "bin" / tool["executable"])
            elif tool["kind"] == "deb":
                install_deb(path, tool, args.prefix)
            else:
                raise InstallError("unsupported artifact kind")
            record.update({"url": artifact["url"], "sha256": artifact["sha256"]})
        for notice in tool.get("notices", []):
            verified = verified_artifact(notice, args.artifact_dir, args.offline)
            with verified.open("rb") as source:
                copy_bounded(source, args.prefix / "licenses" / name / notice["filename"], 0o644)
        manifest["tools"][name] = record
        print("installed " + name + " " + tool["version"], file=sys.stderr)
    manifest["tools_lock_sha256"] = hashlib.sha256(args.lock.read_bytes()).hexdigest()
    temporary = manifest_path.with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, manifest_path)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (InstallError, OSError, ValueError, KeyError, tarfile.TarError,
            subprocess.SubprocessError) as error:
        print("security tool installation failed: " + str(error), file=sys.stderr)
        sys.exit(1)
